//! Edições (ADR-0006 §2): SaaS, enterprise on-premise e pessoal, contra um
//! servidor e um Postgres reais. Cada teste prova um perfil de ponta a ponta:
//! registo, organização criada, definições públicas e o que fica recusado.
mod common;

use common::{TestApp, PASSWORD};
use serde_json::json;

async fn register(app: &TestApp, email: &str, org: Option<&str>) -> (u16, serde_json::Value) {
    let mut body = json!({"email": email, "password": PASSWORD});
    if let Some(o) = org {
        body["org_name"] = json!(o);
    }
    app.post("/api/auth/register", None, body).await
}

#[sqlx::test(migrations = "./migrations")]
async fn saas_default_keeps_one_org_per_domain(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let (st, _) = register(&app, "ana@alfa.ao", Some("Alfa")).await;
    assert_eq!(st, 200);
    let (st, _) = register(&app, "rui@beta.ao", Some("Beta")).await;
    assert_eq!(st, 200, "outro domínio, outra org");
    let (st, v) = register(&app, "joao@alfa.ao", Some("Alfa 2")).await;
    assert_eq!(st, 409);
    assert_eq!(v["code"], "registration.domain_taken");
    assert!(v["error"].as_str().unwrap().contains("alfa.ao"));
    // Sem nome de org em tenancy multi: 400 com campo.
    let (st, v) = register(&app, "x@gama.ao", None).await;
    assert_eq!(st, 400);
    assert_eq!(v["code"], "registration.invalid_org_name");

    let (_, s) = app.get("/api/public/settings", None).await;
    assert_eq!(s["edition"], "saas");
    assert_eq!(s["registration_open"], true);
    assert_eq!(s["capabilities"]["multi_organization"], true);
}

#[sqlx::test(migrations = "./migrations")]
async fn personal_edition_single_user_without_company(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, &[("DELONIX_EDITION", "personal")]).await;
    let (st, v) = register(&app, "eu@gmail.com", None).await;
    assert_eq!(st, 200, "{v}");
    let me = app.login("eu@gmail.com").await;
    let (st, orgs) = app.get("/api/orgs", Some(&me.token)).await;
    assert_eq!(st, 200);
    assert_eq!(orgs.as_array().unwrap().len(), 1);
    let kind: String = sqlx::query_scalar("SELECT kind FROM organizations")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(kind, "personal");

    // A segunda conta não entra por registo.
    let (st, v) = register(&app, "intruso@gmail.com", None).await;
    assert_eq!(st, 403);
    assert_eq!(v["code"], "registration.closed");

    // Nem se cria outra organização.
    let (st, v) = app
        .post("/api/orgs", Some(&me.token), json!({"name": "Outra"}))
        .await;
    assert_eq!(st, 422);
    assert_eq!(v["code"], "organization.single_tenancy");

    let (_, s) = app.get("/api/public/settings", None).await;
    assert_eq!(s["edition"], "personal");
    assert_eq!(s["registration_open"], false);
    assert_eq!(s["hide_org_creation"], true);
    assert_eq!(s["capabilities"]["operator_surface"], false);
}

#[sqlx::test(migrations = "./migrations")]
async fn enterprise_edition_bootstraps_then_invite_only(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, &[("DELONIX_EDITION", "enterprise")]).await;
    let (st, _) = register(&app, "admin@banco.ao", Some("Banco")).await;
    assert_eq!(st, 200);
    let (st, v) = register(&app, "joao@banco.ao", Some("Banco")).await;
    assert_eq!(st, 403);
    assert_eq!(v["code"], "registration.invite_only");
    let (_, s) = app.get("/api/public/settings", None).await;
    assert_eq!(s["edition"], "enterprise");
    assert_eq!(s["tenancy_mode"], "single");
}

#[sqlx::test(migrations = "./migrations")]
async fn single_tenancy_open_registration_joins_the_org(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(
        db,
        &[
            ("DELONIX_EDITION", "enterprise"),
            ("REGISTRATION_MODE", "domain"),
            ("REGISTRATION_DOMAINS", "banco.ao"),
        ],
    )
    .await;
    let (st, _) = register(&app, "admin@banco.ao", Some("Banco")).await;
    assert_eq!(st, 200);
    let (st, v) = register(&app, "joao@banco.ao", None).await;
    assert_eq!(st, 200, "{v}");
    let (st, v) = register(&app, "fora@outro.ao", None).await;
    assert_eq!(st, 403);
    assert_eq!(v["code"], "registration.domain_not_allowed");

    let roles: Vec<(String,)> = sqlx::query_as(
        "SELECT m.role FROM org_members m JOIN users u ON u.id = m.user_id ORDER BY u.email",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(roles, vec![("admin".into(),), ("member".into(),)]);
    let orgs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM organizations")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(orgs, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn internal_listener_takes_ivr_and_metrics_off_the_public_router(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, &[("INTERNAL_BIND_ADDR", "127.0.0.1:0")]).await;
    let (st, _) = app.get("/metrics", None).await;
    assert_eq!(st, 404, "com listener interno, /metrics sai do público");
    let (st, _) = app.post("/api/voice/ivr/validate", None, json!({})).await;
    assert_eq!(st, 404);

    let internal = delonix_server::build_internal_router(app.state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            internal.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap()
    });
    let body = reqwest::get(format!("http://{addr}/metrics"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("delonix_"), "{body}");
}

#[sqlx::test(migrations = "./migrations")]
async fn ui_dir_serves_the_spa_without_swallowing_api_404s(db: sqlx::PgPool) {
    let dir = std::env::temp_dir().join(format!("delonix-ui-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(
        dir.join("index.html"),
        "<!doctype html><title>Delonix</title>",
    )
    .unwrap();
    std::fs::write(dir.join("assets/app-1234.js"), "console.log(1)").unwrap();
    let app = TestApp::spawn_with(db, &[("UI_DIR", dir.to_str().unwrap())]).await;

    let r = app.http.get(app.url("/sala/abc-def")).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["cross-origin-embedder-policy"], "require-corp");
    assert_eq!(r.headers()["cache-control"], "no-cache");
    assert!(r.text().await.unwrap().contains("<title>Delonix</title>"));

    let r = app
        .http
        .get(app.url("/assets/app-1234.js"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert!(r.headers()["cache-control"]
        .to_str()
        .unwrap()
        .contains("immutable"));

    let (st, v) = app.get("/api/nao-existe", None).await;
    assert_eq!(st, 404);
    assert_eq!(v["code"], "not_found");
    std::fs::remove_dir_all(dir).ok();
}
