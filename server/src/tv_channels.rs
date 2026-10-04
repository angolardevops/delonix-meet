//! Canais de TV pela Internet por organização (RFC-0001, Fase 1) — adaptador
//! HTTP + Postgres. As regras de forma estão em
//! `delonix_meet_domain::content::tv_channel`.
//!
//! Contrato (ADR-0004 §4):
//! - `GET    /api/orgs/{org_id}/tv/channels`               lista paginada
//! - `POST   /api/orgs/{org_id}/tv/channels`               `201` + `Location`
//! - `GET    /api/orgs/{org_id}/tv/channels/{channel_id}`  um canal
//! - `PATCH  /api/orgs/{org_id}/tv/channels/{channel_id}`  exige `version`; `409` se estiver velha
//! - `DELETE /api/orgs/{org_id}/tv/channels/{channel_id}`  `204`
//!
//! Só quem tem `broadcast.manage_channels`. Um id de outra org e um id
//! inexistente dão a MESMA resposta (`404`). Isto é a capacidade de PREPARAR um
//! canal; pô-lo no ar é outra capacidade, a vir com o motor de emissão.
//! `status` é só leitura: um canal fica em `draft` até o motor existir.

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
use delonix_meet_domain::content::tv_channel as rules;
use serde::{Deserialize, Deserializer, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct TvChannel {
    pub id: Uuid,
    pub org_id: Uuid,
    /// Endereço público (único na organização).
    pub slug: String,
    pub name: String,
    pub description: String,
    /// Nome IANA, p. ex. `Africa/Luanda`.
    pub timezone: String,
    /// `public` | `private` | `restricted`.
    pub visibility: String,
    /// Só leitura: `draft` até haver motor de emissão.
    pub status: String,
    /// Dias de retenção das gravações do canal; ausente = a política da organização.
    pub recording_retention_days: Option<i32>,
    /// Sobe a cada alteração; o PATCH tem de a trazer (concorrência optimista).
    pub version: i32,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const COLUMNS: &str = "id, org_id, slug, name, description, timezone, visibility, status, \
                       recording_retention_days, version, created_by, created_at, updated_at";

#[derive(Serialize, utoipa::ToSchema)]
pub struct TvChannelPage {
    pub items: Vec<TvChannel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateTvChannelReq {
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Omissão `Africa/Luanda`.
    #[serde(default)]
    pub timezone: Option<String>,
    /// Omissão `private`.
    #[serde(default)]
    pub visibility: Option<String>,
    #[serde(default)]
    pub recording_retention_days: Option<i32>,
}

/// `null` explícito limpa o campo; ausente deixa-o como está.
fn some_nullable<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Some(Option::deserialize(d)?))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateTvChannelReq {
    /// A versão que o cliente viu. Se já não for a actual, `409`.
    pub version: i32,
    pub name: Option<String>,
    pub description: Option<String>,
    pub timezone: Option<String>,
    pub visibility: Option<String>,
    /// Um número define; `null` limpa (volta à política da organização).
    #[serde(default, deserialize_with = "some_nullable")]
    #[schema(value_type = Option<i32>)]
    pub recording_retention_days: Option<Option<i32>>,
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

/// `broadcast.manage_channels` (ADR-0008 §4): o `admin` de sistema tem-na.
async fn require_manage(state: &AppState, org_id: Uuid, user: Uuid) -> Result<(), ApiError> {
    crate::org::require_capability(
        state,
        org_id,
        user,
        delonix_meet_domain::identity::authorization::Capability::BroadcastManageChannels,
        delonix_meet_domain::identity::authorization::ResourceScope::Organization,
    )
    .await
    .map(|_| ())
}

/// O fuso tem de existir na base de fusos IANA do Postgres — a mesma que a
/// grelha de programação vai usar para converter horários (RF-14).
async fn require_known_timezone(state: &AppState, tz: &str) -> Result<(), ApiError> {
    let known: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_timezone_names WHERE name = $1)")
            .bind(tz)
            .fetch_one(&state.db)
            .await?;
    if known {
        Ok(())
    } else {
        Err(DomainError::invalid(
            "tv.channel.unknown_timezone",
            format!("o fuso «{tz}» não existe na base IANA"),
        )
        .with_field("timezone", "nome IANA, por exemplo Africa/Luanda")
        .into())
    }
}

fn slug_taken() -> ApiError {
    DomainError::conflict(
        "tv.channel.slug_taken",
        "já existe um canal com este endereço nesta organização",
    )
    .with_field("slug", "já em uso")
    .into()
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(list, create, get_one, update, delete),
    components(schemas(TvChannel, TvChannelPage, CreateTvChannelReq, UpdateTvChannelReq))
)]
pub struct ApiDoc;

/// Canais da organização, paginados por data de criação.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/tv/channels", tag = "tv-channels",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ListQuery),
    responses(
        (status = 200, body = TvChannelPage),
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
) -> Result<Json<TvChannelPage>, ApiError> {
    require_manage(&state, org_id, auth.user_id).await?;
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<Cursor> = page.cursor()?;
    let rows: Vec<TvChannel> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM tv_channels
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
    let p = Page::from_overfetch(rows, size, |c| Cursor {
        at: c.created_at,
        id: c.id,
    });
    Ok(Json(TvChannelPage {
        items: p.items,
        next_page_token: p.next_page_token,
    }))
}

/// Cria um canal (em `draft`).
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/tv/channels", tag = "tv-channels",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = CreateTvChannelReq,
    responses(
        (status = 201, body = TvChannel, headers(("Location" = String))),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "`tv.channel.slug_taken`"),
    )
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateTvChannelReq>,
) -> Result<Response, ApiError> {
    require_manage(&state, org_id, auth.user_id).await?;
    // Valida tudo ANTES de escrever.
    let slug = rules::validate_slug(&req.slug)?;
    let name = rules::validate_name(&req.name)?;
    let description = rules::validate_description(req.description.as_deref().unwrap_or(""))?;
    let timezone =
        rules::validate_timezone_shape(req.timezone.as_deref().unwrap_or(rules::DEFAULT_TIMEZONE))?;
    let visibility = rules::Visibility::parse(req.visibility.as_deref().unwrap_or("private"))?;
    let retention = req
        .recording_retention_days
        .map(rules::validate_retention_days)
        .transpose()?;
    require_known_timezone(&state, &timezone).await?;

    let id = Uuid::new_v4();
    let inserted: Result<TvChannel, sqlx::Error> = sqlx::query_as(&format!(
        "INSERT INTO tv_channels
            (id, org_id, slug, name, description, timezone, visibility,
             recording_retention_days, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING {COLUMNS}"
    ))
    .bind(id)
    .bind(org_id)
    .bind(&slug)
    .bind(&name)
    .bind(&description)
    .bind(&timezone)
    .bind(visibility.as_str())
    .bind(retention)
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await;
    let channel = match inserted {
        Ok(c) => c,
        Err(e)
            if e.as_database_error()
                .and_then(|d| d.constraint())
                .is_some_and(|c| c == "tv_channels_org_slug_key") =>
        {
            return Err(slug_taken())
        }
        Err(e) => return Err(e.into()),
    };
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "tv.channel.created",
        &format!("{id} ({slug})"),
    )
    .await;
    let location = format!("/api/orgs/{org_id}/tv/channels/{id}");
    Ok((StatusCode::CREATED, [(LOCATION, location)], Json(channel)).into_response())
}

async fn fetch(state: &AppState, org_id: Uuid, id: Uuid) -> Result<TvChannel, ApiError> {
    sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM tv_channels WHERE id = $1 AND org_id = $2"
    ))
    .bind(id)
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)
}

/// Um canal.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/tv/channels/{channel_id}", tag = "tv-channels",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("channel_id" = Uuid, Path)),
    responses(
        (status = 200, body = TvChannel),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_one(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, channel_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<TvChannel>, ApiError> {
    require_manage(&state, org_id, auth.user_id).await?;
    Ok(Json(fetch(&state, org_id, channel_id).await?))
}

/// Altera um canal. O corpo traz a `version` que o cliente viu; se entretanto
/// outra pessoa o alterou, é `409` e nada se escreve (RNF-10). O endereço
/// (`slug`) não se altera: é a identidade pública do canal.
#[utoipa::path(
    patch, path = "/api/orgs/{org_id}/tv/channels/{channel_id}", tag = "tv-channels",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("channel_id" = Uuid, Path)),
    request_body = UpdateTvChannelReq,
    responses(
        (status = 200, body = TvChannel),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "`tv.channel.version_conflict`"),
    )
)]
pub async fn update(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, channel_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateTvChannelReq>,
) -> Result<Json<TvChannel>, ApiError> {
    require_manage(&state, org_id, auth.user_id).await?;
    let name = req.name.as_deref().map(rules::validate_name).transpose()?;
    let description = req
        .description
        .as_deref()
        .map(rules::validate_description)
        .transpose()?;
    let timezone = req
        .timezone
        .as_deref()
        .map(rules::validate_timezone_shape)
        .transpose()?;
    let visibility = req
        .visibility
        .as_deref()
        .map(rules::Visibility::parse)
        .transpose()?;
    let (set_retention, retention) = match req.recording_retention_days {
        None => (false, None),
        Some(None) => (true, None),
        Some(Some(d)) => (true, Some(rules::validate_retention_days(d)?)),
    };
    if let Some(tz) = timezone.as_deref() {
        require_known_timezone(&state, tz).await?;
    }
    let updated: Option<TvChannel> = sqlx::query_as(&format!(
        "UPDATE tv_channels SET
            name = COALESCE($4, name),
            description = COALESCE($5, description),
            timezone = COALESCE($6, timezone),
            visibility = COALESCE($7, visibility),
            recording_retention_days = CASE WHEN $8 THEN $9 ELSE recording_retention_days END,
            version = version + 1, updated_at = now()
          WHERE id = $1 AND org_id = $2 AND version = $3
          RETURNING {COLUMNS}"
    ))
    .bind(channel_id)
    .bind(org_id)
    .bind(req.version)
    .bind(name)
    .bind(description)
    .bind(timezone)
    .bind(visibility.map(rules::Visibility::as_str))
    .bind(set_retention)
    .bind(retention)
    .fetch_optional(&state.db)
    .await?;
    let Some(channel) = updated else {
        // Nenhuma linha: ou não existe (404), ou a versão está velha (409).
        let actual = fetch(&state, org_id, channel_id).await?;
        return Err(DomainError::conflict(
            "tv.channel.version_conflict",
            format!(
                "o canal foi alterado por outra pessoa (versão {} em vez de {}) — releia e volte a tentar",
                actual.version, req.version
            ),
        )
        .into());
    };
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "tv.channel.updated",
        &format!("{channel_id} v{}", channel.version),
    )
    .await;
    Ok(Json(channel))
}

/// Apaga um canal.
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/tv/channels/{channel_id}", tag = "tv-channels",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("channel_id" = Uuid, Path)),
    responses(
        (status = 204, description = "Apagado."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "`tv.channel.on_air`"),
    )
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, channel_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    require_manage(&state, org_id, auth.user_id).await?;
    // Um canal com uma emissão por terminar não se apaga: o `ON DELETE CASCADE`
    // levava a sessão e o executor ficava a emitir para um canal que já não existe.
    let deleted: Option<String> = sqlx::query_scalar(
        "DELETE FROM tv_channels c WHERE c.id = $1 AND c.org_id = $2
            AND NOT EXISTS (SELECT 1 FROM tv_broadcast_sessions s
                             WHERE s.channel_id = c.id AND s.ended_at IS NULL)
          RETURNING c.slug",
    )
    .bind(channel_id)
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?;
    let Some(slug) = deleted else {
        // Nenhuma linha: não existe (404) ou está em emissão (409).
        fetch(&state, org_id, channel_id).await?;
        return Err(DomainError::conflict(
            "tv.channel.on_air",
            "o canal tem uma emissão por terminar — pare-a antes de o apagar",
        )
        .into());
    };
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "tv.channel.deleted",
        &format!("{channel_id} ({slug})"),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}
