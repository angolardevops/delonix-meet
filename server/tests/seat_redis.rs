//! O lugar reservado (R91) copiado para o Redis, contra Postgres e Redis reais e
//! DOIS «pods» (dois `AppState` com a mesma base e o mesmo Redis, cada um com o
//! seu `SignalingHub` em memória) ligados por `/ws` a sério.
//!
//! O que se prova é a morte de um pod: o segredo que o cliente guardou num pod
//! tem de ser reconhecido por OUTRO. Sem a cópia, o segundo pod não conhece o
//! lugar e quem volta é uma pessoa nova — um convidado admitido voltaria à sala
//! de espera (o sintoma do R91, agora por morte do pod e não por F5).
//!
//! Precisa de `TEST_REDIS_URL`: sem ela falha, em vez de passar sem ter corrido
//! (`make infra` sobe o Redis; o CI tem-no como serviço).
mod common;

use std::{net::SocketAddr, sync::atomic::Ordering, time::Duration};

use common::{Account, TestApp};
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

fn redis_url() -> String {
    std::env::var("TEST_REDIS_URL").expect(
        "TEST_REDIS_URL não está definida: estes testes provam o lugar no Redis e não \
         passam sem ele (make infra sobe o Redis; ex.: redis://localhost:6379)",
    )
}

/// Um segundo «pod»: outro `AppState` sobre a MESMA base, com o seu hub em
/// memória. Montado aqui, e não com `TestApp::spawn`, porque cada `TestApp`
/// ocupa uma vaga do tecto de testes activos e dois por teste podiam bloqueá-lo.
struct Pod {
    base: String,
    state: std::sync::Arc<delonix_server::AppState>,
}

async fn spawn_pod(db: sqlx::PgPool, redis: Option<&str>) -> Pod {
    let extra: Vec<(&str, &str)> = redis.map(|u| ("REDIS_URL", u)).into_iter().collect();
    let mut config = common::test_config(&extra);
    let dir = std::env::temp_dir().join(format!("delonix-it-{}", uuid::Uuid::new_v4()));
    config.data_exports_dir = dir.join("exports");
    config.recordings_dir = dir;
    let state = delonix_server::build_state(config, db).await;
    let app = delonix_server::build_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    Pod {
        base: format!("ws://{addr}"),
        state,
    }
}

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

/// Liga ao `/ws` do pod e devolve o socket (a manter vivo) e o `joined`.
async fn join(pod: &Pod, token: &str, reconnect: Option<&str>) -> (Ws, Value) {
    let mut url = format!("{}/ws?token={token}", pod.base);
    if let Some(r) = reconnect {
        url.push_str(&format!("&reconnect={r}"));
    }
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.expect("/ws");
    for _ in 0..50 {
        let msg = tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .expect("sem resposta do /ws")
            .expect("o socket fechou antes do joined")
            .expect("erro no socket");
        if let Message::Text(t) = msg {
            let v: Value = serde_json::from_str(&t).unwrap();
            if v["type"] == "joined" {
                return (ws, v);
            }
        }
    }
    panic!("o joined não chegou");
}

/// Fecha o socket e dá ao pod tempo de registar a queda (o lugar fica reservado).
async fn drop_socket(mut ws: Ws) {
    let _ = ws.close(None).await;
    drop(ws);
    tokio::time::sleep(Duration::from_millis(400)).await;
}

struct Cena {
    app: TestApp,
    pod_a: Pod,
    pod_b: Pod,
    token: String,
}

/// Dois pods com Redis partilhado, e um token de sala do anfitrião.
async fn cena(db: sqlx::PgPool, redis: Option<String>) -> Cena {
    let app = TestApp::spawn(db.clone()).await;
    let host = app.new_org("alfa.test").await;
    let room = app.new_room(&host, "Sala").await;
    let token = room_token(&app, room["code"].as_str().unwrap(), &host).await;
    let pod_a = spawn_pod(db.clone(), redis.as_deref()).await;
    let pod_b = spawn_pod(db, redis.as_deref()).await;
    Cena {
        app,
        pod_a,
        pod_b,
        token,
    }
}

fn secret(joined: &Value) -> String {
    joined["reconnect"]
        .as_str()
        .expect("sem segredo")
        .to_string()
}

fn peer(joined: &Value) -> String {
    joined["peer_id"].as_str().unwrap().to_string()
}

#[sqlx::test(migrations = "./migrations")]
async fn a_seat_survives_a_pod_switch(db: sqlx::PgPool) {
    let c = cena(db, Some(redis_url())).await;
    let (ws, first) = join(&c.pod_a, &c.token, None).await;
    let (id, s1) = (peer(&first), secret(&first));
    drop_socket(ws).await;

    // O pod A «morre» para este cliente: volta por B, com o segredo de A.
    let (_ws, back) = join(&c.pod_b, &c.token, Some(&s1)).await;
    assert_eq!(peer(&back), id, "tinha de herdar o lugar (o peer_id)");
    assert_ne!(secret(&back), s1, "o segredo roda a cada entrada");
    assert_eq!(
        c.pod_b
            .state
            .metrics
            .seats_reclaimed_redis_total
            .load(Ordering::Relaxed),
        1
    );
    drop(c.app);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_seat_is_single_use_across_pods(db: sqlx::PgPool) {
    let c = cena(db, Some(redis_url())).await;
    let (ws, first) = join(&c.pod_a, &c.token, None).await;
    let (id, s1) = (peer(&first), secret(&first));
    drop_socket(ws).await;

    let (ws2, back) = join(&c.pod_b, &c.token, Some(&s1)).await;
    assert_eq!(peer(&back), id);
    drop_socket(ws2).await;

    // O mesmo segredo, outra vez, no pod que o gastou: já não está no Redis.
    //
    // Deliberadamente NÃO é no pod A: o A ainda tem o lugar na memória (a janela
    // de graça é de 45 s) e honra-o. O uso único que o Redis garante é entre os
    // pods que o consultam; um pod VIVO que ainda guarda o lugar não sabe que
    // outro o gastou. Num pod que morreu — o caso para que isto existe — não há
    // memória nenhuma. Ver `redis_state::SeatRecord`.
    let (_ws3, again) = join(&c.pod_b, &c.token, Some(&s1)).await;
    assert_ne!(peer(&again), id, "um segredo gasto não volta a servir");
    drop(c.app);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_live_owner_is_not_displaced_and_the_seat_is_not_spent(db: sqlx::PgPool) {
    let c = cena(db, Some(redis_url())).await;
    let (ws, first) = join(&c.pod_a, &c.token, None).await;
    let (id, s1) = (peer(&first), secret(&first));

    // O dono está VIVO em A. Um segredo copiado a entrar em A não o expulsa…
    let (_thief, other) = join(&c.pod_a, &c.token, Some(&s1)).await;
    assert_ne!(
        peer(&other),
        id,
        "um segredo copiado não toma o lugar de quem está vivo"
    );

    // …e a recusa não gastou o lugar: quando o dono cai, ainda o reclama por B.
    drop_socket(ws).await;
    let (_ws, back) = join(&c.pod_b, &c.token, Some(&s1)).await;
    assert_eq!(peer(&back), id, "a recusa gastou o lugar do dono");
    drop(c.app);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_wrong_or_empty_secret_never_reclaims(db: sqlx::PgPool) {
    let c = cena(db, Some(redis_url())).await;
    let (ws, first) = join(&c.pod_a, &c.token, None).await;
    let id = peer(&first);
    drop_socket(ws).await;

    for bad in ["", "0".repeat(64).as_str(), "lixo"] {
        let (_w, got) = join(&c.pod_b, &c.token, Some(bad)).await;
        assert_ne!(peer(&got), id, "segredo «{bad}» reclamou um lugar");
    }
    assert_eq!(
        c.pod_b
            .state
            .metrics
            .seats_reclaimed_redis_total
            .load(Ordering::Relaxed),
        0
    );
    drop(c.app);
}

#[sqlx::test(migrations = "./migrations")]
async fn without_redis_a_pod_switch_is_a_new_person(db: sqlx::PgPool) {
    // O comportamento anterior, intacto: sem Redis o lugar é do pod.
    let c = cena(db, None).await;
    let (ws, first) = join(&c.pod_a, &c.token, None).await;
    let (id, s1) = (peer(&first), secret(&first));
    drop_socket(ws).await;
    let (_ws, back) = join(&c.pod_b, &c.token, Some(&s1)).await;
    assert_ne!(peer(&back), id);
    drop(c.app);
}
