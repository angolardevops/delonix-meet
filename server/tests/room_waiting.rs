//! `GET /api/rooms/{room_code}/waiting` — espreitar a sala de espera antes de
//! entrar. A fila é a mesma memória do `SignalingHub` que o `/ws` usa; aqui
//! simula-se um convidado à espera directamente no hub (sem WebSocket), o
//! bastante para provar o contrato HTTP: quem pode ver, e o que vê.
mod common;

use common::TestApp;
use uuid::Uuid;

#[sqlx::test(migrations = "./migrations")]
async fn only_admitter_sees_the_queue_and_it_reflects_the_hub(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let room = app.new_room(&a, "sala").await;
    let code = room["code"].as_str().unwrap();
    let room_id: Uuid = room["id"].as_str().unwrap().parse().unwrap();
    let path = format!("/api/rooms/{code}/waiting");

    // Sem sessão: 401.
    let (st, _) = app.get(&path, None).await;
    assert_eq!(st, 401);

    // Dona, ninguém à espera ainda: 200, lista vazia.
    let (st, v) = app.get(&path, Some(&a.token)).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v.as_array().unwrap().len(), 0);

    // Alguém de outra organização não admite: 403, não confirma nem nega
    // que haja gente à espera (o mesmo 403 quer a fila vazia quer cheia).
    let (st, v) = app.get(&path, Some(&b.token)).await;
    assert_eq!(st, 403, "{v}");
    assert_eq!(v["code"], "room.not_admitter");

    // Um convidado chega à fila (simulado directamente no hub — sem abrir
    // WebSocket, que é o que o join real faria).
    let peer_id = Uuid::new_v4();
    let (tx, _rx) = tokio::sync::oneshot::channel();
    app.state
        .hub
        .add_waiting(room_id, peer_id, "convidada".into(), tx);

    let (st, v) = app.get(&path, Some(&a.token)).await;
    assert_eq!(st, 200, "{v}");
    let list = v.as_array().unwrap();
    assert_eq!(list.len(), 1, "{v}");
    assert_eq!(list[0]["peer_id"], peer_id.to_string());
    assert_eq!(list[0]["username"], "convidada");
    assert!(list[0]["since"].as_i64().unwrap() > 0);
    // origin/title ausentes (add_waiting simples não os define) — o campo
    // não aparece de todo, não vem `null` (serde `skip_serializing_if`).
    assert!(list[0].get("origin").is_none(), "{v}");

    // A dona resolve a admissão: sai da fila.
    app.state.hub.remove_waiting(room_id, peer_id);
    let (st, v) = app.get(&path, Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(v.as_array().unwrap().len(), 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn unknown_room_code_is_404(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let (st, _) = app
        .get("/api/rooms/zzz-zzzz-zzz/waiting", Some(&a.token))
        .await;
    assert_eq!(st, 404);
}
