//! Superfície pública (sem sessão): o que responde, o que NÃO revela, e o que
//! continua fechado. Cada rota daqui está em `scripts/rotas-publicas.txt`.
mod common;

use common::TestApp;

#[sqlx::test(migrations = "./migrations")]
async fn public_settings_shape(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let (st, s) = app.get("/api/public/settings", None).await;
    assert_eq!(st, 200);
    for k in [
        "hide_org_creation",
        "hide_sso_button",
        "edition",
        "registration_open",
        "tenancy_mode",
        "capabilities",
    ] {
        assert!(s.get(k).is_some(), "falta {k}: {s}");
    }
    // Nada de dados de inquilino nem de configuração sensível.
    let text = s.to_string();
    assert!(
        !text.contains("secret") && !text.contains("postgres"),
        "{text}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn status_and_metrics(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let (st, s) = app.get("/api/status", None).await;
    assert_eq!(st, 200);
    assert_eq!(s["status"], "ok");
    assert_eq!(s["db"], true);
    assert!(s["version"].is_string());
    let body = app
        .http
        .get(app.url("/metrics"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("delonix_"), "{body}");
}

#[sqlx::test(migrations = "./migrations")]
async fn openapi_specs_are_served_and_match_the_router(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let (st, bff) = app.get("/api/openapi.json", None).await;
    assert_eq!(st, 200);
    assert_eq!(bff["openapi"].as_str().unwrap().chars().next(), Some('3'));
    assert!(bff["paths"]["/api/users/me"].is_object());
    let (st, v1) = app.get("/api/v1/openapi.json", None).await;
    assert_eq!(st, 200);
    assert!(v1["paths"]["/api/v1/meetings"].is_object());
}

#[sqlx::test(migrations = "./migrations")]
async fn closed_without_session(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    for path in [
        "/api/ice-servers",
        "/api/orgs",
        "/api/meetings",
        "/api/recordings",
        "/api/whiteboards",
    ] {
        let (st, v) = app.get(path, None).await;
        assert_eq!(st, 401, "{path}: {v}");
        assert_eq!(v["code"], "auth.unauthenticated", "{path}");
    }
    let (st, _) = app.get("/api/v1/organization", None).await;
    assert_eq!(st, 401);
    // Um token de partilha inventado não revela nada.
    let (st, _) = app.get("/api/public/recordings/nao-existe", None).await;
    assert_eq!(st, 404);
    let (st, _) = app
        .get("/api/public/whiteboards/nao-existe/image", None)
        .await;
    assert_eq!(st, 404);
}
