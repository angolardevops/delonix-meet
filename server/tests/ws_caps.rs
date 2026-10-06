//! Tectos do `/ws` por conta (`MAX_WS_PER_USER`) e por organização (quota de
//! participantes concorrentes, fixada pelo operador). São o que impede que UMA
//! conta ou UMA organização ocupe o nó e deixe as outras sem lugar — por isso
//! cada teste junta DUAS organizações e prova que a recusa de uma não toca na
//! outra.
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

/// Liga e espera pelo `joined`; devolve o socket e a mensagem. `Err` com o
/// código HTTP se o upgrade for recusado.
async fn connect_with(
    app: &TestApp,
    token: &str,
    reconnect: Option<&str>,
) -> Result<(Ws, Value), u16> {
    let mut url = format!(
        "{}/ws?token={token}",
        app.base.replacen("http://", "ws://", 1)
    );
    if let Some(r) = reconnect {
        url.push_str(&format!("&reconnect={r}"));
    }
    let (mut ws, _) = match tokio_tungstenite::connect_async(url).await {
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
            // Um membro que não é o anfitrião pode ficar na sala de espera: o
            // socket foi aceite, que é o que estes testes querem saber.
            if v["type"] == "joined" || v["type"] == "waiting" {
                return Ok((ws, v));
            }
        }
    }
    panic!("o joined não chegou");
}

async fn connect(app: &TestApp, token: &str) -> Result<Ws, u16> {
    connect_with(app, token, None).await.map(|(ws, _)| ws)
}

async fn fill(app: &TestApp, tok: &str, n: usize) -> Vec<Ws> {
    let mut v = Vec::new();
    for i in 0..n {
        v.push(
            connect(app, tok)
                .await
                .unwrap_or_else(|st| panic!("a entrada {i} foi recusada ({st})")),
        );
    }
    v
}

fn user_cap_refused(app: &TestApp) -> u64 {
    app.state
        .metrics
        .ws_refused_user_cap_total
        .load(Ordering::Relaxed)
}

fn org_quota_refused(app: &TestApp) -> u64 {
    app.state
        .metrics
        .ws_refused_org_quota_total
        .load(Ordering::Relaxed)
}

/// Uma conta com o tecto de sockets cheio é recusada; outra conta da MESMA
/// organização e uma conta de OUTRA organização não sentem nada.
#[sqlx::test(migrations = "./migrations")]
async fn an_account_at_its_socket_cap_is_refused_and_nobody_else_is(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, &[("MAX_WS_PER_USER", "2")]).await;
    let a = app.new_org("alfa.test").await;
    let a2 = app.add_member(&a, "colega", "member").await;
    let b = app.new_org("beta.test").await;
    let sala_a = app.new_room(&a, "A").await;
    let sala_b = app.new_room(&b, "B").await;
    let tok_a = room_token(&app, sala_a["code"].as_str().unwrap(), &a).await;
    let tok_a2 = room_token(&app, sala_a["code"].as_str().unwrap(), &a2).await;
    let tok_b = room_token(&app, sala_b["code"].as_str().unwrap(), &b).await;

    let mut a_sockets = fill(&app, &tok_a, 2).await;
    assert_eq!(connect(&app, &tok_a).await.err(), Some(429));
    assert_eq!(user_cap_refused(&app), 1);

    // O colega da mesma organização e o dono da outra entram à vontade, mesmo
    // com a conta A encostada ao tecto.
    let _colega = connect(&app, &tok_a2)
        .await
        .expect("outra conta da mesma org não é afectada");
    // O colega ficou na sala de espera (não é anfitrião): esse socket também
    // conta para o tecto, senão bastava abrir salas para o contornar.
    let colega_id: uuid::Uuid = a2.user_id.parse().unwrap();
    assert_eq!(app.state.hub.user_sockets(colega_id), 1);
    let _b = connect(&app, &tok_b)
        .await
        .expect("outra organização não é afectada");
    assert_eq!(user_cap_refused(&app), 1, "só a conta cheia foi recusada");

    // Libertado um socket, a conta volta a poder ligar.
    drop(a_sockets.pop());
    tokio::time::sleep(Duration::from_millis(500)).await;
    app.state.hub.expire_disconnected(Duration::ZERO);
    let _de_novo = connect(&app, &tok_a)
        .await
        .expect("com um socket livre a conta volta a entrar");
}

/// `MAX_WS_PER_USER=0` desliga o tecto; sem ele nada é recusado por conta.
#[sqlx::test(migrations = "./migrations")]
async fn a_zero_cap_disables_the_per_account_limit(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, &[("MAX_WS_PER_USER", "0")]).await;
    let a = app.new_org("alfa.test").await;
    let sala = app.new_room(&a, "A").await;
    let tok = room_token(&app, sala["code"].as_str().unwrap(), &a).await;
    let _s = fill(&app, &tok, 20).await;
    assert_eq!(user_cap_refused(&app), 0);
}

async fn operator(db: sqlx::PgPool) -> (TestApp, Account) {
    let platform: (uuid::Uuid,) = sqlx::query_as(
        "INSERT INTO users (email, username, password_hash) VALUES ('op@plataforma.ao', 'operador', '') RETURNING id")
        .fetch_one(&db).await.unwrap();
    let app = TestApp::spawn_with(
        db,
        &[
            ("PLATFORM_ADMIN_USER_IDS", &platform.0.to_string()),
            ("MAX_WS_PER_USER", "0"),
        ],
    )
    .await;
    // O operador entra com a password de uma conta qualquer.
    let seed = app.new_org("semente.test").await;
    let hash =
        sqlx::query_scalar::<_, String>("SELECT password_hash FROM users WHERE id = $1::uuid")
            .bind(&seed.user_id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
        .bind(hash)
        .bind(platform.0)
        .execute(&app.db)
        .await
        .unwrap();
    let op = app.login("op@plataforma.ao").await;
    (app, op)
}

/// A quota é da organização e é do operador: uma org cheia não deixa entrar
/// mais ninguém DELA, a outra org continua a entrar, e a própria org não a
/// consegue subir.
#[sqlx::test(migrations = "./migrations")]
async fn an_org_over_its_quota_is_refused_and_another_org_is_not(db: sqlx::PgPool) {
    let (app, op) = operator(db).await;
    let a = app.new_org("alfa.test").await;
    let a2 = app.add_member(&a, "colega", "member").await;
    let b = app.new_org("beta.test").await;
    let sala_a = app.new_room(&a, "A").await;
    let sala_b = app.new_room(&b, "B").await;
    let tok_a = room_token(&app, sala_a["code"].as_str().unwrap(), &a).await;
    let tok_a2 = room_token(&app, sala_a["code"].as_str().unwrap(), &a2).await;
    let tok_b = room_token(&app, sala_b["code"].as_str().unwrap(), &b).await;

    // O admin da org não se auto-licencia.
    let (st, body) = app
        .put(
            &format!("/api/operator/v1/organizations/{}/concurrency", a.org()),
            Some(&a.token),
            json!({"max_concurrent_participants": 1000}),
        )
        .await;
    assert_eq!(st, 403, "{body}");
    // O operador fixa 3, e valores inválidos são recusados.
    let (st, body) = app
        .put(
            &format!("/api/operator/v1/organizations/{}/concurrency", a.org()),
            Some(&op.token),
            json!({"max_concurrent_participants": 0}),
        )
        .await;
    assert_eq!(st, 400, "{body}");
    let (st, body) = app
        .put(
            &format!("/api/operator/v1/organizations/{}/concurrency", a.org()),
            Some(&op.token),
            json!({"max_concurrent_participants": 3}),
        )
        .await;
    assert_eq!(st, 200, "{body}");

    let _a_sockets = fill(&app, &tok_a, 3).await;
    // A org A está no tecto: ninguém mais dela entra, nem de outra conta.
    assert_eq!(connect(&app, &tok_a2).await.err(), Some(429));
    assert_eq!(connect(&app, &tok_a).await.err(), Some(429));
    assert_eq!(org_quota_refused(&app), 2);

    // A org B, no mesmo nó, continua a poder entrar, e a recusa de A não conta
    // contra ela.
    let _b_sockets = fill(&app, &tok_b, 5).await;
    assert_eq!(org_quota_refused(&app), 2);
    assert_eq!(app.state.hub.peers_ligados(), 8);

    // O operador sobe o tecto de A: passa a caber mais um.
    let (st, _) = app
        .put(
            &format!("/api/operator/v1/organizations/{}/concurrency", a.org()),
            Some(&op.token),
            json!({"max_concurrent_participants": 4}),
        )
        .await;
    assert_eq!(st, 200);
    let _quarto = connect(&app, &tok_a)
        .await
        .expect("com o tecto subido o quarto entra");
}

/// Quem volta ao seu lugar em graça não conta como entrada nova — senão uma
/// quebra de rede com a org no tecto custava o lugar; e um segredo inventado
/// NÃO dá essa isenção.
#[sqlx::test(migrations = "./migrations")]
async fn a_returning_seat_is_not_a_new_entry_but_a_made_up_secret_is(db: sqlx::PgPool) {
    let (app, op) = operator(db).await;
    let a = app.new_org("alfa.test").await;
    let sala = app.new_room(&a, "A").await;
    let tok = room_token(&app, sala["code"].as_str().unwrap(), &a).await;
    let (st, body) = app
        .put(
            &format!("/api/operator/v1/organizations/{}/concurrency", a.org()),
            Some(&op.token),
            json!({"max_concurrent_participants": 2}),
        )
        .await;
    assert_eq!(st, 200, "{body}");

    let (ws1, joined1) = connect_with(&app, &tok, None).await.unwrap();
    let _ws2 = connect(&app, &tok).await.unwrap();
    let secret = joined1["reconnect"].as_str().expect("segredo do lugar");
    let peer1 = joined1["peer_id"].clone();

    // O socket 1 cai: o lugar fica em graça e continua a contar contra a quota.
    drop(ws1);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(app.state.hub.peers_ligados(), 2);

    // Um segredo inventado não isenta: a org está cheia.
    assert_eq!(
        connect_with(&app, &tok, Some("segredo-inventado"))
            .await
            .err(),
        Some(429)
    );
    // Uma entrada nova também não.
    assert_eq!(connect(&app, &tok).await.err(), Some(429));
    // O segredo verdadeiro reclama o lugar: entra, e como o MESMO peer.
    let (_ws1b, joined) = connect_with(&app, &tok, Some(secret))
        .await
        .expect("quem volta ao seu lugar não é uma entrada nova");
    assert_eq!(
        joined["peer_id"], peer1,
        "reclamou o lugar, não abriu outro"
    );
    assert_eq!(app.state.hub.peers_ligados(), 2);
}
