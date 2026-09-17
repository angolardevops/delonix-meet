//! Fumo do harness: o servidor arranca sobre uma base migrada e o caminho
//! registo → login → perfil → organização funciona de ponta a ponta.
mod common;

use common::TestApp;

#[sqlx::test(migrations = "./migrations")]
async fn health_ready_and_status(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let r = app.http.get(app.url("/health")).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let r = app.http.get(app.url("/ready")).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let (st, body) = app.get("/api/status", None).await;
    assert_eq!(st, 200);
    assert_eq!(body["db"], true);
}

#[sqlx::test(migrations = "./migrations")]
async fn register_login_me_and_org(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let admin = app.new_org("alfa.test").await;
    let (st, me) = app.get("/api/users/me", Some(&admin.token)).await;
    assert_eq!(st, 200, "{me}");
    assert_eq!(me["email"], admin.email);
    assert!(admin.org_id.is_some());
    // Sem token: 401, nunca dados.
    let (st, _) = app.get("/api/users/me", None).await;
    assert_eq!(st, 401);
}
