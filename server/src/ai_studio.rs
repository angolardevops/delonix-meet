//! IA local no Estúdio: a partir da transcrição que o editor já tem, o LLM
//! in-cluster (Ollama) propõe um resumo com capítulos, o texto de publicação
//! e os bordões a cortar.
//!
//! Três garantias, por esta ordem:
//! - **soberania** — a transcrição vai só ao Ollama do cluster e nada se
//!   guarda: o pedido traz o texto, a resposta leva a proposta;
//! - **isolamento por organização** — no máximo
//!   `AI_STUDIO_CONCURRENCY_PER_ORG` tarefas em simultâneo por organização,
//!   para que uma não monopolize o modelo partilhado;
//! - **honestidade** — um erro diz qual é (não configurado, sem resposta,
//!   modelo em falta, tecto de tempo, resposta inutilizável); nunca se devolve
//!   um resultado vazio ou inventado como se fosse bom. O que o modelo propõe
//!   é validado aqui: um capítulo depois do fim sai, e um bordão que não está
//!   no texto também.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::header,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;

use crate::{
    ai::{self, LlmError},
    auth::AuthUser,
    error::ApiError,
    recording_chapters::{parse_llm_chapters, transcript_digest},
    recording_meta::{normalize_tags, Segment},
    AppState,
};

pub const MAX_SEGMENTS: usize = 5_000;
pub const MAX_SEGMENT_CHARS: usize = 2_000;
pub const MAX_TOTAL_CHARS: usize = 400_000;
/// Menos do que isto não é uma transcrição, é ruído: não vale uma chamada.
pub const MIN_CONTENT_CHARS: usize = 40;
pub const MAX_TITLE_CHARS: usize = 200;
/// Texto da transcrição que vai no prompt (janela do modelo pequeno).
const PROMPT_BUDGET_CHARS: usize = 24_000;
/// Tecto do `GET …/ai/status`: é um estado, não pode pendurar o ecrã.
const STATUS_TIMEOUT: Duration = Duration::from_secs(3);

const MAX_SUMMARY_CHARS: usize = 2_000;
const MAX_PUBLICATION_TITLE_CHARS: usize = 100;
const MAX_PUBLICATION_DESCRIPTION_CHARS: usize = 1_000;
const MAX_PUBLICATION_TAGS: usize = 8;
const MAX_FILLER_TERMS: usize = 20;
const MAX_FILLER_WORDS: usize = 4;
const MAX_FILLER_CHARS: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StudioTask {
    Summary,
    Publication,
    Fillers,
}

impl StudioTask {
    fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "summary" => Self::Summary,
            "publication" => Self::Publication,
            "fillers" => Self::Fillers,
            _ => return None,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct StudioSegmentReq {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct StudioReq {
    pub task: String,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub segments: Vec<StudioSegmentReq>,
}

/// O pedido já validado: é isto que chega ao LLM.
#[derive(Debug)]
pub(crate) struct StudioInput {
    pub task: StudioTask,
    pub language: Option<String>,
    pub title: Option<String>,
    /// Por ordem de início.
    pub segments: Vec<Segment>,
}

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError::BadRequest(msg.into())
}

/// `pt`, `en`, `pt-PT`, `fil` — um código curto, nada que se possa meter num
/// prompt como instrução.
fn valid_language(code: &str) -> bool {
    let mut parts = code.split('-');
    let base = parts.next().unwrap_or("");
    let base_ok = (2..=3).contains(&base.len()) && base.chars().all(|c| c.is_ascii_lowercase());
    let region_ok = match parts.next() {
        None => true,
        Some(r) => r.len() == 2 && r.chars().all(|c| c.is_ascii_alphabetic()),
    };
    base_ok && region_ok && parts.next().is_none()
}

/// Valida o corpo do `POST …/ai/suggestions`. Recusa (`400`) em vez de cortar.
pub(crate) fn validate(req: StudioReq) -> Result<StudioInput, ApiError> {
    let task = StudioTask::parse(req.task.trim())
        .ok_or_else(|| bad("tarefa desconhecida (summary, publication ou fillers)"))?;
    let language = match req.language.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(l) if valid_language(l) => Some(l.to_string()),
        Some(_) => return Err(bad("língua inválida (código curto, ex.: pt, en, fr)")),
    };
    let title = match req.title.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(t) if t.chars().count() > MAX_TITLE_CHARS => {
            return Err(bad(format!(
                "o título tem no máximo {MAX_TITLE_CHARS} caracteres"
            )))
        }
        Some(t) => Some(t.to_string()),
    };
    if req.segments.is_empty() || req.segments.len() > MAX_SEGMENTS {
        return Err(bad(format!(
            "a transcrição tem de ter entre 1 e {MAX_SEGMENTS} segmentos"
        )));
    }
    let mut total = 0usize;
    let mut content = 0usize;
    let mut segments = Vec::with_capacity(req.segments.len());
    for s in req.segments {
        let n = s.text.chars().count();
        if n > MAX_SEGMENT_CHARS {
            return Err(bad(format!(
                "cada segmento tem no máximo {MAX_SEGMENT_CHARS} caracteres"
            )));
        }
        if s.start_ms < 0 || s.end_ms < s.start_ms {
            return Err(bad(
                "tempos inválidos num segmento (start_ms ≥ 0 e end_ms ≥ start_ms)",
            ));
        }
        total += n;
        content += s.text.chars().filter(|c| !c.is_whitespace()).count();
        segments.push(Segment {
            start_ms: s.start_ms,
            end_ms: s.end_ms,
            text: s.text.trim().to_string(),
            confidence: None,
        });
    }
    if total > MAX_TOTAL_CHARS {
        return Err(bad(format!(
            "a transcrição tem no máximo {MAX_TOTAL_CHARS} caracteres"
        )));
    }
    if content < MIN_CONTENT_CHARS {
        return Err(bad("a transcrição tem texto a menos para a IA trabalhar"));
    }
    segments.sort_by_key(|s| s.start_ms);
    Ok(StudioInput {
        task,
        language,
        title,
        segments,
    })
}

// ---------- concorrência por organização ----------

fn limiters() -> &'static Mutex<HashMap<Uuid, Arc<Semaphore>>> {
    static LIMITERS: OnceLock<Mutex<HashMap<Uuid, Arc<Semaphore>>>> = OnceLock::new();
    LIMITERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Uma vaga para uma tarefa do Estúdio da organização, ou `None` se as
/// `permits` vagas dela estão ocupadas. A vaga liberta-se quando o valor cai.
///
/// O semáforo de uma organização nasce com o tecto em vigor e fica: o tecto
/// vem da configuração, que não muda com o processo a correr.
pub(crate) fn try_acquire_slot(org_id: Uuid, permits: usize) -> Option<OwnedSemaphorePermit> {
    let sem = {
        let mut map = limiters().lock().unwrap_or_else(|p| p.into_inner());
        map.entry(org_id)
            .or_insert_with(|| Arc::new(Semaphore::new(permits.max(1))))
            .clone()
    };
    sem.try_acquire_owned().ok()
}

// ---------- prompts e leitura da resposta ----------

fn language_rule(lang: Option<&str>) -> String {
    match lang {
        Some(l) => format!("Write in the SAME language as the transcript (language code \"{l}\")."),
        None => "Write in the SAME language as the transcript.".into(),
    }
}

fn build_prompt(input: &StudioInput) -> String {
    let digest = transcript_digest(&input.segments, PROMPT_BUDGET_CHARS);
    let lang = language_rule(input.language.as_deref());
    let title = input
        .title
        .as_deref()
        .map(|t| format!("The working title is: {t}\n"))
        .unwrap_or_default();
    let task = match input.task {
        StudioTask::Summary => format!(
            "Summarise the recorded session below and split it into chapters.\n\
             Return ONLY a JSON object, no prose, exactly like \
             {{\"summary\": \"…\", \"chapters\": [{{\"start\": \"00:00:00\", \"title\": \"…\"}}]}}.\n\
             Rules: the summary is plain text (no Markdown), 2 to 6 sentences; 3 to 12 \
             chapters; the first starts at 00:00:00; every start MUST be one of the times \
             shown in the transcript; chapter titles have at most 8 words. {lang} Never \
             invent facts, names, numbers or topics that are not in the transcript."
        ),
        StudioTask::Publication => format!(
            "Write the text to publish this recorded session as a video.\n\
             Return ONLY a JSON object, no prose, exactly like \
             {{\"title\": \"…\", \"description\": \"…\", \"tags\": [\"…\"]}}.\n\
             Rules: the title has at most 12 words; the description is plain text (no \
             Markdown), at most 5 sentences; 3 to 8 short tags, lowercase, without #. \
             {lang} Never invent facts, names, numbers or promises that are not in the \
             transcript."
        ),
        StudioTask::Fillers => (
            "List the filler words and hesitation expressions that the speakers actually \
             use in the transcript below (for example «tipo», «pá», «ah», «hum», «né», \
             «basically», «euh»).\n\
             Return ONLY a JSON object, no prose, exactly like {\"terms\": [\"…\"]}.\n\
             Rules: copy each term EXACTLY as it is written in the transcript; at most 20 \
             terms, each at most 4 words; words that carry meaning in the sentence are not \
             fillers; if there are none, return {\"terms\": []}. Never list a term that \
             does not appear in the transcript."
        )
        .to_string(),
    };
    format!("{task}\n{title}\nTranscript (each line starts with [hh:mm:ss]):\n{digest}")
}

/// O primeiro objecto JSON da resposta, mesmo com prosa à volta.
fn extract_json_object(answer: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    let a = answer.find('{')?;
    let b = answer.rfind('}')?;
    if b <= a {
        return None;
    }
    match serde_json::from_str::<serde_json::Value>(&answer[a..=b]).ok()? {
        serde_json::Value::Object(m) => Some(m),
        _ => None,
    }
}

/// Texto simples: sem a marcação Markdown que os modelos pequenos põem
/// mesmo quando se pede que não (cabeçalhos, negrito, código, marcadores).
fn plain_text(raw: &str) -> String {
    let lines: Vec<String> = raw
        .lines()
        .map(|l| {
            let l = l.trim().trim_start_matches('#').trim_start();
            let l = l
                .strip_prefix("- ")
                .or_else(|| l.strip_prefix("* "))
                .unwrap_or(l);
            l.replace("**", "").replace("__", "").replace('`', "")
        })
        .collect();
    lines.join("\n").trim().to_string()
}

/// Corta em `max` caracteres sem partir palavras quando se pode.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    match cut.rfind(char::is_whitespace) {
        Some(i) if i > 0 => cut[..i].trim_end().to_string(),
        _ => cut,
    }
}

fn string_field(obj: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<String> {
    obj.get(key)
        .and_then(|v| v.as_str())
        .map(plain_text)
        .filter(|s| !s.is_empty())
}

#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct StudioChapter {
    pub t_ms: i64,
    pub title: String,
}

fn parse_summary(answer: &str, input: &StudioInput) -> Result<serde_json::Value, LlmError> {
    let obj = extract_json_object(answer).ok_or(LlmError::BadResponse)?;
    let summary = string_field(&obj, "summary").ok_or(LlmError::BadResponse)?;
    let end = input.segments.iter().map(|s| s.end_ms).max().unwrap_or(0);
    let chapters: Vec<StudioChapter> = obj
        .get("chapters")
        .map(|c| parse_llm_chapters(&c.to_string(), end))
        .unwrap_or_default()
        .into_iter()
        .map(|(t_ms, title)| StudioChapter { t_ms, title })
        .collect();
    Ok(serde_json::json!({
        "summary": truncate_chars(&summary, MAX_SUMMARY_CHARS),
        "chapters": chapters,
    }))
}

fn parse_publication(answer: &str) -> Result<serde_json::Value, LlmError> {
    let obj = extract_json_object(answer).ok_or(LlmError::BadResponse)?;
    let title = string_field(&obj, "title")
        .map(|t| t.replace('\n', " ").trim_matches('"').trim().to_string())
        .filter(|t| !t.is_empty())
        .ok_or(LlmError::BadResponse)?;
    let description = string_field(&obj, "description").ok_or(LlmError::BadResponse)?;
    let raw_tags: Vec<String> = match obj.get("tags") {
        Some(serde_json::Value::Array(a)) => a
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        Some(serde_json::Value::String(s)) => s.split(',').map(str::to_string).collect(),
        _ => vec![],
    };
    // As regras são as do PATCH da gravação; uma etiqueta que elas recusam
    // sai, em vez de deitar fora a proposta inteira.
    let mut tags: Vec<String> = Vec::new();
    for t in &raw_tags {
        if let Ok(norm) = normalize_tags(std::slice::from_ref(t)) {
            for n in norm {
                if !tags.contains(&n) && tags.len() < MAX_PUBLICATION_TAGS {
                    tags.push(n);
                }
            }
        }
    }
    Ok(serde_json::json!({
        "title": truncate_chars(&title, MAX_PUBLICATION_TITLE_CHARS),
        "description": truncate_chars(&description, MAX_PUBLICATION_DESCRIPTION_CHARS),
        "tags": tags,
    }))
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '’' || c == '-'
}

/// Palavras em minúsculas, sem pontuação à volta.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !is_word_char(c))
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Fica só o que o modelo propôs E está no texto, como palavras inteiras
/// seguidas (sem distinguir maiúsculas). O modelo nunca consegue fazer entrar
/// um termo que ninguém disse.
pub(crate) fn validate_fillers(proposed: &[String], transcript_words: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in proposed {
        let raw = raw.trim();
        if raw.is_empty() || raw.chars().count() > MAX_FILLER_CHARS {
            continue;
        }
        let term = words(raw);
        if term.is_empty() || term.len() > MAX_FILLER_WORDS {
            continue;
        }
        let present = transcript_words
            .windows(term.len())
            .any(|w| w == term.as_slice());
        let joined = term.join(" ");
        if present && !out.contains(&joined) {
            out.push(joined);
            if out.len() == MAX_FILLER_TERMS {
                break;
            }
        }
    }
    out
}

fn parse_fillers(answer: &str, input: &StudioInput) -> Result<serde_json::Value, LlmError> {
    let obj = extract_json_object(answer).ok_or(LlmError::BadResponse)?;
    let proposed: Vec<String> = match obj.get("terms") {
        Some(serde_json::Value::Array(a)) => a
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => return Err(LlmError::BadResponse),
    };
    let transcript_words: Vec<String> =
        input.segments.iter().flat_map(|s| words(&s.text)).collect();
    Ok(serde_json::json!({ "terms": validate_fillers(&proposed, &transcript_words) }))
}

/// A tarefa inteira contra um Ollama: prompt, chamada e validação da
/// resposta. Sem `AppState`, para se testar contra um Ollama falso.
pub(crate) async fn run_task(
    client: &reqwest::Client,
    base_url: Option<&str>,
    model: &str,
    timeout: Duration,
    input: &StudioInput,
) -> Result<serde_json::Value, LlmError> {
    let prompt = build_prompt(input);
    let answer = ai::ollama_generate(client, base_url, model, &prompt, timeout, true).await?;
    match input.task {
        StudioTask::Summary => parse_summary(&answer, input),
        StudioTask::Publication => parse_publication(&answer),
        StudioTask::Fillers => parse_fillers(&answer, input),
    }
}

// ---------- estado ----------

#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct AiStatus {
    pub configured: bool,
    pub reachable: bool,
    pub model: String,
    pub model_installed: Option<bool>,
    pub error: Option<String>,
}

/// O estado da IA local, para o ecrã decidir o que oferece. Nunca falha: o
/// que correu mal vai em `error`.
pub(crate) async fn probe_status(
    client: &reqwest::Client,
    base_url: Option<&str>,
    model: &str,
    timeout: Duration,
) -> AiStatus {
    let mut st = AiStatus {
        configured: base_url.is_some_and(|b| !b.trim().is_empty()),
        reachable: false,
        model: model.to_string(),
        model_installed: None,
        error: None,
    };
    match ai::ollama_models(client, base_url, timeout).await {
        Ok(models) => {
            st.reachable = true;
            let installed = ai::model_listed(&models, model);
            st.model_installed = Some(installed);
            if !installed {
                st.error = Some(LlmError::ModelMissing(model.to_string()).client_message());
            }
        }
        Err(LlmError::Timeout(t)) => {
            st.error = Some(format!(
                "o serviço Ollama não respondeu em {} s",
                t.as_secs()
            ));
        }
        Err(LlmError::Upstream(code)) => {
            st.reachable = true;
            st.error = Some(format!("o serviço Ollama respondeu com erro (HTTP {code})"));
        }
        Err(LlmError::BadResponse) => {
            st.reachable = true;
            st.error = Some("o serviço Ollama devolveu uma lista de modelos ilegível".into());
        }
        Err(e) => st.error = Some(e.client_message()),
    }
    st
}

// ---------- rotas ----------

/// `GET /api/orgs/{org_id}/ai/status` — a IA local está pronta para o Estúdio?
///
/// Contrato (para o `#[utoipa::path]` na integração com o #81):
/// - método/caminho: `GET /api/orgs/{org_id}/ai/status`, `org_id` UUID;
/// - autenticação: sessão (`AuthUser`), membro activo da organização;
/// - pedido: sem corpo;
/// - `200` [`AiStatus`]: `{"configured": bool, "reachable": bool, "model": string,
///   "model_installed": bool|null, "error": string|null}` — sempre `200` para um
///   membro: é um estado, e a causa vai em `error`;
/// - `401` sem sessão · `404` não é membro (não se confirma que a org existe).
pub async fn status(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<AiStatus>, ApiError> {
    crate::org::require_member_pub(&state, org_id, auth.user_id).await?;
    Ok(Json(
        probe_status(
            state.outbound.operator(),
            state.config.ollama_url.as_deref(),
            &state.config.ollama_model_studio,
            STATUS_TIMEOUT,
        )
        .await,
    ))
}

/// `POST /api/orgs/{org_id}/ai/suggestions` — calcula uma sugestão da IA local
/// sobre a transcrição enviada. Nada se guarda: é `200` com a sugestão, não
/// `201`.
///
/// Contrato (para o `#[utoipa::path]` na integração com o #81):
/// - método/caminho: `POST /api/orgs/{org_id}/ai/suggestions`, `org_id` UUID;
/// - autenticação: sessão (`AuthUser`), membro activo da organização;
/// - pedido [`StudioReq`]: `{"task": "summary"|"publication"|"fillers",
///   "language"?: string, "title"?: string, "segments": [{"start_ms": i64,
///   "end_ms": i64, "text": string}]}`;
/// - `200`: `summary` → `{"summary": string, "chapters": [{"t_ms": i64, "title": string}]}`;
///   `publication` → `{"title": string, "description": string, "tags": [string]}`;
///   `fillers` → `{"terms": [string]}`;
/// - `400` corpo inválido · `401` sem sessão · `404` não é membro · `429` (+
///   `Retry-After`) a organização já tem as suas tarefas a correr · `503` IA
///   indisponível ou resposta inutilizável, com a causa em `{"error": string}`.
pub async fn suggestions(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<StudioReq>,
) -> Result<Response, ApiError> {
    // A pertença decide-se ANTES de olhar para o corpo: um não-membro nunca
    // chega à validação, nem ao modelo.
    crate::org::require_member_pub(&state, org_id, auth.user_id).await?;
    let input = validate(req)?;
    if state.config.ollama_url.is_none() {
        return Err(LlmError::NotConfigured.into());
    }
    let Some(_slot) = try_acquire_slot(org_id, state.config.ai_studio_concurrency_per_org) else {
        return Ok(([(header::RETRY_AFTER, "10")], ApiError::TooManyRequests).into_response());
    };
    let timeout = Duration::from_secs(state.config.ollama_timeout_secs);
    let out = run_task(
        state.outbound.operator(),
        state.config.ollama_url.as_deref(),
        &state.config.ollama_model_studio,
        timeout,
        &input,
    )
    .await
    .inspect_err(
        |e| tracing::warn!(%org_id, task = ?input.task, error = ?e, "Estúdio IA falhou"),
    )?;
    Ok(Json(out).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::fake_ollama::{self, Fake, Generate};

    fn client() -> reqwest::Client {
        crate::webhooks::outbound_http_client()
    }

    const T: Duration = Duration::from_secs(5);

    fn seg(start: i64, end: i64, text: &str) -> StudioSegmentReq {
        StudioSegmentReq {
            start_ms: start,
            end_ms: end,
            text: text.into(),
        }
    }

    fn req(task: &str, segments: Vec<StudioSegmentReq>) -> StudioReq {
        StudioReq {
            task: task.into(),
            language: Some("pt".into()),
            title: Some("Reunião de rede".into()),
            segments,
        }
    }

    fn transcript() -> Vec<StudioSegmentReq> {
        vec![
            seg(0, 20_000, "Bom dia, tipo, vamos falar da rede de Luanda."),
            seg(
                20_000,
                60_000,
                "Pá, o troço do Kilamba está, hum, em manutenção.",
            ),
            seg(
                60_000,
                120_000,
                "Decidimos, tipo, adiar a migração para Outubro.",
            ),
        ]
    }

    fn input(task: &str) -> StudioInput {
        validate(req(task, transcript())).unwrap()
    }

    // ---- validação do corpo ----

    fn msg(e: ApiError) -> String {
        match e {
            ApiError::BadRequest(m) => m,
            other => panic!("esperava 400, veio {other:?}"),
        }
    }

    #[test]
    fn corpo_valido_passa_e_ordena_os_segmentos() {
        let mut s = transcript();
        s.reverse();
        let i = validate(req("fillers", s)).unwrap();
        assert_eq!(i.task, StudioTask::Fillers);
        assert_eq!(i.language.as_deref(), Some("pt"));
        assert_eq!(i.segments[0].start_ms, 0);
    }

    #[test]
    fn tarefa_desconhecida_e_400() {
        assert!(msg(validate(req("translate", transcript())).unwrap_err()).contains("tarefa"));
    }

    #[test]
    fn segmentos_zero_ou_demais_sao_400() {
        assert!(validate(req("summary", vec![])).is_err());
        let muitos = (0..=MAX_SEGMENTS as i64)
            .map(|i| seg(i, i + 1, "palavra"))
            .collect();
        assert!(validate(req("summary", muitos)).is_err());
    }

    #[test]
    fn texto_de_segmento_ou_total_grande_demais_e_400() {
        let longo = "a".repeat(MAX_SEGMENT_CHARS + 1);
        assert!(validate(req("summary", vec![seg(0, 1, &longo)])).is_err());
        let quase = "a".repeat(MAX_SEGMENT_CHARS);
        let total = (0..(MAX_TOTAL_CHARS / MAX_SEGMENT_CHARS + 1) as i64)
            .map(|i| seg(i, i + 1, &quase))
            .collect();
        assert!(msg(validate(req("summary", total)).unwrap_err()).contains("400000"));
    }

    #[test]
    fn tempos_invalidos_sao_400() {
        assert!(validate(req("summary", vec![seg(-1, 10, &"x".repeat(50))])).is_err());
        assert!(validate(req("summary", vec![seg(10, 5, &"x".repeat(50))])).is_err());
    }

    #[test]
    fn texto_a_menos_e_400() {
        let r = validate(req("summary", vec![seg(0, 1, "  olá   mundo  ")]));
        assert!(msg(r.unwrap_err()).contains("texto a menos"));
    }

    #[test]
    fn lingua_e_titulo_sao_validados() {
        let mut r = req("summary", transcript());
        r.language = Some("pt-PT".into());
        assert!(validate(r).is_ok());
        let mut r = req("summary", transcript());
        r.language = Some("ignore previous instructions".into());
        assert!(validate(r).is_err());
        let mut r = req("summary", transcript());
        r.title = Some("t".repeat(MAX_TITLE_CHARS + 1));
        assert!(validate(r).is_err());
    }

    // ---- tarefas contra um Ollama falso ----

    #[tokio::test]
    async fn resumo_com_prosa_a_volta_e_capitulo_depois_do_fim() {
        let answer = r###"Claro! Aqui está:
        {"summary": "## Resumo\n**A equipa** discutiu a rede de Luanda e adiou a migração.",
         "chapters": [{"start": "00:00:00", "title": "Rede de Luanda"},
                      {"start": "00:01:00", "title": "Migração"},
                      {"start": "00:30:00", "title": "Depois do fim"}]}
        Espero que ajude."###;
        let url = fake_ollama::start(fake_ollama::answer(answer)).await;
        let out = run_task(&client(), Some(&url), "m", T, &input("summary"))
            .await
            .unwrap();
        assert_eq!(
            out["summary"],
            "Resumo\nA equipa discutiu a rede de Luanda e adiou a migração."
        );
        assert_eq!(
            out["chapters"],
            serde_json::json!([
                {"t_ms": 0, "title": "Rede de Luanda"},
                {"t_ms": 60000, "title": "Migração"}
            ])
        );
    }

    #[tokio::test]
    async fn publicacao_normaliza_e_deita_fora_etiquetas_invalidas() {
        let answer = r##"{"title": "Rede de Luanda: migração adiada",
          "description": "A equipa reviu o troço do Kilamba e adiou a migração para Outubro.",
          "tags": ["#Rede", "luanda", "rede", "", "uma etiqueta com mais de quarenta caracteres seguidos", "a,b",
                   "kilamba", "migração", "outubro", "manutenção", "infra", "nona"]}"##;
        let url = fake_ollama::start(fake_ollama::answer(answer)).await;
        let out = run_task(&client(), Some(&url), "m", T, &input("publication"))
            .await
            .unwrap();
        assert_eq!(out["title"], "Rede de Luanda: migração adiada");
        assert_eq!(
            out["tags"],
            serde_json::json!([
                "rede",
                "luanda",
                "kilamba",
                "migração",
                "outubro",
                "manutenção",
                "infra",
                "nona"
            ])
        );
        assert!(out["description"].as_str().unwrap().starts_with("A equipa"));
    }

    #[tokio::test]
    async fn bordoes_inventados_nao_passam() {
        let answer = r#"Os bordões: {"terms": ["Tipo", "pá", "hum", "basically", "né", "tipo", "em manutenção", "uma frase com mais de quatro palavras"]}"#;
        let url = fake_ollama::start(fake_ollama::answer(answer)).await;
        let out = run_task(&client(), Some(&url), "m", T, &input("fillers"))
            .await
            .unwrap();
        // «basically» e «né» não foram ditos; «tipo» repetido conta uma vez.
        assert_eq!(
            out["terms"],
            serde_json::json!(["tipo", "pá", "hum", "em manutenção"])
        );
    }

    #[test]
    fn bordao_so_conta_como_palavra_inteira() {
        let w = words("O tipógrafo disse: Hum... ah, tipo-assim.");
        let v = validate_fillers(&["tipo".into(), "hum".into(), "ah".into()], &w);
        // «tipo» só aparece dentro de «tipógrafo» e de «tipo-assim»: não conta.
        assert_eq!(v, vec!["hum".to_string(), "ah".to_string()]);
    }

    #[tokio::test]
    async fn resposta_sem_json_ou_sem_campos_e_bad_response() {
        for a in [
            "Não consigo ajudar com isso.",
            r#"{"chapters": []}"#,
            r#"["tipo"]"#,
        ] {
            let url = fake_ollama::start(fake_ollama::answer(a)).await;
            for task in ["summary", "publication", "fillers"] {
                let r = run_task(&client(), Some(&url), "m", T, &input(task)).await;
                assert_eq!(r, Err(LlmError::BadResponse), "{task}: {a}");
            }
        }
        let e: ApiError = LlmError::BadResponse.into();
        assert!(matches!(e, ApiError::ServiceUnavailable(m)
            if m == "o modelo devolveu uma resposta que não se consegue usar"));
    }

    #[tokio::test]
    async fn modelo_em_falta_tecto_e_porto_fechado() {
        let url = fake_ollama::start(Fake {
            generate: Generate::Status(
                404,
                r#"{"error":"model \"qwen2.5:7b\" not found, try pulling it first"}"#.into(),
            ),
            models: vec![],
        })
        .await;
        let r = run_task(&client(), Some(&url), "qwen2.5:7b", T, &input("summary")).await;
        assert_eq!(r, Err(LlmError::ModelMissing("qwen2.5:7b".into())));

        let url = fake_ollama::start(Fake {
            generate: Generate::Sleep(Duration::from_secs(3)),
            models: vec![],
        })
        .await;
        let t = Duration::from_millis(300);
        let r = run_task(&client(), Some(&url), "m", t, &input("fillers")).await;
        assert_eq!(r, Err(LlmError::Timeout(t)));

        let url = fake_ollama::closed_url().await;
        let r = run_task(&client(), Some(&url), "m", T, &input("publication")).await;
        assert_eq!(r, Err(LlmError::Unreachable));

        let r = run_task(&client(), None, "m", T, &input("publication")).await;
        assert_eq!(r, Err(LlmError::NotConfigured));
    }

    // ---- estado ----

    #[tokio::test]
    async fn estado_com_e_sem_o_modelo() {
        let url = fake_ollama::start(Fake {
            generate: Generate::Answer("x".into()),
            models: vec!["qwen2.5:7b".into()],
        })
        .await;
        let st = probe_status(&client(), Some(&url), "qwen2.5:7b", T).await;
        assert_eq!(
            st,
            AiStatus {
                configured: true,
                reachable: true,
                model: "qwen2.5:7b".into(),
                model_installed: Some(true),
                error: None,
            }
        );
        let st = probe_status(&client(), Some(&url), "qwen2.5:1.5b", T).await;
        assert_eq!(st.model_installed, Some(false));
        assert_eq!(
            st.error.as_deref(),
            Some("o modelo «qwen2.5:1.5b» não está instalado no Ollama")
        );
    }

    #[tokio::test]
    async fn estado_sem_configuracao_e_sem_servico() {
        let st = probe_status(&client(), None, "m", T).await;
        assert!(!st.configured && !st.reachable && st.model_installed.is_none());
        assert_eq!(
            st.error.as_deref(),
            Some("IA local não configurada neste servidor (OLLAMA_URL)")
        );
        let url = fake_ollama::closed_url().await;
        let st = probe_status(&client(), Some(&url), "m", T).await;
        assert!(st.configured && !st.reachable && st.model_installed.is_none());
        assert_eq!(st.error.as_deref(), Some("o serviço Ollama não responde"));
    }

    // ---- concorrência ----

    #[test]
    fn uma_organizacao_nao_ocupa_a_vaga_da_outra() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let slot = try_acquire_slot(a, 1).expect("primeira vaga da A");
        assert!(try_acquire_slot(a, 1).is_none(), "a A não tem segunda vaga");
        assert!(try_acquire_slot(b, 1).is_some(), "a B não espera pela A");
        drop(slot);
        assert!(
            try_acquire_slot(a, 1).is_some(),
            "a vaga volta quando acaba"
        );
    }

    #[test]
    fn tecto_de_duas_vagas() {
        let a = Uuid::new_v4();
        let _s1 = try_acquire_slot(a, 2).unwrap();
        let _s2 = try_acquire_slot(a, 2).unwrap();
        assert!(try_acquire_slot(a, 2).is_none());
    }
}
