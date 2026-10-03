//! Documentos do estúdio de TV (ADR-0014 §5) — adaptador HTTP + Postgres.
//!
//! Seis tipos com UM contrato: cenas de mistura, macros, sobreposições, cenas
//! de luz, perfis de correcção por câmara e alinhamentos. O que muda entre
//! eles é só a forma do `body`, e essa vive toda no domínio
//! (`delonix_meet_domain::studio::document`), que é a única porta de entrada
//! de um corpo. Aqui trata-se do que é IO: quem pode, a página, a versão
//! optimista, a tecla única e o histórico.
//!
//! O tipo vem no CAMINHO (`…/studios/{id}/mixer-scenes`) e é validado contra
//! os seis segmentos conhecidos — um segmento desconhecido é `404`, não um
//! tipo novo. Um handler por operação em vez de seis cópias: o contrato é
//! igual, e seis cópias das mesmas seis queries é a duplicação que a catraca
//! da arquitectura recusa.
//!
//! Quem pode (as mesmas de `studio.rs`): ler = membro activo da org (senão
//! `404`); escrever = `operate` (admin da org ou quem criou o estúdio, senão
//! `403 studio.not_operator`).

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::{header::LOCATION, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::{
    page::{Page, PageRequest},
    DomainError,
};
use delonix_meet_domain::studio::document::{self as rules, Kind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::ApiError,
    studio::{load_studio, member, ListQuery},
    AppState,
};

// ------------------------------------------------------------------- tipos ---

/// O tipo pedido no caminho. Desconhecido = `404` (não existe recurso nenhum
/// nesse caminho), nunca `400`: quem pede um tipo que não existe não chegou ao
/// recurso, é a mesma regra de outra org.
fn kind_from_segment(seg: &str) -> Result<Kind, ApiError> {
    Kind::ALL
        .into_iter()
        .find(|k| k.path_segment() == seg)
        .ok_or(ApiError::NotFound)
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Document {
    pub id: Uuid,
    pub studio_id: Uuid,
    /// `mixer_scene`, `macro`, `overlay`, `light_scene`, `camera_profile`, `rundown`.
    pub kind: String,
    pub name: String,
    #[schema(nullable)]
    pub key: Option<String>,
    pub version: i32,
    #[schema(value_type = Object)]
    pub body: Value,
    /// Calculado no servidor; hoje só o alinhamento o traz.
    #[schema(nullable, value_type = Option<Object>)]
    pub summary: Option<Value>,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const DOC_SELECT: &str = "SELECT id, studio_id, kind, name, key, version, body, summary,
        created_by, updated_by, created_at, updated_at FROM studio_documents";

#[derive(Serialize, utoipa::ToSchema)]
pub struct DocumentPage {
    pub items: Vec<Document>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct DocumentVersion {
    pub version: i32,
    pub name: String,
    #[schema(value_type = Object)]
    pub body: Value,
    #[schema(nullable, value_type = Option<Object>)]
    pub summary: Option<Value>,
    pub updated_by: Uuid,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DocumentVersionPage {
    pub items: Vec<DocumentVersion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateDocumentReq {
    pub name: String,
    #[schema(value_type = Object)]
    pub body: Value,
}

/// `version` é obrigatória: é ela que impede a gravação cega por cima do
/// trabalho de outro operador. `name`/`body` omissos ficam como estavam.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateDocumentReq {
    pub version: i32,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    #[schema(nullable, value_type = Option<Object>)]
    pub body: Option<Value>,
}

#[derive(Serialize, Deserialize)]
struct Cursor {
    at: DateTime<Utc>,
    id: Uuid,
}

#[derive(Serialize, Deserialize)]
struct VersionCursor {
    version: i32,
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        list_documents,
        create_document,
        get_document,
        update_document,
        delete_document,
        list_versions
    ),
    components(schemas(
        Document,
        DocumentPage,
        DocumentVersion,
        DocumentVersionPage,
        CreateDocumentReq,
        UpdateDocumentReq
    ))
)]
pub struct ApiDoc;

// ------------------------------------------------------------------ comum ---

/// A tecla já está tomada neste tipo e estúdio.
fn key_taken(key: &str) -> ApiError {
    DomainError::conflict(
        "studio.key_taken",
        format!("a tecla {key} já está atribuída a outro documento deste tipo neste estúdio"),
    )
    .with_field("key", "uma tecla livre")
    .into()
}

/// Um erro de escrita traduzido: a violação do índice único da tecla é um
/// `409` com código, não um 500. A corrida entre dois operadores a gravar a
/// mesma tecla ao mesmo tempo passa pelo índice, não por um `SELECT` antes —
/// verificar primeiro e inserir depois é exactamente a janela que o índice
/// fecha.
fn write_error(e: sqlx::Error, key: Option<&str>) -> ApiError {
    if let (Some(db), Some(k)) = (e.as_database_error(), key) {
        if db.constraint() == Some("studio_documents_key_uidx") {
            return key_taken(k);
        }
    }
    e.into()
}

/// Os documentos que o corpo refere têm de ser DESTE estúdio. Uma macro que
/// aponta para a cena de outro estúdio é uma fuga entre estúdios (e, se os
/// estúdios forem de orgs diferentes, entre organizações).
async fn check_references(
    state: &AppState,
    studio_id: Uuid,
    references: &[Uuid],
) -> Result<(), ApiError> {
    if references.is_empty() {
        return Ok(());
    }
    let found: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM studio_documents WHERE studio_id = $1 AND id = ANY($2)",
    )
    .bind(studio_id)
    .bind(references)
    .fetch_one(&state.db)
    .await?;
    if found != references.len() as i64 {
        return Err(DomainError::invalid(
            "studio.invalid_document",
            "o documento refere documentos que não existem neste estúdio",
        )
        .with_field("body", "ids de documentos deste estúdio")
        .into());
    }
    Ok(())
}

/// Lê um documento pelo caminho inteiro. Qualquer peça que não bata (org,
/// estúdio, tipo ou id) é `404` — nunca se diz qual.
async fn load_doc(
    state: &AppState,
    studio_id: Uuid,
    kind: Kind,
    document_id: Uuid,
) -> Result<Document, ApiError> {
    sqlx::query_as(&format!(
        "{DOC_SELECT} WHERE id = $1 AND studio_id = $2 AND kind = $3"
    ))
    .bind(document_id)
    .bind(studio_id)
    .bind(kind.as_str())
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)
}

// --------------------------------------------------------------- handlers ---

/// Documentos de um tipo, no estúdio.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/studios/{studio_id}/{kind}", tag = "studio",
    security(("session" = [])),
    params(
        ("org_id" = Uuid, Path),
        ("studio_id" = Uuid, Path),
        ("kind" = String, Path, description =
            "mixer-scenes | macros | overlays | light-scenes | camera-profiles | rundowns"),
        ListQuery
    ),
    responses(
        (status = 200, body = DocumentPage),
        (status = 400, body = crate::openapi::ErrorBody, description = "page_token inválido"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "org, estúdio ou tipo desconhecido"),
    )
)]
pub async fn list_documents(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, studio_id, kind)): Path<(Uuid, Uuid, String)>,
    Query(q): Query<ListQuery>,
) -> Result<Json<DocumentPage>, ApiError> {
    let kind = kind_from_segment(&kind)?;
    member(&state, org_id, auth.user_id).await?;
    load_studio(&state, org_id, studio_id).await?;
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<Cursor> = page.cursor()?;
    let rows: Vec<Document> = sqlx::query_as(&format!(
        "{DOC_SELECT} WHERE studio_id = $1 AND kind = $2
            AND ($3::timestamptz IS NULL OR (created_at, id) > ($3, $4))
          ORDER BY created_at, id LIMIT $5"
    ))
    .bind(studio_id)
    .bind(kind.as_str())
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |d| Cursor {
        at: d.created_at,
        id: d.id,
    });
    Ok(Json(DocumentPage {
        items: p.items,
        next_page_token: p.next_page_token,
    }))
}

/// Cria um documento (`version: 1`).
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/studios/{studio_id}/{kind}", tag = "studio",
    security(("session" = [])),
    params(
        ("org_id" = Uuid, Path),
        ("studio_id" = Uuid, Path),
        ("kind" = String, Path, description =
            "mixer-scenes | macros | overlays | light-scenes | camera-profiles | rundowns")
    ),
    request_body = CreateDocumentReq,
    responses(
        (status = 201, body = Document, headers(("Location" = String))),
        (status = 400, body = crate::openapi::ErrorBody, description = "`studio.invalid_document`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`studio.not_operator`"),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "`studio.key_taken`"),
    )
)]
pub async fn create_document(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, studio_id, kind)): Path<(Uuid, Uuid, String)>,
    Json(req): Json<CreateDocumentReq>,
) -> Result<Response, ApiError> {
    let kind = kind_from_segment(&kind)?;
    let access = member(&state, org_id, auth.user_id).await?;
    let studio = load_studio(&state, org_id, studio_id).await?;
    access.operate(&studio)?;
    let v = rules::validate(kind, &req.name, req.body)?;
    check_references(&state, studio_id, &v.references).await?;

    let id = Uuid::new_v4();
    let mut tx = state.db.begin().await?;
    sqlx::query(
        "INSERT INTO studio_documents
           (id, studio_id, org_id, kind, name, key, version, body, summary, created_by, updated_by)
         VALUES ($1, $2, $3, $4, $5, $6, 1, $7, $8, $9, $9)",
    )
    .bind(id)
    .bind(studio_id)
    .bind(org_id)
    .bind(kind.as_str())
    .bind(&v.name)
    .bind(v.key.as_deref())
    .bind(&v.body)
    .bind(v.summary.as_ref())
    .bind(auth.user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| write_error(e, v.key.as_deref()))?;
    sqlx::query(
        "INSERT INTO studio_document_versions
           (document_id, version, name, body, summary, updated_by)
         VALUES ($1, 1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(&v.name)
    .bind(&v.body)
    .bind(v.summary.as_ref())
    .bind(auth.user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let doc = load_doc(&state, studio_id, kind, id).await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "studio.document.created",
        &format!("{}:{}", kind.as_str(), v.name),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        [(
            LOCATION,
            format!(
                "/api/orgs/{org_id}/studios/{studio_id}/{}/{id}",
                kind.path_segment()
            ),
        )],
        Json(doc),
    )
        .into_response())
}

/// Um documento.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/studios/{studio_id}/{kind}/{document_id}", tag = "studio",
    security(("session" = [])),
    params(
        ("org_id" = Uuid, Path), ("studio_id" = Uuid, Path),
        ("kind" = String, Path), ("document_id" = Uuid, Path)
    ),
    responses(
        (status = 200, body = Document),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_document(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, studio_id, kind, document_id)): Path<(Uuid, Uuid, String, Uuid)>,
) -> Result<Json<Document>, ApiError> {
    let kind = kind_from_segment(&kind)?;
    member(&state, org_id, auth.user_id).await?;
    load_studio(&state, org_id, studio_id).await?;
    Ok(Json(load_doc(&state, studio_id, kind, document_id).await?))
}

/// Grava uma versão nova. `version` tem de ser a actual (`409` se não for).
#[utoipa::path(
    patch, path = "/api/orgs/{org_id}/studios/{studio_id}/{kind}/{document_id}", tag = "studio",
    security(("session" = [])),
    params(
        ("org_id" = Uuid, Path), ("studio_id" = Uuid, Path),
        ("kind" = String, Path), ("document_id" = Uuid, Path)
    ),
    request_body = UpdateDocumentReq,
    responses(
        (status = 200, body = Document),
        (status = 400, body = crate::openapi::ErrorBody, description = "`studio.invalid_document`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`studio.not_operator`"),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody,
            description = "`studio.version_conflict` ou `studio.key_taken`"),
    )
)]
pub async fn update_document(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, studio_id, kind, document_id)): Path<(Uuid, Uuid, String, Uuid)>,
    Json(req): Json<UpdateDocumentReq>,
) -> Result<Json<Document>, ApiError> {
    let kind = kind_from_segment(&kind)?;
    let access = member(&state, org_id, auth.user_id).await?;
    let studio = load_studio(&state, org_id, studio_id).await?;
    access.operate(&studio)?;
    let current = load_doc(&state, studio_id, kind, document_id).await?;
    rules::check_version(current.version, req.version)?;

    // Revalida o documento COMPLETO, não só o campo que mudou: o corpo é uma
    // peça só, e um nome novo com um corpo antigo continua a ter de passar as
    // mesmas regras.
    let name = req.name.unwrap_or_else(|| current.name.clone());
    let body = req.body.unwrap_or_else(|| current.body.clone());
    let v = rules::validate(kind, &name, body)?;
    check_references(&state, studio_id, &v.references).await?;

    let next = current.version + 1;
    let mut tx = state.db.begin().await?;
    // A versão entra na condição: duas gravações em paralelo que passaram o
    // `check_version` acima não podem ambas escrever. A segunda não encontra
    // linha e leva `409`, em vez de a última a chegar ganhar em silêncio.
    let updated = sqlx::query(
        "UPDATE studio_documents
            SET name = $1, key = $2, body = $3, summary = $4,
                version = $5, updated_by = $6, updated_at = now()
          WHERE id = $7 AND version = $8",
    )
    .bind(&v.name)
    .bind(v.key.as_deref())
    .bind(&v.body)
    .bind(v.summary.as_ref())
    .bind(next)
    .bind(auth.user_id)
    .bind(document_id)
    .bind(current.version)
    .execute(&mut *tx)
    .await
    .map_err(|e| write_error(e, v.key.as_deref()))?;
    if updated.rows_affected() == 0 {
        return Err(DomainError::conflict(
            "studio.version_conflict",
            "o documento mudou entretanto — recarregue antes de gravar",
        )
        .into());
    }
    sqlx::query(
        "INSERT INTO studio_document_versions
           (document_id, version, name, body, summary, updated_by)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(document_id)
    .bind(next)
    .bind(&v.name)
    .bind(&v.body)
    .bind(v.summary.as_ref())
    .bind(auth.user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(load_doc(&state, studio_id, kind, document_id).await?))
}

/// Apaga um documento (e o seu histórico).
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/studios/{studio_id}/{kind}/{document_id}", tag = "studio",
    security(("session" = [])),
    params(
        ("org_id" = Uuid, Path), ("studio_id" = Uuid, Path),
        ("kind" = String, Path), ("document_id" = Uuid, Path)
    ),
    responses(
        (status = 204),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`studio.not_operator`"),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete_document(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, studio_id, kind, document_id)): Path<(Uuid, Uuid, String, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let kind = kind_from_segment(&kind)?;
    let access = member(&state, org_id, auth.user_id).await?;
    let studio = load_studio(&state, org_id, studio_id).await?;
    access.operate(&studio)?;
    let doc = load_doc(&state, studio_id, kind, document_id).await?;
    sqlx::query("DELETE FROM studio_documents WHERE id = $1")
        .bind(document_id)
        .execute(&state.db)
        .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "studio.document.deleted",
        &format!("{}:{}", kind.as_str(), doc.name),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// O histórico, mais recente primeiro.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/studios/{studio_id}/{kind}/{document_id}/versions",
    tag = "studio",
    security(("session" = [])),
    params(
        ("org_id" = Uuid, Path), ("studio_id" = Uuid, Path),
        ("kind" = String, Path), ("document_id" = Uuid, Path), ListQuery
    ),
    responses(
        (status = 200, body = DocumentVersionPage),
        (status = 400, body = crate::openapi::ErrorBody, description = "page_token inválido"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_versions(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, studio_id, kind, document_id)): Path<(Uuid, Uuid, String, Uuid)>,
    Query(q): Query<ListQuery>,
) -> Result<Json<DocumentVersionPage>, ApiError> {
    let kind = kind_from_segment(&kind)?;
    member(&state, org_id, auth.user_id).await?;
    load_studio(&state, org_id, studio_id).await?;
    load_doc(&state, studio_id, kind, document_id).await?;
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<VersionCursor> = page.cursor()?;
    let rows: Vec<DocumentVersion> = sqlx::query_as(
        "SELECT version, name, body, summary, updated_by, updated_at
           FROM studio_document_versions
          WHERE document_id = $1 AND ($2::int IS NULL OR version < $2)
          ORDER BY version DESC LIMIT $3",
    )
    .bind(document_id)
    .bind(cursor.as_ref().map(|c| c.version))
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |v| VersionCursor { version: v.version });
    Ok(Json(DocumentVersionPage {
        items: p.items,
        next_page_token: p.next_page_token,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn so_os_seis_segmentos_conhecidos_sao_tipos() {
        for k in Kind::ALL {
            assert_eq!(kind_from_segment(k.path_segment()).unwrap(), k);
        }
        // Um segmento desconhecido não é um tipo novo — é um caminho que não
        // existe. E o valor da COLUNA não é o do caminho: `mixer_scene` não
        // abre `…/mixer_scene`.
        for bad in ["", "cenas", "mixer_scene", "macro", "MACROS", "../macros"] {
            assert!(
                matches!(kind_from_segment(bad), Err(ApiError::NotFound)),
                "{bad} tinha de ser 404"
            );
        }
    }
}
