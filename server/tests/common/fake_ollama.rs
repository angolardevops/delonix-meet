//! Um Ollama FALSO para os testes: responde ao `/api/generate` e ao
//! `/api/tags` com o que o teste pedir, num porto efémero de `127.0.0.1`,
//! dentro do processo de teste. Só existe aqui — em produção não há modo
//! «falso» (sem `OLLAMA_URL`, a API diz `ai.not_configured`).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use tokio::sync::Semaphore;

#[derive(Clone, Debug)]
pub enum Reply {
    /// `200` com `{"response": <texto>}`.
    Answer(String),
    /// Estado e corpo crus.
    Status(u16, String),
    /// Dorme antes de responder `{"response":"tarde"}`.
    Sleep(Duration),
    /// Espera por [`FakeOllama::release`] e depois responde o texto.
    Hold(String),
    /// Traduz: devolve `<prefixo><texto da legenda>`. Na chamada número
    /// `fail_at` (1-based) devolve uma resposta vazia (inutilizável).
    Translate {
        prefix: String,
        fail_at: Option<usize>,
    },
}

struct Shared {
    reply: Mutex<Reply>,
    models: Mutex<Vec<String>>,
    tags_delay: Mutex<Option<Duration>>,
    calls: AtomicUsize,
    release: Semaphore,
    entered: Semaphore,
    prompts: Mutex<Vec<Value>>,
}

pub struct FakeOllama {
    pub url: String,
    shared: Arc<Shared>,
}

async fn generate(State(s): State<Arc<Shared>>, Json(body): Json<Value>) -> Response {
    let n = s.calls.fetch_add(1, Ordering::SeqCst) + 1;
    s.prompts.lock().unwrap().push(body.clone());
    s.entered.add_permits(1);
    let reply = s.reply.lock().unwrap().clone();
    let answer = |t: &str| Json(json!({ "model": body["model"], "response": t, "done": true }));
    match reply {
        Reply::Answer(a) => answer(&a).into_response(),
        Reply::Status(code, raw) => (
            StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            raw,
        )
            .into_response(),
        Reply::Sleep(d) => {
            tokio::time::sleep(d).await;
            answer("tarde").into_response()
        }
        Reply::Hold(a) => {
            let permit = s.release.acquire().await.unwrap();
            permit.forget();
            answer(&a).into_response()
        }
        Reply::Translate { prefix, fail_at } => {
            if fail_at == Some(n) {
                return answer("   ").into_response();
            }
            let prompt = body["prompt"].as_str().unwrap_or_default();
            let caption = prompt.rsplit("Caption: ").next().unwrap_or_default();
            answer(&format!("{prefix}{caption}")).into_response()
        }
    }
}

async fn tags(State(s): State<Arc<Shared>>) -> Response {
    let delay = *s.tags_delay.lock().unwrap();
    if let Some(d) = delay {
        tokio::time::sleep(d).await;
    }
    let models: Vec<Value> = s
        .models
        .lock()
        .unwrap()
        .iter()
        .map(|n| json!({ "name": n, "model": n }))
        .collect();
    Json(json!({ "models": models })).into_response()
}

impl FakeOllama {
    /// Arranca o falso com os modelos instalados e a resposta do `/api/generate`.
    pub async fn start(models: &[&str], reply: Reply) -> Self {
        let shared = Arc::new(Shared {
            reply: Mutex::new(reply),
            models: Mutex::new(models.iter().map(|m| m.to_string()).collect()),
            tags_delay: Mutex::new(None),
            calls: AtomicUsize::new(0),
            release: Semaphore::new(0),
            entered: Semaphore::new(0),
            prompts: Mutex::new(Vec::new()),
        });
        let app = Router::new()
            .route("/api/generate", post(generate))
            .route("/api/tags", get(tags))
            .with_state(shared.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self {
            url: format!("http://{addr}"),
            shared,
        }
    }

    pub fn set_reply(&self, reply: Reply) {
        *self.shared.reply.lock().unwrap() = reply;
    }

    pub fn set_models(&self, models: &[&str]) {
        *self.shared.models.lock().unwrap() = models.iter().map(|m| m.to_string()).collect();
    }

    pub fn set_tags_delay(&self, d: Option<Duration>) {
        *self.shared.tags_delay.lock().unwrap() = d;
    }

    /// Chamadas ao `/api/generate` até agora.
    pub fn calls(&self) -> usize {
        self.shared.calls.load(Ordering::SeqCst)
    }

    /// Os corpos recebidos no `/api/generate`.
    pub fn prompts(&self) -> Vec<Value> {
        self.shared.prompts.lock().unwrap().clone()
    }

    /// Deixa `n` pedidos em `Hold` responder.
    pub fn release(&self, n: usize) {
        self.shared.release.add_permits(n);
    }

    /// Espera até um pedido ao `/api/generate` ter chegado (um por chamada).
    pub async fn wait_entered(&self) {
        tokio::time::timeout(Duration::from_secs(20), self.shared.entered.acquire())
            .await
            .expect("o Ollama falso não recebeu o pedido a tempo")
            .unwrap()
            .forget();
    }

    /// Um URL onde de certeza nada escuta: liga-se um porto e larga-se.
    pub async fn closed_url() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        format!("http://{addr}")
    }
}
