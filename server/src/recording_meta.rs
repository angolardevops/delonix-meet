//! O que o leitor de gravações mostra à volta do vídeo: miniatura,
//! visualizações, participantes e transcrição com tempos (R183; portado do
//! servidor da UI para as convenções desta linha).
//!
//! Todo o acesso passa por `recordings::seen_item`: quem não chega à gravação
//! recebe `404` antes de qualquer outra resposta (não se confirma a outra
//! organização que o id existe). Capítulos e comentários: `recordings.rs`.
//! Legendas: `recording_captions.rs`.

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::page::{Page, PageRequest};
use delonix_meet_domain::content::{recording as rules, transcription};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::ApiError,
    recordings::{is_participant, no_file, room_by_code, seen_item, PageQuery},
    AppState,
};

/// JPEG em bruto (só para o spec).
#[derive(utoipa::ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
pub struct JpegBytes(Vec<u8>);

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        thumbnail,
        record_view,
        recording_participants,
        room_participants,
        transcript
    ),
    components(schemas(Participant, ParticipantPage, TranscriptSegment, Transcript))
)]
pub struct ApiDoc;

// ---------- miniatura e visualizações ----------

/// Miniatura JPEG gerada pelo servidor na medição (`has_thumbnail`).
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/thumbnail", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path)),
    responses(
        (status = 200, body = inline(JpegBytes), content_type = "image/jpeg"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "Sem acesso, ou a gravação não tem miniatura (ffmpeg ausente, só áudio, ficheiro ilegível).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn thumbnail(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let rec = seen_item(&state, id, auth.user_id).await?;
    if !rec.has_thumbnail {
        return Err(ApiError::NotFound);
    }
    let data = tokio::fs::read(crate::media_probe::thumbnail_path(&state, id))
        .await
        .map_err(|_| ApiError::NotFound)?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/jpeg"),
            (header::CACHE_CONTROL, "private, max-age=300"),
        ],
        data,
    ))
}

/// Regista uma visualização: uma por pessoa por dia (repetir no mesmo dia
/// não conta duas vezes).
#[utoipa::path(
    post, path = "/api/recordings/{recording_id}/views", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path)),
    responses(
        (status = 204, description = "Registada (ou já estava, hoje)."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "`recording.no_file`: gravação falhada, não há o que ver."),
    )
)]
pub async fn record_view(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let rec = seen_item(&state, id, auth.user_id).await?;
    if !rec.has_file() {
        return Err(no_file());
    }
    sqlx::query(
        "INSERT INTO recording_views (recording_id, user_id) VALUES ($1, $2)
         ON CONFLICT (recording_id, user_id, viewed_on) DO UPDATE SET last_at = now()",
    )
    .bind(id)
    .bind(auth.user_id)
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- participantes ----------

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[schema(as = RecordingParticipant)]
pub struct Participant {
    pub user_id: Uuid,
    pub username: String,
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[schema(as = RecordingParticipantPage)]
pub struct ParticipantPage {
    pub items: Vec<Participant>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct ParticipantCursor {
    at: DateTime<Utc>,
    id: Uuid,
}

async fn participants_of_room(
    state: &AppState,
    room_id: Uuid,
    q: PageQuery,
) -> Result<ParticipantPage, ApiError> {
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<ParticipantCursor> = page.cursor()?;
    let rows: Vec<Participant> = sqlx::query_as(
        "SELECT p.user_id, u.username, p.joined_at
           FROM room_participants p JOIN users u ON u.id = p.user_id
          WHERE p.room_id = $1
            AND ($2::timestamptz IS NULL OR (p.joined_at, p.user_id) > ($2, $3))
          ORDER BY p.joined_at, p.user_id
          LIMIT $4",
    )
    .bind(room_id)
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |r| ParticipantCursor {
        at: r.joined_at,
        id: r.user_id,
    });
    Ok(ParticipantPage {
        items: p.items,
        next_page_token: p.next_page_token,
    })
}

/// Quem esteve na sala da gravação, por ordem de entrada.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/participants", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path), PageQuery),
    responses(
        (status = 200, body = ParticipantPage),
        (status = 400, body = crate::openapi::ErrorBody, description = "`page.invalid_token`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn recording_participants(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<PageQuery>,
) -> Result<Json<ParticipantPage>, ApiError> {
    let rec = seen_item(&state, id, auth.user_id).await?;
    Ok(Json(participants_of_room(&state, rec.room_id, q).await?))
}

/// Quem esteve na sala, por ordem de entrada. Só para quem também esteve: uma
/// sala que não existe e uma onde não se esteve dão o mesmo `404`.
#[utoipa::path(
    get, path = "/api/rooms/{room_code}/participants", tag = "recordings",
    security(("session" = [])),
    params(("room_code" = String, Path, description = "Código da sala."), PageQuery),
    responses(
        (status = 200, body = ParticipantPage),
        (status = 400, body = crate::openapi::ErrorBody, description = "`page.invalid_token`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "Sala inexistente, ou quem pede não participou nela.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn room_participants(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
    Query(q): Query<PageQuery>,
) -> Result<Json<ParticipantPage>, ApiError> {
    let room = room_by_code(&state, &code).await?;
    if !is_participant(&state, room.id, auth.user_id).await? {
        return Err(ApiError::NotFound);
    }
    Ok(Json(participants_of_room(&state, room.id, q).await?))
}

// ---------- transcrição ----------

/// Um segmento da transcrição com tempos.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[schema(as = TranscriptSegment)]
pub struct TranscriptSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    /// 0–1; `null` = o worker não a deu.
    pub confidence: Option<f32>,
}

impl From<transcription::Segment> for TranscriptSegment {
    fn from(s: transcription::Segment) -> Self {
        Self {
            start_ms: s.start_ms,
            end_ms: s.end_ms,
            text: s.text,
            confidence: s.confidence,
        }
    }
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[schema(as = RecordingTranscript)]
pub struct Transcript {
    pub recording_id: Uuid,
    /// `none` | `transcribing` | `ready` | `failed`.
    pub status: String,
    /// Só enquanto `transcribing`, e só se houver quem o escreva.
    pub progress_pct: Option<i16>,
    pub language: Option<String>,
    /// Média da confiança dos segmentos, 0–1.
    pub confidence: Option<f32>,
    pub transcribed_at: Option<DateTime<Utc>>,
    /// Razão da falha. Só quando `status = failed`.
    pub error: Option<String>,
    /// Texto inteiro (já censurado pelo DLP).
    pub text: String,
    /// Vazio quando o worker entregou só o texto.
    pub segments: Vec<TranscriptSegment>,
}

/// Os segmentos guardados de uma gravação, limpos. Ponto de entrada de quem
/// os consome: a transcrição servida aqui e — fora deste lote — a geração de
/// legendas (`captions/generate`, via `caption::segments_to_cues`) e de
/// capítulos automáticos (`chapters/generate`).
pub(crate) async fn load_segments(
    state: &AppState,
    recording_id: Uuid,
) -> Result<Vec<transcription::Segment>, ApiError> {
    let (raw,): (serde_json::Value,) =
        sqlx::query_as("SELECT transcript_segments FROM recordings WHERE id = $1")
            .bind(recording_id)
            .fetch_one(&state.db)
            .await?;
    Ok(parse_segments(raw))
}

/// Lê o JSONB elemento a elemento: um segmento com forma errada sai sozinho,
/// sem levar os outros.
fn parse_segments(raw: serde_json::Value) -> Vec<transcription::Segment> {
    let items: Vec<serde_json::Value> = serde_json::from_value(raw).unwrap_or_default();
    transcription::sanitize_segments(
        items
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect(),
    )
}

/// Texto e segmentos com tempos da transcrição.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/transcript", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path)),
    responses(
        (status = 200, body = Transcript),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn transcript(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Transcript>, ApiError> {
    let rec = seen_item(&state, id, auth.user_id).await?;
    let status = rules::transcript_status(rec.processing());
    #[allow(clippy::type_complexity)]
    let (text, language, confidence, error): (
        String,
        Option<String>,
        Option<f32>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT coalesce(transcript, ''), transcript_language, transcript_confidence,
                transcription_error
           FROM recordings WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    let segments = load_segments(&state, id).await?;
    Ok(Json(Transcript {
        recording_id: id,
        status: status.as_str().to_string(),
        progress_pct: (status == rules::TranscriptStatus::Transcribing)
            .then_some(rec.progress_pct)
            .flatten(),
        language,
        confidence,
        transcribed_at: rec.transcribed_at,
        // Um erro de uma tentativa que vai ser repetida não é o estado da
        // transcrição: só se mostra quando se desistiu.
        error: (status == rules::TranscriptStatus::Failed)
            .then_some(error)
            .flatten(),
        text,
        segments: segments.into_iter().map(Into::into).collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_segments_with_bad_shapes_do_not_break_the_rest() {
        let v = serde_json::json!([
            {"start_ms": 5000, "end_ms": 6000, "text": " segundo ", "confidence": 0.8},
            {"start_ms": 0, "end_ms": 4000, "text": "primeiro"},
            {"start_ms": "não é número", "text": "forma errada"}
        ]);
        let s = parse_segments(v);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].text, "primeiro");
        assert!(parse_segments(serde_json::json!({"não": "é lista"})).is_empty());
    }
}
