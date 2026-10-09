//! `meeting.ended` e o reenfileiramento defensivo da acta — disparados por
//! `meetings::on_room_emptied` quando uma sala com reunião associada fica
//! genuinamente vazia. O sinal normal é a varredura de lugares expirados
//! (R91, `signaling::expire_disconnected`) — um simples `drop(ws)`, sem
//! `ClientMsg::Leave` nem media WebRTC negociada, só conta como saída
//! depois da janela de graça. Estes testes chamam
//! `delonix_server::sweep_expired_seats(&app.state, Duration::ZERO)`
//! directamente (exposta para isto — `TestApp` não arranca o cron real de
//! `lib.rs`) em vez de esperar por um temporizador.
mod common;

use std::time::Duration;

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Router,
};
use common::{Account, TestApp};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio_tungstenite::tungstenite::Message;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn spawn_app(db: sqlx::PgPool) -> TestApp {
    TestApp::spawn_with(db, &[("OUTBOUND_ALLOW_HOSTS", "127.0.0.1")]).await
}

/// Varre com janela ZERO (expira já qualquer lugar marcado `disconnected_at`,
/// mesmo há 1ms) em repetição curta, até o servidor ter processado o
/// `drop(ws)` (chamado `disconnect()` no fundo de `handle_socket`) — não há
/// um acessor público para "este peer já está marcado", por isso repete a
/// própria varredura em vez de inventar um. Sem efeito e sem custo chamar
/// isto antes de o servidor ter processado o fecho: não há nada para expirar
/// e devolve 0.
async fn sweep_until_expired(app: &TestApp) -> usize {
    for _ in 0..100 {
        let n = delonix_server::sweep_expired_seats(&app.state, Duration::ZERO).await;
        if n > 0 {
            return n;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    0
}

/// Um receptor HTTP real, como em `webhook_deliveries.rs`.
#[derive(Clone)]
struct Receiver {
    got: Arc<Mutex<Vec<Bytes>>>,
    url: String,
}

impl Receiver {
    async fn spawn() -> Self {
        let got: Arc<Mutex<Vec<Bytes>>> = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route(
                "/hook",
                post(
                    |State(got): State<Arc<Mutex<Vec<Bytes>>>>, _h: HeaderMap, body: Bytes| async move {
                        got.lock().unwrap().push(body);
                        StatusCode::OK
                    },
                ),
            )
            .with_state(got.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            got,
            url: format!("http://127.0.0.1:{}/hook", addr.port()),
        }
    }

    fn received(&self) -> Vec<Value> {
        self.got
            .lock()
            .unwrap()
            .iter()
            .map(|b| serde_json::from_slice(b).unwrap())
            .collect()
    }
}

fn hooks(org: &str) -> String {
    format!("/api/orgs/{org}/webhooks")
}

async fn new_hook_for(app: &TestApp, admin: &Account, url: &str, events: &str) -> String {
    let (st, hook) = app
        .post(
            &hooks(admin.org()),
            Some(&admin.token),
            json!({"kind": "generic", "url": url, "secret": "segredo-de-teste", "events": events}),
        )
        .await;
    assert_eq!(st, 200, "{hook}");
    hook["id"].as_str().unwrap().to_string()
}

/// Arranca a reunião (dono) e devolve o código da sala.
async fn start_meeting(app: &TestApp, owner: &Account, meeting_id: &str) -> String {
    let (st, started) = app
        .post(
            &format!("/api/meetings/{meeting_id}/start"),
            Some(&owner.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{started}");
    started["code"].as_str().unwrap().to_string()
}

/// Entra na sala (REST `/join`) e liga o `/ws`; espera pelo `joined`.
async fn join_room_socket(app: &TestApp, code: &str, who: &Account) -> Ws {
    let (st, join) = app
        .post(
            &format!("/api/rooms/{code}/join"),
            Some(&who.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{join}");
    let token = join["room_token"].as_str().unwrap().to_string();
    let url = format!(
        "{}/ws?token={token}",
        app.base.replacen("http://", "ws://", 1)
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(url)
        .await
        .expect("ligação ao /ws recusada");
    for _ in 0..50 {
        let msg = tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .expect("sem resposta do /ws")
            .expect("o socket fechou antes do joined")
            .expect("erro no socket");
        if let Message::Text(txt) = msg {
            let v: Value = serde_json::from_str(&txt).unwrap();
            if v["type"] == "joined" {
                return ws;
            }
        }
    }
    panic!("o joined não chegou");
}

/// Espera até haver `n` entregas no receptor. `webhooks::envia` faz a
/// primeira tentativa em linha (dentro de `on_room_emptied`, que
/// `sweep_until_expired` já esperou terminar) — a entrega local a
/// `127.0.0.1` devia estar lá antes disto sequer repetir uma vez; a margem é
/// só para não depender de nenhuma garantia de escalonamento do tokio.
async fn wait_received(rx: &Receiver, n: usize) -> Vec<Value> {
    for _ in 0..50 {
        let got = rx.received();
        if got.len() >= n {
            return got;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!(
        "o webhook não chegou a tempo (recebidos: {})",
        rx.received().len()
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn meeting_ended_fires_when_the_last_participant_leaves_a_room_with_a_meeting(
    db: sqlx::PgPool,
) {
    let app = spawn_app(db).await;
    let rx = Receiver::spawn().await;
    let a = app.new_org("alfa.test").await;
    new_hook_for(&app, &a, &rx.url, "meeting.ended").await;

    let m = app.new_meeting(&a, "Reunião a terminar", &[]).await;
    let meeting_id = m["id"].as_str().unwrap().to_string();
    let code = start_meeting(&app, &a, &meeting_id).await;

    let ws = join_room_socket(&app, &code, &a).await;
    drop(ws); // única pessoa na sala sai — sala fica vazia

    let n = sweep_until_expired(&app).await;
    assert_eq!(n, 1, "um lugar tinha de expirar");

    let got = wait_received(&rx, 1).await;
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0]["event"], "meeting.ended");
    assert_eq!(got[0]["data"]["meeting_id"], meeting_id);
}

#[sqlx::test(migrations = "./migrations")]
async fn no_meeting_ended_for_an_ad_hoc_room_without_a_meeting(db: sqlx::PgPool) {
    let app = spawn_app(db).await;
    let rx = Receiver::spawn().await;
    let a = app.new_org("alfa.test").await;
    new_hook_for(&app, &a, &rx.url, "meeting.ended").await;

    // Sala pessoal/ad-hoc: criada directamente, SEM passar por
    // `meetings::start` — não tem `room_code` em nenhuma `meetings`.
    let (st, room) = app
        .post(
            "/api/rooms",
            Some(&a.token),
            json!({"name": "Sala solta", "topology": "sfu"}),
        )
        .await;
    assert_eq!(st, 200, "{room}");
    let code = room["code"].as_str().unwrap().to_string();

    let ws = join_room_socket(&app, &code, &a).await;
    drop(ws);

    // A sala fica vazia (e `on_room_emptied` corre) — a ausência que se
    // prova é o `meeting.ended`, não o esvaziamento em si.
    let n = sweep_until_expired(&app).await;
    assert_eq!(n, 1, "um lugar tinha de expirar");
    assert!(
        rx.received().is_empty(),
        "uma sala sem reunião não deveria disparar meeting.ended: {:?}",
        rx.received()
    );
}

/// Cria uma reunião com transcrição/acta já gravadas mas sem resumo de IA, e
/// "encalhada" há mais tempo do que o tempo de graça (campos escritos
/// directamente na base — não há forma pública de simular um Ollama caído
/// até às tentativas se esgotarem). Esvaziar a sala deve reenfileirar.
#[sqlx::test(migrations = "./migrations")]
async fn room_emptying_requeues_a_stuck_minutes_summary(db: sqlx::PgPool) {
    let app = spawn_app(db).await;
    let a = app.new_org("alfa.test").await;

    let m = app.new_meeting(&a, "Reunião com acta encalhada", &[]).await;
    let meeting_id = m["id"].as_str().unwrap().to_string();
    let code = start_meeting(&app, &a, &meeting_id).await;

    // Simula o estado «pedida há muito, nunca ficou pronta, esgotou as
    // tentativas» — exactamente o que a fila `mom_summary_due` (ai.rs) deixa
    // para trás quando o LLM está em baixo para sempre.
    sqlx::query(
        "UPDATE meetings
            SET transcript = 'uma transcrição longa o suficiente para contar como dados reais, repetida para passar dos 80 caracteres mínimos',
                minutes = '',
                minutes_ai_at = NULL,
                mom_queued_at = now() - interval '1 hour',
                mom_attempts = 3
          WHERE id = $1::uuid",
    )
    .bind(&meeting_id)
    .execute(&app.db)
    .await
    .unwrap();

    let ws = join_room_socket(&app, &code, &a).await;
    drop(ws);

    let n = sweep_until_expired(&app).await;
    assert_eq!(n, 1, "um lugar tinha de expirar");

    // `on_room_emptied` já correu dentro de `sweep_until_expired` — o
    // reenfileiramento é síncrono (`enqueue_mom_summary`, um `UPDATE`), sem
    // fila nem rede: lê-se já.
    let (queued_at, attempts): (chrono::DateTime<chrono::Utc>, i32) =
        sqlx::query_as("SELECT mom_queued_at, mom_attempts FROM meetings WHERE id = $1::uuid")
            .bind(&meeting_id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(attempts, 0, "reenfileirar zera as tentativas");
    assert!(
        chrono::Utc::now() - queued_at < chrono::Duration::seconds(30),
        "mom_queued_at devia ter ficado recente"
    );
}

/// Dentro do tempo de graça (acabou de ser pedida), esvaziar a sala NÃO
/// reenfileira por cima — dá tempo ao fluxo normal.
#[sqlx::test(migrations = "./migrations")]
async fn room_emptying_does_not_requeue_within_the_grace_period(db: sqlx::PgPool) {
    let app = spawn_app(db).await;
    let a = app.new_org("alfa.test").await;

    let m = app.new_meeting(&a, "Reunião dentro da graça", &[]).await;
    let meeting_id = m["id"].as_str().unwrap().to_string();
    let code = start_meeting(&app, &a, &meeting_id).await;

    sqlx::query(
        "UPDATE meetings
            SET transcript = 'uma transcrição longa o suficiente para contar como dados reais, repetida para passar dos 80 caracteres mínimos',
                minutes = '',
                minutes_ai_at = NULL,
                mom_queued_at = now() - interval '1 minute',
                mom_attempts = 1
          WHERE id = $1::uuid",
    )
    .bind(&meeting_id)
    .execute(&app.db)
    .await
    .unwrap();

    let ws = join_room_socket(&app, &code, &a).await;
    drop(ws);
    // `on_room_emptied` corre mesmo (a sala fica vazia) — a prova é que,
    // apesar disso, o tempo de graça de 15 MIN da acta (não o da reserva de
    // lugar, que aqui é irrelevante) continua a impedir o reenfileiramento.
    let n = sweep_until_expired(&app).await;
    assert_eq!(n, 1, "um lugar tinha de expirar");

    let (queued_at, attempts): (chrono::DateTime<chrono::Utc>, i32) =
        sqlx::query_as("SELECT mom_queued_at, mom_attempts FROM meetings WHERE id = $1::uuid")
            .bind(&meeting_id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(attempts, 1, "não devia ter mexido nas tentativas");
    assert!(
        chrono::Utc::now() - queued_at > chrono::Duration::seconds(30),
        "não devia ter reenfileirado dentro do tempo de graça"
    );
}
