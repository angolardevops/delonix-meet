//! Harness dos testes de integração: um servidor A SÉRIO numa porta efémera,
//! sobre uma base Postgres própria do teste (`#[sqlx::test]`), e um cliente
//! HTTP a sério.
//!
//! Porquê um listener e não `Router::oneshot`: os handlers de auth e da v1
//! extraem `ConnectInfo<SocketAddr>` (rate-limit por IP). Um `oneshot` não o
//! tem e respondia 500 — o teste provaria o harness, não o produto.
//!
//! Correr: `DATABASE_URL=postgres://delonix:delonix_dev@localhost:5435/delonix_meet cargo test --release --test '*'`
//! Sem `DATABASE_URL`, o `#[sqlx::test]` falha a dizer que a variável falta —
//! não passa a verde por engano.
#![allow(dead_code)]

use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use delonix_server::{build_router, build_state, config::Config, AppState};
use serde_json::{json, Value};
use sqlx::PgPool;

pub const PASSWORD: &str = "UmaPasswordForte123!";

pub struct TestApp {
    pub base: String,
    pub http: reqwest::Client,
    pub state: Arc<AppState>,
    pub db: PgPool,
    /// Vaga no tecto de testes activos em simultâneo (ver `ACTIVE_TESTS`).
    _slot: tokio::sync::OwnedSemaphorePermit,
}

/// Tecto de testes ACTIVOS em simultâneo, partilhado por todos os testes do
/// mesmo binário.
///
/// Porquê: as pools do `#[sqlx::test]` são filhas de UMA pool mestre com 20
/// ligações para o binário inteiro. Com 16+ testes em paralelo, um handler que
/// segura duas ligações ao mesmo tempo (o `org::create_group` abre a
/// `tenant_tx` e depois pede outra ligação em `role_in_org`) espera 30 s pelo
/// `acquire` e responde 500 — medido a 2026-09-16. O tecto não esconde esse
/// defeito (está no relatório da bateria); impede que ele torne a bateria
/// intermitente. Ajustável com `DELONIX_IT_CONCURRENCY`. Medido a 2026-09-16
/// (máquina partilhada, load ~15-40): com 6 ou sem tecto, 2 em 5 corridas de
/// `organization`+`scheduling` falharam; com 3, 0 em 5. O tecto só cobre o
/// CORPO do teste — a criação da base e as migrações do `#[sqlx::test]`
/// correm antes e podem ainda dar `PoolTimedOut` sob carga extrema.
///
/// `DELONIX_IT_LOG=<filtro tracing>` (ex.: `delonix_server=error`) mostra os
/// erros internos que o cliente só vê como `{"error":"internal error"}`.
static ACTIVE_TESTS: std::sync::LazyLock<Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| {
        let n = std::env::var("DELONIX_IT_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3);
        Arc::new(tokio::sync::Semaphore::new(n))
    });

/// Configuração de teste: segredos de dev, cookies sem `Secure`, e um tecto
/// de autenticação alto (os testes registam muitas contas do mesmo IP).
pub fn test_config(extra: &[(&str, &str)]) -> Config {
    let mut vars: HashMap<&str, &str> = HashMap::from([
        ("DELONIX_ALLOW_INSECURE", "1"),
        ("COOKIE_INSECURE", "1"),
        ("AUTH_RATE_PER_MIN", "10000"),
        ("BIND_ADDR", "127.0.0.1:0"),
    ]);
    for (k, v) in extra {
        vars.insert(k, v);
    }
    Config::from_map(&vars)
}

impl TestApp {
    pub async fn spawn(db: PgPool) -> Self {
        Self::spawn_with(db, &[]).await
    }

    pub async fn spawn_with(db: PgPool, extra: &[(&str, &str)]) -> Self {
        if let Ok(filter) = std::env::var("DELONIX_IT_LOG") {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_test_writer()
                .try_init();
        }
        let slot = ACTIVE_TESTS
            .clone()
            .acquire_owned()
            .await
            .expect("semáforo dos testes fechado");
        let mut config = test_config(extra);
        let dir = std::env::temp_dir().join(format!("delonix-it-{}", uuid::Uuid::new_v4()));
        config.recordings_dir = dir;
        let state = build_state(config, db.clone()).await;
        let app = build_router(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        Self {
            base: format!("http://{addr}"),
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            state,
            db,
            _slot: slot,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    /// Pedido JSON com token opcional. Devolve (status, corpo JSON ou `Null`).
    pub async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut rb = self.http.request(method, self.url(path));
        if let Some(t) = token {
            rb = rb.bearer_auth(t);
        }
        if let Some(b) = body {
            rb = rb.json(&b);
        }
        let res = rb.send().await.expect("pedido HTTP falhou");
        let status = res.status().as_u16();
        let text = res.text().await.unwrap_or_default();
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    pub async fn get(&self, path: &str, token: Option<&str>) -> (u16, Value) {
        self.call(reqwest::Method::GET, path, token, None).await
    }
    pub async fn post(&self, path: &str, token: Option<&str>, body: Value) -> (u16, Value) {
        self.call(reqwest::Method::POST, path, token, Some(body))
            .await
    }
    pub async fn patch(&self, path: &str, token: Option<&str>, body: Value) -> (u16, Value) {
        self.call(reqwest::Method::PATCH, path, token, Some(body))
            .await
    }
    pub async fn put(&self, path: &str, token: Option<&str>, body: Value) -> (u16, Value) {
        self.call(reqwest::Method::PUT, path, token, Some(body))
            .await
    }
    pub async fn delete(&self, path: &str, token: Option<&str>) -> (u16, Value) {
        self.call(reqwest::Method::DELETE, path, token, None).await
    }

    /// Regista uma organização nova (domínio próprio) com o seu administrador.
    pub async fn new_org(&self, domain: &str) -> Account {
        let email = format!("admin@{domain}");
        let (st, reg) = self
            .post(
                "/api/auth/register",
                None,
                json!({"org_name": format!("Org {domain}"), "email": email,
                       "username": format!("admin-{domain}"), "password": PASSWORD}),
            )
            .await;
        assert!(st < 300, "registo de {email} falhou: {st} {reg}");
        let acc = self.login(&email).await;
        let (st, orgs) = self.get("/api/orgs", Some(&acc.token)).await;
        assert_eq!(st, 200, "GET /api/orgs: {orgs}");
        Account {
            org_id: orgs[0]["id"].as_str().map(str::to_string),
            ..acc
        }
    }

    pub async fn login(&self, email: &str) -> Account {
        let (st, body) = self
            .post(
                "/api/auth/login",
                None,
                json!({"email": email, "password": PASSWORD}),
            )
            .await;
        assert_eq!(st, 200, "login de {email} falhou: {body}");
        Account {
            email: email.to_string(),
            token: body["access_token"].as_str().unwrap().to_string(),
            user_id: body["user"]["id"].as_str().unwrap().to_string(),
            org_id: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Account {
    pub email: String,
    pub token: String,
    pub user_id: String,
    pub org_id: Option<String>,
}

impl Account {
    pub fn org(&self) -> &str {
        self.org_id.as_deref().expect("conta sem org")
    }
}

// ---------------------------------------------------------------------------
//  Ajudas acrescentadas pela bateria de caracterização (identity, organization,
//  scheduling, api_v1, content, public). Só acrescentam — as assinaturas de
//  cima ficam como estavam.
// ---------------------------------------------------------------------------

/// Um id que ninguém criou. Onde é usado, um `404` NÃO distingue «não é teu»
/// de «não existe» — e o teste di-lo.
pub const INVENTED_ID: &str = "00000000-0000-4000-8000-000000000000";

/// PNG mínimo de 1×1 — o handler dos quadros valida a assinatura PNG.
pub const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

/// Resposta crua: estado, cabeçalhos e texto. Para o que o `call` JSON esconde
/// (cookies, `text/calendar`, `/metrics`).
pub struct RawResponse {
    pub status: u16,
    pub headers: reqwest::header::HeaderMap,
    pub text: String,
}

impl RawResponse {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.text).unwrap_or(Value::Null)
    }
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    }
    /// Todos os `Set-Cookie` da resposta.
    pub fn set_cookies(&self) -> Vec<String> {
        self.headers
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok().map(str::to_string))
            .collect()
    }
}

impl TestApp {
    /// Pedido com cabeçalhos arbitrários; devolve a resposta crua.
    pub async fn raw(
        &self,
        method: reqwest::Method,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<Value>,
    ) -> RawResponse {
        let mut rb = self.http.request(method, self.url(path));
        for (k, v) in headers {
            rb = rb.header(*k, *v);
        }
        if let Some(b) = body {
            rb = rb.json(&b);
        }
        let res = rb.send().await.expect("pedido HTTP falhou");
        let status = res.status().as_u16();
        let headers = res.headers().clone();
        let text = res.text().await.unwrap_or_default();
        RawResponse {
            status,
            headers,
            text,
        }
    }

    /// Adiciona um colaborador à org de `admin` (pelo BFF) e entra com ele.
    /// `local` é a parte local do email; o domínio é o do administrador.
    pub async fn add_member(&self, admin: &Account, local: &str, role: &str) -> Account {
        let domain = admin.email.split('@').nth(1).unwrap();
        let email = format!("{local}@{domain}");
        let (st, body) = self
            .post(
                &format!("/api/orgs/{}/employees", admin.org()),
                Some(&admin.token),
                json!({"email": email, "username": format!("{local}-{domain}"),
                       "password": PASSWORD, "role": role, "title": local}),
            )
            .await;
        assert_eq!(st, 200, "add_employee {email}: {body}");
        let acc = self.login(&email).await;
        Account {
            org_id: admin.org_id.clone(),
            ..acc
        }
    }

    /// Cria uma chave de API da org de `admin`. Devolve (id, chave `dlx_`).
    pub async fn api_key(&self, admin: &Account) -> (String, String) {
        let (st, body) = self
            .post(
                &format!("/api/orgs/{}/api-keys", admin.org()),
                Some(&admin.token),
                json!({"name": "chave-de-teste"}),
            )
            .await;
        assert_eq!(st, 200, "criar chave de API: {body}");
        (
            body["id"].as_str().unwrap().to_string(),
            body["key"].as_str().unwrap().to_string(),
        )
    }

    /// Cria uma sala (BFF) e devolve o JSON da sala.
    pub async fn new_room(&self, owner: &Account, name: &str) -> Value {
        let (st, body) = self
            .post(
                "/api/rooms",
                Some(&owner.token),
                json!({"name": name, "topology": "sfu"}),
            )
            .await;
        assert_eq!(st, 200, "criar sala: {body}");
        body
    }

    /// Cria uma reunião (BFF) daqui a uma hora e devolve o JSON.
    pub async fn new_meeting(&self, owner: &Account, title: &str, invitees: &[&str]) -> Value {
        let starts = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        let (st, body) = self
            .post(
                "/api/meetings",
                Some(&owner.token),
                json!({"title": title, "kind": "video", "starts_at": starts,
                       "duration_min": 30, "invitee_ids": invitees}),
            )
            .await;
        assert_eq!(st, 200, "criar reunião: {body}");
        body
    }

    /// Arquiva um membro directamente na base (como os e2e fazem): marca
    /// `archived_at`, que é o que o `remove_employee` faz.
    pub async fn archive_member(&self, org_id: &str, user_id: &str) {
        sqlx::query(
            "UPDATE org_members SET archived_at = now()
             WHERE org_id = $1::uuid AND user_id = $2::uuid",
        )
        .bind(org_id)
        .bind(user_id)
        .execute(&self.db)
        .await
        .unwrap();
    }

    /// Insere uma gravação `ready` SEM ficheiro. Serve para provar
    /// autorização: com acesso, o download chega à leitura do ficheiro e dá
    /// 404; sem acesso, é recusado antes (401). Não se usa o upload porque o
    /// handler escreve em `RECORDINGS_DIR` do processo (por omissão
    /// `./recordings`), ignorando o `config.recordings_dir` do harness.
    pub async fn insert_recording(&self, room_id: &str, uploader_id: &str) -> String {
        let (id,): (uuid::Uuid,) = sqlx::query_as(
            "INSERT INTO recordings (room_id, uploader_id, filename, size_bytes)
             VALUES ($1::uuid, $2::uuid, 'teste.webm', 4) RETURNING id",
        )
        .bind(room_id)
        .bind(uploader_id)
        .fetch_one(&self.db)
        .await
        .unwrap();
        id.to_string()
    }
}

/// Descodifica o payload de um JWT (sem verificar a assinatura — só para ler
/// as claims que o servidor emitiu).
pub fn jwt_claims(token: &str) -> Value {
    use base64::Engine;
    let payload = token.split('.').nth(1).expect("JWT sem payload");
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .expect("payload base64url");
    serde_json::from_slice(&bytes).expect("payload JSON")
}

/// Verifica que uma resposta NÃO é 2xx e que o corpo não menciona `leak`.
#[track_caller]
pub fn assert_denied(what: &str, st: u16, body: &Value, leak: &str) {
    assert!(
        !(200..300).contains(&st),
        "{what}: devia ser recusado e devolveu {st}: {body}"
    );
    assert!(
        !body.to_string().contains(leak),
        "{what}: o corpo da recusa traz dados do outro inquilino: {body}"
    );
}
