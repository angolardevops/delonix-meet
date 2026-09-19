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
pub const MAX_DESCRIPTION_CHARS: usize = 8000;

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

#[derive(Deserialize)]
pub struct PatchRecordingReq {
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

/// `PATCH /api/recordings/{id}` — título (nome), descrição e etiquetas.
pub async fn patch(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<PatchRecordingReq>,
) -> Result<Json<RecordingItem>, ApiError> {
    let a = access(&state, id, auth.user_id).await?;
    a.require_manage()?;
    let filename = match req.filename.as_deref().map(str::trim) {
        Some(f) if f.is_empty() || f.chars().count() > 200 => {
            return Err(ApiError::BadRequest("filename must be 1-200 chars".into()))
        }
        other => other.map(str::to_string),
    };
    let description = match req.description.as_deref().map(str::trim) {
        Some(d) if d.chars().count() > MAX_DESCRIPTION_CHARS => {
            return Err(ApiError::BadRequest(format!(
                "a descrição tem no máximo {MAX_DESCRIPTION_CHARS} caracteres"
            )))
        }
        other => other.map(str::to_string),
    };
    let tags = req.tags.as_deref().map(normalize_tags).transpose()?;
    sqlx::query(
        "UPDATE recordings SET filename = COALESCE($2, filename),
                description = COALESCE($3, description),
                tags = COALESCE($4, tags)
         WHERE id = $1",
    )
    .bind(id)
    .bind(filename)
    .bind(description)
    .bind(tags)
    .execute(&state.db)
    .await?;
    item_for(&state, id, auth.user_id).await.map(Json)
}

// ---------- publicação ----------

#[derive(Deserialize)]
pub struct PublishReq {
    /// Só `org` por agora: publicar para a organização do autor.
    #[serde(default = "default_visibility")]
    pub visibility: String,
}

fn default_visibility() -> String {
    "org".into()
}

/// `POST /api/recordings/{id}/publish` — publica para a organização do autor.
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

#[derive(Debug, Serialize, sqlx::FromRow)]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Serialize)]
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

// ---------- comentários ----------

pub const MAX_COMMENT_CHARS: usize = 2000;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Comment {
    pub id: Uuid,
    pub recording_id: Uuid,
    pub user_id: Uuid,
    pub username: String,
    pub t_ms: Option<i64>,
    pub body: String,
    pub created_at: DateTime<Utc>,
    /// Quem pede pode apagar (autor do comentário ou gestor da gravação).
    pub can_delete: bool,
}

const COMMENT_SELECT: &str = "SELECT c.id, c.recording_id, c.user_id, u.username, c.t_ms, c.body,
        c.created_at, (c.user_id = $2 OR $3) AS can_delete
 FROM recording_comments c JOIN users u ON u.id = c.user_id";

/// `GET /api/recordings/{id}/comments` — por ordem de criação, paginado.
pub async fn list_comments(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<PageQuery>,
) -> Result<Json<Page<Comment>>, ApiError> {
    let a = access(&state, id, auth.user_id).await?;
    let size = q.size()?;
    let cursor = q.cursor()?;
    let rows: Vec<Comment> = sqlx::query_as(&format!(
        "{COMMENT_SELECT}
         WHERE c.recording_id = $1
           AND ($4::timestamptz IS NULL OR (c.created_at, c.id) > ($4, $5))
         ORDER BY c.created_at, c.id
         LIMIT $6"
    ))
    .bind(id)
    .bind(auth.user_id)
    .bind(a.can_manage)
    .bind(cursor.map(|c| c.0))
    .bind(cursor.map(|c| c.1))
    .bind(size + 1)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(paginate(rows, size, |c| (c.created_at, c.id))))
}

async fn comment_by_id(
    state: &AppState,
    rec: Uuid,
    comment: Uuid,
    viewer: Uuid,
    can_manage: bool,
) -> Result<Comment, ApiError> {
    sqlx::query_as(&format!(
        "{COMMENT_SELECT} WHERE c.recording_id = $1 AND c.id = $4"
    ))
    .bind(rec)
    .bind(viewer)
    .bind(can_manage)
    .bind(comment)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)
}

/// `GET /api/recordings/{id}/comments/{comment_id}`.
pub async fn get_comment(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, comment_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Comment>, ApiError> {
    let a = access(&state, id, auth.user_id).await?;
    comment_by_id(&state, id, comment_id, auth.user_id, a.can_manage)
        .await
        .map(Json)
}

#[derive(Deserialize)]
pub struct CreateCommentReq {
    pub body: String,
    #[serde(default)]
    pub t_ms: Option<i64>,
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

/// `POST /api/recordings/{id}/comments` — quem vê a gravação pode comentar.
pub async fn create_comment(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateCommentReq>,
) -> Result<impl IntoResponse, ApiError> {
    let a = access(&state, id, auth.user_id).await?;
    let body = req.body.trim();
    if body.is_empty() || body.chars().count() > MAX_COMMENT_CHARS {
        return Err(ApiError::BadRequest(format!(
            "body must be 1-{MAX_COMMENT_CHARS} chars"
        )));
    }
    if let Some(t) = req.t_ms {
        check_t_ms(t, a.duration_ms)?;
    }
    let (cid,): (Uuid,) = sqlx::query_as(
        "INSERT INTO recording_comments (recording_id, user_id, t_ms, body)
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(id)
    .bind(auth.user_id)
    .bind(req.t_ms)
    .bind(body)
    .fetch_one(&state.db)
    .await?;
    let c = comment_by_id(&state, id, cid, auth.user_id, a.can_manage).await?;
    Ok((
        StatusCode::CREATED,
        [(
            header::LOCATION,
            format!("/api/recordings/{id}/comments/{cid}"),
        )],
        Json(c),
    ))
}

/// `DELETE /api/recordings/{id}/comments/{comment_id}` — autor ou gestor.
pub async fn delete_comment(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, comment_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let a = access(&state, id, auth.user_id).await?;
    let c = comment_by_id(&state, id, comment_id, auth.user_id, a.can_manage).await?;
    if !c.can_delete {
        return Err(ApiError::Forbidden);
    }
    sqlx::query("DELETE FROM recording_comments WHERE id = $1 AND recording_id = $2")
        .bind(comment_id)
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
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
