//! IA local no Estúdio, por organização: o estado do modelo e as três
//! sugestões sobre a transcrição que o editor já tem (resumo e capítulos,
//! texto de publicação, palavras de preenchimento).
//!
//! Três garantias, por esta ordem:
//! - **soberania** — a transcrição vai só ao Ollama do operador, pela guarda de
//!   saída (`ai::Ollama`), e nada se guarda: o pedido traz o texto, a resposta
//!   leva a proposta;
//! - **isolamento por organização** — a pertença decide-se antes de olhar para
//!   o corpo (quem não é membro recebe `404`, nunca um `400` do validador), e
//!   no máximo `AI_STUDIO_CONCURRENCY_PER_ORG` trabalhos do modelo em
//!   simultâneo por organização ([`OrgSlots`]), para que uma não monopolize o
//!   modelo partilhado;
//! - **honestidade** — um erro diz qual é, com código (`ai.not_configured`,
//!   `ai.unreachable`, `ai.timeout`, `ai.model_missing`, `ai.bad_response`, …);
//!   nunca um resultado vazio ou inventado. As regras do que o modelo pode
//!   propor estão em `delonix_meet_domain::content::ai_assist`.
//!
//! Capacidade proposta (ADR-0008, na integração passa a `require_capability`):
//! `ai.use` nas duas rotas. Hoje: membro activo da organização
//! (`org::require_member_pub`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header::RETRY_AFTER, HeaderValue},
    response::{IntoResponse, Response},
    Json,
};
use delonix_meet_core::{DomainError, ErrorKind};
use delonix_meet_domain::content::{ai_assist as rules, transcription::Segment};
use delonix_meet_domain::integration::llm::LlmFailure;
use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;

use crate::{ai::Ollama, auth::AuthUser, error::ApiError, AppState};

/// Tecto do corpo do `POST …/ai/suggestions`: 400 000 caracteres de texto em
/// UTF-8 e JSON, com folga.
pub const MAX_SUGGESTION_BODY_BYTES: usize = 2 * 1024 * 1024;
/// Tecto do `GET …/ai/status`: é um estado, não pode pendurar o ecrã.
pub const STATUS_TIMEOUT: Duration = Duration::from_secs(3);
/// O `Retry-After` de um `429 ai.busy` (segundos): um trabalho do modelo
/// demora dezenas de segundos, e voltar a tentar já só gasta pedidos.
const BUSY_RETRY_AFTER_SECS: u64 = 10;

// ---------- concorrência por organização ----------

/// Vagas de trabalho do modelo por organização. O semáforo de uma organização
/// nasce com o tecto em vigor e fica: o tecto vem da configuração, que não
/// muda com o processo a correr.
///
/// **Por processo.** Com K réplicas o tecto efectivo é N×K por organização; um
/// tecto global precisa de Redis e de um ADR (registado no relatório).
pub struct OrgSlots {
    permits: usize,
    map: Mutex<HashMap<Uuid, Arc<Semaphore>>>,
}

impl OrgSlots {
    pub fn new(permits: usize) -> Self {
        Self {
            permits: permits.max(1),
            map: Mutex::new(HashMap::new()),
        }
    }

    /// Uma vaga, ou `None` se as da organização estão ocupadas. A vaga
    /// liberta-se quando o valor cai (também se a tarefa entrar em pânico).
    pub fn try_acquire(&self, org_id: Uuid) -> Option<OwnedSemaphorePermit> {
        let sem = {
            let mut map = self.map.lock().unwrap_or_else(|p| p.into_inner());
            map.entry(org_id)
                .or_insert_with(|| Arc::new(Semaphore::new(self.permits)))
                .clone()
        };
        sem.try_acquire_owned().ok()
    }
}

/// `429 ai.busy` com o `Retry-After` do modelo (não o de 60 s do rate-limit).
pub(crate) fn busy() -> Response {
    let mut res = ApiError::Domain(DomainError::new(
        ErrorKind::ResourceExhausted,
        "ai.busy",
        "a organização já tem trabalhos da IA local a correr; tenta daqui a pouco",
    ))
    .into_response();
    res.headers_mut()
        .insert(RETRY_AFTER, HeaderValue::from(BUSY_RETRY_AFTER_SECS));
    res
}

// ---------- estado ----------

#[derive(Debug, Serialize, PartialEq, utoipa::ToSchema)]
#[schema(as = AiStatus)]
pub struct AiStatus {
    /// Há `OLLAMA_URL`.
    pub configured: bool,
    /// O Ollama respondeu.
    pub reachable: bool,
    /// O modelo do Estúdio (`OLLAMA_MODEL_STUDIO`).
    pub model: String,
    /// `null` quando não se chegou a perguntar (não configurado ou sem resposta).
    pub model_installed: Option<bool>,
    /// Pronto a usar: configurado, alcançável e com o modelo instalado.
    pub ready: bool,
    /// Porque não está pronto: `not_configured` | `url_rejected` |
    /// `unreachable` | `timeout` | `model_missing` | `upstream_error` |
    /// `bad_response`. `null` quando está pronto.
    pub reason: Option<String>,
    /// A mesma razão, para pessoas. `null` quando está pronto.
    pub error: Option<String>,
}

/// O estado da IA local, para o ecrã decidir o que oferece. Nunca falha: o que
/// correu mal vai em `reason`/`error`.
pub(crate) async fn probe_status(ollama: &Ollama<'_>, model: &str, timeout: Duration) -> AiStatus {
    let mut st = AiStatus {
        configured: ollama.configured(),
        reachable: false,
        model: model.to_string(),
        model_installed: None,
        ready: false,
        reason: None,
        error: None,
    };
    let failure = match ollama.models(timeout).await {
        Ok(models) => {
            st.reachable = true;
            let installed = delonix_meet_domain::integration::llm::model_listed(&models, model);
            st.model_installed = Some(installed);
            st.ready = installed;
            (!installed).then(|| LlmFailure::ModelMissing(model.to_string()))
        }
        Err(e @ (LlmFailure::Upstream(_) | LlmFailure::BadResponse)) => {
            // Respondeu — com erro, ou com uma lista ilegível.
            st.reachable = true;
            Some(e)
        }
        Err(e) => Some(e),
    };
    if let Some(f) = failure {
        st.reason = Some(f.reason().to_string());
        st.error = Some(f.message());
    }
    st
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(get_status, suggestions),
    components(schemas(
        AiStatus,
        SuggestionReq,
        SuggestionSegment,
        SuggestionResponse,
        SummarySuggestion,
        SuggestedChapter,
        PublicationSuggestion,
        FillersSuggestion
    ))
)]
pub struct ApiDoc;

/// Estado da IA local para o Estúdio da organização. Sempre `200` para um
/// membro: é um estado, e a causa vai em `reason`.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/ai/status", tag = "ai",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses(
        (status = 200, body = AiStatus),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "Não é membro activo da organização (não se confirma que existe)."),
    )
)]
pub async fn get_status(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<AiStatus>, ApiError> {
    crate::org::require_member_pub(&state, org_id, auth.user_id).await?;
    Ok(Json(
        probe_status(
            &Ollama::of(&state),
            &state.config.ollama_model_studio,
            STATUS_TIMEOUT,
        )
        .await,
    ))
}

// ---------- sugestões ----------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[schema(as = AiSuggestionSegment)]
pub struct SuggestionSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[schema(as = AiSuggestionReq)]
pub struct SuggestionReq {
    /// `summary` | `publication` | `fillers`.
    pub task: String,
    /// Língua da transcrição (código curto: `pt`, `en`, `pt-PT`).
    #[serde(default)]
    pub language: Option<String>,
    /// Título de trabalho (≤ 200 caracteres, uma linha).
    #[serde(default)]
    pub title: Option<String>,
    /// 1-5000 segmentos, ≤ 2000 caracteres cada, ≤ 400 000 no total, ≥ 40 de texto.
    #[serde(default)]
    pub segments: Vec<SuggestionSegment>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[schema(as = AiSuggestedChapter)]
pub struct SuggestedChapter {
    pub t_ms: i64,
    pub title: String,
}

/// `task = summary`.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[schema(as = AiSummarySuggestion)]
pub struct SummarySuggestion {
    /// Texto simples, ≤ 2000 caracteres.
    pub summary: String,
    /// Dentro da duração da transcrição; pode vir vazio.
    pub chapters: Vec<SuggestedChapter>,
}

/// `task = publication`.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[schema(as = AiPublicationSuggestion)]
pub struct PublicationSuggestion {
    /// ≤ 100 caracteres.
    pub title: String,
    /// ≤ 1000 caracteres.
    pub description: String,
    /// ≤ 8, com as regras das etiquetas da gravação.
    pub tags: Vec<String>,
}

/// `task = fillers`.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[schema(as = AiFillersSuggestion)]
pub struct FillersSuggestion {
    /// ≤ 20; só termos que existem na transcrição como palavras inteiras.
    pub terms: Vec<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(untagged)]
#[schema(as = AiSuggestion)]
pub enum SuggestionResponse {
    Summary(SummarySuggestion),
    Publication(PublicationSuggestion),
    Fillers(FillersSuggestion),
}

impl From<rules::Suggestion> for SuggestionResponse {
    fn from(s: rules::Suggestion) -> Self {
        match s {
            rules::Suggestion::Summary(s) => Self::Summary(SummarySuggestion {
                summary: s.summary,
                chapters: s
                    .chapters
                    .into_iter()
                    .map(|c| SuggestedChapter {
                        t_ms: c.t_ms,
                        title: c.title,
                    })
                    .collect(),
            }),
            rules::Suggestion::Publication(p) => Self::Publication(PublicationSuggestion {
                title: p.title,
                description: p.description,
                tags: p.tags,
            }),
            rules::Suggestion::Fillers(f) => Self::Fillers(FillersSuggestion { terms: f.terms }),
        }
    }
}

/// O corpo lido DEPOIS da pertença: um não-membro nunca chega ao validador.
fn parse_body(body: &[u8]) -> Result<rules::Input, DomainError> {
    let req: SuggestionReq = serde_json::from_slice(body).map_err(|e| {
        DomainError::invalid("ai.invalid_body", format!("corpo JSON inválido: {e}"))
    })?;
    rules::validate(
        &req.task,
        req.language.as_deref(),
        req.title.as_deref(),
        req.segments
            .into_iter()
            .map(|s| Segment {
                start_ms: s.start_ms,
                end_ms: s.end_ms,
                text: s.text,
                confidence: None,
            })
            .collect(),
    )
}

/// Uma sugestão do modelo local sobre a transcrição enviada. Nada se guarda:
/// `200` com a proposta, não `201`.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/ai/suggestions", tag = "ai",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = SuggestionReq,
    responses(
        (status = 200, body = SuggestionResponse, description = "`summary` → `{summary, chapters}`; `publication` → `{title, description, tags}`; `fillers` → `{terms}`."),
        (status = 400, body = crate::openapi::ErrorBody, description = "`ai.invalid_body` / `ai.invalid_task` / `ai.invalid_language` / `ai.invalid_title` / `ai.invalid_segments` / `ai.transcript_too_large` / `ai.transcript_too_short`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "Não é membro activo da organização."),
        (status = 429, body = crate::openapi::ErrorBody, headers(("Retry-After" = u64)), description = "`ai.busy`: a organização já tem os seus trabalhos do modelo a correr."),
        (status = 503, body = crate::openapi::ErrorBody, description = "`ai.not_configured` / `ai.url_rejected` / `ai.unreachable` / `ai.timeout` / `ai.model_missing` / `ai.upstream_error` / `ai.bad_response`"),
    )
)]
pub async fn suggestions(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    body: Bytes,
) -> Result<Response, ApiError> {
    crate::org::require_member_pub(&state, org_id, auth.user_id).await?;
    let input = parse_body(&body)?;
    let ollama = Ollama::of(&state);
    if !ollama.configured() {
        return Err(LlmFailure::NotConfigured.into());
    }
    let Some(_slot) = state.ai_slots.try_acquire(org_id) else {
        return Ok(busy());
    };
    let timeout = Duration::from_secs(state.config.ollama_timeout_secs);
    let answer = ollama
        .generate(
            &state.config.ollama_model_studio,
            &rules::suggestion_prompt(&input),
            timeout,
            true,
        )
        .await;
    let suggestion = answer.and_then(|a| rules::parse_suggestion(&a, &input));
    match suggestion {
        Ok(s) => Ok(Json(SuggestionResponse::from(s)).into_response()),
        Err(e) => {
            tracing::warn!(%org_id, task = ?input.task, code = e.code(), "sugestão da IA local falhou");
            Err(e.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_org_does_not_take_the_other_org_slot() {
        let slots = OrgSlots::new(1);
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let slot = slots.try_acquire(a).expect("primeira vaga da A");
        assert!(slots.try_acquire(a).is_none(), "a A não tem segunda vaga");
        assert!(slots.try_acquire(b).is_some(), "a B não espera pela A");
        drop(slot);
        assert!(slots.try_acquire(a).is_some(), "a vaga volta quando acaba");
        let two = OrgSlots::new(2);
        let _s1 = two.try_acquire(a).unwrap();
        let _s2 = two.try_acquire(a).unwrap();
        assert!(two.try_acquire(a).is_none());
    }

    #[test]
    fn body_errors_have_codes() {
        assert_eq!(parse_body(b"{").unwrap_err().code, "ai.invalid_body");
        assert_eq!(
            parse_body(br#"{"task":"x","segments":[]}"#)
                .unwrap_err()
                .code,
            "ai.invalid_task"
        );
    }

    #[tokio::test]
    async fn busy_is_429_with_short_retry_after() {
        let res = busy();
        assert_eq!(res.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(res.headers()[RETRY_AFTER], "10");
    }
}
