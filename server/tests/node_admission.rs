//! Admissão por capacidade: um nó no limite de `NODE_PEER_CAPACITY` recusa salas
//! NOVAS e continua a admitir quem entra numa sala que já tem, contra Postgres
//! real e `/ws` a sério.
//!
//! Capacidade 4 e limite de 85% (`NEW_ROOM_LOAD_PERCENT`): o nó aceita salas
//! novas até 3 participantes e deixa de as aceitar a partir de 4. Os limites
//! finos (capacidade 10, 200, sem capacidade) estão no teste de domínio; aqui
//! pouco chega, e cada ligação `/ws` é real e custa tempo sob carga.
mod common;

use std::{sync::atomic::Ordering, time::Duration};

use common::{Account, TestApp};
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{Error as WsError, Message};

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn room_token(app: &TestApp, code: &str, who: &Account) -> String {
    let (st, join) = app
        .post(
            &format!("/api/rooms/{code}/join"),
            Some(&who.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{join}");
    join["room_token"].as_str().unwrap().to_string()
}

fn ws_url(app: &TestApp, token: &str) -> String {
    format!(
        "{}/ws?token={token}",
        app.base.replacen("http://", "ws://", 1)
    )
}

/// Liga e espera pelo `joined`. `Err` com o código HTTP se o upgrade for recusado.
async fn connect(app: &TestApp, token: &str) -> Result<Ws, u16> {
    let (mut ws, _) = match tokio_tungstenite::connect_async(ws_url(app, token)).await {
        Ok(ok) => ok,
        Err(WsError::Http(resp)) => return Err(resp.status().as_u16()),
        Err(e) => panic!("falha de ligação que não é uma recusa HTTP: {e}"),
    };
    for _ in 0..50 {
        let msg = tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .expect("sem resposta do /ws")
            .expect("o socket fechou antes do joined")
            .expect("erro no socket");
        if let Message::Text(t) = msg {
            let v: Value = serde_json::from_str(&t).unwrap();
            if v["type"] == "joined" {
                return Ok(ws);
            }
        }
    }
    panic!("o joined não chegou");
}

async fn app_with_capacity(db: sqlx::PgPool, capacity: Option<&str>) -> TestApp {
    match capacity {
        Some(c) => TestApp::spawn_with(db, &[("NODE_PEER_CAPACITY", c)]).await,
        None => TestApp::spawn(db).await,
    }
}

fn refused(app: &TestApp) -> u64 {
    app.state
        .metrics
        .node_new_rooms_refused_total
        .load(Ordering::Relaxed)
}

#[sqlx::test(migrations = "./migrations")]
async fn a_full_node_refuses_new_rooms_but_not_the_ones_it_has(db: sqlx::PgPool) {
    let app = app_with_capacity(db, Some("4")).await;
    let owner = app.new_org("alfa.test").await;
    let sala_a = app.new_room(&owner, "A").await;
    let sala_b = app.new_room(&owner, "B").await;
    let tok_a = room_token(&app, sala_a["code"].as_str().unwrap(), &owner).await;
    let tok_b = room_token(&app, sala_b["code"].as_str().unwrap(), &owner).await;

    // 4 participantes na sala A (a mesma conta em 4 dispositivos): o nó chega ao
    // limite de 85% de 4.
    let mut sockets: Vec<Ws> = Vec::new();
    for i in 0..4 {
        sockets.push(
            connect(&app, &tok_a)
                .await
                .unwrap_or_else(|st| panic!("a entrada {i} na sala A foi recusada ({st})")),
        );
    }
    assert_eq!(app.state.hub.peers_ligados(), 4);

    // Uma sala NOVA é recusada com 503, e conta-se.
    assert_eq!(connect(&app, &tok_b).await.err(), Some(503));
    assert_eq!(refused(&app), 1);

    // Uma sala que JÁ existe continua a admitir — não pode mudar de nó — e a
    // recusa de há pouco não a afectou.
    sockets.push(
        connect(&app, &tok_a)
            .await
            .expect("a sala A tem de admitir"),
    );
    assert_eq!(app.state.hub.peers_ligados(), 5);
    assert_eq!(
        refused(&app),
        1,
        "entrar numa sala existente não conta como recusa"
    );

    // Esvaziado o nó (os lugares em graça expiram), a sala B volta a ser aceite.
    drop(sockets);
    tokio::time::sleep(Duration::from_millis(500)).await;
    app.state.hub.expire_disconnected_seats(Duration::ZERO);
    assert_eq!(app.state.hub.peers_ligados(), 0);
    let _b = connect(&app, &tok_b)
        .await
        .expect("com o nó vazio a sala B tem de ser aceite");
}

#[sqlx::test(migrations = "./migrations")]
async fn without_a_declared_capacity_no_room_is_refused(db: sqlx::PgPool) {
    // Sem NODE_PEER_CAPACITY não se inventa um limite: o comportamento anterior.
    let app = app_with_capacity(db, None).await;
    let owner = app.new_org("alfa.test").await;
    let sala_a = app.new_room(&owner, "A").await;
    let sala_b = app.new_room(&owner, "B").await;
    let tok_a = room_token(&app, sala_a["code"].as_str().unwrap(), &owner).await;
    let tok_b = room_token(&app, sala_b["code"].as_str().unwrap(), &owner).await;

    let mut sockets: Vec<Ws> = Vec::new();
    for _ in 0..6 {
        sockets.push(
            connect(&app, &tok_a)
                .await
                .expect("sem limite, entra sempre"),
        );
    }
    let _b = connect(&app, &tok_b)
        .await
        .expect("sem limite, a sala nova entra");
    assert_eq!(refused(&app), 0);
}
