//! Inventário de nós de media (G10) na superfície de operador, contra
//! Postgres real: batimento, estado derivado, e quem pode ler.
mod common;

use common::TestApp;
use serde_json::json;

#[sqlx::test(migrations = "./migrations")]
async fn operator_sees_nodes_with_derived_status(db: sqlx::PgPool) {
    // 1.ª instância só para criar a conta que vai ser administradora.
    let boot = TestApp::spawn(db.clone()).await;
    let op = boot.new_org("plataforma.ao").await;
    let other = boot.new_org("cliente.ao").await;

    let app = TestApp::spawn_with(
        db,
        &[
            ("PLATFORM_ADMIN_USER_IDS", op.user_id.as_str()),
            ("NODE_PEER_CAPACITY", "200"),
        ],
    )
    .await;

    // Batimento real deste nó.
    delonix_server::nodes::heartbeat(&app.state, chrono::Utc::now())
        .await
        .unwrap();
    // Um nó a drenar e um morto, escritos como um batimento antigo os deixaria.
    sqlx::query(
        "INSERT INTO media_nodes (node_id, hostname, version, edition, started_at, last_seen_at, draining, rooms, peers)
         VALUES (gen_random_uuid(), 'pod-a', '0.1.0', 'saas', now(), now(), TRUE, 3, 40),
                (gen_random_uuid(), 'pod-b', '0.1.0', 'saas', now() - interval '1 hour', now() - interval '5 minutes', FALSE, 9, 90)",
    )
    .execute(&app.db)
    .await
    .unwrap();

    let (st, v) = app.get("/api/operator/v1/nodes", Some(&op.token)).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["items"].as_array().unwrap().len(), 3);
    assert_eq!(v["serving"], 1);
    assert_eq!(v["draining"], 1);
    assert_eq!(v["unreachable"], 1);
    assert_eq!(v["peers"], 40, "o nó morto não conta participantes");
    let me = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["hostname"] != "pod-a" && n["hostname"] != "pod-b")
        .unwrap();
    assert_eq!(me["status"], "serving");
    assert_eq!(me["peer_capacity"], 200);
    assert_eq!(me["load"], json!(0.0));

    // Admin de uma org qualquer não é operador.
    let (st, v) = app.get("/api/operator/v1/nodes", Some(&other.token)).await;
    assert_eq!(st, 403, "{v}");
    let (st, _) = app.get("/api/operator/v1/nodes", None).await;
    assert_eq!(st, 401);
}

#[sqlx::test(migrations = "./migrations")]
async fn personal_edition_has_no_operator_surface(db: sqlx::PgPool) {
    let boot = TestApp::spawn_with(db.clone(), &[("DELONIX_EDITION", "personal")]).await;
    let (st, _) = boot
        .post(
            "/api/auth/register",
            None,
            json!({"email": "eu@gmail.com", "password": common::PASSWORD}),
        )
        .await;
    assert_eq!(st, 200);
    let me = boot.login("eu@gmail.com").await;
    let app = TestApp::spawn_with(
        db,
        &[
            ("DELONIX_EDITION", "personal"),
            ("PLATFORM_ADMIN_USER_IDS", me.user_id.as_str()),
        ],
    )
    .await;
    let (st, v) = app.get("/api/operator/v1/nodes", Some(&me.token)).await;
    assert_eq!(st, 404, "{v}");
    assert_eq!(v["code"], "operator.surface_disabled");
}
