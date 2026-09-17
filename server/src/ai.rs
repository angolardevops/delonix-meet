//! IA local via Ollama (07-ollama.yaml) — tradução de legendas em tempo real e
//! resumo elegante da ata (MoM). Soberania by design: o texto das reuniões vai
//! apenas ao LLM in-cluster, nunca a uma cloud externa. Sem OLLAMA_URL tudo
//! degrada silenciosamente (fail-open): o MoM fica por regras no cliente.
//!
//! O cliente do Ollama ([`Ollama`]) é o único caminho até ao modelo: passa
//! pela guarda de saída do operador (`net_guard`) e devolve o erro verdadeiro
//! ([`LlmFailure`], com código estável). O Estúdio (`ai_assist.rs`) e a geração
//! de capítulos e legendas (`recording_ai.rs`) usam-no directamente.

use std::sync::Arc;
use std::time::Duration;

use axum::{extract::State, Json};
use delonix_meet_domain::content::ai_assist;
use delonix_meet_domain::integration::llm::{self, LlmFailure};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, net_guard::Outbound, AppState};

impl From<LlmFailure> for ApiError {
    fn from(e: LlmFailure) -> Self {
        ApiError::Domain(e.into_domain())
    }
}

#[derive(Deserialize)]
struct GenResponse {
    response: String,
}

#[derive(Deserialize)]
struct TagsResponse {
    #[serde(default)]
    models: Vec<TagModel>,
}

#[derive(Deserialize)]
struct TagModel {
    name: String,
}

/// O Ollama do operador (`OLLAMA_URL`), visto através da guarda de saída.
///
/// Não guarda o `AppState` para se poder testar contra um Ollama falso: só
/// precisa dos clientes de saída e do URL.
pub(crate) struct Ollama<'a> {
    outbound: &'a Outbound,
    base_url: Option<&'a str>,
}

impl<'a> Ollama<'a> {
    pub(crate) fn new(outbound: &'a Outbound, base_url: Option<&'a str>) -> Self {
        Self { outbound, base_url }
    }

    pub(crate) fn of(state: &'a AppState) -> Self {
        Self::new(&state.outbound, state.config.ollama_url.as_deref())
    }

    /// O URL está configurado (não diz nada sobre se responde).
    pub(crate) fn configured(&self) -> bool {
        self.base_url.is_some_and(|b| !b.trim().is_empty())
    }

    /// O URL base, já passado pela guarda de saída do OPERADOR: rede privada
    /// sim (o Ollama vive no cluster), metadados da cloud nunca.
    async fn base(&self) -> Result<String, LlmFailure> {
        let raw = self
            .base_url
            .map(str::trim)
            .filter(|b| !b.is_empty())
            .ok_or(LlmFailure::NotConfigured)?;
        match self.outbound.check_operator_url(raw).await {
            Ok(url) => Ok(url.as_str().trim_end_matches('/').to_string()),
            Err(e) => Err(classify_guard(&e)),
        }
    }

    /// Chamada única ao `/api/generate` (sem streaming), com o erro verdadeiro.
    /// `json_output` pede ao Ollama `"format": "json"`.
    pub(crate) async fn generate(
        &self,
        model: &str,
        prompt: &str,
        timeout: Duration,
        json_output: bool,
    ) -> Result<String, LlmFailure> {
        let base = self.base().await?;
        let mut body = serde_json::json!({
            "model": model,
            "prompt": prompt,
            "stream": false,
            "options": { "temperature": 0.2 }
        });
        if json_output {
            body["format"] = "json".into();
        }
        let resp = self
            .outbound
            .operator()
            .post(format!("{base}/api/generate"))
            .timeout(timeout)
            .json(&body)
            .send()
            .await
            .map_err(|e| classify_transport(&e, timeout))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| classify_transport(&e, timeout))?;
        if !status.is_success() {
            return Err(classify_status(status, &text, model));
        }
        let parsed: GenResponse =
            serde_json::from_str(&text).map_err(|_| LlmFailure::BadResponse)?;
        let out = parsed.response.trim().to_string();
        if out.is_empty() {
            return Err(LlmFailure::BadResponse);
        }
        Ok(out)
    }

    /// Nomes dos modelos instalados (`GET /api/tags`).
    pub(crate) async fn models(&self, timeout: Duration) -> Result<Vec<String>, LlmFailure> {
        let base = self.base().await?;
        let resp = self
            .outbound
            .operator()
            .get(format!("{base}/api/tags"))
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| classify_transport(&e, timeout))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(LlmFailure::Upstream(status.as_u16()));
        }
        let text = resp
            .text()
            .await
            .map_err(|e| classify_transport(&e, timeout))?;
        let tags: TagsResponse =
            serde_json::from_str(&text).map_err(|_| LlmFailure::BadResponse)?;
        Ok(tags.models.into_iter().map(|m| m.name).collect())
    }

    /// O modelo está pronto a usar: configurado, alcançável e instalado.
    pub(crate) async fn ensure_model(
        &self,
        model: &str,
        timeout: Duration,
    ) -> Result<(), LlmFailure> {
        let models = self.models(timeout).await?;
        if llm::model_listed(&models, model) {
            Ok(())
        } else {
            Err(LlmFailure::ModelMissing(model.to_string()))
        }
    }
}

/// A recusa da guarda de saída. Um nome que não resolve é um serviço que não
/// se alcança; o resto (esquema, credenciais, metadados) é um URL recusado.
fn classify_guard(e: &ApiError) -> LlmFailure {
    match e {
        ApiError::BadRequest(m) if m.contains("não resolve") => LlmFailure::Unreachable,
        _ => LlmFailure::Rejected,
    }
}

/// Um erro do `reqwest` classificado: tecto de tempo ou falta de ligação.
fn classify_transport(e: &reqwest::Error, timeout: Duration) -> LlmFailure {
    if e.is_timeout() {
        LlmFailure::Timeout(timeout.as_secs().max(1))
    } else if e.is_connect() || e.is_request() {
        LlmFailure::Unreachable
    } else {
        LlmFailure::BadResponse
    }
}

/// Um `404` do Ollama no `/api/generate` quer dizer modelo em falta
/// (`{"error":"model \"x\" not found, try pulling it first"}`).
fn classify_status(status: reqwest::StatusCode, body: &str, model: &str) -> LlmFailure {
    if status == reqwest::StatusCode::NOT_FOUND && body.contains("not found") {
        LlmFailure::ModelMissing(model.to_string())
    } else {
        LlmFailure::Upstream(status.as_u16())
    }
}

/// Chamada ao LLM para quem só quer saber se há texto (tradução em tempo
/// real, acta). A causa da falha fica no log; o cliente destes caminhos tem
/// sempre um recurso (acta por regras, legenda por traduzir).
async fn generate(
    state: &AppState,
    model: &str,
    prompt: String,
    timeout: Duration,
) -> Option<String> {
    match Ollama::of(state)
        .generate(model, &prompt, timeout, false)
        .await
    {
        Ok(t) => Some(t),
        Err(LlmFailure::NotConfigured) => None,
        Err(e) => {
            tracing::warn!(
                model,
                code = e.code(),
                "LLM local sem resposta aproveitável"
            );
            None
        }
    }
}

/// Traduz uma linha de legenda para a língua alvo (`pt`, `en-GB`, …).
pub async fn translate(state: &AppState, text: &str, target: &str) -> Option<String> {
    let prompt = ai_assist::translation_prompt(text, target)?;
    let answer = generate(
        state,
        &state.config.ollama_model_translate,
        prompt,
        Duration::from_secs(20),
    )
    .await?;
    ai_assist::parse_translation(&answer).ok()
}

/// Resumo organizado da ata a partir da transcrição bruta (a "ata bruta" é a
/// própria transcrição, que fica SEMPRE preservada na coluna `transcript`).
pub async fn summarize_minutes(state: &AppState, title: &str, transcript: &str) -> Option<String> {
    // Janela de contexto: mantém o FIM da transcrição (decisões/ações tendem
    // a acontecer no fecho da reunião).
    let window: String = if transcript.chars().count() > 24_000 {
        transcript
            .chars()
            .skip(transcript.chars().count() - 24_000)
            .collect()
    } else {
        transcript.to_string()
    };
    let prompt = format!(
        "És um assistente de atas de reunião. A transcrição abaixo vem de \
         reconhecimento de voz automático e PODE conter erros (palavras trocadas \
         por outras de som parecido, pontuação/maiúsculas em falta, frases \
         cortadas). Ao redigir a ata, INFERE pelo contexto a palavra que fez \
         sentido — corrige silenciosamente os erros óbvios de transcrição, mas \
         NUNCA inventes factos, nomes, números ou decisões que não estejam lá.\n\n\
         A partir da transcrição da reunião \"{title}\", escreve uma ata (Minutes \
         of Meeting) organizada e elegante em português europeu, em Markdown, com \
         EXATAMENTE estas secções:\n\
         ## Resumo\n(2-4 frases)\n## Pontos discutidos\n(lista)\n## Decisões\n(lista; \
         'Nenhuma registada.' se não houver)\n## Decisões e ações\n(uma linha `- [ ] \
         tarefa — responsável` por ação; 'Nenhuma registada.' se não houver)\n\n\
         Transcrição:\n{window}"
    );
    generate(
        state,
        &state.config.ollama_model_summary,
        prompt,
        Duration::from_secs(600),
    )
    .await
}

/// Gera o resumo AI em background e substitui a ata da reunião. Chamado depois
/// de `save_minutes` persistir a versão por regras + a transcrição (ata bruta):
/// se o LLM falhar, a ata por regras fica — nunca se perde nada.
pub fn spawn_mom_summary(state: Arc<AppState>, meeting_id: Uuid) {
    if state.config.ollama_url.is_none() {
        return;
    }
    tokio::spawn(async move {
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT title, transcript FROM meetings WHERE id = $1")
                .bind(meeting_id)
                .fetch_optional(&state.db)
                .await
                .ok()
                .flatten();
        let Some((title, transcript)) = row else {
            return;
        };
        if transcript.trim().len() < 80 {
            return; // transcrição a menos para valer um resumo
        }
        let Some(summary) = summarize_minutes(&state, &title, &transcript).await else {
            tracing::warn!(%meeting_id, "MoM AI: Ollama indisponível — mantém ata por regras");
            return;
        };
        let _ =
            sqlx::query("UPDATE meetings SET minutes = $1, minutes_ai_at = now() WHERE id = $2")
                .bind(summary.chars().take(200_000).collect::<String>())
                .bind(meeting_id)
                .execute(&state.db)
                .await;
        tracing::info!(%meeting_id, "MoM AI: ata resumida via Ollama");
        // Notifica integrações (ex.: nk_delonix_meet no Odoo) que o MoM final
        // está pronto — o webhook só acelera o pull; o cron do Odoo apanha na
        // mesma se este ping se perder.
        let owner: Option<(Uuid, String)> =
            sqlx::query_as("SELECT owner_id, title FROM meetings WHERE id = $1")
                .bind(meeting_id)
                .fetch_optional(&state.db)
                .await
                .ok()
                .flatten();
        if let Some((owner_id, title)) = owner {
            let payload = serde_json::json!({ "meeting_id": meeting_id, "title": title });
            for org_id in crate::org::orgs_of_user(&state, owner_id).await {
                crate::webhooks::fire(
                    state.clone(),
                    org_id,
                    crate::webhooks::Event {
                        name: "meeting.mom_ready".into(),
                        title: "Delonix Meet".into(),
                        text: format!("Ata pronta: {title}"),
                        payload: payload.clone(),
                    },
                );
            }
        }
    });
}

// ---------- Endpoint de tradução (legendas em tempo real) ----------

#[derive(Deserialize, utoipa::ToSchema)]
pub struct TranslateReq {
    /// Linha de legenda; cortada a 500 caracteres.
    pub text: String,
    /// Língua de destino: `pt` | `en` | `fr` | `es` | `de` | `zh`. Outra dá 400.
    pub target: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct TranslateResp {
    /// O texto traduzido.
    pub text: String,
}

/// Documentação OpenAPI das rotas HTTP deste módulo (`openapi.rs` junta-as).
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(translate_caption),
    components(schemas(TranslateReq, TranslateResp))
)]
pub struct ApiDoc;

/// POST /api/ai/translations — traduz uma linha de legenda. Autenticado; o texto é
/// curto (legendas) e o rate-limit global de /api aplica-se por IP.
#[utoipa::path(
    post, path = "/api/ai/translations", tag = "ai",
    security(("session" = [])),
    request_body = TranslateReq,
    responses(
        (status = 200, body = TranslateResp),
        (status = 400, body = crate::openapi::ErrorBody, description = "texto vazio, língua não suportada, LLM local não configurado, ou a tradução falhou"),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn translate_caption(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(req): Json<TranslateReq>,
) -> Result<Json<TranslateResp>, ApiError> {
    if state.config.ollama_url.is_none() {
        return Err(ApiError::BadRequest(
            "tradução indisponível (sem LLM local)".into(),
        ));
    }
    let text: String = req.text.trim().chars().take(500).collect();
    if text.is_empty() {
        return Err(ApiError::BadRequest("texto vazio".into()));
    }
    match translate(&state, &text, &req.target).await {
        Some(t) => Ok(Json(TranslateResp { text: t })),
        None => Err(ApiError::BadRequest("tradução falhou".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A classificação depende da mensagem da guarda de saída (`net_guard`):
    /// se ela mudar, este teste diz que «não resolve» deixou de ser
    /// «inalcançável».
    #[tokio::test]
    async fn guard_refusals_are_classified() {
        let outbound = Outbound::new(vec![]);
        let o = Ollama::new(&outbound, Some("http://nao-existe.invalid:11434"));
        assert_eq!(
            o.models(Duration::from_secs(1)).await,
            Err(LlmFailure::Unreachable)
        );
        let o = Ollama::new(&outbound, Some("http://169.254.169.254/"));
        assert_eq!(
            o.models(Duration::from_secs(1)).await,
            Err(LlmFailure::Rejected)
        );
        let o = Ollama::new(&outbound, Some("ftp://ollama:11434"));
        assert_eq!(
            o.models(Duration::from_secs(1)).await,
            Err(LlmFailure::Rejected)
        );
        let o = Ollama::new(&outbound, Some("  "));
        assert!(!o.configured());
        assert_eq!(
            o.models(Duration::from_secs(1)).await,
            Err(LlmFailure::NotConfigured)
        );
    }
}
