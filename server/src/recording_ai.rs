//! Capítulos e legendas de uma gravação gerados a partir da transcrição, com o
//! LLM local (Ollama do operador, `ai::Ollama`). Trabalho assíncrono: o pedido
//! é aceite (`202`) e o estado lê-se a seguir.
//!
//! - `POST …/chapters/generate` → `202` + `GET …/chapters/generation`. Os
//!   capítulos ficam `source = auto` e são gravados pelo mesmo serviço dos
//!   manuais (`recordings::replace_auto_chapters`); um manual nunca é tocado.
//!   **Uma resposta sem capítulos utilizáveis é falha** (`ai.bad_response`) e
//!   não marca a gravação como tendo capítulos gerados (B12).
//! - `POST …/captions/generate` → na língua da transcrição, o VTT sai já dos
//!   segmentos (`201`/`200`, rascunho); noutra língua, cada segmento é
//!   traduzido em segundo plano (`202`, `status = generating` com
//!   `progress_pct`, lido em `GET …/captions/{lang}`). Enquanto gera ou se
//!   falhou, o VTT dá `409` (`recording_captions::vtt`).
//!
//! Acesso: só quem gere a gravação (`recordings::managed_item`: `404` a quem não
//! a vê, `403 recording.not_manager` a quem só a vê). Capacidade proposta
//! (ADR-0008): `recordings.edit`.
//!
//! O modelo conta para o tecto por organização do Estúdio
//! (`AI_STUDIO_CONCURRENCY_PER_ORG`, `ai_assist::OrgSlots`): a vaga é tomada ao
//! aceitar o pedido e libertada quando o trabalho acaba.
//!
//! **Interrompido.** Um trabalho que deixou de dar sinal há mais do que o tecto
//! de uma chamada ao modelo + 60 s (o pod morreu a meio) é mostrado como
//! falhado (`ai.interrupted`) e pode voltar a pedir-se, sem varredor.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::DomainError;
use delonix_meet_domain::content::{ai_assist, caption as caption_rules, transcription::Segment};
use delonix_meet_domain::integration::llm::{self, LlmFailure};
use serde::{Deserialize, Serialize};
use tokio::sync::OwnedSemaphorePermit;
use uuid::Uuid;

use crate::{
    ai::Ollama,
    ai_assist::{busy, STATUS_TIMEOUT},
    auth::AuthUser,
    error::ApiError,
    recording_captions::{CaptionMeta, InsertedMeta, MAX_CAPTIONS, META_COLS},
    recording_meta::load_segments,
    recordings::{managed_item, replace_auto_chapters, ItemRow},
    AppState,
};

/// Folga sobre o tecto de UMA chamada ao modelo antes de dar um trabalho
/// `running`/`generating` como interrompido.
const STALE_MARGIN_SECS: u64 = 60;
/// Um trabalho de legendas toca no `updated_at` pelo menos a este ritmo
/// enquanto traduz (e sempre que o progresso sobe 5 pontos).
const HEARTBEAT: Duration = Duration::from_secs(10);

fn stale_after_secs(state: &AppState) -> i64 {
    (state.config.ollama_timeout_secs + STALE_MARGIN_SECS) as i64
}

fn no_transcript() -> ApiError {
    DomainError::conflict(
        "recording.no_transcript",
        "a gravação ainda não tem transcrição com tempos",
    )
    .into()
}

/// A vaga do modelo para a organização da gravação (a do dono, vista por quem
/// pede — é quem gere). Sem organização (edição pessoal), a do próprio dono.
fn slot_for(state: &AppState, rec: &ItemRow) -> Option<OwnedSemaphorePermit> {
    state
        .ai_slots
        .try_acquire(rec.uploader_org_id.unwrap_or(rec.uploader_id))
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(generate_chapters, chapter_generation, generate_caption),
    components(schemas(ChapterGeneration, GenerateCaptionReq))
)]
pub struct ApiDoc;

// ---------------------------------------------------------------------------
//  Capítulos
// ---------------------------------------------------------------------------

/// O estado da última geração de capítulos de uma gravação.
#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[schema(as = RecordingChapterGeneration)]
pub struct ChapterGeneration {
    pub recording_id: Uuid,
    /// `idle` (nunca pedida) | `running` | `succeeded` | `failed`.
    pub status: String,
    /// Só com `failed`: `ai.bad_response`, `ai.timeout`, `ai.model_missing`,
    /// `ai.unreachable`, `ai.interrupted`, `internal`, …
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
    .bind(stale_after_secs(state) as f64)
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

/// Estado da geração de capítulos. Só quem gere a gravação.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/chapters/generation", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path)),
    responses(
        (status = 200, body = ChapterGeneration),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`recording.not_manager`"),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn chapter_generation(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<ChapterGeneration>, ApiError> {
    managed_item(&state, id, auth.user_id).await?;
    Ok(Json(load_generation(&state, id).await?))
}

/// Gera (ou volta a gerar) os capítulos automáticos a partir da transcrição.
/// Os manuais ficam. Assíncrono: `202` e o estado em `…/chapters/generation`.
#[utoipa::path(
    post, path = "/api/recordings/{recording_id}/chapters/generate", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path)),
    responses(
        (status = 202, body = ChapterGeneration, headers(("Location" = String)), description = "Aceite: `status = running`. O resultado lê-se em `Location`."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`recording.not_manager`"),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "`recording.no_transcript` / `recording.chapter_generation_running`"),
        (status = 429, body = crate::openapi::ErrorBody, headers(("Retry-After" = u64)), description = "`ai.busy`"),
        (status = 503, body = crate::openapi::ErrorBody, description = "`ai.not_configured` / `ai.url_rejected` / `ai.unreachable` / `ai.timeout` / `ai.model_missing` / `ai.upstream_error` / `ai.bad_response`"),
    )
)]
pub async fn generate_chapters(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let rec = managed_item(&state, id, auth.user_id).await?;
    let segments = load_segments(&state, id).await?;
    if segments.is_empty() {
        return Err(no_transcript());
    }
    if load_generation(&state, id).await?.status == "running" {
        return Err(running_chapters());
    }
    let model = state.config.ollama_model_studio.clone();
    // Antes de aceitar: um modelo que não está lá é um `503` honesto agora, não
    // um `202` que falha daqui a pouco.
    Ollama::of(&state)
        .ensure_model(&model, STATUS_TIMEOUT)
        .await?;
    let Some(slot) = slot_for(&state, &rec) else {
        return Ok(busy());
    };
    // Condicional: dois pedidos ao mesmo tempo não arrancam dois trabalhos.
    let accepted: Option<ChapterGeneration> = sqlx::query_as(
        "INSERT INTO recording_chapter_generations (recording_id, status, requested_by)
         VALUES ($1, 'running', $2)
         ON CONFLICT (recording_id) DO UPDATE SET
             status = 'running', error_code = NULL, error = NULL, chapter_count = NULL,
             requested_by = EXCLUDED.requested_by, started_at = now(), updated_at = now(),
             finished_at = NULL
          WHERE recording_chapter_generations.status <> 'running'
             OR recording_chapter_generations.updated_at < now() - make_interval(secs => $3)
         RETURNING recording_id, status, error_code, error, chapter_count, started_at, finished_at",
    )
    .bind(id)
    .bind(auth.user_id)
    .bind(stale_after_secs(&state) as f64)
    .fetch_optional(&state.db)
    .await?;
    let Some(accepted) = accepted else {
        return Err(running_chapters());
    };
    crate::audit::log(
        &state.db,
        None,
        auth.user_id,
        "recording.chapters_generation_requested",
        &id.to_string(),
    )
    .await;
    let end_ms = rec
        .duration_ms
        .unwrap_or_else(|| segments.iter().map(|s| s.end_ms).max().unwrap_or(0));
    tokio::spawn(run_chapters(
        state.clone(),
        ChaptersJob {
            recording_id: id,
            duration_ms: rec.duration_ms,
            end_ms,
            segments,
            model,
            actor: auth.user_id,
        },
        slot,
    ));
    Ok((
        StatusCode::ACCEPTED,
        [(
            header::LOCATION,
            format!("/api/recordings/{id}/chapters/generation"),
        )],
        Json(accepted),
    )
        .into_response())
}

fn running_chapters() -> ApiError {
    DomainError::conflict(
        "recording.chapter_generation_running",
        "já há uma geração de capítulos a decorrer para esta gravação",
    )
    .into()
}

struct ChaptersJob {
    recording_id: Uuid,
    duration_ms: Option<i64>,
    end_ms: i64,
    segments: Vec<Segment>,
    model: String,
    actor: Uuid,
}

/// O trabalho: prompt, chamada, leitura estrita da resposta, e só depois a
/// escrita. Qualquer falha fica na linha do estado com o código.
async fn run_chapters(state: Arc<AppState>, job: ChaptersJob, slot: OwnedSemaphorePermit) {
    let id = job.recording_id;
    let timeout = Duration::from_secs(state.config.ollama_timeout_secs);
    let proposed = Ollama::of(&state)
        .generate(
            &job.model,
            &ai_assist::chapters_prompt(&job.segments),
            timeout,
            true,
        )
        .await
        .and_then(|a| ai_assist::parse_generated_chapters(&a, job.end_ms));
    let outcome = match proposed {
        Err(e) => Err((e.code().to_string(), e.message())),
        Ok(chapters) => {
            let pairs: Vec<(i64, String)> =
                chapters.into_iter().map(|c| (c.t_ms, c.title)).collect();
            replace_auto_chapters(&state, id, job.duration_ms, &pairs, job.actor)
                .await
                .map_err(|e| {
                    tracing::error!(recording = %id, error = %e, "capítulos: não gravou");
                    (
                        "internal".to_string(),
                        "erro interno ao gravar os capítulos".to_string(),
                    )
                })
        }
    };
    // A vaga do modelo volta ANTES de o estado dizer que acabou: quem lê
    // «terminado» pode pedir outra vez sem levar um `429`.
    drop(slot);
    let r = match &outcome {
        Ok(n) => {
            tracing::info!(recording = %id, chapters = n, "capítulos automáticos gerados");
            sqlx::query(
                "UPDATE recording_chapter_generations
                    SET status = 'succeeded', chapter_count = $2, updated_at = now(), finished_at = now()
                  WHERE recording_id = $1 AND status = 'running'",
            )
            .bind(id)
            .bind(*n as i32)
            .execute(&state.db)
            .await
        }
        Err((code, msg)) => {
            tracing::warn!(recording = %id, code = %code, "capítulos automáticos falharam");
            sqlx::query(
                "UPDATE recording_chapter_generations
                    SET status = 'failed', error_code = $2, error = $3, updated_at = now(), finished_at = now()
                  WHERE recording_id = $1 AND status = 'running'",
            )
            .bind(id)
            .bind(code)
            .bind(msg)
            .execute(&state.db)
            .await
        }
    };
    if let Err(e) = r {
        tracing::error!(recording = %id, error = %e, "capítulos: estado não gravado");
    }
}

// ---------------------------------------------------------------------------
//  Legendas
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[schema(as = RecordingCaptionGenerateReq)]
pub struct GenerateCaptionReq {
    /// Língua pedida (BCP 47 curto). Omitida: a da transcrição.
    #[serde(default)]
    pub lang: Option<String>,
    /// Substituir uma legenda que já existe nessa língua (enviada, rascunho ou
    /// publicada). Sem isto, `409 recording.caption_exists`: gerar não apaga o
    /// trabalho de uma pessoa por engano. Uma legenda `failed` substitui-se
    /// sempre.
    #[serde(default)]
    pub replace: bool,
}

/// A legenda que está na língua, se está, e se a sua geração está viva.
async fn existing_caption(
    state: &AppState,
    recording_id: Uuid,
    lang: &str,
) -> Result<Option<(String, bool)>, ApiError> {
    Ok(sqlx::query_as(
        "SELECT status,
                (status = 'generating' AND updated_at >= now() - make_interval(secs => $3)) AS live
           FROM recording_captions WHERE recording_id = $1 AND lang = $2",
    )
    .bind(recording_id)
    .bind(lang)
    .bind(stale_after_secs(state) as f64)
    .fetch_optional(&state.db)
    .await?)
}

/// Escreve a legenda gerada (ou o seu início) respeitando o tecto de línguas
/// e sem atropelar uma geração viva. `None` = recusado (tecto ou corrida).
async fn upsert_generated(
    state: &AppState,
    recording_id: Uuid,
    lang: &str,
    vtt: &str,
    source: &str,
    status: &str,
    by: Uuid,
) -> Result<Option<InsertedMeta>, ApiError> {
    Ok(sqlx::query_as::<_, InsertedMeta>(&format!(
        "INSERT INTO recording_captions (recording_id, lang, vtt, source, status, progress_pct, updated_by)
         SELECT $1, $2, $3, $4, $5, CASE WHEN $5 = 'generating' THEN 0 END, $6
          WHERE EXISTS (SELECT 1 FROM recording_captions WHERE recording_id = $1 AND lang = $2)
             OR (SELECT COUNT(*) FROM recording_captions WHERE recording_id = $1) < $7
         ON CONFLICT (recording_id, lang) DO UPDATE SET
             vtt = EXCLUDED.vtt, source = EXCLUDED.source, status = EXCLUDED.status,
             progress_pct = EXCLUDED.progress_pct, error = NULL,
             updated_by = EXCLUDED.updated_by, updated_at = now(), published_at = NULL
          WHERE recording_captions.status <> 'generating'
             OR recording_captions.updated_at < now() - make_interval(secs => $8)
         RETURNING (xmax = 0) AS inserted, {META_COLS}"
    ))
    .bind(recording_id)
    .bind(lang)
    .bind(vtt)
    .bind(source)
    .bind(status)
    .bind(by)
    .bind(MAX_CAPTIONS)
    .bind(stale_after_secs(state) as f64)
    .fetch_optional(&state.db)
    .await?)
}

fn caption_running() -> ApiError {
    DomainError::conflict(
        "recording.caption_generation_running",
        "já há uma geração a decorrer para esta língua",
    )
    .into()
}

fn too_many_captions() -> ApiError {
    DomainError::precondition(
        "recording.too_many_captions",
        format!("uma gravação tem no máximo {MAX_CAPTIONS} legendas"),
    )
    .into()
}

/// Gera a legenda de uma língua a partir da transcrição. Só quem gere a gravação.
#[utoipa::path(
    post, path = "/api/recordings/{recording_id}/captions/generate", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path)),
    request_body(content = GenerateCaptionReq, description = "Pode ir vazio: a língua da transcrição."),
    responses(
        (status = 201, body = CaptionMeta, headers(("Location" = String)), description = "Na língua da transcrição: criada já, rascunho (`source = transcript`)."),
        (status = 200, body = CaptionMeta, description = "Na língua da transcrição, substituindo (`replace: true`)."),
        (status = 202, body = CaptionMeta, headers(("Location" = String)), description = "Noutra língua: `status = generating`, `source = translation`; o progresso lê-se em `Location`."),
        (status = 400, body = crate::openapi::ErrorBody, description = "`recording.invalid_generate_body` / `recording.invalid_caption_lang` / `recording.caption_lang_required` / `ai.unsupported_language` / `recording.caption_too_large`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`recording.not_manager`"),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "`recording.no_transcript` / `recording.caption_exists` / `recording.caption_generation_running`"),
        (status = 422, body = crate::openapi::ErrorBody, description = "`recording.too_many_captions`"),
        (status = 429, body = crate::openapi::ErrorBody, headers(("Retry-After" = u64)), description = "`ai.busy`"),
        (status = 503, body = crate::openapi::ErrorBody, description = "Só para tradução: `ai.not_configured` / `ai.url_rejected` / `ai.unreachable` / `ai.timeout` / `ai.model_missing` / `ai.upstream_error` / `ai.bad_response`"),
    )
)]
pub async fn generate_caption(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let rec = managed_item(&state, id, auth.user_id).await?;
    let req: GenerateCaptionReq = if body.iter().all(u8::is_ascii_whitespace) {
        GenerateCaptionReq::default()
    } else {
        serde_json::from_slice(&body).map_err(|e| {
            DomainError::invalid(
                "recording.invalid_generate_body",
                format!("corpo JSON inválido: {e}"),
            )
        })?
    };
    let segments = load_segments(&state, id).await?;
    if segments.is_empty() {
        return Err(no_transcript());
    }
    let source_lang = rec
        .transcript_language
        .as_deref()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string);
    let lang = match req.lang.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
        Some(l) => l.to_string(),
        None => source_lang.clone().ok_or_else(|| {
            DomainError::invalid(
                "recording.caption_lang_required",
                "a transcrição não diz a língua: indica `lang`",
            )
            .with_field("lang", "BCP 47 curto")
        })?,
    };
    caption_rules::check_lang(&lang)?;
    let same_language = source_lang
        .as_deref()
        .is_some_and(|s| llm::primary_subtag(s) == llm::primary_subtag(&lang));
    if !same_language && llm::target_name(&lang).is_none() {
        return Err(llm::unsupported_target(&lang).into());
    }
    match existing_caption(&state, id, &lang).await? {
        Some((_, true)) => return Err(caption_running()),
        Some((status, false)) if status != "failed" && status != "generating" && !req.replace => {
            return Err(DomainError::conflict(
                "recording.caption_exists",
                format!(
                    "já há uma legenda em «{lang}»; para a substituir pela gerada, pede com `replace: true`"
                ),
            )
            .into())
        }
        _ => {}
    }
    let location = format!("/api/recordings/{id}/captions/{lang}");

    if same_language {
        let vtt = caption_rules::cues_to_vtt(&caption_rules::segments_to_cues(&segments));
        caption_rules::parse_vtt(&vtt)?;
        let Some(row) =
            upsert_generated(&state, id, &lang, &vtt, "transcript", "draft", auth.user_id).await?
        else {
            return Err(refused(&state, id, &lang).await);
        };
        audit_caption(&state, auth.user_id, id).await;
        return Ok(if row.inserted {
            (
                StatusCode::CREATED,
                [(header::LOCATION, location)],
                Json(row.meta),
            )
                .into_response()
        } else {
            Json(row.meta).into_response()
        });
    }

    let model = state.config.ollama_model_translate.clone();
    Ollama::of(&state)
        .ensure_model(&model, STATUS_TIMEOUT)
        .await?;
    let Some(slot) = slot_for(&state, &rec) else {
        return Ok(busy());
    };
    let Some(row) = upsert_generated(
        &state,
        id,
        &lang,
        "",
        "translation",
        "generating",
        auth.user_id,
    )
    .await?
    else {
        return Err(refused(&state, id, &lang).await);
    };
    audit_caption(&state, auth.user_id, id).await;
    tokio::spawn(run_translation(
        state.clone(),
        TranslationJob {
            recording_id: id,
            lang: lang.clone(),
            segments,
            model,
        },
        slot,
    ));
    Ok((
        StatusCode::ACCEPTED,
        [(header::LOCATION, location)],
        Json(row.meta),
    )
        .into_response())
}

/// Porque é que o `upsert` não escreveu: uma geração viva chegou primeiro, ou
/// o tecto de línguas.
async fn refused(state: &AppState, id: Uuid, lang: &str) -> ApiError {
    match existing_caption(state, id, lang).await {
        Ok(Some(_)) => caption_running(),
        Ok(None) => too_many_captions(),
        Err(e) => e,
    }
}

async fn audit_caption(state: &AppState, actor: Uuid, id: Uuid) {
    crate::audit::log(
        &state.db,
        None,
        actor,
        "recording.caption_generation_requested",
        &id.to_string(),
    )
    .await;
}

struct TranslationJob {
    recording_id: Uuid,
    lang: String,
    segments: Vec<Segment>,
    model: String,
}

/// Traduz os segmentos um a um e guarda o VTT como rascunho.
///
/// Um segmento que o modelo não traduz faz FALHAR a legenda inteira: uma
/// legenda com buracos, ou com frases deixadas na língua original, é pior do
/// que dizer que falhou. Se a legenda mudou entretanto (alguém a enviou ou a
/// apagou), as escritas deste trabalho não lhe tocam (`WHERE status =
/// 'generating' AND source = 'translation'`).
async fn run_translation(state: Arc<AppState>, job: TranslationJob, slot: OwnedSemaphorePermit) {
    let (id, lang) = (job.recording_id, job.lang.as_str());
    let ollama = Ollama::of(&state);
    let timeout = Duration::from_secs(state.config.ollama_timeout_secs);
    let total = job.segments.len().max(1);
    let mut cues = Vec::with_capacity(job.segments.len());
    let (mut last_pct, mut last_touch) = (0i16, Instant::now());
    for (i, s) in job.segments.iter().enumerate() {
        let translated = match ai_assist::translation_prompt(&s.text, lang) {
            None => Err(LlmFailure::BadResponse),
            Some(prompt) => ollama
                .generate(&job.model, &prompt, timeout, false)
                .await
                .and_then(|a| ai_assist::parse_translation(&a)),
        };
        let text = match translated {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(recording = %id, %lang, segment = i, code = e.code(), "tradução de legendas falhou");
                let msg = format!("{} (segmento {} de {total})", e.message(), i + 1);
                drop(slot);
                finish_caption(&state, id, lang, None, Some(&msg)).await;
                return;
            }
        };
        cues.push(caption_rules::Cue {
            start_ms: s.start_ms,
            end_ms: s.end_ms.max(s.start_ms + 1),
            text,
        });
        let pct = ((i + 1) * 100 / total) as i16;
        if pct < 100 && (pct >= last_pct + 5 || last_touch.elapsed() >= HEARTBEAT) {
            last_pct = pct;
            last_touch = Instant::now();
            let r = sqlx::query(
                "UPDATE recording_captions SET progress_pct = $3, updated_at = now()
                  WHERE recording_id = $1 AND lang = $2
                    AND status = 'generating' AND source = 'translation'",
            )
            .bind(id)
            .bind(lang)
            .bind(pct)
            .execute(&state.db)
            .await;
            match r {
                // Alguém substituiu ou apagou a legenda: o trabalho já não serve.
                Ok(done) if done.rows_affected() == 0 => return,
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(recording = %id, %lang, error = %e, "legendas: progresso não gravado")
                }
            }
        }
    }
    drop(slot);
    let vtt = caption_rules::cues_to_vtt(&cues);
    match caption_rules::parse_vtt(&vtt) {
        Ok(_) => finish_caption(&state, id, lang, Some(&vtt), None).await,
        Err(e) => finish_caption(&state, id, lang, None, Some(&e.message)).await,
    }
}

async fn finish_caption(
    state: &AppState,
    id: Uuid,
    lang: &str,
    vtt: Option<&str>,
    error: Option<&str>,
) {
    let r = sqlx::query(
        "UPDATE recording_captions
            SET vtt = COALESCE($3, vtt),
                status = CASE WHEN $3 IS NULL THEN 'failed' ELSE 'draft' END,
                error = $4, progress_pct = NULL, updated_at = now()
          WHERE recording_id = $1 AND lang = $2
            AND status = 'generating' AND source = 'translation'",
    )
    .bind(id)
    .bind(lang)
    .bind(vtt)
    .bind(error)
    .execute(&state.db)
    .await;
    match r {
        Ok(_) if vtt.is_some() => tracing::info!(recording = %id, %lang, "legendas traduzidas"),
        Ok(_) => {}
        Err(e) => {
            tracing::error!(recording = %id, %lang, error = %e, "legendas: estado final não gravado")
        }
    }
}
