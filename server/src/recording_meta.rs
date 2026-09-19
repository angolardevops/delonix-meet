//! O que o leitor de gravações mostra à volta do vídeo: descrição e
//! etiquetas, publicação, miniatura, visualizações, participantes,
//! transcrição com tempos e comentários com marca temporal.
//!
//! Todo o acesso passa por `recordings::access`: quem não vê a gravação
//! recebe `404` (não se confirma a outra organização que o id existe); quem vê
//! mas não gere recebe `403` ao tentar escrever.
//!
//! Capítulos: `recording_chapters.rs`. Legendas: `recording_captions.rs`.

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::ApiError,
    recordings::{access, item_for, RecordingItem},
    AppState,
};

// ---------- paginação por cursor ----------

pub(crate) const DEFAULT_PAGE_SIZE: i64 = 50;
pub(crate) const MAX_PAGE_SIZE: i64 = 100;

#[derive(Deserialize, Default)]
pub struct PageQuery {
    #[serde(default)]
    pub page_size: Option<i64>,
    #[serde(default)]
    pub page_token: Option<String>,
}

impl PageQuery {
    pub(crate) fn size(&self) -> Result<i64, ApiError> {
        match self.page_size {
            None => Ok(DEFAULT_PAGE_SIZE),
            Some(n) if (1..=MAX_PAGE_SIZE).contains(&n) => Ok(n),
            Some(_) => Err(ApiError::BadRequest(format!(
                "page_size must be 1-{MAX_PAGE_SIZE}"
            ))),
        }
    }

    pub(crate) fn cursor(&self) -> Result<Option<(DateTime<Utc>, Uuid)>, ApiError> {
        match self.page_token.as_deref() {
            None | Some("") => Ok(None),
            Some(t) => decode_cursor(t)
                .map(Some)
                .ok_or_else(|| ApiError::BadRequest("page_token inválido".into())),
        }
    }
}

/// Cursor opaco `(instante, id)`. O cliente não o interpreta.
pub(crate) fn encode_cursor(at: DateTime<Utc>, id: Uuid) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!(
        "{}.{}",
        at.timestamp_micros(),
        id
    ))
}

pub(crate) fn decode_cursor(token: &str) -> Option<(DateTime<Utc>, Uuid)> {
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(token)
        .ok()?;
    let s = String::from_utf8(raw).ok()?;
    let (micros, id) = s.split_once('.')?;
    let at = DateTime::<Utc>::from_timestamp_micros(micros.parse().ok()?)?;
    Some((at, id.parse().ok()?))
}

#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_page_token: Option<String>,
}

/// Corta a página pedida (a consulta traz `size + 1`) e calcula o cursor.
pub(crate) fn paginate<T>(
    mut rows: Vec<T>,
    size: i64,
    key: impl Fn(&T) -> (DateTime<Utc>, Uuid),
) -> Page<T> {
    let more = rows.len() as i64 > size;
    rows.truncate(size as usize);
    let next_page_token = if more {
        rows.last().map(|r| {
            let (at, id) = key(r);
            encode_cursor(at, id)
        })
    } else {
        None
    };
    Page {
        items: rows,
        next_page_token,
    }
}

// ---------- descrição e etiquetas ----------

pub const MAX_TAGS: usize = 20;
pub const MAX_TAG_CHARS: usize = 40;

/// Normaliza etiquetas: sem `#`, minúsculas, sem espaços nas pontas, sem
/// repetidas. Recusa em vez de cortar em silêncio.
pub(crate) fn normalize_tags(raw: &[String]) -> Result<Vec<String>, ApiError> {
    let mut out: Vec<String> = Vec::new();
    for t in raw {
        let t = t.trim().trim_start_matches('#').trim().to_lowercase();
        if t.is_empty() {
            continue;
        }
        if t.chars().count() > MAX_TAG_CHARS {
            return Err(ApiError::BadRequest(format!(
                "cada etiqueta tem no máximo {MAX_TAG_CHARS} caracteres"
            )));
        }
        if t.chars().any(|c| c.is_control() || c == ',') {
            return Err(ApiError::BadRequest(
                "etiqueta com caracteres inválidos".into(),
            ));
        }
        if !out.contains(&t) {
            out.push(t);
        }
    }
    if out.len() > MAX_TAGS {
        return Err(ApiError::BadRequest(format!(
            "no máximo {MAX_TAGS} etiquetas"
        )));
    }
    Ok(out)
}

// ---------- publicação ----------

#[derive(Deserialize, utoipa::ToSchema)]
pub struct PublishReq {
    /// Só `org` por agora: publicar para a organização do autor.
    #[serde(default = "default_visibility")]
    pub visibility: String,
}

fn default_visibility() -> String {
    "org".into()
}

/// `POST /api/recordings/{id}/publish` — publica para a organização do autor.
#[utoipa::path(
    post, path = "/api/recordings/{recording_id}/publish", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path, description = "Gravação.")),
    request_body = PublishReq,
    responses(
        (status = 200, body = crate::recordings::RecordingItem),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn publish(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<PublishReq>,
) -> Result<Json<RecordingItem>, ApiError> {
    let a = access(&state, id, auth.user_id).await?;
    a.require_manage()?;
    if req.visibility != "org" {
        return Err(ApiError::BadRequest("visibility must be 'org'".into()));
    }
    if !a.has_file() {
        return Err(ApiError::Conflict(
            "só se publica uma gravação que tem ficheiro".into(),
        ));
    }
    sqlx::query(
        "UPDATE recordings SET visibility = $2, published_at = COALESCE(published_at, now())
         WHERE id = $1",
    )
    .bind(id)
    .bind(&req.visibility)
    .execute(&state.db)
    .await?;
    crate::audit::log(
        &state.db,
        None,
        auth.user_id,
        "recording.published",
        &id.to_string(),
    )
    .await;
    item_for(&state, id, auth.user_id).await.map(Json)
}

/// `POST /api/recordings/{id}/unpublish` — volta a privada.
#[utoipa::path(
    post, path = "/api/recordings/{recording_id}/unpublish", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path, description = "Gravação.")),
    responses(
        (status = 200, body = crate::recordings::RecordingItem),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn unpublish(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<RecordingItem>, ApiError> {
    let a = access(&state, id, auth.user_id).await?;
    a.require_manage()?;
    sqlx::query("UPDATE recordings SET visibility = 'private', published_at = NULL WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    crate::audit::log(
        &state.db,
        None,
        auth.user_id,
        "recording.unpublished",
        &id.to_string(),
    )
    .await;
    item_for(&state, id, auth.user_id).await.map(Json)
}

// ---------- miniatura e visualizações ----------

/// `GET /api/recordings/{id}/thumbnail` — JPEG gerado pelo servidor.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/thumbnail", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path, description = "Gravação.")),
    responses(
        (status = 200, body = Vec<u8>, content_type = "image/jpeg", description = "Miniatura JPEG gerada pelo servidor."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn thumbnail(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    access(&state, id, auth.user_id).await?;
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

/// `POST /api/recordings/{id}/views` — regista uma visualização (uma por
/// pessoa por dia; repetir no mesmo dia não conta duas vezes).
#[utoipa::path(
    post, path = "/api/recordings/{recording_id}/views", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path, description = "Gravação.")),
    responses(
        (status = 204, description = "Visualização registada (no máximo uma por pessoa e período)."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn record_view(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let a = access(&state, id, auth.user_id).await?;
    if !a.has_file() {
        return Err(ApiError::Conflict("a gravação não tem ficheiro".into()));
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
pub struct Participant {
    pub user_id: Uuid,
    pub username: String,
    pub joined_at: DateTime<Utc>,
}

async fn participants_of_room(
    state: &AppState,
    room_id: Uuid,
    q: &PageQuery,
) -> Result<Page<Participant>, ApiError> {
    let size = q.size()?;
    let cursor = q.cursor()?;
    let rows: Vec<Participant> = sqlx::query_as(
        "SELECT p.user_id, u.username, p.joined_at
         FROM room_participants p JOIN users u ON u.id = p.user_id
         WHERE p.room_id = $1
           AND ($2::timestamptz IS NULL OR (p.joined_at, p.user_id) > ($2, $3))
         ORDER BY p.joined_at, p.user_id
         LIMIT $4",
    )
    .bind(room_id)
    .bind(cursor.map(|c| c.0))
    .bind(cursor.map(|c| c.1))
    .bind(size + 1)
    .fetch_all(&state.db)
    .await?;
    Ok(paginate(rows, size, |p| (p.joined_at, p.user_id)))
}

/// `GET /api/recordings/{id}/participants` — quem esteve na sala da gravação.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/participants", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path, description = "Gravação."), ("page_size" = Option<u32>, Query, description = "Itens por página."), ("page_token" = Option<String>, Query, description = "Cursor da página seguinte.")),
    responses(
        (status = 200, body = serde_json::Value, description = "Página `{items: [Participant], next_page_token}`."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn recording_participants(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<PageQuery>,
) -> Result<Json<Page<Participant>>, ApiError> {
    let a = access(&state, id, auth.user_id).await?;
    participants_of_room(&state, a.room_id, &q).await.map(Json)
}

/// `GET /api/rooms/{code}/participants` — só para quem esteve na sala.
#[utoipa::path(
    get, path = "/api/rooms/{room_code}/participants", tag = "rooms",
    security(("session" = [])),
    params(("room_code" = String, Path, description = "Código da sala."), ("page_size" = Option<u32>, Query, description = "Itens por página."), ("page_token" = Option<String>, Query, description = "Cursor da página seguinte.")),
    responses(
        (status = 200, body = serde_json::Value, description = "Página `{items: [Participant], next_page_token}`."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn room_participants(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
    Query(q): Query<PageQuery>,
) -> Result<Json<Page<Participant>>, ApiError> {
    let room = crate::recordings::participated_room(&state, &code, auth.user_id).await?;
    participants_of_room(&state, room.id, &q).await.map(Json)
}

// ---------- transcrição ----------

/// Um segmento da transcrição (forma guardada pelo ai-worker, migração 0054).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
pub struct Segment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    #[serde(default)]
    pub confidence: Option<f32>,
}

/// Lê os segmentos guardados, deitando fora os que não fazem sentido
/// (texto vazio, fim antes do início) em vez de falhar o leitor inteiro.
pub(crate) fn parse_segments(v: serde_json::Value) -> Vec<Segment> {
    let raw: Vec<serde_json::Value> = serde_json::from_value(v).unwrap_or_default();
    let mut out: Vec<Segment> = raw
        .into_iter()
        .filter_map(|x| serde_json::from_value::<Segment>(x).ok())
        .filter(|s| s.start_ms >= 0 && s.end_ms >= s.start_ms && !s.text.trim().is_empty())
        .map(|mut s| {
            s.text = s.text.trim().to_string();
            s
        })
        .collect();
    out.sort_by_key(|s| s.start_ms);
    out
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Transcript {
    pub recording_id: Uuid,
    /// `none` | `transcribing` | `ready` | `failed`.
    pub status: String,
    pub progress_pct: Option<i16>,
    pub language: Option<String>,
    pub confidence: Option<f32>,
    pub transcribed_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
    pub text: String,
    pub segments: Vec<Segment>,
}

pub(crate) struct StoredTranscript {
    pub status: String,
    pub progress_pct: Option<i16>,
    pub language: Option<String>,
    pub confidence: Option<f32>,
    pub transcribed_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
    pub text: String,
    pub segments: Vec<Segment>,
}

pub(crate) async fn load_transcript(
    state: &AppState,
    id: Uuid,
) -> Result<StoredTranscript, ApiError> {
    type Row = (
        String,
        Option<i16>,
        Option<String>,
        Option<f32>,
        Option<DateTime<Utc>>,
        Option<String>,
        String,
        serde_json::Value,
    );
    let (status, progress, language, confidence, at, error, text, segs): Row = sqlx::query_as(
        "SELECT status, progress_pct, transcript_language, transcript_confidence,
                transcribed_at, transcript_error, transcript, transcript_segments
         FROM recordings WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    let t_status = if status == "transcribing" {
        "transcribing"
    } else if at.is_none() {
        "none"
    } else if error.is_some() {
        "failed"
    } else {
        "ready"
    };
    Ok(StoredTranscript {
        status: t_status.into(),
        progress_pct: (status == "transcribing").then_some(progress).flatten(),
        language,
        confidence,
        transcribed_at: at,
        error,
        text,
        segments: parse_segments(segs),
    })
}

/// `GET /api/recordings/{id}/transcript` — texto e segmentos com tempos.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/transcript", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path, description = "Gravação.")),
    responses(
        (status = 200, body = Transcript),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn transcript(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Transcript>, ApiError> {
    access(&state, id, auth.user_id).await?;
    let t = load_transcript(&state, id).await?;
    Ok(Json(Transcript {
        recording_id: id,
        status: t.status,
        progress_pct: t.progress_pct,
        language: t.language,
        confidence: t.confidence,
        transcribed_at: t.transcribed_at,
        error: t.error,
        text: t.text,
        segments: t.segments,
    }))
}

/// Valida o instante de um comentário/capítulo contra a duração medida.
pub(crate) fn check_t_ms(t_ms: i64, duration_ms: Option<i64>) -> Result<(), ApiError> {
    if t_ms < 0 {
        return Err(ApiError::BadRequest("t_ms must be >= 0".into()));
    }
    if let Some(d) = duration_ms {
        if t_ms > d {
            return Err(ApiError::BadRequest(format!(
                "t_ms ({t_ms}) passa do fim da gravação ({d} ms)"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_ida_e_volta() {
        let at = DateTime::<Utc>::from_timestamp_micros(1_789_000_000_123_456).unwrap();
        let id = Uuid::new_v4();
        assert_eq!(decode_cursor(&encode_cursor(at, id)), Some((at, id)));
        assert_eq!(decode_cursor("lixo"), None);
        assert_eq!(decode_cursor(""), None);
    }

    #[test]
    fn page_size_fora_do_intervalo_e_recusado_e_nao_cortado() {
        let q = PageQuery {
            page_size: Some(500),
            page_token: None,
        };
        assert!(q.size().is_err());
        assert_eq!(PageQuery::default().size().unwrap(), DEFAULT_PAGE_SIZE);
    }

    #[test]
    fn paginate_so_da_cursor_quando_ha_mais() {
        // Microssegundos: é a resolução do `timestamptz` de onde o cursor vem.
        let at = DateTime::<Utc>::from_timestamp_micros(Utc::now().timestamp_micros()).unwrap();
        let rows: Vec<(DateTime<Utc>, Uuid)> = (0..3).map(|_| (at, Uuid::new_v4())).collect();
        let p = paginate(rows.clone(), 2, |r| *r);
        assert_eq!(p.items.len(), 2);
        assert_eq!(
            p.next_page_token.as_deref().and_then(decode_cursor),
            Some(rows[1])
        );
        let p = paginate(rows, 3, |r| *r);
        assert!(p.next_page_token.is_none());
    }

    #[test]
    fn etiquetas_normalizadas_e_limites_recusados() {
        let t =
            normalize_tags(&["#Aula".into(), " aula ".into(), "".into(), "Redes".into()]).unwrap();
        assert_eq!(t, vec!["aula".to_string(), "redes".to_string()]);
        assert!(normalize_tags(&["x".repeat(41)]).is_err());
        let muitas: Vec<String> = (0..21).map(|i| format!("t{i}")).collect();
        assert!(normalize_tags(&muitas).is_err());
        assert!(normalize_tags(&["a,b".into()]).is_err());
    }

    #[test]
    fn segmentos_invalidos_saem_sem_partir_o_resto() {
        let v = serde_json::json!([
            {"start_ms": 5000, "end_ms": 6000, "text": " segundo ", "confidence": 0.8},
            {"start_ms": 0, "end_ms": 4000, "text": "primeiro"},
            {"start_ms": 7000, "end_ms": 6500, "text": "fim antes do início"},
            {"start_ms": 8000, "end_ms": 9000, "text": "   "},
            {"start_ms": "não é número", "text": "forma errada"}
        ]);
        let s = parse_segments(v);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].text, "primeiro");
        assert_eq!(s[1].text, "segundo");
        assert!(parse_segments(serde_json::json!({"não": "é lista"})).is_empty());
    }

    #[test]
    fn instante_depois_do_fim_e_recusado() {
        assert!(check_t_ms(1000, Some(2000)).is_ok());
        assert!(check_t_ms(3000, Some(2000)).is_err());
        assert!(check_t_ms(-1, None).is_err());
        assert!(check_t_ms(99_999_999, None).is_ok());
    }
}

/// Documentação OpenAPI do leitor de gravações (`openapi.rs` junta-a).
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        publish,
        unpublish,
        thumbnail,
        record_view,
        recording_participants,
        room_participants,
        transcript
    ),
    components(schemas(PublishReq, Participant, Segment, Transcript))
)]
pub struct ApiDoc;
