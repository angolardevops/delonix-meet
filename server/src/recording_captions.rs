//! Legendas por língua, servidas ao leitor como WebVTT (`<track>`).
//!
//! Três origens para a mesma tabela (`recording_captions`, migração 0055):
//! - `upload` — um VTT enviado por quem gere a gravação (validado);
//! - `transcript` — construído dos segmentos da transcrição, na língua dela;
//! - `translation` — cada segmento traduzido pelo LLM local (`ai::translate`),
//!   em segundo plano, com progresso.
//!
//! Quem só vê a gravação vê só as legendas PUBLICADAS; rascunhos, falhadas e
//! as que estão a ser geradas são de quem gere.

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::ApiError,
    recording_meta::{load_transcript, Segment},
    recordings::{access, Access},
    AppState,
};

pub const MAX_VTT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_CUES: usize = 20_000;
/// Uma geração parada há mais do que isto conta como abandonada (pod reiniciado).
const STALE_GENERATING_MINUTES: i32 = 30;

// ---------- WebVTT ----------

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// `hh:mm:ss.ttt` ou `mm:ss.ttt` → milissegundos (a forma que o WebVTT exige).
fn parse_timestamp(s: &str) -> Option<i64> {
    let (hms, frac) = s.split_once('.')?;
    if frac.len() != 3 || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let ms: i64 = frac.parse().ok()?;
    let parts: Vec<&str> = hms.split(':').collect();
    let nums: Vec<i64> = parts
        .iter()
        .map(|p| {
            (p.len() >= 2 && p.bytes().all(|b| b.is_ascii_digit()))
                .then(|| p.parse().ok())
                .flatten()
        })
        .collect::<Option<_>>()?;
    let (h, m, sec) = match nums.as_slice() {
        [m, s] => (0, *m, *s),
        [h, m, s] => (*h, *m, *s),
        _ => return None,
    };
    if m > 59 || sec > 59 {
        return None;
    }
    Some(((h * 60 + m) * 60 + sec) * 1000 + ms)
}

fn fmt_timestamp(ms: i64) -> String {
    let ms = ms.max(0);
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3_600_000,
        (ms / 60_000) % 60,
        (ms / 1000) % 60,
        ms % 1000
    )
}

/// Valida um ficheiro WebVTT e devolve as cues. A mensagem de erro diz a linha.
pub fn parse_vtt(src: &str) -> Result<Vec<Cue>, String> {
    let src = src.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut lines = src.split('\n').enumerate().peekable();
    match lines.next() {
        Some((_, first))
            if first == "WEBVTT"
                || first.starts_with("WEBVTT ")
                || first.starts_with("WEBVTT\t") => {}
        _ => return Err("a primeira linha tem de ser WEBVTT".into()),
    }
    // Cabeçalho: até à primeira linha em branco.
    for (_, l) in lines.by_ref() {
        if l.trim().is_empty() {
            break;
        }
    }
    let mut cues = Vec::new();
    while let Some((n, line)) = lines.next() {
        if line.trim().is_empty() {
            continue;
        }
        // Blocos que não são cues: saltam até à linha em branco.
        if line.starts_with("NOTE") || line == "STYLE" || line == "REGION" {
            for (_, l) in lines.by_ref() {
                if l.trim().is_empty() {
                    break;
                }
            }
            continue;
        }
        // Identificador opcional antes da linha de tempos.
        let (n, timing) = if line.contains("-->") {
            (n, line)
        } else {
            match lines.next() {
                Some((m, l)) if l.contains("-->") => (m, l),
                _ => return Err(format!("linha {}: falta a linha de tempos", n + 1)),
            }
        };
        let (a, rest) = timing
            .split_once("-->")
            .ok_or_else(|| format!("linha {}: tempos inválidos", n + 1))?;
        let b = rest.split_whitespace().next().unwrap_or("");
        let start =
            parse_timestamp(a.trim()).ok_or_else(|| format!("linha {}: início inválido", n + 1))?;
        let end = parse_timestamp(b).ok_or_else(|| format!("linha {}: fim inválido", n + 1))?;
        if end < start {
            return Err(format!("linha {}: o fim é anterior ao início", n + 1));
        }
        let mut text = Vec::new();
        while let Some((_, l)) = lines.peek() {
            if l.trim().is_empty() {
                break;
            }
            if l.contains("-->") {
                return Err(format!("linha {}: cue sem linha em branco antes", n + 2));
            }
            text.push(lines.next().map(|x| x.1).unwrap_or_default());
        }
        cues.push(Cue {
            start_ms: start,
            end_ms: end,
            text: text.join("\n"),
        });
        if cues.len() > MAX_CUES {
            return Err(format!("no máximo {MAX_CUES} legendas por ficheiro"));
        }
    }
    Ok(cues)
}

/// Texto de uma cue sem o que o WebVTT interpretaria como marcação ou fim de bloco.
fn cue_text(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace("-->", "→")
}

pub fn cues_to_vtt(cues: &[Cue]) -> String {
    let mut out = String::from("WEBVTT\n");
    for c in cues {
        out.push_str(&format!(
            "\n{} --> {}\n{}\n",
            fmt_timestamp(c.start_ms),
            fmt_timestamp(c.end_ms),
            cue_text(&c.text)
        ));
    }
    out
}

pub fn segments_to_cues(segments: &[Segment]) -> Vec<Cue> {
    segments
        .iter()
        .map(|s| Cue {
            start_ms: s.start_ms,
            end_ms: s.end_ms.max(s.start_ms + 1),
            text: s.text.clone(),
        })
        .collect()
}

/// `pt-AO` → `pt`.
fn primary(lang: &str) -> &str {
    lang.split('-').next().unwrap_or(lang)
}

pub(crate) fn valid_lang(lang: &str) -> bool {
    let mut parts = lang.split('-');
    let p = parts.next().unwrap_or("");
    let ok_primary = (2..=3).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_lowercase());
    let ok_rest = match (parts.next(), parts.next()) {
        (None, _) => true,
        (Some(r), None) => {
            (2..=8).contains(&r.len()) && r.bytes().all(|b| b.is_ascii_alphanumeric())
        }
        _ => false,
    };
    ok_primary && ok_rest
}

fn check_lang(lang: &str) -> Result<(), ApiError> {
    if valid_lang(lang) {
        Ok(())
    } else {
        Err(ApiError::BadRequest(
            "lang must be a short BCP 47 tag (pt, pt-AO, en)".into(),
        ))
    }
}

// ---------- rotas ----------

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct CaptionMeta {
    pub recording_id: Uuid,
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

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CaptionFull {
    #[serde(flatten)]
    pub meta: CaptionMeta,
    pub vtt: String,
}

/// Uma legenda que `a` pode ler: publicada, ou qualquer uma para quem gere.
async fn readable(
    state: &AppState,
    a: &Access,
    lang: &str,
) -> Result<(CaptionMeta, String), ApiError> {
    #[derive(sqlx::FromRow)]
    struct Row {
        #[sqlx(flatten)]
        meta: CaptionMeta,
        vtt: String,
    }
    let row: Row = sqlx::query_as(&format!(
        "SELECT {META_COLS}, vtt FROM recording_captions WHERE recording_id = $1 AND lang = $2"
    ))
    .bind(a.id)
    .bind(lang)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    if row.meta.status != "published" && !a.can_manage {
        return Err(ApiError::NotFound);
    }
    Ok((row.meta, row.vtt))
}

async fn meta_of(state: &AppState, rec: Uuid, lang: &str) -> Result<CaptionMeta, ApiError> {
    sqlx::query_as(&format!(
        "SELECT {META_COLS} FROM recording_captions WHERE recording_id = $1 AND lang = $2"
    ))
    .bind(rec)
    .bind(lang)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)
}

/// `GET /api/recordings/{id}/captions`.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/captions", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path, description = "Gravação.")),
    responses(
        (status = 200, body = Vec<CaptionMeta>),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<CaptionMeta>>, ApiError> {
    let a = access(&state, id, auth.user_id).await?;
    let rows: Vec<CaptionMeta> = sqlx::query_as(&format!(
        "SELECT {META_COLS} FROM recording_captions
         WHERE recording_id = $1 AND ($2 OR status = 'published') ORDER BY lang"
    ))
    .bind(id)
    .bind(a.can_manage)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

/// `GET /api/recordings/{id}/captions/{lang}` — metadados e o VTT.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/captions/{lang}", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path, description = "Gravação."), ("lang" = String, Path, description = "Língua BCP-47 (ex.: pt, en).")),
    responses(
        (status = 200, body = CaptionFull),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, lang)): Path<(Uuid, String)>,
) -> Result<Json<CaptionFull>, ApiError> {
    let a = access(&state, id, auth.user_id).await?;
    let (meta, vtt) = readable(&state, &a, &lang).await?;
    Ok(Json(CaptionFull { meta, vtt }))
}

/// `GET /api/recordings/{id}/captions/{lang}/vtt` — `text/vtt` para o `<track>`.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/captions/{lang}/vtt", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path, description = "Gravação."), ("lang" = String, Path, description = "Língua BCP-47 (ex.: pt, en).")),
    responses(
        (status = 200, body = String, content_type = "text/vtt", description = "A legenda em WebVTT."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn vtt(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, lang)): Path<(Uuid, String)>,
) -> Result<impl IntoResponse, ApiError> {
    let a = access(&state, id, auth.user_id).await?;
    let (meta, vtt) = readable(&state, &a, &lang).await?;
    if meta.status == "generating" || meta.status == "failed" {
        return Err(ApiError::Conflict(
            "esta legenda não tem conteúdo pronto".into(),
        ));
    }
    Ok((
        [
            (header::CONTENT_TYPE, "text/vtt; charset=utf-8"),
            (header::CACHE_CONTROL, "private, no-cache"),
        ],
        vtt,
    ))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct PutCaptionReq {
    pub vtt: String,
    /// Publicar já (por omissão fica rascunho).
    #[serde(default)]
    pub publish: bool,
}

/// `PUT /api/recordings/{id}/captions/{lang}` — envia (ou substitui) um VTT.
#[utoipa::path(
    put, path = "/api/recordings/{recording_id}/captions/{lang}", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path, description = "Gravação."), ("lang" = String, Path, description = "Língua BCP-47 (ex.: pt, en).")),
    request_body = PutCaptionReq,
    responses(
        (status = 200, body = CaptionMeta),
        (status = 400, body = crate::openapi::ErrorBody, description = "VTT inválido ou grande demais."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn put(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, lang)): Path<(Uuid, String)>,
    Json(req): Json<PutCaptionReq>,
) -> Result<Json<CaptionMeta>, ApiError> {
    let a = access(&state, id, auth.user_id).await?;
    a.require_manage()?;
    check_lang(&lang)?;
    if req.vtt.len() > MAX_VTT_BYTES {
        return Err(ApiError::BadRequest("VTT maior do que 2 MB".into()));
    }
    parse_vtt(&req.vtt).map_err(|e| ApiError::BadRequest(format!("VTT inválido: {e}")))?;
    let status = if req.publish { "published" } else { "draft" };
    let meta: CaptionMeta = sqlx::query_as(&format!(
        "INSERT INTO recording_captions (recording_id, lang, vtt, source, status, updated_by, published_at)
         VALUES ($1, $2, $3, 'upload', $4, $5, CASE WHEN $4 = 'published' THEN now() END)
         ON CONFLICT (recording_id, lang) DO UPDATE SET
             vtt = EXCLUDED.vtt, source = 'upload', status = EXCLUDED.status,
             progress_pct = NULL, error = NULL, updated_by = EXCLUDED.updated_by,
             updated_at = now(), published_at = EXCLUDED.published_at
         RETURNING {META_COLS}"
    ))
    .bind(id)
    .bind(&lang)
    .bind(req.vtt.trim_start_matches('\u{feff}'))
    .bind(status)
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(meta))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct PatchCaptionReq {
    /// `draft` | `published`.
    pub status: String,
}

/// `PATCH /api/recordings/{id}/captions/{lang}` — publicar ou despublicar.
#[utoipa::path(
    patch, path = "/api/recordings/{recording_id}/captions/{lang}", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path, description = "Gravação."), ("lang" = String, Path, description = "Língua BCP-47 (ex.: pt, en).")),
    request_body = PatchCaptionReq,
    responses(
        (status = 200, body = CaptionMeta),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn patch(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, lang)): Path<(Uuid, String)>,
    Json(req): Json<PatchCaptionReq>,
) -> Result<Json<CaptionMeta>, ApiError> {
    let a = access(&state, id, auth.user_id).await?;
    a.require_manage()?;
    if !matches!(req.status.as_str(), "draft" | "published") {
        return Err(ApiError::BadRequest(
            "status must be 'draft' or 'published'".into(),
        ));
    }
    let cur = meta_of(&state, id, &lang).await?;
    if !matches!(cur.status.as_str(), "draft" | "published") {
        return Err(ApiError::Conflict(format!(
            "a legenda está em «{}» e não se pode publicar",
            cur.status
        )));
    }
    let meta: CaptionMeta = sqlx::query_as(&format!(
        "UPDATE recording_captions SET status = $3, updated_by = $4, updated_at = now(),
                published_at = CASE WHEN $3 = 'published' THEN COALESCE(published_at, now()) END
         WHERE recording_id = $1 AND lang = $2 RETURNING {META_COLS}"
    ))
    .bind(id)
    .bind(&lang)
    .bind(&req.status)
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(meta))
}

/// `DELETE /api/recordings/{id}/captions/{lang}`.
#[utoipa::path(
    delete, path = "/api/recordings/{recording_id}/captions/{lang}", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path, description = "Gravação."), ("lang" = String, Path, description = "Língua BCP-47 (ex.: pt, en).")),
    responses(
        (status = 204, description = "Legenda apagada."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, lang)): Path<(Uuid, String)>,
) -> Result<StatusCode, ApiError> {
    let a = access(&state, id, auth.user_id).await?;
    a.require_manage()?;
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

#[derive(Deserialize, utoipa::ToSchema)]
pub struct GenerateCaptionReq {
    /// Língua pedida. Omitida = a da transcrição.
    #[serde(default)]
    pub lang: Option<String>,
}

/// `POST /api/recordings/{id}/captions/generate`.
///
/// - Na língua da transcrição: constrói o VTT dos segmentos já — `201` + rascunho.
/// - Noutra língua: tradução segmento a segmento pelo LLM local, em segundo
///   plano — `202` + a legenda em `generating`; o progresso lê-se em
///   `GET …/captions/{lang}`.
#[utoipa::path(
    post, path = "/api/recordings/{recording_id}/captions/generate", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path, description = "Gravação.")),
    request_body = GenerateCaptionReq,
    responses(
        (status = 201, body = CaptionMeta, description = "Na língua da transcrição: rascunho construído dos segmentos."),
        (status = 202, body = CaptionMeta, description = "Noutra língua: tradução em segundo plano; o progresso lê-se na legenda."),
        (status = 409, body = crate::openapi::ErrorBody, description = "Sem transcrição com tempos, ou uma geração já a decorrer."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn generate(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<GenerateCaptionReq>,
) -> Result<impl IntoResponse, ApiError> {
    let a = access(&state, id, auth.user_id).await?;
    a.require_manage()?;
    let t = load_transcript(&state, id).await?;
    if t.segments.is_empty() {
        return Err(ApiError::Conflict(
            "a gravação ainda não tem transcrição com tempos".into(),
        ));
    }
    let source_lang = t.language.clone().unwrap_or_default();
    let lang = req.lang.unwrap_or_else(|| source_lang.clone());
    check_lang(&lang)?;
    let location = format!("/api/recordings/{id}/captions/{lang}");

    // Uma geração a decorrer (e viva) não se atropela.
    let busy: Option<bool> = sqlx::query_scalar(&format!(
        "SELECT updated_at > now() - make_interval(mins => {STALE_GENERATING_MINUTES})
         FROM recording_captions WHERE recording_id = $1 AND lang = $2 AND status = 'generating'"
    ))
    .bind(id)
    .bind(&lang)
    .fetch_optional(&state.db)
    .await?;
    if busy == Some(true) {
        return Err(ApiError::Conflict(
            "já há uma geração a decorrer para esta língua".into(),
        ));
    }

    if !source_lang.is_empty() && primary(&lang) == primary(&source_lang) {
        let vtt = cues_to_vtt(&segments_to_cues(&t.segments));
        let meta =
            upsert_generated(&state, id, &lang, &vtt, "transcript", "draft", auth.user_id).await?;
        return Ok((
            StatusCode::CREATED,
            [(header::LOCATION, location)],
            Json(meta),
        ));
    }

    if state.config.ollama_url.is_none() {
        return Err(ApiError::ServiceUnavailable(
            "tradução indisponível (sem LLM local)".into(),
        ));
    }
    if !crate::ai::supports_target(primary(&lang)) {
        return Err(ApiError::BadRequest(format!(
            "tradução para «{lang}» não suportada; línguas: {}",
            crate::ai::TRANSLATE_TARGETS.join(", ")
        )));
    }
    let meta = upsert_generated(
        &state,
        id,
        &lang,
        "",
        "translation",
        "generating",
        auth.user_id,
    )
    .await?;
    spawn_translation(state.clone(), id, lang.clone(), t.segments);
    Ok((
        StatusCode::ACCEPTED,
        [(header::LOCATION, location)],
        Json(meta),
    ))
}

async fn upsert_generated(
    state: &AppState,
    rec: Uuid,
    lang: &str,
    vtt: &str,
    source: &str,
    status: &str,
    by: Uuid,
) -> Result<CaptionMeta, ApiError> {
    Ok(sqlx::query_as(&format!(
        "INSERT INTO recording_captions (recording_id, lang, vtt, source, status, progress_pct, updated_by)
         VALUES ($1, $2, $3, $4, $5, CASE WHEN $5 = 'generating' THEN 0 END, $6)
         ON CONFLICT (recording_id, lang) DO UPDATE SET
             vtt = EXCLUDED.vtt, source = EXCLUDED.source, status = EXCLUDED.status,
             progress_pct = EXCLUDED.progress_pct, error = NULL,
             updated_by = EXCLUDED.updated_by, updated_at = now(), published_at = NULL
         RETURNING {META_COLS}"
    ))
    .bind(rec)
    .bind(lang)
    .bind(vtt)
    .bind(source)
    .bind(status)
    .bind(by)
    .fetch_one(&state.db)
    .await?)
}

/// Traduz os segmentos um a um e guarda o VTT como rascunho.
///
/// Um segmento que o LLM não traduz faz FALHAR a legenda inteira: uma legenda
/// com buracos na língua de chegada, ou com frases deixadas na original, é
/// pior do que dizer que falhou.
fn spawn_translation(state: Arc<AppState>, rec: Uuid, lang: String, segments: Vec<Segment>) {
    tokio::spawn(async move {
        let total = segments.len().max(1);
        let mut cues = Vec::with_capacity(segments.len());
        let mut last_pct = 0;
        for (i, s) in segments.iter().enumerate() {
            let text: String = s.text.chars().take(500).collect();
            let Some(tr) = crate::ai::translate(&state, &text, primary(&lang)).await else {
                let _ = sqlx::query(
                    "UPDATE recording_captions SET status = 'failed', progress_pct = NULL,
                            error = $3, updated_at = now()
                     WHERE recording_id = $1 AND lang = $2",
                )
                .bind(rec)
                .bind(&lang)
                .bind(format!(
                    "o LLM local não traduziu o segmento {} de {total}",
                    i + 1
                ))
                .execute(&state.db)
                .await;
                tracing::warn!(recording = %rec, %lang, segment = i, "tradução de legendas falhou");
                return;
            };
            cues.push(Cue {
                start_ms: s.start_ms,
                end_ms: s.end_ms.max(s.start_ms + 1),
                text: tr,
            });
            let pct = ((i + 1) * 100 / total) as i16;
            if pct >= last_pct + 5 && pct < 100 {
                last_pct = pct;
                let _ = sqlx::query(
                    "UPDATE recording_captions SET progress_pct = $3, updated_at = now()
                     WHERE recording_id = $1 AND lang = $2 AND status = 'generating'",
                )
                .bind(rec)
                .bind(&lang)
                .bind(pct)
                .execute(&state.db)
                .await;
            }
        }
        let r = sqlx::query(
            "UPDATE recording_captions SET vtt = $3, status = 'draft', progress_pct = NULL,
                    updated_at = now()
             WHERE recording_id = $1 AND lang = $2 AND status = 'generating'",
        )
        .bind(rec)
        .bind(&lang)
        .bind(cues_to_vtt(&cues))
        .execute(&state.db)
        .await;
        match r {
            Ok(_) => {
                tracing::info!(recording = %rec, %lang, cues = cues.len(), "legendas traduzidas")
            }
            Err(e) => tracing::error!(recording = %rec, %lang, error = %e, "legendas: não gravou"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vtt_valido_com_identificadores_notas_e_definicoes() {
        let src = "\u{feff}WEBVTT - legendas pt-AO\r\n\r\nNOTE isto é um comentário\r\nque continua\r\n\r\n1\r\n00:00:01.000 --> 00:00:04.500 line:90%\r\nOlá a todos\r\nsegunda linha\r\n\r\n01:05.250 --> 01:07.000\r\nBoa tarde\r\n";
        let c = parse_vtt(src).unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].start_ms, 1000);
        assert_eq!(c[0].end_ms, 4500);
        assert_eq!(c[0].text, "Olá a todos\nsegunda linha");
        assert_eq!(c[1].start_ms, 65_250);
    }

    #[test]
    fn vtt_invalido_diz_a_linha() {
        assert!(parse_vtt("SRT\n\n1\n00:00:01,000 --> 00:00:02,000\nx").is_err());
        let e = parse_vtt("WEBVTT\n\n00:00:05.000 --> 00:00:01.000\nx\n").unwrap_err();
        assert!(e.contains("linha 3"), "{e}");
        assert!(parse_vtt("WEBVTT\n\n00:00:01,000 --> 00:00:02.000\nx\n").is_err());
        assert!(parse_vtt("WEBVTT\n\n00:61.000 --> 00:62.000\nx\n").is_err());
    }

    #[test]
    fn transcricao_vira_vtt_que_o_proprio_parser_aceita() {
        let segs = vec![
            Segment {
                start_ms: 0,
                end_ms: 3_725_004,
                text: "a <b>marcação</b> --> não passa & escapa".into(),
                confidence: Some(0.9),
            },
            Segment {
                start_ms: 5000,
                end_ms: 5000,
                text: "instantâneo".into(),
                confidence: None,
            },
        ];
        let vtt = cues_to_vtt(&segments_to_cues(&segs));
        assert!(vtt.starts_with("WEBVTT\n"));
        assert!(vtt.contains("00:00:00.000 --> 01:02:05.004"));
        assert!(!vtt.contains("<b>"), "{vtt}");
        let back = parse_vtt(&vtt).unwrap();
        assert_eq!(back.len(), 2);
        assert!(back[1].end_ms > back[1].start_ms);
    }

    #[test]
    fn linguas_aceites() {
        for ok in ["pt", "pt-AO", "en", "zh", "kmb", "zh-Hans"] {
            assert!(valid_lang(ok), "{ok}");
        }
        for bad in ["", "PT", "portugues", "pt_AO", "pt-", "pt-AO-x", "../x"] {
            assert!(!valid_lang(bad), "{bad}");
        }
        assert_eq!(primary("pt-AO"), "pt");
    }
}

/// Documentação OpenAPI das legendas (`openapi.rs` junta-a).
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(list, get, vtt, put, patch, delete, generate),
    components(schemas(
        CaptionMeta,
        CaptionFull,
        PutCaptionReq,
        PatchCaptionReq,
        GenerateCaptionReq
    ))
)]
pub struct ApiDoc;
