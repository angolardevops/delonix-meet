//! Centro de notificações (G8) contra Postgres real: produtores (convite,
//! cancelamento, transcrição entregue por gRPC), contrato da caixa pessoal
//! (paginação, `unread_count`, PATCH, mark-all-read, 204/404), isolamento entre
//! utilizadores, coalescência e o push `notification` pelo `/rtc`.
mod common;

use std::{future::pending, net::SocketAddr, sync::Arc, time::Duration};

use common::{TestApp, INVENTED_ID};
use futures_util::StreamExt;
use serde_json::{json, Value};
use uuid::Uuid;

const INBOX: &str = "/api/users/me/notifications";

async fn inbox(app: &TestApp, token: &str) -> Value {
    let (st, v) = app.get(INBOX, Some(token)).await;
    assert_eq!(st, 200, "{v}");
    v
}

/// Insere `n` notificações para `user_id` com datas distintas (a mais antiga
/// primeiro). Devolve os ids pela mesma ordem.
async fn seed(app: &TestApp, user_id: &str, n: i64) -> Vec<String> {
    let mut ids = Vec::new();
    for i in 0..n {
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO notifications (user_id, kind, title, body, link, created_at)
             VALUES ($1::uuid, 'recording.ready', $2, '', '/#/recordings',
                     now() - make_interval(mins => $3::int))
             RETURNING id",
        )
        .bind(user_id)
        .bind(format!("n{i}"))
        .bind((n - i) as i32)
        .fetch_one(&app.db)
        .await
        .unwrap();
        ids.push(id.to_string());
    }
    ids
}

#[sqlx::test(migrations = "./migrations")]
async fn invitation_and_cancellation_notify_the_invitee_not_the_host(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let host = app.new_org("alfa.ao").await;
    let guest = app.add_member(&host, "ana", "member").await;
    // Uma conta de OUTRA org posta como convidada não recebe nada.
    let outsider = app.new_org("beta.ao").await;

    let meeting = app
        .new_meeting(
            &host,
            "Revisão trimestral",
            &[&guest.user_id, &host.user_id, &outsider.user_id],
        )
        .await;
    let meeting_id = meeting["id"].as_str().unwrap().to_string();

    let g = inbox(&app, &guest.token).await;
    assert_eq!(g["unread_count"], 1, "{g}");
    let items = g["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["kind"], "meeting.invited");
    assert_eq!(items[0]["title"], "Convite: Revisão trimestral");
    assert_eq!(items[0]["link"], "/#/calendar");
    assert_eq!(items[0]["data"]["meeting_id"], meeting_id.as_str());
    assert!(items[0]["read_at"].is_null());

    let h = inbox(&app, &host.token).await;
    assert_eq!(h["unread_count"], 0, "o anfitrião não é notificado: {h}");
    let o = inbox(&app, &outsider.token).await;
    assert_eq!(o["unread_count"], 0, "conta de outra org: {o}");

    let (st, v) = app
        .delete(&format!("/api/meetings/{meeting_id}"), Some(&host.token))
        .await;
    assert_eq!(st, 204, "{v}");
    let g = inbox(&app, &guest.token).await;
    assert_eq!(g["unread_count"], 2, "{g}");
    assert_eq!(g["items"][0]["kind"], "meeting.cancelled");
    assert_eq!(g["items"][0]["data"]["meeting_id"], meeting_id.as_str());
    assert_eq!(inbox(&app, &host.token).await["unread_count"], 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn inbox_contract_pagination_read_mark_all_and_delete(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let ids = seed(&app, &a.user_id, 5).await;

    // Paginação: mais recentes primeiro, limitada e completa.
    let mut seen = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let q = match &token {
            Some(t) => format!("{INBOX}?page_size=2&page_token={t}"),
            None => format!("{INBOX}?page_size=2"),
        };
        let (st, p) = app.get(&q, Some(&a.token)).await;
        assert_eq!(st, 200, "{p}");
        assert_eq!(p["unread_count"], 5);
        let items = p["items"].as_array().unwrap();
        assert!(items.len() <= 2);
        seen.extend(
            items
                .iter()
                .map(|n| n["title"].as_str().unwrap().to_string()),
        );
        token = p["next_page_token"].as_str().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    assert_eq!(seen, vec!["n4", "n3", "n2", "n1", "n0"]);
    let (st, v) = app
        .get(&format!("{INBOX}?page_token=%%%lixo"), Some(&a.token))
        .await;
    assert_eq!(st, 400, "{v}");

    // PATCH lida → unread_count desce e unread_only filtra.
    let one = format!("{INBOX}/{}", ids[4]);
    let (st, n) = app.patch(&one, Some(&a.token), json!({"read": true})).await;
    assert_eq!(st, 200, "{n}");
    assert!(n["read_at"].is_string());
    let first_read = n["read_at"].clone();
    let (_, again) = app.patch(&one, Some(&a.token), json!({"read": true})).await;
    assert_eq!(
        again["read_at"], first_read,
        "marcar outra vez não mexe na data"
    );
    let (st, p) = app
        .get(&format!("{INBOX}?unread_only=true"), Some(&a.token))
        .await;
    assert_eq!(st, 200);
    assert_eq!(p["unread_count"], 4);
    assert_eq!(p["items"].as_array().unwrap().len(), 4);
    assert!(!p.to_string().contains(&ids[4]));
    let (st, n) = app
        .patch(&one, Some(&a.token), json!({"read": false}))
        .await;
    assert_eq!(st, 200);
    assert!(n["read_at"].is_null());
    let (st, _) = app.patch(&one, Some(&a.token), json!({"lida": 1})).await;
    assert!((400..500).contains(&st), "corpo sem `read`: {st}");

    // GET um.
    let (st, n) = app.get(&one, Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(n["id"], ids[4].as_str());

    // mark-all-read devolve a contagem, não `{"ok":true}`.
    let (st, r) = app
        .post(&format!("{INBOX}/mark-all-read"), Some(&a.token), json!({}))
        .await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r, json!({"updated": 5}));
    let (_, r) = app
        .post(&format!("{INBOX}/mark-all-read"), Some(&a.token), json!({}))
        .await;
    assert_eq!(r["updated"], 0);
    assert_eq!(inbox(&app, &a.token).await["unread_count"], 0);

    // DELETE 204; outra vez 404; e o GET também.
    let (st, _) = app.delete(&one, Some(&a.token)).await;
    assert_eq!(st, 204);
    let (st, v) = app.delete(&one, Some(&a.token)).await;
    assert_eq!(st, 404);
    assert_eq!(v["code"], "not_found");
    let (st, _) = app.get(&one, Some(&a.token)).await;
    assert_eq!(st, 404);
    let (st, _) = app
        .delete(&format!("{INBOX}/{INVENTED_ID}"), Some(&a.token))
        .await;
    assert_eq!(st, 404);
    // Sem sessão: 401.
    let (st, _) = app.get(INBOX, None).await;
    assert_eq!(st, 401);
}

#[sqlx::test(migrations = "./migrations")]
async fn another_user_cannot_see_patch_or_delete(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    // B é da MESMA org: a caixa é pessoal, não da organização.
    let b = app.add_member(&a, "bruno", "admin").await;
    let ids = seed(&app, &a.user_id, 1).await;
    let one = format!("{INBOX}/{}", ids[0]);

    let (st, v) = app.get(&one, Some(&b.token)).await;
    assert_eq!(st, 404, "{v}");
    let (st, v) = app.patch(&one, Some(&b.token), json!({"read": true})).await;
    assert_eq!(st, 404, "{v}");
    let (st, v) = app.delete(&one, Some(&b.token)).await;
    assert_eq!(st, 404, "{v}");
    let (_, r) = app
        .post(&format!("{INBOX}/mark-all-read"), Some(&b.token), json!({}))
        .await;
    assert_eq!(r["updated"], 0);
    let bl = inbox(&app, &b.token).await;
    assert_eq!(bl["items"].as_array().unwrap().len(), 0);
    assert_eq!(bl["unread_count"], 0);

    // A continua intacta e por ler.
    let (st, n) = app.get(&one, Some(&a.token)).await;
    assert_eq!(st, 200);
    assert!(n["read_at"].is_null());
}

#[sqlx::test(migrations = "./migrations")]
async fn same_event_is_coalesced_per_recipient(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let insert = |key: &'static str| {
        sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO notifications (user_id, kind, title, link, dedupe_key)
             VALUES ($1::uuid, 'meeting.starting', 't', '/#/calendar', $2)
             ON CONFLICT (user_id, dedupe_key) WHERE dedupe_key IS NOT NULL DO NOTHING
             RETURNING id",
        )
        .bind(a.user_id.clone())
        .bind(key)
        .fetch_optional(&app.db)
    };
    assert!(insert("meeting.starting:m1").await.unwrap().is_some());
    assert!(insert("meeting.starting:m1").await.unwrap().is_none());
    assert!(insert("meeting.starting:m2").await.unwrap().is_some());
    // A base recusa um link externo mesmo que um produtor o tente.
    let bad = sqlx::query(
        "INSERT INTO notifications (user_id, kind, title, link)
         VALUES ($1::uuid, 'call.missed', 't', 'https://evil.example/')",
    )
    .bind(&a.user_id)
    .execute(&app.db)
    .await;
    assert!(bad.is_err());
}

async fn spawn_grpc(state: Arc<delonix_server::AppState>) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        delonix_server::grpc::serve(state, listener, pending())
            .await
            .unwrap()
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    addr
}

#[sqlx::test(migrations = "./migrations")]
async fn transcription_completed_over_grpc_notifies_the_uploader(db: sqlx::PgPool) {
    use delonix_meet_protocol::transcription::v1::{
        transcription_service_client::TranscriptionServiceClient, ClaimJobRequest,
        CompleteJobRequest,
    };
    let app = TestApp::spawn(db).await;
    let admin = app.new_org("grpc.test").await;
    let colleague = app.add_member(&admin, "carla", "member").await;
    let room = app.new_room(&admin, "Sala").await;
    let rec = app
        .insert_recording(room["id"].as_str().unwrap(), &colleague.user_id)
        .await;

    let addr = spawn_grpc(app.state.clone()).await;
    let channel = tonic::transport::Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap();
    let mut c = TranscriptionServiceClient::new(channel);
    let job = c
        .claim_job(ClaimJobRequest {
            worker_id: "gpu-1".into(),
            lease_seconds: 600,
        })
        .await
        .unwrap()
        .into_inner()
        .job
        .expect("havia uma gravação na fila");
    assert_eq!(job.recording_id, rec);
    assert_eq!(
        inbox(&app, &colleague.token).await["unread_count"],
        0,
        "reservar não notifica"
    );
    c.complete_job(CompleteJobRequest {
        recording_id: job.recording_id.clone(),
        lease_token: job.lease_token.clone(),
        transcript: "olá".into(),
        minutes: String::new(),
    })
    .await
    .unwrap();

    let v = inbox(&app, &colleague.token).await;
    assert_eq!(v["unread_count"], 1, "{v}");
    assert_eq!(v["items"][0]["kind"], "transcription.ready");
    assert_eq!(v["items"][0]["data"]["recording_id"], rec.as_str());
    assert_eq!(v["items"][0]["link"], "/#/recordings");
    assert_eq!(
        inbox(&app, &admin.token).await["unread_count"],
        0,
        "só quem carregou a gravação"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn new_notification_is_pushed_over_rtc(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let host = app.new_org("alfa.ao").await;
    let guest = app.add_member(&host, "ana", "member").await;

    let ws_url = format!(
        "{}/rtc?token={}",
        app.base.replacen("http://", "ws://", 1),
        guest.token
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url)
        .await
        .expect("ligar ao /rtc");
    // A ligação só está registada no hub depois do estado inicial (`presence`).
    let first = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("sem estado inicial no /rtc")
        .unwrap()
        .unwrap();
    let first: Value = serde_json::from_str(first.to_text().unwrap()).unwrap();
    assert_eq!(first["type"], "presence", "{first}");

    let meeting = app.new_meeting(&host, "Stand-up", &[&guest.user_id]).await;
    let meeting_id = meeting["id"].as_str().unwrap();

    let pushed = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(msg) = ws.next().await {
            let msg = msg.unwrap();
            let Ok(text) = msg.to_text() else { continue };
            let v: Value = match serde_json::from_str(text) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if v["type"] == "notification" {
                return v;
            }
        }
        panic!("o /rtc fechou sem a notificação");
    })
    .await
    .expect("a notificação não chegou pelo /rtc em 5 s");
    let n = &pushed["notification"];
    assert_eq!(n["kind"], "meeting.invited");
    assert_eq!(n["data"]["meeting_id"], meeting_id);
    // O que chegou por push é o mesmo que está na caixa.
    let v = inbox(&app, &guest.token).await;
    assert_eq!(v["items"][0]["id"], n["id"]);
}
