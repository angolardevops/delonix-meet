//! IA local via Ollama (07-ollama.yaml) — tradução de legendas em tempo real e
//! resumo elegante da ata (MoM). Soberania by design: o texto das reuniões vai
//! apenas ao LLM in-cluster, nunca a uma cloud externa. Sem OLLAMA_URL tudo
//! degrada silenciosamente (fail-open): o MoM fica por regras no cliente.

use std::sync::Arc;
use std::time::Duration;

use axum::{extract::State, Json};
use serde::Deserialize;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

#[derive(Deserialize)]
struct GenResponse {
    response: String,
}

/// Chamada única ao /api/generate do Ollama (sem streaming).
async fn generate(
    state: &AppState,
    model: &str,
    prompt: String,
    timeout: Duration,
) -> Option<String> {
    let base = state.config.ollama_url.as_ref()?;
    let client = reqwest::Client::builder().timeout(timeout).build().ok()?;
    let resp = client
        .post(format!("{base}/api/generate"))
        .json(&serde_json::json!({
            "model": model,
            "prompt": prompt,
            "stream": false,
            "options": { "temperature": 0.2 }
        }))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    let body: GenResponse = resp.json().await.ok()?;
    let out = body.response.trim().to_string();
    (!out.is_empty()).then_some(out)
}

/// Traduz uma linha de legenda para o idioma alvo (código curto: pt/en/fr/es…).
pub async fn translate(state: &AppState, text: &str, target: &str) -> Option<String> {
    let lang = match target {
        "pt" => "European Portuguese",
        "en" => "English",
        "fr" => "French",
        "es" => "Spanish",
        "de" => "German",
        _ => return None,
    };
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

#[derive(Deserialize)]
pub struct TranslateReq {
    pub text: String,
    pub target: String,
}

/// POST /api/translate — traduz uma linha de legenda. Autenticado; o texto é
/// curto (legendas) e o rate-limit global de /api aplica-se por IP.
pub async fn translate_caption(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(req): Json<TranslateReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
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
        Some(t) => Ok(Json(serde_json::json!({ "text": t }))),
        None => Err(ApiError::BadRequest("tradução falhou".into())),
    }
}
