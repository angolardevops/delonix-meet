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
}

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
