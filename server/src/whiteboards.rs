//! Biblioteca de quadros brancos: guardados como PNG por organização.
//!
//! Isolamento multi-tenant: um quadro pertence a uma organização e só é
//! listado/acedido por membros dessa org (via [`crate::org::orgs_of_user`]).
//! Partilha só-leitura por link público com token (`share_token`).

use axum::{
    extract::{Path, State},
    http::header,
    response::IntoResponse,
    Json,
};
use base64::{engine::general_purpose, Engine};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, org::orgs_of_user, AppState};

/// PNG máximo aceite (evita abusos): 8 MB.
const MAX_PNG_BYTES: usize = 8 * 1024 * 1024;

/// Imagem PNG em bruto (só para o spec).
#[derive(utoipa::ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
pub struct PngBytes(Vec<u8>);

/// Documentação OpenAPI das rotas deste módulo (`openapi.rs` junta-as).
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(list, save, delete, png, set_share, shared_png),
    components(schemas(WhiteboardMeta, SaveReq, ShareReq))
)]
pub struct ApiDoc;

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct WhiteboardMeta {
    pub id: Uuid,
    pub title: String,
    pub room_code: String,
    pub is_public: bool,
    /// Token do link público; string vazia enquanto `is_public = false`.
    pub share_token: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = WhiteboardSaveReq)]
pub struct SaveReq {
    /// Truncado a 120 caracteres; vazio = «Quadro sem título».
    #[serde(default)]
    pub title: String,
    /// Truncado a 64 caracteres. Não é verificado.
    #[serde(default)]
    pub room_code: String,
    /// PNG em base64 (com ou sem prefixo `data:image/png;base64,`).
    pub png_base64: String,
}

const PNG_MAGIC: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

fn decode_png(s: &str) -> Result<Vec<u8>, ApiError> {
    let raw = s.split(',').next_back().unwrap_or(s).trim();
    // Rejeita ANTES de descodificar (evita alocar ~500MB só para rejeitar).
    if raw.len() > MAX_PNG_BYTES * 4 / 3 + 4 {
        return Err(ApiError::BadRequest(
            "tamanho do quadro fora do limite".into(),
        ));
    }
    let bytes = general_purpose::STANDARD
        .decode(raw)
        .map_err(|_| ApiError::BadRequest("PNG inválido".into()))?;
    if bytes.len() > MAX_PNG_BYTES {
        return Err(ApiError::BadRequest(
            "tamanho do quadro fora do limite".into(),
        ));
    }
    // Valida a assinatura PNG (impede guardar bytes arbitrários servidos como image/png).
    if !bytes.starts_with(PNG_MAGIC) {
        return Err(ApiError::BadRequest(
            "o conteúdo não é um PNG válido".into(),
        ));
    }
    Ok(bytes)
}

/// Não expõe o token de partilha enquanto o quadro não for público.
fn mask_token(mut m: WhiteboardMeta) -> WhiteboardMeta {
    if !m.is_public {
        m.share_token = String::new();
    }
    m
}

/// Guarda um quadro na biblioteca da organização do utilizador (a primeira,
/// se tiver várias). PNG até 8 MiB, validado pela assinatura.
#[utoipa::path(
    post, path = "/api/whiteboards", tag = "whiteboards",
    security(("session" = [])),
    request_body = SaveReq,
    responses(
        (status = 200, body = WhiteboardMeta),
        (status = 400, description = "Utilizador sem organização, base64 inválido, acima de 8 MiB, ou não é PNG.", body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 413, description = "Corpo acima do limite do router (texto simples)."),
    )
)]
pub async fn save(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<SaveReq>,
) -> Result<Json<WhiteboardMeta>, ApiError> {
    let org_id = *orgs_of_user(&state, auth.user_id)
        .await
        .first()
        .ok_or(ApiError::BadRequest("utilizador sem organização".into()))?;
    let png = decode_png(&req.png_base64)?;
    let title = if req.title.trim().is_empty() {
        "Quadro sem título".to_string()
    } else {
        req.title.trim().chars().take(120).collect()
    };
    let meta: WhiteboardMeta = sqlx::query_as(
        "INSERT INTO whiteboards (org_id, owner_id, title, room_code, png)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, title, room_code, is_public, share_token, created_at",
    )
    .bind(org_id)
    .bind(auth.user_id)
    .bind(title)
    .bind(req.room_code.chars().take(64).collect::<String>())
    .bind(png)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(mask_token(meta)))
}

/// Lista os quadros das organizações a que o utilizador pertence (máx. 200,
/// mais recentes primeiro).
#[utoipa::path(
    get, path = "/api/whiteboards", tag = "whiteboards",
    security(("session" = [])),
    responses(
        (status = 200, body = Vec<WhiteboardMeta>),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Vec<WhiteboardMeta>>, ApiError> {
    let orgs = orgs_of_user(&state, auth.user_id).await;
    if orgs.is_empty() {
        return Ok(Json(vec![]));
    }
    let items: Vec<WhiteboardMeta> = sqlx::query_as(
        "SELECT id, title, room_code, is_public, share_token, created_at
         FROM whiteboards WHERE org_id = ANY($1) ORDER BY created_at DESC LIMIT 200",
    )
    .bind(&orgs)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(items.into_iter().map(mask_token).collect()))
}

/// Imagem PNG de um quadro — apenas membros da org dona.
#[utoipa::path(
    get, path = "/api/whiteboards/{id}/png", tag = "whiteboards",
    security(("session" = [])),
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, body = inline(PngBytes), content_type = "image/png"),
        (status = 401, description = "Sessão inválida.", body = crate::openapi::ErrorBody),
        (status = 404, description = "Não existe, ou é de uma organização de que não és membro.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn png(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let orgs = orgs_of_user(&state, auth.user_id).await;
    let row: (Vec<u8>, Uuid) = sqlx::query_as("SELECT png, org_id FROM whiteboards WHERE id = $1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    if !orgs.contains(&row.1) {
        // Quadro de outra organização: não se confirma que existe.
        return Err(ApiError::NotFound);
    }
    Ok(([(header::CONTENT_TYPE, "image/png")], row.0))
}

/// Apaga um quadro — dono ou admin da org.
#[utoipa::path(
    delete, path = "/api/whiteboards/{id}", tag = "whiteboards",
    security(("session" = [])),
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, description = "`{\"ok\": true}` (forma herdada)"),
        (status = 401, description = "Sessão inválida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Nem dono nem admin.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let row: (Uuid, Uuid) =
        sqlx::query_as("SELECT owner_id, org_id FROM whiteboards WHERE id = $1")
            .bind(id)
            .fetch_one(&state.db)
            .await?;
    let is_owner = row.0 == auth.user_id;
    let is_admin = crate::org::require_admin_pub(&state, row.1, auth.user_id)
        .await
        .is_ok();
    if !is_owner && !is_admin {
        return Err(ApiError::Forbidden);
    }
    sqlx::query("DELETE FROM whiteboards WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = WhiteboardShareReq)]
pub struct ShareReq {
    pub public: bool,
}

/// Ativa/desativa a partilha por link público. Dono ou admin.
/// Desativar roda o token: o link antigo deixa de funcionar.
#[utoipa::path(
    post, path = "/api/whiteboards/{id}/share", tag = "whiteboards",
    security(("session" = [])),
    params(("id" = Uuid, Path)),
    request_body = ShareReq,
    responses(
        (status = 200, body = WhiteboardMeta),
        (status = 401, description = "Sessão inválida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Nem dono nem admin.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn set_share(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<ShareReq>,
) -> Result<Json<WhiteboardMeta>, ApiError> {
    let row: (Uuid, Uuid) =
        sqlx::query_as("SELECT owner_id, org_id FROM whiteboards WHERE id = $1")
            .bind(id)
            .fetch_one(&state.db)
            .await?;
    let is_owner = row.0 == auth.user_id;
    let is_admin = crate::org::require_admin_pub(&state, row.1, auth.user_id)
        .await
        .is_ok();
    if !is_owner && !is_admin {
        return Err(ApiError::Forbidden);
    }
    // Ao desativar a partilha, roda o token (o link antigo deixa de funcionar).
    let meta: WhiteboardMeta = sqlx::query_as(
        "UPDATE whiteboards SET is_public = $1,
             share_token = CASE WHEN $1 THEN share_token ELSE encode(gen_random_bytes(12), 'hex') END
         WHERE id = $2
         RETURNING id, title, room_code, is_public, share_token, created_at",
    )
    .bind(req.public)
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(mask_token(meta)))
}

/// Vista pública só-leitura por token — sem autenticação, se `is_public`.
#[utoipa::path(
    get, path = "/api/whiteboards/shared/{token}", tag = "whiteboards",
    params(("token" = String, Path, description = "`share_token` do quadro.")),
    responses(
        (status = 200, body = inline(PngBytes), content_type = "image/png"),
        (status = 404, description = "Token inexistente ou quadro não público.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn shared_png(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let row: (Vec<u8>, bool) =
        sqlx::query_as("SELECT png, is_public FROM whiteboards WHERE share_token = $1")
            .bind(token)
            .fetch_one(&state.db)
            .await?;
    if !row.1 {
        return Err(ApiError::NotFound);
    }
    Ok(([(header::CONTENT_TYPE, "image/png")], row.0))
}
