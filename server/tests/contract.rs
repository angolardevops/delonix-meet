//! Contrato transversal: envelope de erro, `X-Request-Id`, recusas do axum.
mod common;

use common::TestApp;

#[sqlx::test(migrations = "./migrations")]
async fn error_envelope_carries_code_and_request_id(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let res = app
        .http
        .get(app.url("/api/users/me"))
        .header("x-request-id", "rastreio-123")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
    assert_eq!(res.headers()["x-request-id"], "rastreio-123");
    let v: serde_json::Value = res.json().await.unwrap();
    assert_eq!(v["error"], "unauthorized"); // o texto que o web já lê
    assert_eq!(v["code"], "auth.unauthenticated");
    assert_eq!(v["request_id"], "rastreio-123");
}

#[sqlx::test(migrations = "./migrations")]
async fn unsafe_request_id_is_replaced(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let res = app
        .http
        .get(app.url("/health"))
        .header("x-request-id", "a b;<script>")
        .send()
        .await
        .unwrap();
    let id = res.headers()["x-request-id"].to_str().unwrap().to_string();
    assert_eq!(id.len(), 32, "gerado: {id}");
    assert_eq!(res.text().await.unwrap(), "ok"); // sonda continua em texto
}

#[sqlx::test(migrations = "./migrations")]
async fn axum_rejections_use_the_envelope(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    // JSON mal formado no login: recusa do extractor, não do handler.
    let res = app
        .http
        .post(app.url("/api/auth/login"))
        .header("content-type", "application/json")
        .body("{nao-e-json")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400);
    let v: serde_json::Value = res.json().await.unwrap();
    assert_eq!(v["code"], "invalid_argument");
    assert!(v["request_id"].is_string());

    // Rota inexistente.
    let (st, v) = app.get("/api/nao-existe", None).await;
    assert_eq!(st, 404);
    assert_eq!(v["code"], "not_found");
}
