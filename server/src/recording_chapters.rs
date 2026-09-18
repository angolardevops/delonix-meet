//! Capítulos de uma gravação.
//!
//! Dois caminhos para a mesma tabela (`recording_chapters`, migração 0056):
//! - **automático** — o LLM local (`ai::generate`, Ollama in-cluster) lê os
//!   segmentos da transcrição e propõe capítulos; corre sozinho depois da
//!   transcrição (`auto_chapters_sweep`) ou a pedido
//!   (`POST …/chapters/generate`);
//! - **manual** — quem gere a gravação cria, edita e apaga.
//!
//! Gerar outra vez substitui só os automáticos: um capítulo escrito ou
//! corrigido à mão nunca é apagado por uma máquina.
//!
//! O ESTADO da última geração (a correr, bem sucedida, falhada e porquê) vive
//! em `recording_chapter_generations` (migração 0087) e lê-se em
//! `GET …/chapters/generation`. `recordings.chapters_generated_at` só diz que
//! houve uma geração BEM SUCEDIDA: uma resposta do modelo sem capítulos
//! utilizáveis fica `failed`, não apaga os automáticos que havia nem marca a
//! gravação como gerada.

use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::ApiError,
    recording_meta::{check_t_ms, load_transcript, Segment},
    recordings::access,
    AppState,
};

pub const MAX_CHAPTER_TITLE_CHARS: usize = 200;
/// Tecto de capítulos que se aceitam de uma resposta do LLM.
const MAX_AUTO_CHAPTERS: usize = 30;
/// Texto da transcrição que se envia ao LLM (janela de contexto do modelo pequeno).
const PROMPT_BUDGET_CHARS: usize = 24_000;
/// Tecto da chamada ao modelo numa geração de capítulos.
const GENERATE_TIMEOUT: Duration = Duration::from_secs(300);
/// Uma geração `running` sem sinal há mais do que o tecto da chamada mais esta
/// folga lê-se como interrompida (o pod morreu a meio), sem varredor.
const STALE_MARGIN_SECS: u64 = 60;

fn stale_after_secs() -> f64 {
    (GENERATE_TIMEOUT.as_secs() + STALE_MARGIN_SECS) as f64
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Chapter {
    pub id: Uuid,
    pub recording_id: Uuid,
    pub t_ms: i64,
    pub title: String,
    /// `auto` | `manual`.
    pub source: String,
    pub created_at: DateTime<Utc>,
}

const CHAPTER_COLS: &str = "id, recording_id, t_ms, title, source, created_at";

fn clean_title(raw: &str) -> Result<String, ApiError> {
    let t = raw.trim();
    if t.is_empty() || t.chars().count() > MAX_CHAPTER_TITLE_CHARS {
        return Err(ApiError::BadRequest(format!(
            "title must be 1-{MAX_CHAPTER_TITLE_CHARS} chars"
        )));
    }
    Ok(t.to_string())
}

fn unique_to_conflict(e: sqlx::Error) -> ApiError {
    match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => {
            ApiError::Conflict("já existe um capítulo nesse instante".into())
        }
        _ => e.into(),
    }
}

async fn list_of(state: &AppState, rec: Uuid) -> Result<Vec<Chapter>, ApiError> {
    Ok(sqlx::query_as(&format!(
        "SELECT {CHAPTER_COLS} FROM recording_chapters WHERE recording_id = $1 ORDER BY t_ms"
    ))
    .bind(rec)
    .fetch_all(&state.db)
    .await?)
}

async fn by_id(state: &AppState, rec: Uuid, chapter: Uuid) -> Result<Chapter, ApiError> {
    sqlx::query_as(&format!(
        "SELECT {CHAPTER_COLS} FROM recording_chapters WHERE recording_id = $1 AND id = $2"
    ))
    .bind(rec)
    .bind(chapter)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct PatchChapterReq {
    #[serde(default)]
    pub t_ms: Option<i64>,
    #[serde(default)]
    pub title: Option<String>,
}

/// `PATCH /api/recordings/{id}/chapters/{chapter_id}` — corrigir um capítulo
/// torna-o manual (a próxima geração automática já não lhe toca). Um corpo
/// sem `t_ms` nem `title` não corrige nada e deixa a origem como está.
#[utoipa::path(
    patch, path = "/api/recordings/{recording_id}/chapters/{chapter_id}", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path, description = "Gravação."), ("chapter_id" = Uuid, Path, description = "Capítulo.")),
    request_body = PatchChapterReq,
    responses(
        (status = 200, body = serde_json::Value, description = "O capítulo: `{id, recording_id, t_ms, title, source, ...}`. Corrigir um capítulo torna-o manual."),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "Já existe um capítulo nesse instante."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn patch(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, chapter_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<PatchChapterReq>,
) -> Result<Json<Chapter>, ApiError> {
    let a = access(&state, id, auth.user_id).await?;
    a.require_manage()?;
    by_id(&state, id, chapter_id).await?;
    if let Some(t) = req.t_ms {
        check_t_ms(t, a.duration_ms)?;
    }
    let title = req.title.as_deref().map(clean_title).transpose()?;
    // `manual` só quando algo mudou de facto: um PATCH sem campos (resave da
    // UI, retry) não pode converter em silêncio um capítulo automático,
    // tirando-o para sempre da geração futura (R232).
    let ch: Chapter = sqlx::query_as(&format!(
        "UPDATE recording_chapters SET t_ms = COALESCE($3, t_ms), title = COALESCE($4, title),
                source = CASE WHEN $3 IS NOT NULL OR $4 IS NOT NULL
                              THEN 'manual' ELSE source END
         WHERE recording_id = $1 AND id = $2 RETURNING {CHAPTER_COLS}"
    ))
    .bind(id)
    .bind(chapter_id)
    .bind(req.t_ms)
    .bind(title)
    .fetch_one(&state.db)
    .await
    .map_err(unique_to_conflict)?;
    Ok(Json(ch))
}

// ---------- geração automática ----------

fn fmt_secs(ms: i64) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

/// Linhas `[hh:mm:ss] texto` que cabem no orçamento do prompt.
///
/// Uma transcrição longa não se corta no fim (os últimos capítulos
/// desapareciam): juntam-se segmentos seguidos em blocos maiores até caber.
pub(crate) fn transcript_digest(segments: &[Segment], budget: usize) -> String {
    let mut window_ms: i64 = 0;
    loop {
        let mut lines: Vec<String> = Vec::new();
        let mut cur: Option<(i64, i64, String)> = None;
        for s in segments {
            match &mut cur {
                Some((start, _end, text)) if s.start_ms - *start < window_ms => {
                    text.push(' ');
                    text.push_str(&s.text);
                }
                _ => {
                    if let Some((start, _, text)) = cur.take() {
                        lines.push(format!("[{}] {text}", fmt_secs(start)));
                    }
                    cur = Some((s.start_ms, s.end_ms, s.text.clone()));
                }
            }
        }
        if let Some((start, _, text)) = cur {
            lines.push(format!("[{}] {text}", fmt_secs(start)));
        }
        let out = lines.join("\n");
        if out.chars().count() <= budget || window_ms >= 600_000 {
            // Com blocos de 10 min e ainda grande demais, corta-se cada bloco.
            return if out.chars().count() <= budget {
                out
            } else {
                let per = budget / lines.len().max(1);
                lines
                    .iter()
                    .map(|l| l.chars().take(per.max(40)).collect::<String>())
                    .collect::<Vec<_>>()
                    .join("\n")
                    .chars()
                    .take(budget)
                    .collect()
            };
        }
        window_ms = if window_ms == 0 {
            30_000
        } else {
            window_ms * 2
        };
    }
}

#[derive(Deserialize)]
struct RawChapter {
    #[serde(alias = "t", alias = "time", alias = "start_seconds")]
    start: serde_json::Value,
    title: String,
}

/// `12`, `"12"`, `"00:12"`, `"01:02:03"` → milissegundos.
fn raw_time_ms(v: &serde_json::Value) -> Option<i64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64().map(|f| (f * 1000.0).round() as i64),
        serde_json::Value::String(s) => {
            let s = s.trim().trim_matches(|c| c == '[' || c == ']');
            if let Ok(f) = s.parse::<f64>() {
                return Some((f * 1000.0).round() as i64);
            }
            let parts: Vec<f64> = s
                .split(':')
                .map(|p| p.parse::<f64>())
                .collect::<Result<_, _>>()
                .ok()?;
            let secs = parts.iter().fold(0.0, |acc, p| acc * 60.0 + p);
            (parts.len() <= 3).then_some((secs * 1000.0).round() as i64)
        }
        _ => None,
    }
}

/// Lê a resposta do LLM: o primeiro array JSON, com `start` (segundos ou
/// `hh:mm:ss`) e `title`. O que não se aproveita sai, sem inventar nada.
pub(crate) fn parse_llm_chapters(answer: &str, duration_ms: i64) -> Vec<(i64, String)> {
    let (Some(a), Some(b)) = (answer.find('['), answer.rfind(']')) else {
        return vec![];
    };
    if b <= a {
        return vec![];
    }
    let raw: Vec<serde_json::Value> = serde_json::from_str(&answer[a..=b]).unwrap_or_default();
    let mut out: Vec<(i64, String)> = raw
        .into_iter()
        .filter_map(|v| serde_json::from_value::<RawChapter>(v).ok())
        .filter_map(|c| {
            let t = raw_time_ms(&c.start)?;
            let title: String = c
                .title
                .trim()
                .chars()
                .take(MAX_CHAPTER_TITLE_CHARS)
                .collect();
            (t >= 0 && t <= duration_ms && !title.is_empty()).then_some((t, title))
        })
        .collect();
    out.sort_by_key(|c| c.0);
    out.dedup_by_key(|c| c.0 / 1000);
    out.truncate(MAX_AUTO_CHAPTERS);
    out
}

pub(crate) enum GenerateOutcome {
    Generated(usize),
    /// Não há transcrição com segmentos: não há de onde gerar.
    NoTranscript,
    /// Sem LLM configurado, ou não respondeu.
    LlmUnavailable,
    /// O modelo respondeu, mas sem um único capítulo utilizável.
    BadResponse,
    /// Já há uma geração a decorrer para esta gravação.
    AlreadyRunning,
}

/// O estado da última geração de capítulos de uma gravação.
#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[schema(as = RecordingChapterGeneration)]
pub struct ChapterGeneration {
    pub recording_id: Uuid,
    /// `idle` (nunca pedida) | `running` | `succeeded` | `failed`.
    pub status: String,
    /// Só com `failed`: `ai.unavailable`, `ai.bad_response`, `ai.interrupted`
    /// ou `internal`.
    pub error_code: Option<String>,
    pub error: Option<String>,
    /// Capítulos automáticos gravados. Só com `succeeded`.
    pub chapter_count: Option<i32>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// A linha, com um `running` sem sinal há tempo demais lido como interrompido.
async fn load_generation(
    state: &AppState,
    recording_id: Uuid,
) -> Result<ChapterGeneration, ApiError> {
    let row: Option<ChapterGeneration> = sqlx::query_as(
        "SELECT recording_id,
                CASE WHEN status = 'running' AND updated_at < now() - make_interval(secs => $2)
                     THEN 'failed' ELSE status END AS status,
                CASE WHEN status = 'running' AND updated_at < now() - make_interval(secs => $2)
                     THEN 'ai.interrupted' ELSE error_code END AS error_code,
                CASE WHEN status = 'running' AND updated_at < now() - make_interval(secs => $2)
                     THEN 'a geração foi interrompida antes de acabar; pode pedir-se outra vez'
                     ELSE error END AS error,
                chapter_count, started_at, finished_at
           FROM recording_chapter_generations WHERE recording_id = $1",
    )
    .bind(recording_id)
    .bind(stale_after_secs())
    .fetch_optional(&state.db)
    .await?;
    Ok(row.unwrap_or(ChapterGeneration {
        recording_id,
        status: "idle".into(),
        error_code: None,
        error: None,
        chapter_count: None,
        started_at: None,
        finished_at: None,
    }))
}

/// `GET /api/recordings/{id}/chapters/generation` — o estado da última
/// geração de capítulos. Só quem gere a gravação.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/chapters/generation", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path, description = "Gravação.")),
    responses(
        (status = 200, body = ChapterGeneration),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn generation(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<ChapterGeneration>, ApiError> {
    let a = access(&state, id, auth.user_id).await?;
    a.require_manage()?;
    load_generation(&state, id).await.map(Json)
}

/// Fecha a linha do estado. Só fecha uma geração que ainda está `running`.
async fn finish_generation(state: &AppState, rec_id: Uuid, result: Result<usize, (&str, &str)>) {
    let r = match result {
        Ok(n) => {
            sqlx::query(
                "UPDATE recording_chapter_generations
                    SET status = 'succeeded', chapter_count = $2,
                        updated_at = now(), finished_at = now()
                  WHERE recording_id = $1 AND status = 'running'",
            )
            .bind(rec_id)
            .bind(n as i32)
            .execute(&state.db)
            .await
        }
        Err((code, msg)) => {
            sqlx::query(
                "UPDATE recording_chapter_generations
                    SET status = 'failed', error_code = $2, error = $3,
                        updated_at = now(), finished_at = now()
                  WHERE recording_id = $1 AND status = 'running'",
            )
            .bind(rec_id)
            .bind(code)
            .bind(msg)
            .execute(&state.db)
            .await
        }
    };
    if let Err(e) = r {
        tracing::error!(recording = %rec_id, error = %e, "capítulos: estado não gravado");
    }
}

/// Gera os capítulos automáticos de uma gravação e substitui os anteriores
/// automáticos (os manuais ficam). `requested_by` é quem pediu (`None` na
/// varredura). O resultado fica também em `recording_chapter_generations`.
pub(crate) async fn generate_for(
    state: &AppState,
    rec_id: Uuid,
    requested_by: Option<Uuid>,
) -> Result<GenerateOutcome, ApiError> {
    let t = load_transcript(state, rec_id).await?;
    if t.segments.is_empty() {
        return Ok(GenerateOutcome::NoTranscript);
    }
    if state.config.ollama_url.is_none() {
        return Ok(GenerateOutcome::LlmUnavailable);
    }
    // Condicional: dois pedidos ao mesmo tempo não arrancam duas gerações.
    let accepted: Option<(Uuid,)> = sqlx::query_as(
        "INSERT INTO recording_chapter_generations (recording_id, status, requested_by)
         VALUES ($1, 'running', $2)
         ON CONFLICT (recording_id) DO UPDATE SET
             status = 'running', error_code = NULL, error = NULL, chapter_count = NULL,
             requested_by = EXCLUDED.requested_by, started_at = now(), updated_at = now(),
             finished_at = NULL
          WHERE recording_chapter_generations.status <> 'running'
             OR recording_chapter_generations.updated_at < now() - make_interval(secs => $3)
         RETURNING recording_id",
    )
    .bind(rec_id)
    .bind(requested_by)
    .bind(stale_after_secs())
    .fetch_optional(&state.db)
    .await?;
    if accepted.is_none() {
        return Ok(GenerateOutcome::AlreadyRunning);
    }
    let outcome = run_generation(state, rec_id, t.segments).await;
    let result = match &outcome {
        Ok(GenerateOutcome::Generated(n)) => Ok(*n),
        Ok(GenerateOutcome::BadResponse) => Err((
            "ai.bad_response",
            "o modelo devolveu uma resposta sem capítulos utilizáveis",
        )),
        Ok(_) => Err(("ai.unavailable", "IA local não configurada ou sem resposta")),
        Err(_) => Err(("internal", "erro interno ao gerar os capítulos")),
    };
    finish_generation(state, rec_id, result).await;
    outcome
}

/// O trabalho em si: prompt, chamada, leitura da resposta e só depois a escrita.
async fn run_generation(
    state: &AppState,
    rec_id: Uuid,
    segments: Vec<Segment>,
) -> Result<GenerateOutcome, ApiError> {
    let duration: Option<i64> =
        sqlx::query_scalar("SELECT duration_ms FROM recordings WHERE id = $1")
            .bind(rec_id)
            .fetch_one(&state.db)
            .await?;
    let end = duration.unwrap_or_else(|| segments.iter().map(|s| s.end_ms).max().unwrap_or(0));
    let digest = transcript_digest(&segments, PROMPT_BUDGET_CHARS);
    let prompt = format!(
        "You split a recorded session into chapters. Below is its transcript; each line \
         starts with the time [hh:mm:ss] when that passage begins.\n\
         Return ONLY a JSON array, no prose, like \
         [{{\"start\": \"00:00:00\", \"title\": \"…\"}}]. Rules: 3 to 12 chapters; the \
         first starts at 00:00:00; every start MUST be one of the times shown in the \
         transcript; titles are short (max 8 words) and in the SAME language as the \
         transcript; never invent topics that are not in the text.\n\nTranscript:\n{digest}"
    );
    let Some(answer) = crate::ai::generate(
        state,
        &state.config.ollama_model_summary,
        prompt,
        GENERATE_TIMEOUT,
    )
    .await
    else {
        return Ok(GenerateOutcome::LlmUnavailable);
    };
    let chapters = parse_llm_chapters(&answer, end);
    // Sem um único capítulo utilizável não se toca em nada: os automáticos que
    // havia ficam e a gravação NÃO fica marcada como gerada.
    if chapters.is_empty() {
        return Ok(GenerateOutcome::BadResponse);
    }
    let manual: Vec<(i64,)> = sqlx::query_as(
        "SELECT t_ms FROM recording_chapters WHERE recording_id = $1 AND source = 'manual'",
    )
    .bind(rec_id)
    .fetch_all(&state.db)
    .await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM recording_chapters WHERE recording_id = $1 AND source = 'auto'")
        .bind(rec_id)
        .execute(&mut *tx)
        .await?;
    let mut n = 0;
    for (t_ms, title) in &chapters {
        if manual.iter().any(|(m,)| m == t_ms) {
            continue;
        }
        sqlx::query(
            "INSERT INTO recording_chapters (recording_id, t_ms, title, source)
             VALUES ($1, $2, $3, 'auto') ON CONFLICT (recording_id, t_ms) DO NOTHING",
        )
        .bind(rec_id)
        .bind(t_ms)
        .bind(title)
        .execute(&mut *tx)
        .await?;
        n += 1;
    }
    sqlx::query("UPDATE recordings SET chapters_generated_at = now() WHERE id = $1")
        .bind(rec_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(GenerateOutcome::Generated(n))
}

/// `POST /api/recordings/{id}/chapters/generate` — gera (ou volta a gerar) os
/// capítulos automáticos e devolve a lista completa.
///
/// Síncrono com tecto de 300 s: é o LLM local, e o resultado é o que o ecrã
/// mostra a seguir. Sem transcrição, ou com outra geração a decorrer → `409`;
/// sem LLM, ou com uma resposta sem capítulos utilizáveis → `503`. O estado da
/// última geração lê-se em `GET …/chapters/generation`.
#[utoipa::path(
    post, path = "/api/recordings/{recording_id}/chapters/generate", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path, description = "Gravação.")),
    responses(
        (status = 200, body = Vec<serde_json::Value>, description = "Os capítulos depois de gerar; os manuais nunca se apagam."),
        (status = 409, body = crate::openapi::ErrorBody, description = "Sem transcrição, ou já há uma geração a decorrer."),
        (status = 503, body = crate::openapi::ErrorBody, description = "IA local indisponível, ou resposta do modelo sem capítulos utilizáveis."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn generate(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Chapter>>, ApiError> {
    let a = access(&state, id, auth.user_id).await?;
    a.require_manage()?;
    match generate_for(&state, id, Some(auth.user_id)).await? {
        GenerateOutcome::Generated(_) => list_of(&state, id).await.map(Json),
        GenerateOutcome::AlreadyRunning => Err(ApiError::Conflict(
            "já há uma geração de capítulos a decorrer para esta gravação".into(),
        )),
        GenerateOutcome::BadResponse => Err(ApiError::ServiceUnavailable(
            "o modelo devolveu uma resposta sem capítulos utilizáveis".into(),
        )),
        GenerateOutcome::NoTranscript => Err(ApiError::Conflict(
            "a gravação ainda não tem transcrição com tempos".into(),
        )),
        GenerateOutcome::LlmUnavailable => Err(ApiError::ServiceUnavailable(
            "geração de capítulos indisponível (LLM local não configurado ou sem resposta)".into(),
        )),
    }
}

/// Varredura: capítulos automáticos para as gravações transcritas que ainda
/// não os têm. Poucas por volta — é o mesmo LLM que resume as actas.
pub async fn auto_chapters_sweep(state: &Arc<AppState>) {
    if state.config.ollama_url.is_none() {
        return;
    }
    let pending: Vec<(Uuid,)> = match sqlx::query_as(
        "SELECT id FROM recordings
         WHERE transcribed_at IS NOT NULL AND transcript_error IS NULL
           AND chapters_generated_at IS NULL AND status = 'ready'
           AND jsonb_array_length(transcript_segments) > 0
           AND NOT EXISTS (SELECT 1 FROM recording_chapter_generations g
                            WHERE g.recording_id = recordings.id
                              AND g.status = 'failed' AND g.error_code = 'ai.bad_response')
         ORDER BY transcribed_at LIMIT 2",
    )
    .fetch_all(&state.db)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "auto chapters: consulta falhou");
            return;
        }
    };
    // (O `NOT EXISTS` acima: uma resposta inutilizável não se repete sozinha a
    // cada volta — a gravação ficava à frente da fila para sempre. Volta-se a
    // pedir à mão.)
    for (id,) in pending {
        match generate_for(state, id, None).await {
            Ok(GenerateOutcome::Generated(n)) => {
                tracing::info!(recording = %id, chapters = n, "capítulos automáticos gerados")
            }
            Ok(GenerateOutcome::LlmUnavailable) => {
                tracing::warn!(recording = %id, "capítulos: LLM sem resposta — fica para a próxima volta");
                return;
            }
            Ok(GenerateOutcome::BadResponse) => {
                tracing::warn!(recording = %id, "capítulos: resposta do modelo sem capítulos utilizáveis")
            }
            Ok(GenerateOutcome::NoTranscript | GenerateOutcome::AlreadyRunning) => {}
            Err(e) => tracing::warn!(recording = %id, error = %e, "capítulos: falhou"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: i64, text: &str) -> Segment {
        Segment {
            start_ms: start,
            end_ms: start + 4000,
            text: text.into(),
            confidence: None,
        }
    }

    #[test]
    fn resposta_do_llm_com_prosa_a_volta() {
        let a = r#"Aqui estão os capítulos:
        [{"start": "00:00:00", "title": "Abertura"},
         {"start": 95, "title": "  Rede de Luanda  "},
         {"t": "00:05:10", "title": "Perguntas"},
         {"start": "99:00:00", "title": "Depois do fim"},
         {"start": "00:05:10.4", "title": "Repetido no mesmo segundo"},
         {"start": 30, "title": ""},
         {"title": "Sem instante"}]
        Espero que ajude."#;
        let c = parse_llm_chapters(a, 600_000);
        assert_eq!(
            c,
            vec![
                (0, "Abertura".to_string()),
                (95_000, "Rede de Luanda".to_string()),
                (310_000, "Perguntas".to_string()),
            ]
        );
    }

    #[test]
    fn resposta_sem_json_nao_inventa_capitulos() {
        assert!(parse_llm_chapters("Não consegui.", 1000).is_empty());
        assert!(parse_llm_chapters("] [", 1000).is_empty());
    }

    #[test]
    fn digest_curto_fica_linha_a_linha() {
        let s = vec![seg(0, "olá"), seg(65_000, "segundo tema")];
        assert_eq!(
            transcript_digest(&s, 1000),
            "[00:00:00] olá\n[00:01:05] segundo tema"
        );
    }

    #[test]
    fn digest_longo_junta_blocos_e_cobre_ate_ao_fim() {
        let s: Vec<Segment> = (0..2000)
            .map(|i| seg(i * 5000, "uma frase razoavelmente comprida da transcrição"))
            .collect();
        let d = transcript_digest(&s, 24_000);
        assert!(d.chars().count() <= 24_000, "{}", d.chars().count());
        // O último bloco (perto de 2h46) tem de lá estar: cortar no fim perdia capítulos.
        assert!(d.contains("[02:4"), "o fim da sessão desapareceu do prompt");
    }
}

/// Documentação OpenAPI da edição e geração de capítulos (`openapi.rs` junta-a).
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(patch, generate, generation),
    components(schemas(PatchChapterReq, ChapterGeneration))
)]
pub struct ApiDoc;
