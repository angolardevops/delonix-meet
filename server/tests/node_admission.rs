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
    app.state.hub.expire_disconnected(Duration::ZERO);
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

fn refused_org_quota(app: &TestApp) -> u64 {
    app.state
        .metrics
        .ws_refused_org_quota_total
        .load(Ordering::Relaxed)
}

fn refused_fair_share(app: &TestApp) -> u64 {
    app.state
        .metrics
        .node_new_rooms_refused_fair_share_total
        .load(Ordering::Relaxed)
}

/// `n` ligações do dono à sala com o token `tok`.
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

/// DUAS organizações: capacidade 7, limite mole em 6 (85%). A usa 5, B usa 1, o
/// nó está a 6 — na zona de margem. A parte justa com dois inquilinos é 3,5:
/// A (5) já a ultrapassou e não abre salas novas; B (1) continua a poder.
/// Sem isto, uma organização com muitas ligações fechava o nó às salas novas de
/// todas as outras.
#[sqlx::test(migrations = "./migrations")]
async fn in_the_margin_only_the_tenant_over_its_share_is_refused(db: sqlx::PgPool) {
    let app = app_with_capacity(db, Some("7")).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let a1 = app.new_room(&a, "A1").await;
    let a2 = app.new_room(&a, "A2").await;
    let b1 = app.new_room(&b, "B1").await;
    let b2 = app.new_room(&b, "B2").await;
    let tok_a1 = room_token(&app, a1["code"].as_str().unwrap(), &a).await;
    let tok_a2 = room_token(&app, a2["code"].as_str().unwrap(), &a).await;
    let tok_b1 = room_token(&app, b1["code"].as_str().unwrap(), &b).await;
    let tok_b2 = room_token(&app, b2["code"].as_str().unwrap(), &b).await;

    let _a_sockets = fill(&app, &tok_a1, 5).await;
    let _b_sockets = fill(&app, &tok_b1, 1).await;
    assert_eq!(app.state.hub.peers_ligados(), 6);

    // A já usa a sua parte: a sala nova dela é recusada, e é recusa por parte
    // justa e não por o nó estar cheio.
    assert_eq!(connect(&app, &tok_a2).await.err(), Some(503));
    assert_eq!((refused(&app), refused_fair_share(&app)), (1, 1));

    // B está abaixo da sua parte: continua a poder abrir salas, e a recusa de A
    // não lhe tirou nada.
    let _b2 = connect(&app, &tok_b2)
        .await
        .expect("a organização B, abaixo da sua parte, tem de poder abrir salas");
    assert_eq!(app.state.hub.peers_ligados(), 7);

    // A sala que A já tem continua a admitir (não pode mudar de nó).
    let _a6 = connect(&app, &tok_a1)
        .await
        .expect("uma sala existente admite sempre");
}

/// Uma organização SOZINHA no nó tem a capacidade toda: na zona de margem não é
/// penalizada. Só a capacidade total recusa toda a gente, e essa recusa não é
/// «por parte justa».
#[sqlx::test(migrations = "./migrations")]
async fn a_tenant_alone_on_the_node_is_refused_only_at_the_capacity(db: sqlx::PgPool) {
    let app = app_with_capacity(db, Some("7")).await;
    let a = app.new_org("alfa.test").await;
    let a1 = app.new_room(&a, "A1").await;
    let a2 = app.new_room(&a, "A2").await;
    let a3 = app.new_room(&a, "A3").await;
    let tok_a1 = room_token(&app, a1["code"].as_str().unwrap(), &a).await;
    let tok_a2 = room_token(&app, a2["code"].as_str().unwrap(), &a).await;
    let tok_a3 = room_token(&app, a3["code"].as_str().unwrap(), &a).await;

    let _s1 = fill(&app, &tok_a1, 6).await;
    // 6 de 7: zona de margem, mas A é o único inquilino.
    let _s2 = connect(&app, &tok_a2)
        .await
        .expect("sozinha no nó, a organização não é penalizada na margem");
    assert_eq!(app.state.hub.peers_ligados(), 7);

    // Na capacidade ninguém abre salas novas, e já não é «parte justa».
    assert_eq!(connect(&app, &tok_a3).await.err(), Some(503));
    assert_eq!((refused(&app), refused_fair_share(&app)), (1, 0));
}

/// O defeito que a omissão do tecto do nó fecha, observado pela API do
/// OPERADOR e não pela opinião do cliente de teste: **sem `NODE_PEER_CAPACITY`
/// declarado**, `peer_capacity` era `null` e nada se recusava. Era o estado de
/// uma implantação por compose, que não declarava nenhuma das duas variáveis.
///
/// Agora a ausência vale `DEFAULT_NODE_PEER_CAPACITY`, e o inventário mostra-o:
/// o nó publica a capacidade com que vai mesmo recusar, e `load` deixa de ser
/// `null`. A quota de ORGANIZAÇÃO continua ausente de propósito — é um limite
/// de plano, e dar-lhe omissão cortava a 80 % uma org sozinha no nó, contra o
/// que o `admit_new_room` calcula dinamicamente.
#[sqlx::test(migrations = "./migrations")]
async fn without_any_variable_the_node_still_declares_a_ceiling(db: sqlx::PgPool) {
    // 1.ª instância só para criar a conta que vai ser administradora.
    let boot = TestApp::spawn(db.clone()).await;
    let op = boot.new_org("plataforma.ao").await;
    // A instância sob teste: administrador declarado, e NENHUM dos dois tectos.
    let app = TestApp::spawn_with(db, &[("PLATFORM_ADMIN_USER_IDS", op.user_id.as_str())]).await;

    assert_eq!(
        app.state.config.node_peer_capacity,
        Some(delonix_server::config::DEFAULT_NODE_PEER_CAPACITY),
        "sem a variável, o tecto do nó TEM de existir — `None` não recusa ninguém"
    );
    assert_eq!(
        app.state.config.org_max_participants, None,
        "a quota da organização é declarado-só: é um limite de plano"
    );

    delonix_server::nodes::heartbeat(&app.state, chrono::Utc::now())
        .await
        .unwrap();

    // Observação de FORA: o inventário do operador, não o estado em memória.
    let (st, v) = app.get("/api/operator/v1/nodes", Some(&op.token)).await;
    assert_eq!(st, 200, "{v}");
    let eu = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["hostname"].is_string())
        .cloned()
        .unwrap_or_else(|| panic!("o nó não se anunciou: {v}"));
    assert_eq!(
        eu["peer_capacity"].as_i64(),
        Some(delonix_server::config::DEFAULT_NODE_PEER_CAPACITY as i64),
        "o nó publica a capacidade com que vai recusar: {v}"
    );
    assert!(
        eu["load"].as_f64().is_some(),
        "sem capacidade o `load` era null — o operador não via ocupação nenhuma: {v}"
    );
}

/// A recusa tem de chegar a quem a sofre. Um `/ws` recusado no HANDSHAKE não
/// entrega ao browser o estado HTTP nem o corpo — chega um `onclose` com
/// `code: 1006`, igual a uma falha de rede, e a pessoa via «Ligação instável.
/// Verifica a rede» com a rede dela perfeita.
///
/// A sondagem `/ws/admission` corre a MESMA `admitir()` e devolve a razão com
/// `code` estável. Vive no prefixo `/ws` porque é o único com afinidade por
/// `$arg_room` (ADR-0001): em `/api` responderia um pod ao acaso, com a
/// contagem de outro nó.
#[sqlx::test(migrations = "./migrations")]
async fn the_refusal_says_why_through_the_admission_probe(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(
        db,
        &[("NODE_PEER_CAPACITY", "5"), ("ORG_MAX_PARTICIPANTS", "4")],
    )
    .await;
    let dona = app.new_org("alfa.test").await;
    let sala = app.new_room(&dona, "A").await;
    let code = sala["code"].as_str().unwrap().to_string();
    let tok = room_token(&app, &code, &dona).await;

    // Com lugar, a sondagem diz «entrarias»: 204 e nada a explicar.
    let (st, _) = app
        .get(&format!("/ws/admission?token={tok}&room={code}"), None)
        .await;
    assert_eq!(st, 204, "com lugar livre a sondagem não inventa recusa");

    let _ocupados = fill(&app, &tok, 4).await;

    // O socket recusa — e o browser não saberia porquê.
    assert_eq!(connect(&app, &tok).await.err(), Some(429));

    // A sondagem diz porquê, com código estável e frase legível.
    let (st, corpo) = app
        .get(&format!("/ws/admission?token={tok}&room={code}"), None)
        .await;
    assert_eq!(st, 429, "{corpo}");
    assert_eq!(
        corpo["code"], "admission.org_at_capacity",
        "sem código próprio isto era `rate_limited` — indistinguível de «pediste depressa de mais»: {corpo}"
    );
    assert!(
        corpo["error"]
            .as_str()
            .unwrap_or("")
            .contains("organização"),
        "a frase tem de explicar-se a quem a lê: {corpo}"
    );
    // A recusa não expõe o vizinho (ADR-0017): não diz quanto ocupam os outros.
    let frase = corpo["error"].as_str().unwrap_or("");
    assert!(!frase.contains('%') && !frase.contains(" de 5"), "{frase}");

    // A SONDAGEM NÃO CONTA COMO RECUSA. Uma recusa é um cliente que não
    // entrou (é o que a anotação do alerta diz); se a explicação também
    // contasse, cada pessoa somava 2 — e qualquer cliente com um room token
    // válido inflacionava `MeetSalasRecusadasPorCapacidade` em ciclo.
    let antes = refused_org_quota(&app);
    for _ in 0..5 {
        let (st, _) = app
            .get(&format!("/ws/admission?token={tok}&room={code}"), None)
            .await;
        assert_eq!(st, 429);
    }
    assert_eq!(
        refused_org_quota(&app),
        antes,
        "cinco sondagens não são cinco recusas"
    );
}
