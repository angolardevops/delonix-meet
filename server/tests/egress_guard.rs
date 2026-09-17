//! Guarda de saída (anti-SSRF, S4) contra o servidor a sério: cada recusa com o
//! controlo positivo ao lado (R51/R94) — senão o teste mediria uma avaria.
mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use common::{TestApp, PASSWORD};
use serde_json::json;

/// Um «Odoo» falso que só conta ligações e responde credenciais inválidas.
async fn odoo_falso() -> (u16, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let ligacoes = Arc::new(AtomicUsize::new(0));
    let conta = ligacoes.clone();
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        loop {
            let Ok((mut s, _)) = listener.accept().await else {
                return;
            };
            conta.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf).await;
                let corpo = r#"{"jsonrpc":"2.0","id":1,"result":null}"#;
                let _ = s
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{corpo}",
                            corpo.len()
                        )
                        .as_bytes(),
                    )
                    .await;
            });
        }
    });
    (port, ligacoes)
}

/// A conta passa a ter o Odoo da org como autoridade, com o URL escrito
/// DIRECTAMENTE na base — como ficaria uma org configurada antes desta guarda,
/// ou por um caminho que se esquecesse de validar ao gravar. Prova a guarda da
/// LIGAÇÃO, não a do formulário.
async fn apontar_odoo(app: &TestApp, org: &str, user_id: &str, url: &str) {
    sqlx::query(
        "UPDATE organizations SET odoo_enabled = TRUE, odoo_url = $1, odoo_db = 'prod' WHERE id = $2::uuid",
    )
    .bind(url)
    .bind(org)
    .execute(&app.db)
    .await
    .unwrap();
    sqlx::query("UPDATE users SET odoo_org_id = $1::uuid WHERE id = $2::uuid")
        .bind(org)
        .bind(user_id)
        .execute(&app.db)
        .await
        .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn login_odoo_nao_liga_a_um_endereco_interno(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let (port, ligacoes) = odoo_falso().await;
    apontar_odoo(
        &app,
        a.org(),
        &a.user_id,
        &format!("http://127.0.0.1:{port}"),
    )
    .await;

    let _ = app
        .post(
            "/api/auth/login",
            None,
            json!({"email": a.email, "password": PASSWORD}),
        )
        .await;
    assert_eq!(
        ligacoes.load(Ordering::SeqCst),
        0,
        "a password não pode ter saído para 127.0.0.1"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn login_odoo_liga_ao_host_declarado_pelo_operador(db: sqlx::PgPool) {
    // Controlo positivo do teste de cima: o MESMO destino, declarado.
    let app = TestApp::spawn_with(db, &[("OUTBOUND_ALLOW_HOSTS", "127.0.0.1")]).await;
    let a = app.new_org("alfa.test").await;
    let (port, ligacoes) = odoo_falso().await;
    apontar_odoo(
        &app,
        a.org(),
        &a.user_id,
        &format!("http://127.0.0.1:{port}"),
    )
    .await;

    let _ = app
        .post(
            "/api/auth/login",
            None,
            json!({"email": a.email, "password": PASSWORD}),
        )
        .await;
    assert!(
        ligacoes.load(Ordering::SeqCst) >= 1,
        "o Odoo declarado devia ser contactado"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn odoo_url_interno_e_recusado_ao_gravar(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let path = format!("/api/orgs/{}/integrations/odoo", a.org());
    let corpo = |url: &str| {
        json!({"odoo_enabled": true, "odoo_url": url, "odoo_db": "prod",
               "hide_org_creation": false, "hide_sso_button": false})
    };
    for url in [
        "http://127.0.0.1:8069",
        "http://169.254.169.254",
        "http://[::ffff:10.0.0.1]:8069",
        "file:///etc/passwd",
        "http://admin:x@erp.alfa.test",
    ] {
        let (st, body) = app.put(&path, Some(&a.token), corpo(url)).await;
        assert_eq!(st, 400, "{url}: {body}");
    }
    // Controlo: um nome (ainda sem DNS) grava.
    let (st, body) = app
        .put(&path, Some(&a.token), corpo("https://erp.alfa.test"))
        .await;
    assert_eq!(st, 200, "{body}");
}

#[sqlx::test(migrations = "./migrations")]
async fn emissor_oidc_interno_e_recusado_ao_gravar_e_ao_entrar(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let path = format!("/api/orgs/{}/sso", a.org());
    for url in [
        "https://127.0.0.1",
        "https://169.254.169.254",
        "https://[fd00:ec2::254]",
    ] {
        let (st, body) = app
            .put(
                &path,
                Some(&a.token),
                json!({"issuer_url": url, "client_id": "c", "client_secret": "s", "enforce_sso": false}),
            )
            .await;
        assert_eq!(st, 400, "{url}: {body}");
    }
    let (st, body) = app
        .put(
            &path,
            Some(&a.token),
            json!({"issuer_url": "https://idp.alfa.test", "client_id": "c", "client_secret": "s", "enforce_sso": false}),
        )
        .await;
    assert_eq!(st, 200, "{body}");

    // Um emissor interno já guardado (fora do formulário): o `authorize` não
    // vai buscar a descoberta a ele.
    sqlx::query(
        "UPDATE org_sso_configs SET issuer_url = 'https://127.0.0.1:1' WHERE org_id = $1::uuid",
    )
    .bind(a.org())
    .execute(&app.db)
    .await
    .unwrap();
    let (st, body) = app
        .get("/api/auth/sso/authorize?domain=alfa.test", None)
        .await;
    assert_eq!(st, 400, "{body}");
}

#[sqlx::test(migrations = "./migrations")]
async fn webhook_com_ipv6_que_embute_loopback_e_recusado(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let path = format!("/api/orgs/{}/webhooks", a.org());
    for url in [
        "http://[::ffff:127.0.0.1]/x",
        "http://[64:ff9b::a9fe:a9fe]/x",
        "http://100.64.1.1/x",
    ] {
        let (st, body) = app
            .post(
                &path,
                Some(&a.token),
                json!({"kind": "generic", "url": url, "secret": "s"}),
            )
            .await;
        assert_eq!(st, 400, "{url}: {body}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn webdav_do_operador_alcanca_a_rede_privada_mas_nao_os_metadados(db: sqlx::PgPool) {
    let admin = uuid::Uuid::new_v4();
    let app = TestApp::spawn_with(db, &[("PLATFORM_ADMIN_USER_IDS", &admin.to_string())]).await;
    sqlx::query("INSERT INTO users (id, email, username, password_hash) VALUES ($1, 'op@x.test', 'op', 'x')")
        .bind(admin)
        .execute(&app.db)
        .await
        .unwrap();
    let now = chrono::Utc::now().timestamp();
    let tok = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &json!({"sub": admin, "typ": "access", "iat": now, "exp": now + 600}),
        &jsonwebtoken::EncodingKey::from_secret(app.state.config.jwt_secret.as_bytes()),
    )
    .unwrap();
    let put = |url: &'static str| {
        let app = &app;
        let tok = tok.clone();
        async move {
            app.put(
                "/api/operator/v1/storage",
                Some(&tok),
                json!({"storage_type": "webdav", "webdav_url": url, "webdav_user": "u"}),
            )
            .await
        }
    };
    let (st, body) = put("http://169.254.169.254/dav").await;
    assert_eq!(st, 400, "{body}");
    let (st, body) = put("http://10.0.0.7/dav").await;
    assert_eq!(st, 200, "{body}");
}

#[sqlx::test(migrations = "./migrations")]
async fn provisionamento_recusa_urls_internos_antes_de_escrever(db: sqlx::PgPool) {
    let secret = "segredo-de-plataforma-de-teste";
    let app = TestApp::spawn_with(db, &[("PROVISIONING_SECRET", secret)]).await;
    let provisionar = |corpo: serde_json::Value| {
        let app = &app;
        async move {
            app.raw(
                reqwest::Method::POST,
                "/api/operator/v1/organizations",
                &[("X-Provisioning-Secret", secret)],
                Some(corpo),
            )
            .await
        }
    };
    let r = provisionar(json!({
        "name": "Delta SA", "email_domain": "delta.test",
        "sso": {"issuer_url": "https://169.254.169.254", "client_id": "meet",
                "client_secret": "s", "enforce_sso": false}
    }))
    .await;
    assert_eq!(r.status, 400, "{}", r.text);
    let r = provisionar(json!({
        "name": "Delta SA", "odoo_url": "http://127.0.0.1:8069", "odoo_db": "prod", "odoo_company_id": 1
    }))
    .await;
    assert_eq!(r.status, 400, "{}", r.text);
    let orgs: i64 =
        sqlx::query_scalar("SELECT count(*) FROM organizations WHERE name = 'Delta SA'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(orgs, 0, "a recusa não pode deixar uma organização a meio");

    // Controlo: os mesmos campos com destinos públicos provisionam.
    let r = provisionar(json!({
        "name": "Delta SA", "email_domain": "delta.test",
        "odoo_url": "https://erp.delta.test", "odoo_db": "prod", "odoo_company_id": 1,
        "sso": {"issuer_url": "https://idp.delta.test", "client_id": "meet",
                "client_secret": "s", "enforce_sso": false}
    }))
    .await;
    assert_eq!(r.status, 200, "{}", r.text);
}
