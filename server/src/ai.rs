//! IA local via Ollama (07-ollama.yaml) — tradução de legendas em tempo real e
//! resumo elegante da ata (MoM). Soberania by design: o texto das reuniões vai
//! apenas ao LLM in-cluster, nunca a uma cloud externa. Sem OLLAMA_URL tudo
//! degrada silenciosamente (fail-open): o MoM fica por regras no cliente.

use std::sync::Arc;
use std::time::Duration;

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

#[derive(Deserialize)]
struct GenResponse {
    response: String,
}

/// Porque é que o LLM local não deu uma resposta aproveitável.
///
/// Existe para que quem chama possa dizer a VERDADE ao cliente: «o modelo não
/// está instalado» e «o serviço não responde» resolvem-se de maneiras
/// diferentes, e um `None` fazia de tudo «LLM sem resposta».
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LlmError {
    /// `OLLAMA_URL` vazio: a IA está desligada neste servidor.
    NotConfigured,
    /// Não se conseguiu ligar ao Ollama (recusado, DNS, rede).
    Unreachable,
    /// O Ollama não respondeu dentro do tecto dado.
    Timeout(Duration),
    /// O Ollama respondeu que o modelo não está instalado (`404`).
    ModelMissing(String),
    /// O Ollama respondeu com um estado de erro.
    Upstream(u16),
    /// A resposta chegou mas não se consegue usar (não é JSON, vem vazia,
    /// ou o texto do modelo não tem a forma pedida).
    BadResponse,
}

impl LlmError {
    /// Mensagem para o cliente, em português europeu. Nunca inclui o URL do
    /// Ollama: é um endereço interno do cluster.
    pub(crate) fn client_message(&self) -> String {
        match self {
            LlmError::NotConfigured => {
                "IA local não configurada neste servidor (OLLAMA_URL)".into()
            }
            LlmError::Unreachable => "o serviço Ollama não responde".into(),
            LlmError::Timeout(t) => format!("o modelo não respondeu em {} s", t.as_secs()),
            LlmError::ModelMissing(m) => format!("o modelo «{m}» não está instalado no Ollama"),
            LlmError::Upstream(_) | LlmError::BadResponse => {
                "o modelo devolveu uma resposta que não se consegue usar".into()
            }
        }
    }
}

impl From<LlmError> for ApiError {
    fn from(e: LlmError) -> Self {
        ApiError::ServiceUnavailable(e.client_message())
    }
}

/// Um erro do `reqwest` classificado: tecto de tempo ou falta de ligação.
fn classify_transport(e: &reqwest::Error, timeout: Duration) -> LlmError {
    if e.is_timeout() {
        LlmError::Timeout(timeout)
    } else if e.is_connect() || e.is_request() {
        LlmError::Unreachable
    } else {
        LlmError::BadResponse
    }
}

/// Um `404` do Ollama no `/api/generate` quer dizer modelo em falta
/// (`{"error":"model \"x\" not found, try pulling it first"}`).
fn classify_status(status: reqwest::StatusCode, body: &str, model: &str) -> LlmError {
    if status == reqwest::StatusCode::NOT_FOUND && body.contains("not found") {
        LlmError::ModelMissing(model.to_string())
    } else {
        LlmError::Upstream(status.as_u16())
    }
}

/// Chamada única ao `/api/generate` do Ollama (sem streaming), com o erro
/// verdadeiro. Não depende do `AppState` para se poder testar contra um
/// Ollama falso. `json_output` pede ao Ollama `"format": "json"`.
pub(crate) async fn ollama_generate(
    client: &reqwest::Client,
    base_url: Option<&str>,
    model: &str,
    prompt: &str,
    timeout: Duration,
    json_output: bool,
) -> Result<String, LlmError> {
    let base = base_url
        .map(|b| b.trim_end_matches('/'))
        .filter(|b| !b.is_empty())
        .ok_or(LlmError::NotConfigured)?;
    let mut body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "stream": false,
        "options": { "temperature": 0.2 }
    });
    if json_output {
        body["format"] = "json".into();
    }
    let resp = client
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
    let parsed: GenResponse = serde_json::from_str(&text).map_err(|_| LlmError::BadResponse)?;
    let out = parsed.response.trim().to_string();
    if out.is_empty() {
        return Err(LlmError::BadResponse);
    }
    Ok(out)
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

/// Nomes dos modelos instalados (`GET /api/tags`).
pub(crate) async fn ollama_models(
    client: &reqwest::Client,
    base_url: Option<&str>,
    timeout: Duration,
) -> Result<Vec<String>, LlmError> {
    let base = base_url
        .map(|b| b.trim_end_matches('/'))
        .filter(|b| !b.is_empty())
        .ok_or(LlmError::NotConfigured)?;
    let resp = client
        .get(format!("{base}/api/tags"))
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| classify_transport(&e, timeout))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(LlmError::Upstream(status.as_u16()));
    }
    let text = resp
        .text()
        .await
        .map_err(|e| classify_transport(&e, timeout))?;
    let tags: TagsResponse = serde_json::from_str(&text).map_err(|_| LlmError::BadResponse)?;
    Ok(tags.models.into_iter().map(|m| m.name).collect())
}

/// `model` está na lista do Ollama — igual, ou igual sem o sufixo `:latest`
/// (o Ollama lista `llama3:latest` para quem pediu `llama3`).
pub(crate) fn model_listed(installed: &[String], model: &str) -> bool {
    let bare = |n: &str| n.strip_suffix(":latest").unwrap_or(n).to_string();
    let want = bare(model.trim());
    installed.iter().any(|n| n == model || bare(n) == want)
}

/// Chamada ao LLM para quem só quer saber se há texto (tradução, acta,
/// capítulos). A causa da falha fica no log; o cliente destes caminhos tem
/// sempre um recurso (acta por regras, legenda por traduzir).
pub(crate) async fn generate(
    state: &AppState,
    model: &str,
    prompt: String,
    timeout: Duration,
) -> Option<String> {
    // Destino do operador (OLLAMA_URL): rede privada sim, metadados não.
    match ollama_generate(
        state.outbound.operator(),
        state.config.ollama_url.as_deref(),
        model,
        &prompt,
        timeout,
        false,
    )
    .await
    {
        Ok(t) => Some(t),
        Err(LlmError::NotConfigured) => None,
        Err(e) => {
            tracing::warn!(model, error = ?e, "LLM local sem resposta aproveitável");
            None
        }
    }
}

/// Línguas de chegada que o prompt de tradução conhece (código curto).
///
/// Umbundu, Kimbundu e Kikongo NÃO estão: nenhum modelo local as traduz com
/// qualidade que se possa pôr numa legenda, e uma língua anunciada que devolve
/// texto inventado é pior do que uma que não está na lista.
pub const TRANSLATE_TARGETS: &[&str] = &["pt", "en", "fr", "es", "de", "zh"];

fn target_name(target: &str) -> Option<&'static str> {
    Some(match target {
        "pt" => "European Portuguese",
        "en" => "English",
        "fr" => "French",
        "es" => "Spanish",
        "de" => "German",
        "zh" => "Simplified Chinese",
        _ => return None,
    })
}

/// A tradução para `target` (código curto) é suportada.
pub fn supports_target(target: &str) -> bool {
    target_name(target).is_some()
}

/// Traduz uma linha de legenda para o idioma alvo (código curto: pt/en/fr/es…).
pub async fn translate(state: &AppState, text: &str, target: &str) -> Option<String> {
    let lang = target_name(target)?;
    let prompt = format!(
        "Translate the following spoken caption to {lang}. \
         Output ONLY the translation, no quotes, no explanations.\n\nCaption: {text}"
    );
    generate(
        state,
        &state.config.ollama_model_translate,
        prompt,
        Duration::from_secs(20),
    )
    .await
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
                        name: "meeting.mom_ready",
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
    /// Língua de destino: `pt` | `en` | `fr` | `es` | `de`. Outra dá 400.
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

/// Um Ollama falso para os testes: responde ao `/api/generate` e ao
/// `/api/tags` com o que o teste pedir. Corre num porto efémero de
/// `127.0.0.1`, dentro do processo de teste.
#[cfg(test)]
pub(crate) mod fake_ollama {
    use axum::{
        extract::State,
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::{get, post},
        Json, Router,
    };
    use std::sync::Arc;
    use std::time::Duration;

    #[derive(Clone)]
    pub(crate) enum Generate {
        /// `200` com `{"response": <texto>}`.
        Answer(String),
        /// Estado e corpo cru.
        Status(u16, String),
        /// Dorme antes de responder `{"response":"tarde"}`.
        Sleep(Duration),
    }

    #[derive(Clone)]
    pub(crate) struct Fake {
        pub generate: Generate,
        pub models: Vec<String>,
    }

    async fn generate(State(f): State<Arc<Fake>>) -> Response {
        match &f.generate {
            Generate::Answer(a) => Json(serde_json::json!({ "response": a })).into_response(),
            Generate::Status(code, body) => (
                StatusCode::from_u16(*code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                body.clone(),
            )
                .into_response(),
            Generate::Sleep(d) => {
                tokio::time::sleep(*d).await;
                Json(serde_json::json!({ "response": "tarde" })).into_response()
            }
        }
    }

    async fn tags(State(f): State<Arc<Fake>>) -> Response {
        let models: Vec<_> = f
            .models
            .iter()
            .map(|n| serde_json::json!({ "name": n, "model": n }))
            .collect();
        Json(serde_json::json!({ "models": models })).into_response()
    }

    /// Arranca o falso e devolve o URL base (`http://127.0.0.1:<porto>`).
    pub(crate) async fn start(fake: Fake) -> String {
        let app = Router::new()
            .route("/api/generate", post(generate))
            .route("/api/tags", get(tags))
            .with_state(Arc::new(fake));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://{addr}")
    }

    /// Um URL onde de certeza nada escuta: liga-se um porto e larga-se.
    pub(crate) async fn closed_url() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        format!("http://{addr}")
    }

    pub(crate) fn answer(a: &str) -> Fake {
        Fake {
            generate: Generate::Answer(a.into()),
            models: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake_ollama::{self, Fake, Generate};
    use super::*;

    fn client() -> reqwest::Client {
        crate::net_guard::Outbound::new(Vec::new())
            .operator()
            .clone()
    }

    const T: Duration = Duration::from_secs(5);

    #[tokio::test]
    async fn resposta_do_modelo_chega_inteira() {
        let url = fake_ollama::start(fake_ollama::answer("  olá mundo \n")).await;
        let r = ollama_generate(&client(), Some(&url), "m", "p", T, false).await;
        assert_eq!(r, Ok("olá mundo".to_string()));
    }

    #[tokio::test]
    async fn sem_url_diz_que_nao_esta_configurado() {
        let r = ollama_generate(&client(), None, "m", "p", T, false).await;
        assert_eq!(r, Err(LlmError::NotConfigured));
        let r = ollama_generate(&client(), Some(""), "m", "p", T, false).await;
        assert_eq!(r, Err(LlmError::NotConfigured));
    }

    #[tokio::test]
    async fn modelo_em_falta_e_nomeado() {
        let url = fake_ollama::start(Fake {
            generate: Generate::Status(
                404,
                r#"{"error":"model \"qwen2.5:7b\" not found, try pulling it first"}"#.into(),
            ),
            models: vec![],
        })
        .await;
        let r = ollama_generate(&client(), Some(&url), "qwen2.5:7b", "p", T, false).await;
        assert_eq!(r, Err(LlmError::ModelMissing("qwen2.5:7b".into())));
        assert_eq!(
            r.unwrap_err().client_message(),
            "o modelo «qwen2.5:7b» não está instalado no Ollama"
        );
    }

    #[tokio::test]
    async fn outro_erro_do_ollama_e_upstream() {
        let url = fake_ollama::start(Fake {
            generate: Generate::Status(500, r#"{"error":"out of memory"}"#.into()),
            models: vec![],
        })
        .await;
        let r = ollama_generate(&client(), Some(&url), "m", "p", T, false).await;
        assert_eq!(r, Err(LlmError::Upstream(500)));
    }

    #[tokio::test]
    async fn tecto_de_tempo_e_timeout_e_nao_ligacao() {
        let url = fake_ollama::start(Fake {
            generate: Generate::Sleep(Duration::from_secs(3)),
            models: vec![],
        })
        .await;
        let t = Duration::from_millis(300);
        let r = ollama_generate(&client(), Some(&url), "m", "p", t, false).await;
        assert_eq!(r, Err(LlmError::Timeout(t)));
    }

    #[tokio::test]
    async fn porto_fechado_e_inalcancavel() {
        let url = fake_ollama::closed_url().await;
        let r = ollama_generate(&client(), Some(&url), "m", "p", T, false).await;
        assert_eq!(r, Err(LlmError::Unreachable));
        assert_eq!(
            r.unwrap_err().client_message(),
            "o serviço Ollama não responde"
        );
    }

    #[tokio::test]
    async fn resposta_que_nao_e_json_e_bad_response() {
        let url = fake_ollama::start(Fake {
            generate: Generate::Status(200, "<html>proxy</html>".into()),
            models: vec![],
        })
        .await;
        let r = ollama_generate(&client(), Some(&url), "m", "p", T, false).await;
        assert_eq!(r, Err(LlmError::BadResponse));
        // Resposta vazia do modelo também não serve.
        let url = fake_ollama::start(fake_ollama::answer("   ")).await;
        let r = ollama_generate(&client(), Some(&url), "m", "p", T, false).await;
        assert_eq!(r, Err(LlmError::BadResponse));
    }

    #[tokio::test]
    async fn lista_de_modelos_do_tags() {
        let url = fake_ollama::start(Fake {
            generate: Generate::Answer("x".into()),
            models: vec!["qwen2.5:1.5b".into(), "llama3:latest".into()],
        })
        .await;
        let m = ollama_models(&client(), Some(&url), T).await.unwrap();
        assert!(model_listed(&m, "qwen2.5:1.5b"));
        assert!(model_listed(&m, "llama3"));
        assert!(model_listed(&m, "llama3:latest"));
        assert!(!model_listed(&m, "qwen2.5:7b"));
        assert!(!model_listed(&m, "qwen2.5"));
    }

    #[test]
    fn mensagens_nunca_levam_o_url() {
        for e in [
            LlmError::NotConfigured,
            LlmError::Unreachable,
            LlmError::Timeout(Duration::from_secs(120)),
            LlmError::ModelMissing("m".into()),
            LlmError::Upstream(502),
            LlmError::BadResponse,
        ] {
            let m = e.client_message();
            assert!(!m.contains("http"), "{m}");
        }
        assert_eq!(
            LlmError::Timeout(Duration::from_secs(120)).client_message(),
            "o modelo não respondeu em 120 s"
        );
    }
}
