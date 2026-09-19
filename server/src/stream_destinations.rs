//! Destinos de emissão em directo por organização (G1) — adaptador HTTP +
//! Postgres. As regras de forma estão em
//! `delonix_meet_domain::content::stream_destination`.
//!
//! Contrato (rotas NOVAS seguem as regras do ADR-0004 §4):
//! - `GET    /api/orgs/{org_id}/stream-destinations`          lista paginada (sem chaves)
//! - `POST   /api/orgs/{org_id}/stream-destinations`          `201` + `Location`, chave devolvida UMA vez
//! - `GET    /api/orgs/{org_id}/stream-destinations/{id}`     um destino (sem chave)
//! - `PATCH  /api/orgs/{org_id}/stream-destinations/{id}`     rótulo, URL, estado (nunca a chave)
//! - `POST   /api/orgs/{org_id}/stream-destinations/{id}/rotate-key`  método personalizado: nova chave, devolvida UMA vez
//! - `DELETE /api/orgs/{org_id}/stream-destinations/{id}`     `204`
//!
//! Só quem tem `broadcast.manage_rtmp_keys` (o `admin` de sistema, como antes). Um id de outra org e um id inexistente dão a
//! MESMA resposta (`404`).

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
use delonix_meet_domain::content::stream_destination as rules;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct StreamDestination {
    pub id: Uuid,
    pub org_id: Uuid,
    /// `youtube` | `facebook` | `linkedin` | `rtmp` | `internal`.
    pub kind: String,
    pub label: String,
    pub url: String,
    /// Primeiros caracteres da chave, para a reconhecer.
    pub key_prefix: String,
    pub has_key: bool,
    /// `ready` | `expired` | `error`.
    pub state: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const COLUMNS: &str =
    "id, org_id, kind, label, url, key_prefix, (stream_key_sealed <> '') AS has_key, \
                       state, created_by, created_at, updated_at";

/// O destino e a chave completa — só na criação e na rotação.
#[derive(Serialize, utoipa::ToSchema)]
pub struct StreamDestinationWithKey {
    #[serde(flatten)]
    pub destination: StreamDestination,
    /// A chave em claro, mostrada esta única vez. Ausente se o destino não tem chave.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_key: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct StreamDestinationPage {
    pub items: Vec<StreamDestination>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateStreamDestinationReq {
    pub kind: String,
    pub label: String,
    pub url: String,
    #[serde(default)]
    pub stream_key: Option<String>,
}

#[derive(Deserialize, Default, utoipa::ToSchema)]
pub struct UpdateStreamDestinationReq {
    pub label: Option<String>,
    pub url: Option<String>,
    pub state: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct RotateKeyReq {
    pub stream_key: String,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListQuery {
    /// 1-100, omissão 50.
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Cursor {
    at: DateTime<Utc>,
    id: Uuid,
}

/// `broadcast.manage_rtmp_keys` (ADR-0008 §4): o `admin` de sistema tem-na, como hoje.
async fn require_rtmp_keys(state: &AppState, org_id: Uuid, user: Uuid) -> Result<(), ApiError> {
    crate::org::require_capability(
        state,
        org_id,
        user,
        delonix_meet_domain::identity::authorization::Capability::BroadcastManageRtmpKeys,
        delonix_meet_domain::identity::authorization::ResourceScope::Organization,
    )
    .await
    .map(|_| ())
}

fn secret_box(state: &AppState) -> Result<&delonix_meet_core::secret_box::SecretBox, ApiError> {
    state.config.secret_box.as_deref().ok_or_else(|| {
        DomainError::precondition(
            "secrets.encryption_unconfigured",
            "esta instalação não tem DATA_ENCRYPTION_KEYS — não se guardam chaves de terceiros em claro",
        )
        .into()
    })
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(list, create, get_one, update, rotate_key, delete),
    components(schemas(
        StreamDestination,
        StreamDestinationWithKey,
        StreamDestinationPage,
        CreateStreamDestinationReq,
        UpdateStreamDestinationReq,
        RotateKeyReq
    ))
)]
pub struct ApiDoc;

/// Destinos da organização, paginados por data de criação.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/stream-destinations", tag = "stream-destinations",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ListQuery),
    responses(
        (status = 200, body = StreamDestinationPage),
        (status = 400, body = crate::openapi::ErrorBody, description = "page_token inválido"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Query(q): Query<ListQuery>,
) -> Result<Json<StreamDestinationPage>, ApiError> {
    require_rtmp_keys(&state, org_id, auth.user_id).await?;
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<Cursor> = page.cursor()?;
    let rows: Vec<StreamDestination> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM stream_destinations
          WHERE org_id = $1
            AND ($2::timestamptz IS NULL OR (created_at, id) > ($2, $3))
          ORDER BY created_at, id
          LIMIT $4"
    ))
    .bind(org_id)
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |d| Cursor {
        at: d.created_at,
        id: d.id,
    });
    Ok(Json(StreamDestinationPage {
        items: p.items,
        next_page_token: p.next_page_token,
    }))
}

/// Cria um destino. A chave (se vier) é cifrada e devolvida só nesta resposta.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/stream-destinations", tag = "stream-destinations",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = CreateStreamDestinationReq,
    responses(
        (status = 201, body = StreamDestinationWithKey, headers(("Location" = String))),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 422, body = crate::openapi::ErrorBody, description = "sem DATA_ENCRYPTION_KEYS: não se guarda uma chave"),
    )
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateStreamDestinationReq>,
) -> Result<Response, ApiError> {
    require_rtmp_keys(&state, org_id, auth.user_id).await?;
    let kind = rules::Kind::parse(&req.kind)?;
    let label = rules::validate_label(&req.label)?;
    let url = rules::validate_url(&req.url)?;
    let key = req.stream_key.as_deref().filter(|k| !k.is_empty());
    let id = Uuid::new_v4();
    let (sealed, prefix) = match key {
        Some(k) => {
            rules::validate_key(k)?;
            (
                secret_box(&state)?.seal(k, &rules::key_aad(&id)),
                rules::key_prefix(k),
            )
        }
        None => (String::new(), String::new()),
    };
    let destination: StreamDestination = sqlx::query_as(&format!(
        "INSERT INTO stream_destinations (id, org_id, kind, label, url, stream_key_sealed, key_prefix, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING {COLUMNS}"
    ))
    .bind(id)
    .bind(org_id)
    .bind(kind.as_str())
    .bind(&label)
    .bind(&url)
    .bind(&sealed)
    .bind(&prefix)
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "stream_destination.created",
        &label,
    )
    .await;
    let location = format!("/api/orgs/{org_id}/stream-destinations/{id}");
    Ok((
        StatusCode::CREATED,
        [(LOCATION, location)],
        Json(StreamDestinationWithKey {
            destination,
            stream_key: key.map(str::to_string),
        }),
    )
        .into_response())
}

async fn fetch(state: &AppState, org_id: Uuid, id: Uuid) -> Result<StreamDestination, ApiError> {
    sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM stream_destinations WHERE id = $1 AND org_id = $2"
    ))
    .bind(id)
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)
}

/// Um destino (sem a chave).
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/stream-destinations/{dest_id}", tag = "stream-destinations",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("dest_id" = Uuid, Path)),
    responses(
        (status = 200, body = StreamDestination),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_one(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, dest_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<StreamDestination>, ApiError> {
    require_rtmp_keys(&state, org_id, auth.user_id).await?;
    Ok(Json(fetch(&state, org_id, dest_id).await?))
}

/// Altera rótulo, URL ou estado. A chave NÃO se altera aqui: é `rotate-key`.
#[utoipa::path(
    patch, path = "/api/orgs/{org_id}/stream-destinations/{dest_id}", tag = "stream-destinations",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("dest_id" = Uuid, Path)),
    request_body = UpdateStreamDestinationReq,
    responses(
        (status = 200, body = StreamDestination),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn update(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, dest_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateStreamDestinationReq>,
) -> Result<Json<StreamDestination>, ApiError> {
    require_rtmp_keys(&state, org_id, auth.user_id).await?;
    // Valida tudo ANTES de escrever (sem escritas parciais).
    let label = req
        .label
        .as_deref()
        .map(rules::validate_label)
        .transpose()?;
    let url = req.url.as_deref().map(rules::validate_url).transpose()?;
    if let Some(s) = &req.state {
        rules::validate_state(s)?;
    }
    let d: Option<StreamDestination> = sqlx::query_as(&format!(
        "UPDATE stream_destinations SET
            label = COALESCE($3, label), url = COALESCE($4, url),
            state = COALESCE($5, state), updated_at = now()
          WHERE id = $1 AND org_id = $2 RETURNING {COLUMNS}"
    ))
    .bind(dest_id)
    .bind(org_id)
    .bind(label)
    .bind(url)
    .bind(req.state.as_deref())
    .fetch_optional(&state.db)
    .await?;
    let d = d.ok_or(ApiError::NotFound)?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "stream_destination.updated",
        &dest_id.to_string(),
    )
    .await;
    Ok(Json(d))
}

/// Método personalizado: substitui a chave (cifrada) e devolve-a esta única vez.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/stream-destinations/{dest_id}/rotate-key", tag = "stream-destinations",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("dest_id" = Uuid, Path)),
    request_body = RotateKeyReq,
    responses(
        (status = 200, body = StreamDestinationWithKey),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 422, body = crate::openapi::ErrorBody, description = "sem DATA_ENCRYPTION_KEYS"),
    )
)]
pub async fn rotate_key(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, dest_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<RotateKeyReq>,
) -> Result<Json<StreamDestinationWithKey>, ApiError> {
    require_rtmp_keys(&state, org_id, auth.user_id).await?;
    rules::validate_key(&req.stream_key)?;
    if req.stream_key.is_empty() {
        return Err(DomainError::invalid(
            "stream_destination.invalid_key",
            "a chave nova não pode ser vazia",
        )
        .with_field("stream_key", "obrigatória")
        .into());
    }
    let sealed = secret_box(&state)?.seal(&req.stream_key, &rules::key_aad(&dest_id));
    let d: Option<StreamDestination> = sqlx::query_as(&format!(
        "UPDATE stream_destinations SET stream_key_sealed = $3, key_prefix = $4,
            state = 'ready', updated_at = now()
          WHERE id = $1 AND org_id = $2 RETURNING {COLUMNS}"
    ))
    .bind(dest_id)
    .bind(org_id)
    .bind(&sealed)
    .bind(rules::key_prefix(&req.stream_key))
    .fetch_optional(&state.db)
    .await?;
    let destination = d.ok_or(ApiError::NotFound)?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "stream_destination.key_rotated",
        &dest_id.to_string(),
    )
    .await;
    Ok(Json(StreamDestinationWithKey {
        destination,
        stream_key: Some(req.stream_key),
    }))
}

/// Apaga um destino.
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/stream-destinations/{dest_id}", tag = "stream-destinations",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("dest_id" = Uuid, Path)),
    responses(
        (status = 204, description = "Apagado."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, dest_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    require_rtmp_keys(&state, org_id, auth.user_id).await?;
    let r = sqlx::query("DELETE FROM stream_destinations WHERE id = $1 AND org_id = $2")
        .bind(dest_id)
        .bind(org_id)
        .execute(&state.db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "stream_destination.deleted",
        &dest_id.to_string(),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// Para o directo (ADR-0003): os destinos guardados de uma org, com a chave
/// DECIFRADA no servidor — para o `ffmpeg` a receber sem ela ter de voltar ao
/// browser. `(url, chave, rótulo)`.
pub(crate) async fn resolve_for_broadcast(
    state: &AppState,
    org_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<(String, String, String)>, ApiError> {
    let rows: Vec<(Uuid, String, String, String)> = sqlx::query_as(
        "SELECT id, url, stream_key_sealed, label FROM stream_destinations
          WHERE org_id = $1 AND id = ANY($2) AND state = 'ready'",
    )
    .bind(org_id)
    .bind(ids)
    .fetch_all(&state.db)
    .await?;
    if rows.len() != ids.len() {
        return Err(ApiError::NotFound);
    }
    let sb = secret_box(state)?;
    rows.into_iter()
        .map(|(id, url, sealed, label)| {
            let key = sb.open(&sealed, &rules::key_aad(&id))?;
            Ok((url, key, label))
        })
        .collect()
}
