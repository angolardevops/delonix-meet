//! Segredos de integração em repouso (auditoria 2026-09-16, S5) contra
//! Postgres real, um receptor HTTP real em 127.0.0.1 e o servidor a sério.
//!
//! Três colunas: `org_webhooks.secret`, `org_sso_configs.client_secret`,
//! `platform_storage.webdav_password`. Para cada uma prova-se: a coluna fica
//! `enc:v1:` e sem o texto; quem USA o segredo recebe o original; nenhuma
//! resposta o devolve; o herdado em claro continua a servir e é cifrado pela
//! tarefa; sem chaves a escrita é `422` e a leitura herdada continua; e um
//! valor cifrado copiado para outra linha não abre.
mod common;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::any,
    Router,
};
use base64::Engine;
use common::{test_config, Account, TestApp};
use delonix_meet_core::secret_box::SecretBox;
use delonix_server::secrets_at_rest::{count_legacy, reseal_legacy, ResealReport};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;

const HOOK_SECRET: &str = "hmac-original-do-cliente";
const SSO_SECRET: &str = "oidc-client-secret-do-idp";
const DAV_PASSWORD: &str = "password-do-nextcloud";

/// Um pedido recebido: método, cabeçalhos e corpo.
#[derive(Clone)]
struct Received {
    method: String,
    headers: HeaderMap,
    body: Bytes,
}

#[derive(Clone)]
struct Receiver {
    got: Arc<Mutex<Vec<Received>>>,
    url: String,
}

impl Receiver {
    /// Aceita qualquer método em `/hook` (o teste WebDAV faz `PROPFIND`) e
    /// responde `207` a um `PROPFIND`, `200` ao resto.
    async fn spawn() -> Self {
        let got = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route(
                "/hook",
                any(
                    |State(got): State<Arc<Mutex<Vec<Received>>>>,
                     method: axum::http::Method,
                     headers: HeaderMap,
                     body: Bytes| async move {
                        let propfind = method.as_str() == "PROPFIND";
                        got.lock().unwrap().push(Received {
                            method: method.to_string(),
                            headers,
                            body,
                        });
                        if propfind {
                            StatusCode::MULTI_STATUS
                        } else {
                            StatusCode::OK
                        }
                    },
                ),
            )
            .with_state(got.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            got,
            url: format!("http://127.0.0.1:{}/hook", addr.port()),
        }
    }

    fn received(&self) -> Vec<Received> {
        self.got.lock().unwrap().clone()
    }
}

fn signature(secret: &str, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

fn basic(user: &str, password: &str) -> String {
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"))
    )
}

fn hooks(org: &str) -> String {
    format!("/api/orgs/{org}/webhooks")
}

fn access_token_for(app: &TestApp, user_id: uuid::Uuid) -> String {
    let now = chrono::Utc::now().timestamp();
    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &json!({"sub": user_id, "typ": "access", "iat": now, "exp": now + 600}),
        &jsonwebtoken::EncodingKey::from_secret(app.state.config.jwt_secret.as_bytes()),
    )
    .unwrap()
}

async fn stored(app: &TestApp, sql: &str, id: &str) -> String {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

async fn hook_secret(app: &TestApp, hook: &str) -> String {
    stored(
        app,
        "SELECT secret FROM org_webhooks WHERE id = $1::uuid",
        hook,
    )
    .await
}

async fn sso_secret(app: &TestApp, org: &str) -> String {
    stored(
        app,
        "SELECT client_secret FROM org_sso_configs WHERE org_id = $1::uuid",
        org,
    )
    .await
}

async fn dav_password(app: &TestApp) -> String {
    sqlx::query_scalar("SELECT webdav_password FROM platform_storage WHERE id = 1")
        .fetch_one(&app.db)
        .await
        .unwrap()
}

fn assert_sealed(value: &str, plain: &str) {
    assert!(value.starts_with("enc:v1:"), "não está cifrado: {value}");
    assert!(!value.contains(plain), "o texto claro está na coluna");
}

/// Espera até haver `n` entregas FECHADAS do webhook e devolve-as.
async fn wait_final(app: &TestApp, admin: &Account, hook: &str, n: usize) -> Vec<Value> {
    let path = format!("{}/{hook}/deliveries?page_size=100", hooks(admin.org()));
    for _ in 0..200 {
        let (st, page) = app.get(&path, Some(&admin.token)).await;
        assert_eq!(st, 200, "{page}");
        let items = page["items"].as_array().unwrap().clone();
        if items.len() >= n && items.iter().all(|d| d["status"] != "pending") {
            return items;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("as entregas não fecharam a tempo");
}

/// Webhook legado inserido por SQL, com o segredo tal como vier.
async fn insert_hook(app: &TestApp, admin: &Account, url: &str, secret: &str) -> String {
    let id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO org_webhooks (org_id, kind, url, secret, events, created_by)
         VALUES ($1::uuid, 'generic', $2, $3, 'meeting.created', $4::uuid) RETURNING id",
    )
    .bind(admin.org())
    .bind(url)
    .bind(secret)
    .bind(&admin.user_id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    id.to_string()
}

async fn put_storage(
    app: &TestApp,
    token: &str,
    url: &str,
    password: Option<&str>,
) -> (u16, Value) {
    let mut body = json!({"storage_type": "webdav", "webdav_url": url, "webdav_user": "meet"});
    if let Some(p) = password {
        body["webdav_password"] = json!(p);
    }
    app.put("/api/operator/v1/storage", Some(token), body).await
}

async fn test_storage(app: &TestApp, token: &str) -> (u16, Value) {
    app.post("/api/operator/v1/storage/test", Some(token), json!({}))
        .await
}

fn open_sso(
    app: &TestApp,
    stored: &str,
    org: &str,
) -> Result<String, delonix_meet_core::DomainError> {
    app.state
        .config
        .secret_box
        .as_ref()
        .unwrap()
        .open(stored, &format!("org_sso_configs.client_secret:{org}"))
}

#[sqlx::test(migrations = "./migrations")]
async fn webhook_secret_is_sealed_and_delivery_signs_with_the_original(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, &[("WEBHOOK_ALLOW_HOSTS", "127.0.0.1")]).await;
    let rx = Receiver::spawn().await;
    let a = app.new_org("alfa.test").await;
    let (st, created) = app
        .post(
            &hooks(a.org()),
            Some(&a.token),
            json!({"kind": "generic", "url": rx.url, "secret": HOOK_SECRET, "events": "meeting.created"}),
        )
        .await;
    assert_eq!(st, 200, "{created}");
    let hook = created["id"].as_str().unwrap().to_string();
    assert_sealed(&hook_secret(&app, &hook).await, HOOK_SECRET);

    // Nenhuma resposta traz o segredo, nem cifrado.
    let (_, one) = app
        .get(&format!("{}/{hook}", hooks(a.org())), Some(&a.token))
        .await;
    let (_, list) = app.get(&hooks(a.org()), Some(&a.token)).await;
    for v in [&created, &one, &list] {
        let s = v.to_string();
        assert!(!s.contains(HOOK_SECRET) && !s.contains("enc:v1:"), "{s}");
    }

    // O receptor recebe a assinatura calculada com o segredo ORIGINAL.
    app.new_meeting(&a, "Reunião assinada", &[]).await;
    let items = wait_final(&app, &a, &hook, 1).await;
    assert_eq!(items[0]["status"], "succeeded", "{}", items[0]);
    let got = rx.received();
    assert_eq!(got.len(), 1);
    assert_eq!(
        got[0].headers["x-delonix-signature"],
        signature(HOOK_SECRET, &got[0].body).as_str()
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn sso_client_secret_is_sealed_and_never_returned(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let sso = format!("/api/orgs/{}/sso", a.org());

    let (st, v) = app
        .put(
            &sso,
            Some(&a.token),
            json!({"issuer_url": "https://idp.alfa.test", "client_id": "meet", "enforce_sso": false}),
        )
        .await;
    assert_eq!(st, 200, "{v}");
    let (_, cfg) = app.get(&sso, Some(&a.token)).await;
    assert_eq!(cfg["has_client_secret"], false, "{cfg}");

    let (st, v) = app
        .put(
            &sso,
            Some(&a.token),
            json!({"issuer_url": "https://idp.alfa.test", "client_id": "meet",
                   "client_secret": SSO_SECRET, "enforce_sso": false}),
        )
        .await;
    assert_eq!(st, 200, "{v}");
    let sealed = sso_secret(&app, a.org()).await;
    assert_sealed(&sealed, SSO_SECRET);
    // O fluxo OIDC abre com o aad desta org e obtém o original.
    assert_eq!(open_sso(&app, &sealed, a.org()).unwrap(), SSO_SECRET);

    let (st, cfg) = app.get(&sso, Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(cfg["has_client_secret"], true, "{cfg}");
    assert_eq!(cfg["client_id"], "meet");
    let s = cfg.to_string();
    assert!(!s.contains(SSO_SECRET) && !s.contains("enc:v1:"), "{s}");
    assert!(cfg.get("client_secret").is_none());

    // Actualizar sem segredo mantém o cifrado tal como estava.
    let (st, _) = app
        .put(
            &sso,
            Some(&a.token),
            json!({"issuer_url": "https://idp2.alfa.test", "client_id": "meet", "enforce_sso": true}),
        )
        .await;
    assert_eq!(st, 200);
    assert_eq!(sso_secret(&app, a.org()).await, sealed);
}

#[sqlx::test(migrations = "./migrations")]
async fn webdav_password_is_sealed_and_the_test_uses_the_original(db: sqlx::PgPool) {
    let admin = uuid::Uuid::new_v4();
    let app = TestApp::spawn_with(db, &[("PLATFORM_ADMIN_USER_IDS", &admin.to_string())]).await;
    let tok = access_token_for(&app, admin);
    let rx = Receiver::spawn().await;

    let (st, v) = put_storage(&app, &tok, &rx.url, Some(DAV_PASSWORD)).await;
    assert_eq!(st, 200, "{v}");
    let sealed = dav_password(&app).await;
    assert_sealed(&sealed, DAV_PASSWORD);

    let (st, cfg) = app.get("/api/operator/v1/storage", Some(&tok)).await;
    assert_eq!(st, 200);
    assert_eq!(cfg["webdav_password_set"], true);
    let s = cfg.to_string();
    assert!(!s.contains(DAV_PASSWORD) && !s.contains("enc:v1:"), "{s}");

    let (st, v) = test_storage(&app, &tok).await;
    assert_eq!(st, 200, "{v}");
    let got = rx.received();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].method, "PROPFIND");
    assert_eq!(
        got[0].headers["authorization"],
        basic("meet", DAV_PASSWORD).as_str()
    );

    // Guardar sem password mantém a cifrada.
    let (st, _) = put_storage(&app, &tok, &rx.url, None).await;
    assert_eq!(st, 200);
    assert_eq!(dav_password(&app).await, sealed);
}

#[sqlx::test(migrations = "./migrations")]
async fn legacy_plaintext_keeps_working_and_is_resealed_idempotently(db: sqlx::PgPool) {
    let admin = uuid::Uuid::new_v4();
    let app = TestApp::spawn_with(
        db,
        &[
            ("WEBHOOK_ALLOW_HOSTS", "127.0.0.1"),
            ("PLATFORM_ADMIN_USER_IDS", &admin.to_string()),
        ],
    )
    .await;
    let tok = access_token_for(&app, admin);
    let hook_rx = Receiver::spawn().await;
    let dav_rx = Receiver::spawn().await;
    let a = app.new_org("alfa.test").await;

    // Três linhas herdadas, em claro, como uma instalação antiga as tem.
    let hook = insert_hook(&app, &a, &hook_rx.url, HOOK_SECRET).await;
    sqlx::query(
        "INSERT INTO org_sso_configs (org_id, issuer_url, client_id, client_secret)
         VALUES ($1::uuid, 'https://idp.alfa.test', 'meet', $2)",
    )
    .bind(a.org())
    .bind(SSO_SECRET)
    .execute(&app.db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO platform_storage (id, storage_type, webdav_url, webdav_user, webdav_password)
         VALUES (1, 'webdav', $1, 'meet', $2)",
    )
    .bind(&dav_rx.url)
    .bind(DAV_PASSWORD)
    .execute(&app.db)
    .await
    .unwrap();
    assert_eq!(count_legacy(&app.db).await.unwrap(), 3);

    // Antes da tarefa: o herdado serve.
    app.new_meeting(&a, "antes", &[]).await;
    wait_final(&app, &a, &hook, 1).await;
    let (st, v) = test_storage(&app, &tok).await;
    assert_eq!(st, 200, "{v}");

    let sb = app.state.config.secret_box.clone().unwrap();
    let report = reseal_legacy(&app.db, &sb).await.unwrap();
    assert_eq!(
        report,
        ResealReport {
            webhook_secrets: 1,
            sso_client_secrets: 1,
            webdav_passwords: 1
        }
    );
    let sealed_hook = hook_secret(&app, &hook).await;
    let sealed_sso = sso_secret(&app, a.org()).await;
    let sealed_dav = dav_password(&app).await;
    assert_sealed(&sealed_hook, HOOK_SECRET);
    assert_sealed(&sealed_sso, SSO_SECRET);
    assert_sealed(&sealed_dav, DAV_PASSWORD);
    assert_eq!(count_legacy(&app.db).await.unwrap(), 0);

    // Idempotente: a segunda passagem não toca em nada.
    assert_eq!(reseal_legacy(&app.db, &sb).await.unwrap().total(), 0);
    assert_eq!(hook_secret(&app, &hook).await, sealed_hook);

    // Depois da tarefa: os mesmos segredos originais chegam a quem os usa.
    assert_eq!(open_sso(&app, &sealed_sso, a.org()).unwrap(), SSO_SECRET);
    app.new_meeting(&a, "depois", &[]).await;
    let items = wait_final(&app, &a, &hook, 2).await;
    assert!(
        items.iter().all(|d| d["status"] == "succeeded"),
        "{items:?}"
    );
    let got = hook_rx.received();
    assert_eq!(got.len(), 2);
    for r in &got {
        assert_eq!(
            r.headers["x-delonix-signature"],
            signature(HOOK_SECRET, &r.body).as_str()
        );
    }
    let (st, v) = test_storage(&app, &tok).await;
    assert_eq!(st, 200, "{v}");
    for r in dav_rx.received() {
        assert_eq!(
            r.headers["authorization"],
            basic("meet", DAV_PASSWORD).as_str()
        );
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn without_keys_writes_are_refused_and_legacy_reads_work(db: sqlx::PgPool) {
    let admin = uuid::Uuid::new_v4();
    let admin_s = admin.to_string();
    let mut config = test_config(&[
        ("WEBHOOK_ALLOW_HOSTS", "127.0.0.1"),
        ("PLATFORM_ADMIN_USER_IDS", &admin_s),
    ]);
    config.secret_box = None;
    let app = TestApp::spawn_with_config(db, config).await;
    let tok = access_token_for(&app, admin);
    let rx = Receiver::spawn().await;
    let sealed_rx = Receiver::spawn().await;
    let a = app.new_org("alfa.test").await;

    // Webhook COM segredo: 422, e nada gravado.
    let (st, v) = app
        .post(
            &hooks(a.org()),
            Some(&a.token),
            json!({"kind": "generic", "url": rx.url, "secret": HOOK_SECRET}),
        )
        .await;
    assert_eq!(st, 422, "{v}");
    assert_eq!(v["code"], "secrets.encryption_unconfigured");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM org_webhooks")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 0);
    // SEM segredo: continua a funcionar sem chaves.
    let (st, v) = app
        .post(
            &hooks(a.org()),
            Some(&a.token),
            json!({"kind": "slack", "url": rx.url, "events": "meeting.started"}),
        )
        .await;
    assert_eq!(st, 200, "{v}");

    // SSO com segredo: 422; sem segredo: 200.
    let sso = format!("/api/orgs/{}/sso", a.org());
    let (st, v) = app
        .put(
            &sso,
            Some(&a.token),
            json!({"issuer_url": "https://idp.alfa.test", "client_id": "meet",
                   "client_secret": SSO_SECRET, "enforce_sso": false}),
        )
        .await;
    assert_eq!(st, 422, "{v}");
    assert_eq!(v["code"], "secrets.encryption_unconfigured");
    let (st, _) = app
        .put(
            &sso,
            Some(&a.token),
            json!({"issuer_url": "https://idp.alfa.test", "client_id": "meet", "enforce_sso": false}),
        )
        .await;
    assert_eq!(st, 200);
    assert_eq!(sso_secret(&app, a.org()).await, "");

    // WebDAV com password: 422, nada gravado; sem password: 200.
    let (st, v) = put_storage(&app, &tok, &rx.url, Some(DAV_PASSWORD)).await;
    assert_eq!(st, 422, "{v}");
    assert_eq!(v["code"], "secrets.encryption_unconfigured");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM platform_storage")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 0);

    // Leituras herdadas em claro continuam a servir.
    let legacy = insert_hook(&app, &a, &rx.url, HOOK_SECRET).await;
    // E um valor cifrado por uma chave que esta instalação já não tem.
    let foreign_box = SecretBox::derived_for_dev("chave-retirada");
    let sealed_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO org_webhooks (id, org_id, kind, url, secret, events, created_by)
         VALUES ($1, $2::uuid, 'generic', $3, $4, 'meeting.created', $5::uuid)",
    )
    .bind(sealed_id)
    .bind(a.org())
    .bind(&sealed_rx.url)
    .bind(foreign_box.seal(HOOK_SECRET, &format!("org_webhooks.secret:{sealed_id}")))
    .bind(&a.user_id)
    .execute(&app.db)
    .await
    .unwrap();

    app.new_meeting(&a, "sem chaves", &[]).await;
    let items = wait_final(&app, &a, &legacy, 1).await;
    assert_eq!(items[0]["status"], "succeeded", "{}", items[0]);
    let got = rx.received();
    assert_eq!(
        got.len(),
        1,
        "só o legado subscreveu meeting.created neste receptor"
    );
    assert_eq!(
        got[0].headers["x-delonix-signature"],
        signature(HOOK_SECRET, &got[0].body).as_str()
    );
    // O cifrado sem chave: entrega `failed`, e nada é enviado sem assinatura.
    let sealed_items = wait_final(&app, &a, &sealed_id.to_string(), 1).await;
    assert_eq!(sealed_items[0]["status"], "failed", "{}", sealed_items[0]);
    assert!(sealed_rx.received().is_empty());

    // WebDAV herdado em claro: o teste usa-o; cifrado sem chave: 500.
    sqlx::query(
        "INSERT INTO platform_storage (id, storage_type, webdav_url, webdav_user, webdav_password)
         VALUES (1, 'webdav', $1, 'meet', $2)",
    )
    .bind(&rx.url)
    .bind(DAV_PASSWORD)
    .execute(&app.db)
    .await
    .unwrap();
    let (st, v) = test_storage(&app, &tok).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        rx.received().last().unwrap().headers["authorization"],
        basic("meet", DAV_PASSWORD).as_str()
    );
    sqlx::query("UPDATE platform_storage SET webdav_password = $1 WHERE id = 1")
        .bind(foreign_box.seal(DAV_PASSWORD, "platform_storage.webdav_password:1"))
        .execute(&app.db)
        .await
        .unwrap();
    let before = rx.received().len();
    let (st, v) = test_storage(&app, &tok).await;
    assert_eq!(st, 500, "{v}");
    assert!(!v.to_string().contains(DAV_PASSWORD));
    assert_eq!(rx.received().len(), before, "nada enviado sem a password");
}

#[sqlx::test(migrations = "./migrations")]
async fn ciphertext_copied_to_another_row_does_not_open(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, &[("WEBHOOK_ALLOW_HOSTS", "127.0.0.1")]).await;
    let rx_a = Receiver::spawn().await;
    let rx_b = Receiver::spawn().await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;

    let create = |admin: &Account, url: &str, secret: &str| {
        let body =
            json!({"kind": "generic", "url": url, "secret": secret, "events": "meeting.created"});
        let path = hooks(admin.org());
        let token = admin.token.clone();
        let app = &app;
        async move {
            let (st, v) = app.post(&path, Some(&token), body).await;
            assert_eq!(st, 200, "{v}");
            v["id"].as_str().unwrap().to_string()
        }
    };
    let hook_a = create(&a, &rx_a.url, HOOK_SECRET).await;
    let hook_b = create(&a, &rx_b.url, "segredo-do-hook-b").await;

    // O cifrado de A colado na linha de B (quem tem acesso à base, não à chave).
    sqlx::query(
        "UPDATE org_webhooks SET secret = (SELECT secret FROM org_webhooks WHERE id = $1::uuid)
          WHERE id = $2::uuid",
    )
    .bind(&hook_a)
    .bind(&hook_b)
    .execute(&app.db)
    .await
    .unwrap();

    app.new_meeting(&a, "cópia", &[]).await;
    let da = wait_final(&app, &a, &hook_a, 1).await;
    let db_ = wait_final(&app, &a, &hook_b, 1).await;
    assert_eq!(da[0]["status"], "succeeded");
    assert_eq!(db_[0]["status"], "failed", "{}", db_[0]);
    assert!(
        rx_b.received().is_empty(),
        "B não envia — nem sem assinatura, nem assinado com o segredo de A"
    );
    let got = rx_a.received();
    assert_eq!(
        got[0].headers["x-delonix-signature"],
        signature(HOOK_SECRET, &got[0].body).as_str()
    );

    // SSO: o cifrado da org A colado na org B não abre com o contexto de B.
    for (org, secret) in [(&a, SSO_SECRET), (&b, "segredo-da-org-b")] {
        let (st, v) = app
            .put(
                &format!("/api/orgs/{}/sso", org.org()),
                Some(&org.token),
                json!({"issuer_url": "https://idp.test", "client_id": "meet",
                       "client_secret": secret, "enforce_sso": false}),
            )
            .await;
        assert_eq!(st, 200, "{v}");
    }
    let sealed_a = sso_secret(&app, a.org()).await;
    assert_eq!(open_sso(&app, &sealed_a, a.org()).unwrap(), SSO_SECRET);
    assert!(open_sso(&app, &sealed_a, b.org()).is_err());
}

/// O provisionamento de org (módulo Odoo) também grava o `client_secret` do
/// SSO: era a última escrita em claro depois do R160.
#[sqlx::test(migrations = "./migrations")]
async fn provisioning_seals_the_sso_client_secret(db: sqlx::PgPool) {
    let secret = "segredo-de-plataforma-de-teste";
    let app = TestApp::spawn_with(db, &[("PROVISIONING_SECRET", secret)]).await;
    let r = app
        .raw(
            reqwest::Method::POST,
            "/api/operator/v1/organizations",
            &[("X-Provisioning-Secret", secret)],
            Some(serde_json::json!({
                "name": "Delta SA", "email_domain": "delta.test",
                "sso": {"issuer_url": "https://idp.delta.test", "client_id": "meet",
                        "client_secret": "segredo-oidc-da-delta", "enforce_sso": false}
            })),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.text);
    assert!(!r.text.contains("segredo-oidc-da-delta"));
    let stored: String = sqlx::query_scalar("SELECT client_secret FROM org_sso_configs")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert!(stored.starts_with("enc:v1:"), "{stored}");
    assert!(!stored.contains("segredo-oidc"));
}
