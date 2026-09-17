//! Legendas por língua, servidas ao leitor como WebVTT (`<track>`) — R183,
//! portado do servidor da UI para as convenções desta linha.
//!
//! Quem só VÊ a gravação vê só as legendas PUBLICADAS (uma não publicada é
//! `404`, como se não existisse); rascunhos, falhadas e as que estão a ser
//! geradas são de quem gere. As regras (língua, VTT, estados) estão em
//! `delonix_meet_domain::content::caption`.
//!
//! **Ponto de extensão — geração (fora deste lote).** `POST
//! …/captions/generate` (VTT dos segmentos na língua da transcrição, ou
//! tradução pelo LLM local em segundo plano) entra aqui quando o cliente
//! Ollama estiver portado. Tem o que precisa: os segmentos em
//! `recording_meta::load_segments`, as cues em `caption::segments_to_cues` +
//! `caption::cues_to_vtt`, e a mesma tabela com `source = transcript |
//! translation` e `status = generating` + `progress_pct`. Não há rota nem stub
//! enquanto não houver implementação: uma rota que responde «ainda não» é um
//! contrato falso.

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::DomainError;
use delonix_meet_domain::content::caption as rules;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::ApiError,
    recordings::{managed_item, seen_item},
    AppState,
};

/// Línguas por gravação. A listagem devolve-as todas (≤ este tecto).
pub const MAX_CAPTIONS: i64 = 50;
/// Tecto do corpo JSON do `PUT` (o VTT de 2 MiB escapado em JSON, com folga).
pub const MAX_CAPTION_BODY_BYTES: usize = 5 * 1024 * 1024;

/// Texto WebVTT em bruto (só para o spec).
#[derive(utoipa::ToSchema)]
#[schema(value_type = String)]
#[allow(dead_code)]
pub struct VttText(String);

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(list, get, vtt, put, patch, delete),
    components(schemas(CaptionMeta, CaptionFull, PutCaptionReq, PatchCaptionReq))
)]
pub struct ApiDoc;

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[schema(as = RecordingCaption)]
pub struct CaptionMeta {
    pub recording_id: Uuid,
    /// BCP 47 curto (`pt`, `pt-AO`, `en`).
    pub lang: String,
    /// `upload` | `transcript` | `translation`.
    pub source: String,
    /// `generating` | `draft` | `published` | `failed`.
    pub status: String,
    pub progress_pct: Option<i16>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
}

const META_COLS: &str =
    "recording_id, lang, source, status, progress_pct, error, created_at, updated_at, published_at";

/// Metadados e o VTT.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[schema(as = RecordingCaptionWithVtt)]
pub struct CaptionFull {
    #[serde(flatten)]
    pub meta: CaptionMeta,
    pub vtt: String,
}

#[derive(sqlx::FromRow)]
struct CaptionRow {
    #[sqlx(flatten)]
    meta: CaptionMeta,
    vtt: String,
}

/// Uma legenda que quem pede pode ler. Não existir e não poder ler (viewer +
/// não publicada) dão o mesmo `404`.
async fn readable(
    state: &AppState,
    recording_id: Uuid,
    lang: &str,
    user_id: Uuid,
) -> Result<CaptionRow, ApiError> {
    let rec = seen_item(state, recording_id, user_id).await?;
    let row: CaptionRow = sqlx::query_as(&format!(
        "SELECT {META_COLS}, vtt FROM recording_captions WHERE recording_id = $1 AND lang = $2"
    ))
    .bind(recording_id)
    .bind(lang)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    if !rec.facts().can_read_caption(&row.meta.status) {
        return Err(ApiError::NotFound);
    }
    Ok(row)
}

/// Legendas da gravação, por língua. Quem vê: só as publicadas; quem gere: todas.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/captions", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path)),
    responses(
        (status = 200, body = Vec<CaptionMeta>, description = "Por `lang`; no máximo 50."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<CaptionMeta>>, ApiError> {
    let rec = seen_item(&state, id, auth.user_id).await?;
    let rows: Vec<CaptionMeta> = sqlx::query_as(&format!(
        "SELECT {META_COLS} FROM recording_captions
          WHERE recording_id = $1 AND ($2 OR status = 'published')
          ORDER BY lang LIMIT $3"
    ))
    .bind(id)
    .bind(rec.facts().can_manage())
    .bind(MAX_CAPTIONS)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

/// Uma legenda: metadados e o VTT.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/captions/{lang}", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path), ("lang" = String, Path, description = "BCP 47 curto.")),
    responses(
        (status = 200, body = CaptionFull),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "Inexistente, sem acesso, ou não publicada para quem só vê.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn get(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, lang)): Path<(Uuid, String)>,
) -> Result<Json<CaptionFull>, ApiError> {
    let row = readable(&state, id, &lang, auth.user_id).await?;
    Ok(Json(CaptionFull {
        meta: row.meta,
        vtt: row.vtt,
    }))
}

/// O VTT como `text/vtt`, para o `<track>` do leitor.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/captions/{lang}/vtt", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path), ("lang" = String, Path, description = "BCP 47 curto.")),
    responses(
        (status = 200, body = inline(VttText), content_type = "text/vtt"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "`recording.caption_not_ready`: a gerar ou falhada (só quem gere as vê)."),
    )
)]
pub async fn vtt(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, lang)): Path<(Uuid, String)>,
) -> Result<impl IntoResponse, ApiError> {
    let row = readable(&state, id, &lang, auth.user_id).await?;
    rules::publishable(&row.meta.status)?;
    Ok((
        [
            (header::CONTENT_TYPE, "text/vtt; charset=utf-8"),
            (header::CACHE_CONTROL, "private, no-cache"),
        ],
        row.vtt,
    ))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = RecordingCaptionPutReq)]
pub struct PutCaptionReq {
    /// WebVTT válido, até 2 MiB.
    pub vtt: String,
    /// Publicar já (por omissão fica rascunho).
    #[serde(default)]
    pub publish: bool,
}

/// Envia (ou substitui) a legenda de uma língua. Só quem gere a gravação.
#[utoipa::path(
    put, path = "/api/recordings/{recording_id}/captions/{lang}", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path), ("lang" = String, Path, description = "BCP 47 curto.")),
    request_body = PutCaptionReq,
    responses(
        (status = 201, body = CaptionMeta, headers(("Location" = String)), description = "Criada."),
        (status = 200, body = CaptionMeta, description = "Substituída."),
        (status = 400, body = crate::openapi::ErrorBody, description = "`recording.invalid_caption_lang` / `recording.invalid_vtt` (a mensagem diz a linha) / `recording.caption_too_large`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`recording.not_manager`"),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 422, body = crate::openapi::ErrorBody, description = "`recording.too_many_captions` (máx. 50 línguas)"),
    )
)]
pub async fn put(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, lang)): Path<(Uuid, String)>,
    Json(req): Json<PutCaptionReq>,
) -> Result<Response, ApiError> {
    managed_item(&state, id, auth.user_id).await?;
    rules::check_lang(&lang)?;
    rules::parse_vtt(&req.vtt)?;
    let status = if req.publish { "published" } else { "draft" };
    // O tecto conta só línguas NOVAS: substituir uma existente passa sempre.
    let row: Option<(bool, CaptionMeta)> = sqlx::query_as::<_, InsertedMeta>(&format!(
        "INSERT INTO recording_captions (recording_id, lang, vtt, source, status, updated_by, published_at)
         SELECT $1, $2, $3, 'upload', $4, $5, CASE WHEN $4 = 'published' THEN now() END
          WHERE EXISTS (SELECT 1 FROM recording_captions WHERE recording_id = $1 AND lang = $2)
             OR (SELECT COUNT(*) FROM recording_captions WHERE recording_id = $1) < $6
         ON CONFLICT (recording_id, lang) DO UPDATE SET
             vtt = EXCLUDED.vtt, source = 'upload', status = EXCLUDED.status,
             progress_pct = NULL, error = NULL, updated_by = EXCLUDED.updated_by,
             updated_at = now(),
             published_at = CASE WHEN EXCLUDED.status = 'published'
                                 THEN COALESCE(recording_captions.published_at, now()) END
         RETURNING (xmax = 0) AS inserted, {META_COLS}"
    ))
    .bind(id)
    .bind(&lang)
    .bind(req.vtt.trim_start_matches('\u{feff}'))
    .bind(status)
    .bind(auth.user_id)
    .bind(MAX_CAPTIONS)
    .fetch_optional(&state.db)
    .await?
    .map(|r| (r.inserted, r.meta));
    let Some((inserted, meta)) = row else {
        return Err(DomainError::precondition(
            "recording.too_many_captions",
            format!("uma gravação tem no máximo {MAX_CAPTIONS} legendas"),
        )
        .into());
    };
    if inserted {
        let location = format!("/api/recordings/{id}/captions/{lang}");
        return Ok((
            StatusCode::CREATED,
            [(header::LOCATION, location)],
            Json(meta),
        )
            .into_response());
    }
    Ok(Json(meta).into_response())
}

#[derive(sqlx::FromRow)]
struct InsertedMeta {
    inserted: bool,
    #[sqlx(flatten)]
    meta: CaptionMeta,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = RecordingCaptionPatchReq)]
pub struct PatchCaptionReq {
    /// `draft` | `published`.
    pub status: String,
}

/// Publica ou volta a rascunho. Só quem gere a gravação.
#[utoipa::path(
    patch, path = "/api/recordings/{recording_id}/captions/{lang}", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path), ("lang" = String, Path, description = "BCP 47 curto.")),
    request_body = PatchCaptionReq,
    responses(
        (status = 200, body = CaptionMeta),
        (status = 400, body = crate::openapi::ErrorBody, description = "`recording.invalid_caption_status`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`recording.not_manager`"),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "`recording.caption_not_ready`: a gerar ou falhada."),
    )
)]
pub async fn patch(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, lang)): Path<(Uuid, String)>,
    Json(req): Json<PatchCaptionReq>,
) -> Result<Json<CaptionMeta>, ApiError> {
    managed_item(&state, id, auth.user_id).await?;
    let status = rules::parse_patch_status(&req.status)?;
    let (current,): (String,) = sqlx::query_as(
        "SELECT status FROM recording_captions WHERE recording_id = $1 AND lang = $2",
    )
    .bind(id)
    .bind(&lang)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    rules::publishable(&current)?;
    let meta: CaptionMeta = sqlx::query_as(&format!(
        "UPDATE recording_captions SET status = $3, updated_by = $4, updated_at = now(),
                published_at = CASE WHEN $3 = 'published' THEN COALESCE(published_at, now()) END
          WHERE recording_id = $1 AND lang = $2 AND status IN ('draft', 'published')
         RETURNING {META_COLS}"
    ))
    .bind(id)
    .bind(&lang)
    .bind(status)
    .bind(auth.user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(Json(meta))
}

/// Apaga a legenda de uma língua. Só quem gere a gravação.
#[utoipa::path(
    delete, path = "/api/recordings/{recording_id}/captions/{lang}", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path), ("lang" = String, Path, description = "BCP 47 curto.")),
    responses(
        (status = 204, description = "Apagada."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`recording.not_manager`"),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, lang)): Path<(Uuid, String)>,
) -> Result<StatusCode, ApiError> {
    managed_item(&state, id, auth.user_id).await?;
    let res = sqlx::query("DELETE FROM recording_captions WHERE recording_id = $1 AND lang = $2")
        .bind(id)
        .bind(&lang)
        .execute(&state.db)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}
