//! Registo de entregas de webhooks e reenvio (G7) contra Postgres real e um
//! receptor HTTP real em 127.0.0.1 (na allowlist `OUTBOUND_ALLOW_HOSTS`).
//!
//! O evento dispara-se pelo caminho do produto — criar uma reunião dispara
//! `meeting.created` —, não por uma chamada directa a `fire()`.
mod common;

use std::{
    sync::{
        atomic::{AtomicU16, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Router,
};
use common::{Account, TestApp, INVENTED_ID};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;

const SECRET: &str = "segredo-hmac-de-teste";

/// Um pedido recebido pelo receptor: cabeçalhos e bytes do corpo.
#[derive(Clone)]
struct Received {
    headers: HeaderMap,
    body: Bytes,
}

/// Estado do receptor: o código a devolver e o que já chegou.
type ReceiverState = (Arc<AtomicU16>, Arc<Mutex<Vec<Received>>>);

#[derive(Clone)]
struct Receiver {
    status: Arc<AtomicU16>,
    got: Arc<Mutex<Vec<Received>>>,
    url: String,
}

impl Receiver {
    async fn spawn() -> Self {
        let status = Arc::new(AtomicU16::new(200));
        let got = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route(
                "/hook",
                post(
                    |State((status, got)): State<ReceiverState>,
                     headers: HeaderMap,
                     body: Bytes| async move {
                        got.lock().unwrap().push(Received { headers, body });
                        StatusCode::from_u16(status.load(Ordering::SeqCst)).unwrap()
                    },
                ),
            )
            .with_state((status.clone(), got.clone()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            status,
            got,
            url: format!("http://127.0.0.1:{}/hook", addr.port()),
        }
    }

    fn received(&self) -> Vec<Received> {
        self.got.lock().unwrap().clone()
    }
}

async fn spawn_app(db: sqlx::PgPool) -> TestApp {
    TestApp::spawn_with(db, &[("OUTBOUND_ALLOW_HOSTS", "127.0.0.1")]).await
}

fn hooks(org: &str) -> String {
    format!("/api/orgs/{org}/webhooks")
}

async fn new_hook(app: &TestApp, admin: &Account, url: &str) -> String {
    let (st, hook) = app
        .post(
            &hooks(admin.org()),
            Some(&admin.token),
            json!({"kind": "generic", "url": url, "secret": SECRET, "events": "meeting.created"}),
        )
        .await;
    assert_eq!(st, 200, "{hook}");
    hook["id"].as_str().unwrap().to_string()
}

/// Espera até haver `n` entregas FECHADAS (nenhuma `pending`) e devolve-as,
/// mais recentes primeiro.
async fn wait_final(app: &TestApp, admin: &Account, hook: &str, n: usize) -> Vec<Value> {
    let path = format!("{}/{hook}/deliveries?page_size=100", hooks(admin.org()));
    for _ in 0..200 {
        let (st, page) = app.get(&path, Some(&admin.token)).await;
        assert_eq!(st, 200, "{page}");
        let items = page["items"].as_array().unwrap().clone();
        if items.len() >= n && items.iter().all(|d| d["status"] != "pending") {
            return items;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("as entregas não fecharam a tempo");
}

fn signature(body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(SECRET.as_bytes()).unwrap();
    mac.update(body);
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

#[sqlx::test(migrations = "./migrations")]
async fn delivery_is_recorded_and_redelivery_sends_identical_payload(db: sqlx::PgPool) {
    let app = spawn_app(db).await;
    let rx = Receiver::spawn().await;
    let a = app.new_org("alfa.test").await;
    let hook = new_hook(&app, &a, &rx.url).await;

    // O recurso webhook está completo: GET por id.
    let (st, one) = app
        .get(&format!("{}/{hook}", hooks(a.org())), Some(&a.token))
        .await;
    assert_eq!(st, 200, "{one}");
    assert_eq!(one["id"], hook.as_str());
    assert!(one.get("secret").is_none());

    app.new_meeting(&a, "Reunião do registo", &[]).await;
    let items = wait_final(&app, &a, &hook, 1).await;
    assert_eq!(items.len(), 1);
    let d = &items[0];
    assert_eq!(d["status"], "succeeded", "{d}");
    assert_eq!(d["event"], "meeting.created");
    assert_eq!(d["response_status"], 200);
    assert!(d["response_ms"].as_i64().unwrap() >= 0);
    assert!(d["error"].is_null());
    assert_eq!(d["attempt"], 1);
    assert!(d["delivered_at"].is_string());
    assert!(d.get("payload").is_none(), "a listagem não traz o payload");
    let first_id = d["id"].as_str().unwrap().to_string();

    // O detalhe traz o corpo EXACTO que o receptor recebeu.
    let loc = format!("{}/{hook}/deliveries/{first_id}", hooks(a.org()));
    let (st, detail) = app.get(&loc, Some(&a.token)).await;
    assert_eq!(st, 200, "{detail}");
    assert_eq!(detail["payload"]["event"], "meeting.created");
    assert_eq!(detail["payload"]["data"]["title"], "Reunião do registo");
    let got = rx.received();
    assert_eq!(got.len(), 1);
    let sent: Value = serde_json::from_slice(&got[0].body).unwrap();
    assert_eq!(sent, detail["payload"]);
    assert_eq!(got[0].headers["x-delonix-delivery"], first_id.as_str());
    assert_eq!(
        got[0].headers["x-delonix-signature"],
        signature(&got[0].body).as_str()
    );

    // Nem o segredo nem a assinatura ficam guardados.
    let leaked: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM webhook_deliveries
          WHERE payload::text LIKE '%' || $1 || '%' OR coalesce(error, '') LIKE '%sha256=%'",
    )
    .bind(SECRET)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(leaked, 0);

    // Reenvio: 202 + Location + entrega nova `pending` ligada à original.
    let res = app
        .http
        .post(app.url(&format!("{loc}/redeliver")))
        .bearer_auth(&a.token)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 202);
    let location = res.headers()["location"].to_str().unwrap().to_string();
    let new: Value = res.json().await.unwrap();
    let new_id = new["id"].as_str().unwrap().to_string();
    assert_eq!(
        location,
        format!("{}/{hook}/deliveries/{new_id}", hooks(a.org()))
    );
    assert_eq!(new["status"], "pending");
    assert_eq!(new["redelivery_of"], first_id.as_str());
    assert_eq!(new["attempt"], 2);
    assert_ne!(new_id, first_id);

    let items = wait_final(&app, &a, &hook, 2).await;
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["id"], new_id.as_str(), "mais recentes primeiro");
    assert_eq!(items[0]["status"], "succeeded");
    let got = rx.received();
    assert_eq!(got.len(), 2);
    assert_eq!(got[1].body, got[0].body, "o reenvio manda os MESMOS bytes");
    assert_eq!(got[1].headers["x-delonix-delivery"], new_id.as_str());
    assert_eq!(
        got[1].headers["x-delonix-signature"],
        signature(&got[1].body).as_str()
    );
    let (st, via_location) = app.get(&location, Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(via_location["payload"], detail["payload"]);

    // Apagar o webhook leva o registo (ON DELETE CASCADE).
    let (st, _) = app
        .delete(&format!("{}/{hook}", hooks(a.org())), Some(&a.token))
        .await;
    assert_eq!(st, 204);
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM webhook_deliveries")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn failed_deliveries_filter_and_pagination(db: sqlx::PgPool) {
    let app = spawn_app(db).await;
    let rx = Receiver::spawn().await;
    rx.status.store(500, Ordering::SeqCst);
    let a = app.new_org("alfa.test").await;
    let hook = new_hook(&app, &a, &rx.url).await;

    for i in 0..3 {
        app.new_meeting(&a, &format!("r{i}"), &[]).await;
    }
    let items = wait_final(&app, &a, &hook, 3).await;
    assert_eq!(items.len(), 3);
    for d in &items {
        assert_eq!(d["status"], "failed", "{d}");
        assert_eq!(d["response_status"], 500);
        assert_eq!(d["error"], "o destino respondeu HTTP 500");
    }

    // O destino recupera; o reenvio de uma falhada fica `succeeded`.
    rx.status.store(204, Ordering::SeqCst);
    let failed_id = items[2]["id"].as_str().unwrap();
    let (st, v) = app
        .post(
            &format!("{}/{hook}/deliveries/{failed_id}/redeliver", hooks(a.org())),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 202, "{v}");
    let all = wait_final(&app, &a, &hook, 4).await;
    assert_eq!(all.len(), 4);
    assert_eq!(all[0]["status"], "succeeded");
    assert_eq!(all[0]["response_status"], 204);

    // Paginação: 2 a 2, mais recentes primeiro, completa e sem repetições.
    let base = format!("{}/{hook}/deliveries", hooks(a.org()));
    let mut seen: Vec<Value> = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let q = match &token {
            Some(t) => format!("{base}?page_size=2&page_token={t}"),
            None => format!("{base}?page_size=2"),
        };
        let (st, p) = app.get(&q, Some(&a.token)).await;
        assert_eq!(st, 200, "{p}");
        let page = p["items"].as_array().unwrap();
        assert!(page.len() <= 2);
        seen.extend(page.iter().cloned());
        token = p["next_page_token"].as_str().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    let ids: Vec<&str> = seen.iter().map(|d| d["id"].as_str().unwrap()).collect();
    let expected: Vec<&str> = all.iter().map(|d| d["id"].as_str().unwrap()).collect();
    assert_eq!(ids, expected, "a paginação percorre a mesma ordem da lista");

    // Filtro por estado.
    let (_, f) = app
        .get(&format!("{base}?status=failed"), Some(&a.token))
        .await;
    assert_eq!(f["items"].as_array().unwrap().len(), 3);
    let (_, s) = app
        .get(&format!("{base}?status=succeeded"), Some(&a.token))
        .await;
    assert_eq!(s["items"].as_array().unwrap().len(), 1);
    let (st, bad) = app
        .get(&format!("{base}?status=zombie"), Some(&a.token))
        .await;
    assert_eq!(st, 400, "{bad}");
    assert_eq!(bad["code"], "webhook_delivery.invalid_status");
    let (st, bad) = app
        .get(&format!("{base}?page_token=%%%lixo"), Some(&a.token))
        .await;
    assert_eq!(st, 400, "{bad}");
}

#[sqlx::test(migrations = "./migrations")]
async fn redelivery_is_rate_limited_and_guarded(db: sqlx::PgPool) {
    let app = spawn_app(db).await;
    let rx = Receiver::spawn().await;
    let a = app.new_org("alfa.test").await;
    let hook = new_hook(&app, &a, &rx.url).await;
    app.new_meeting(&a, "ritmo", &[]).await;
    let items = wait_final(&app, &a, &hook, 1).await;
    let id = items[0]["id"].as_str().unwrap();
    let redeliver = format!("{}/{hook}/deliveries/{id}/redeliver", hooks(a.org()));

    for i in 0..10 {
        let (st, v) = app.post(&redeliver, Some(&a.token), json!({})).await;
        assert_eq!(st, 202, "reenvio {i}: {v}");
    }
    let res = app
        .http
        .post(app.url(&redeliver))
        .bearer_auth(&a.token)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 429);
    assert!(res.headers().get("retry-after").is_some());
    let v: Value = res.json().await.unwrap();
    assert_eq!(v["code"], "webhook_delivery.redelivery_rate_limited");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM webhook_deliveries")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 11, "o 11.º reenvio não criou linha");

    // A guarda anti-SSRF é reaplicada ao URL ACTUAL: se o destino passar a ser
    // uma rede interna não nomeada, o reenvio é recusado e nada se cria.
    sqlx::query("UPDATE org_webhooks SET url = 'http://10.0.0.1/hook' WHERE id = $1::uuid")
        .bind(&hook)
        .execute(&app.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM webhook_deliveries WHERE redelivery_of IS NOT NULL")
        .execute(&app.db)
        .await
        .unwrap();
    let (st, v) = app.post(&redeliver, Some(&a.token), json!({})).await;
    assert_eq!(st, 400, "{v}");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM webhook_deliveries")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 1);

    // Webhook inactivo: 422, sem linha nova.
    sqlx::query("UPDATE org_webhooks SET url = $2, active = FALSE WHERE id = $1::uuid")
        .bind(&hook)
        .bind(&rx.url)
        .execute(&app.db)
        .await
        .unwrap();
    let (st, v) = app.post(&redeliver, Some(&a.token), json!({})).await;
    assert_eq!(st, 422, "{v}");
    assert_eq!(v["code"], "webhook.inactive");
}

#[sqlx::test(migrations = "./migrations")]
async fn other_org_and_non_admin_are_refused(db: sqlx::PgPool) {
    let app = spawn_app(db).await;
    let rx = Receiver::spawn().await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let hook_b = new_hook(&app, &b, &rx.url).await;
    app.new_meeting(&b, "da B", &[]).await;
    let items = wait_final(&app, &b, &hook_b, 1).await;
    let del_b = items[0]["id"].as_str().unwrap().to_string();
    let hook_a = new_hook(&app, &a, &rx.url).await;

    let b_hook = format!("{}/{hook_b}", hooks(b.org()));
    let b_del = format!("{b_hook}/deliveries/{del_b}");
    // A com o caminho da org B, e A com o SEU caminho mas ids da B: tudo 404.
    let a_path_b_ids = format!("{}/{hook_b}/deliveries/{del_b}", hooks(a.org()));
    let a_hook_b_del = format!("{}/{hook_a}/deliveries/{del_b}", hooks(a.org()));
    for url in [
        b_hook.clone(),
        format!("{b_hook}/deliveries"),
        b_del.clone(),
        a_path_b_ids.clone(),
        a_hook_b_del.clone(),
        format!("{}/{hook_a}/deliveries/{INVENTED_ID}", hooks(a.org())),
    ] {
        let (st, v) = app.get(&url, Some(&a.token)).await;
        assert_eq!(st, 404, "GET {url}: {v}");
        assert!(!v.to_string().contains("da B"), "{v}");
    }
    for url in [&b_del, &a_path_b_ids, &a_hook_b_del] {
        let (st, v) = app
            .post(&format!("{url}/redeliver"), Some(&a.token), json!({}))
            .await;
        assert_eq!(st, 404, "POST {url}/redeliver: {v}");
    }

    // Um membro da B que não é admin é recusado (401: forma herdada de
    // `org::require_admin`) em todas as rotas.
    let member = app.add_member(&b, "colega", "member").await;
    for url in [
        b_hook.clone(),
        format!("{b_hook}/deliveries"),
        b_del.clone(),
    ] {
        let (st, v) = app.get(&url, Some(&member.token)).await;
        assert!([401, 403].contains(&st), "GET {url}: {st} {v}");
    }
    let (st, v) = app
        .post(
            &format!("{b_del}/redeliver"),
            Some(&member.token),
            json!({}),
        )
        .await;
    assert!([401, 403].contains(&st), "{st} {v}");

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM webhook_deliveries")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 1, "nenhuma recusa criou um reenvio");
    assert_eq!(rx.received().len(), 1);
}

/// Há `retry_at` agendado na entrega `id`?
async fn scheduled(app: &TestApp, id: &str) -> bool {
    sqlx::query_scalar("SELECT retry_at IS NOT NULL FROM webhook_deliveries WHERE id = $1::uuid")
        .bind(id)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

/// Faz passar o relógio: o que está agendado fica vencido.
async fn make_due(app: &TestApp) {
    sqlx::query(
        "UPDATE webhook_deliveries SET retry_at = now() - interval '1 second'
          WHERE retry_at IS NOT NULL",
    )
    .execute(&app.db)
    .await
    .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn a_transient_failure_is_retried_until_it_succeeds(db: sqlx::PgPool) {
    let app = spawn_app(db).await;
    let rx = Receiver::spawn().await;
    rx.status.store(503, Ordering::SeqCst);
    let a = app.new_org("alfa.test").await;
    let hook = new_hook(&app, &a, &rx.url).await;
    app.new_meeting(&a, "r", &[]).await;

    let items = wait_final(&app, &a, &hook, 1).await;
    let first = items[0]["id"].as_str().unwrap().to_string();
    assert_eq!(items[0]["status"], "failed");
    assert!(
        scheduled(&app, &first).await,
        "503 é repetível: tinha de ficar agendado"
    );
    // Ainda não venceu: um passo do worker agora não manda nada.
    assert_eq!(
        delonix_server::webhook_retry_due(&app.state).await.unwrap(),
        0
    );
    assert_eq!(rx.received().len(), 1);

    // Venceu mas o destino continua em baixo: nova tentativa, também falhada,
    // e agendada outra vez — com a história na linha nova.
    make_due(&app).await;
    assert_eq!(
        delonix_server::webhook_retry_due(&app.state).await.unwrap(),
        1
    );
    let items = wait_final(&app, &a, &hook, 2).await;
    assert_eq!(items[0]["status"], "failed");
    assert_eq!(items[0]["attempt"], 2);
    assert_eq!(items[0]["redelivery_of"], first.as_str());
    assert!(
        !scheduled(&app, &first).await,
        "a original deixa de estar agendada"
    );
    assert!(scheduled(&app, items[0]["id"].as_str().unwrap()).await);

    // O destino recupera: a seguinte fecha com sucesso e nada fica agendado.
    rx.status.store(204, Ordering::SeqCst);
    make_due(&app).await;
    assert_eq!(
        delonix_server::webhook_retry_due(&app.state).await.unwrap(),
        1
    );
    let items = wait_final(&app, &a, &hook, 3).await;
    assert_eq!(items[0]["status"], "succeeded");
    assert_eq!(items[0]["attempt"], 3);
    let left: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM webhook_deliveries WHERE retry_at IS NOT NULL")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(left, 0);
    assert_eq!(
        delonix_server::webhook_retry_due(&app.state).await.unwrap(),
        0
    );

    // Os três envios levaram o MESMO corpo, bit a bit, e assinaturas válidas.
    let got = rx.received();
    assert_eq!(got.len(), 3);
    assert!(
        got.iter().all(|r| r.body == got[0].body),
        "o corpo muda entre tentativas"
    );
    for r in &got {
        assert_eq!(
            r.headers["x-delonix-signature"].to_str().unwrap(),
            signature(&r.body)
        );
    }
    let ids: std::collections::HashSet<_> = got
        .iter()
        .map(|r| {
            r.headers["x-delonix-delivery"]
                .to_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(ids.len(), 3, "cada tentativa tem o seu X-Delonix-Delivery");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_permanent_failure_is_not_retried(db: sqlx::PgPool) {
    let app = spawn_app(db).await;
    let rx = Receiver::spawn().await;
    rx.status.store(404, Ordering::SeqCst);
    let a = app.new_org("alfa.test").await;
    let hook = new_hook(&app, &a, &rx.url).await;
    app.new_meeting(&a, "r", &[]).await;

    let items = wait_final(&app, &a, &hook, 1).await;
    assert_eq!(items[0]["status"], "failed");
    assert_eq!(items[0]["response_status"], 404);
    assert!(
        !scheduled(&app, items[0]["id"].as_str().unwrap()).await,
        "um 404 não passa sozinho"
    );
    make_due(&app).await;
    assert_eq!(
        delonix_server::webhook_retry_due(&app.state).await.unwrap(),
        0
    );
    assert_eq!(rx.received().len(), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn retries_stop_after_the_last_attempt(db: sqlx::PgPool) {
    let app = spawn_app(db).await;
    let rx = Receiver::spawn().await;
    rx.status.store(500, Ordering::SeqCst);
    let a = app.new_org("alfa.test").await;
    let hook = new_hook(&app, &a, &rx.url).await;
    app.new_meeting(&a, "r", &[]).await;
    wait_final(&app, &a, &hook, 1).await;

    let max = delonix_meet_domain::integration::webhook_delivery::MAX_AUTO_ATTEMPTS as usize;
    for n in 2..=max {
        make_due(&app).await;
        assert_eq!(
            delonix_server::webhook_retry_due(&app.state).await.unwrap(),
            1,
            "tentativa {n}"
        );
        wait_final(&app, &a, &hook, n).await;
    }
    let left: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM webhook_deliveries WHERE retry_at IS NOT NULL")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(left, 0, "esgotadas as tentativas não fica nada agendado");
    make_due(&app).await;
    assert_eq!(
        delonix_server::webhook_retry_due(&app.state).await.unwrap(),
        0
    );
    assert_eq!(rx.received().len(), max);
}

#[sqlx::test(migrations = "./migrations")]
async fn two_workers_never_retry_the_same_delivery(db: sqlx::PgPool) {
    let app = spawn_app(db).await;
    let rx = Receiver::spawn().await;
    rx.status.store(502, Ordering::SeqCst);
    let a = app.new_org("alfa.test").await;
    let hook = new_hook(&app, &a, &rx.url).await;
    for i in 0..6 {
        app.new_meeting(&a, &format!("r{i}"), &[]).await;
    }
    wait_final(&app, &a, &hook, 6).await;
    make_due(&app).await;

    // Quatro passos em simultâneo (quatro nós) sobre seis entregas vencidas.
    let results = futures_util::future::join_all(
        (0..4).map(|_| delonix_server::webhook_retry_due(&app.state)),
    )
    .await;
    let sent: usize = results.into_iter().map(|r| r.unwrap()).sum();
    assert_eq!(
        sent, 6,
        "cada entrega vencida é repetida exactamente uma vez"
    );
    let items = wait_final(&app, &a, &hook, 12).await;
    assert_eq!(items.len(), 12);
    let retries: Vec<&Value> = items.iter().filter(|d| d["attempt"] == 2).collect();
    assert_eq!(retries.len(), 6);
    let originals: std::collections::HashSet<&str> = retries
        .iter()
        .map(|d| d["redelivery_of"].as_str().unwrap())
        .collect();
    assert_eq!(originals.len(), 6, "duas repetições da mesma entrega");
}

#[sqlx::test(migrations = "./migrations")]
async fn manual_redelivery_takes_over_the_scheduled_retry(db: sqlx::PgPool) {
    let app = spawn_app(db).await;
    let rx = Receiver::spawn().await;
    rx.status.store(503, Ordering::SeqCst);
    let a = app.new_org("alfa.test").await;
    let hook = new_hook(&app, &a, &rx.url).await;
    app.new_meeting(&a, "r", &[]).await;
    let items = wait_final(&app, &a, &hook, 1).await;
    let first = items[0]["id"].as_str().unwrap().to_string();
    assert!(scheduled(&app, &first).await);

    rx.status.store(204, Ordering::SeqCst);
    let (st, v) = app
        .post(
            &format!("{}/{hook}/deliveries/{first}/redeliver", hooks(a.org())),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 202, "{v}");
    wait_final(&app, &a, &hook, 2).await;
    assert!(
        !scheduled(&app, &first).await,
        "o reenvio manual cancela o agendado"
    );
    make_due(&app).await;
    assert_eq!(
        delonix_server::webhook_retry_due(&app.state).await.unwrap(),
        0
    );
    assert_eq!(
        rx.received().len(),
        2,
        "o destino recebeu o evento duas vezes, não três"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_removed_or_disabled_webhook_ends_the_retries(db: sqlx::PgPool) {
    let app = spawn_app(db).await;
    let rx = Receiver::spawn().await;
    rx.status.store(503, Ordering::SeqCst);
    let a = app.new_org("alfa.test").await;
    let hook = new_hook(&app, &a, &rx.url).await;
    app.new_meeting(&a, "r", &[]).await;
    let items = wait_final(&app, &a, &hook, 1).await;
    let first = items[0]["id"].as_str().unwrap().to_string();

    sqlx::query("UPDATE org_webhooks SET active = FALSE WHERE id = $1::uuid")
        .bind(&hook)
        .execute(&app.db)
        .await
        .unwrap();
    make_due(&app).await;
    assert_eq!(
        delonix_server::webhook_retry_due(&app.state).await.unwrap(),
        0
    );
    assert!(
        !scheduled(&app, &first).await,
        "desligado: a repetição acaba, não fica a pairar"
    );
    assert_eq!(rx.received().len(), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn the_event_id_is_stable_across_attempts_and_distinct_across_events(db: sqlx::PgPool) {
    use std::collections::{BTreeMap, HashSet};

    let app = spawn_app(db).await;
    let rx = Receiver::spawn().await;
    rx.status.store(503, Ordering::SeqCst);
    let a = app.new_org("alfa.test").await;
    let hook = new_hook(&app, &a, &rx.url).await;
    app.new_meeting(&a, "r1", &[]).await;
    app.new_meeting(&a, "r2", &[]).await;
    wait_final(&app, &a, &hook, 2).await;

    // Repetição automática dos dois eventos…
    make_due(&app).await;
    assert_eq!(
        delonix_server::webhook_retry_due(&app.state).await.unwrap(),
        2
    );
    let items = wait_final(&app, &a, &hook, 4).await;

    // …e um reenvio MANUAL de uma segunda tentativa.
    let segunda = items.iter().find(|d| d["attempt"] == 2).unwrap();
    let id = segunda["id"].as_str().unwrap();
    let (st, v) = app
        .post(
            &format!("{}/{hook}/deliveries/{id}/redeliver", hooks(a.org())),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 202, "{v}");
    let items = wait_final(&app, &a, &hook, 5).await;

    // Na API: duas cadeias (2 e 3 tentativas), cada uma com UM event_id.
    let mut por_evento: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    for d in &items {
        por_evento
            .entry(d["event_id"].as_str().expect("sem event_id").to_string())
            .or_default()
            .push(d["attempt"].as_i64().unwrap());
    }
    assert_eq!(
        por_evento.len(),
        2,
        "dois eventos, dois event_id: {por_evento:?}"
    );
    let mut tentativas: Vec<usize> = por_evento.values().map(Vec::len).collect();
    tentativas.sort_unstable();
    assert_eq!(tentativas, vec![2, 3], "{por_evento:?}");

    // No cabeçalho: o MESMO conjunto de event_id, e um X-Delonix-Delivery
    // diferente por tentativa.
    let got = rx.received();
    assert_eq!(got.len(), 5);
    let header = |r: &Received, name: &str| r.headers[name].to_str().unwrap().to_string();
    let eventos_no_cabecalho: HashSet<String> = got
        .iter()
        .map(|r| header(r, "x-delonix-event-id"))
        .collect();
    let eventos_na_api: HashSet<String> = por_evento.keys().cloned().collect();
    assert_eq!(eventos_no_cabecalho, eventos_na_api);
    let entregas: HashSet<String> = got
        .iter()
        .map(|r| header(r, "x-delonix-delivery"))
        .collect();
    assert_eq!(
        entregas.len(),
        5,
        "cada tentativa tem o seu X-Delonix-Delivery"
    );

    // O event_id não é o id de nenhuma tentativa: não se confunde com ele.
    assert!(
        eventos_na_api.is_disjoint(&entregas),
        "o event_id não pode ser o id de uma tentativa: {eventos_na_api:?} / {entregas:?}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn next_retry_at_is_exposed_while_a_retry_is_scheduled(db: sqlx::PgPool) {
    let app = spawn_app(db).await;
    let rx = Receiver::spawn().await;
    rx.status.store(503, Ordering::SeqCst);
    let a = app.new_org("alfa.test").await;
    let hook = new_hook(&app, &a, &rx.url).await;
    app.new_meeting(&a, "r", &[]).await;
    let items = wait_final(&app, &a, &hook, 1).await;
    assert!(
        items[0]["next_retry_at"].is_string(),
        "falha repetível: a API tem de dizer quando vai repetir: {}",
        items[0]
    );

    // Falha definitiva: nada agendado, e a API di-lo com nulo.
    rx.status.store(404, Ordering::SeqCst);
    app.new_meeting(&a, "r2", &[]).await;
    let items = wait_final(&app, &a, &hook, 2).await;
    let definitiva = items.iter().find(|d| d["response_status"] == 404).unwrap();
    assert!(definitiva["next_retry_at"].is_null(), "{definitiva}");
}

/// DUAS organizações: a B não reenvia, não lista nem lê as entregas da A, nem
/// pelo caminho da A nem pelo da B com os ids da A — e nada chega ao destino da
/// A por causa disso. A própria A continua a poder reenviar (é o controlo de que
/// as recusas são por organização e não a rota partida).
#[sqlx::test(migrations = "./migrations")]
async fn another_organization_cannot_redeliver_list_or_read_my_deliveries(db: sqlx::PgPool) {
    let app = spawn_app(db).await;
    let rx = Receiver::spawn().await;
    rx.status.store(204, Ordering::SeqCst);
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let hook = new_hook(&app, &a, &rx.url).await;
    app.new_meeting(&a, "r", &[]).await;
    let items = wait_final(&app, &a, &hook, 1).await;
    let delivery = items[0]["id"].as_str().unwrap().to_string();
    assert_eq!(rx.received().len(), 1);

    let a_path = format!("{}/{hook}/deliveries", hooks(a.org()));
    let b_path_com_ids_da_a = format!("{}/{hook}/deliveries", hooks(b.org()));

    // 1. Pelo caminho da A, com a sessão da B: B não é admin da A.
    let (st, v) = app
        .post(
            &format!("{a_path}/{delivery}/redeliver"),
            Some(&b.token),
            json!({}),
        )
        .await;
    assert!([403, 404].contains(&st), "B reenviou na org da A: {st} {v}");
    // 2. Pelo caminho da B, com o webhook e a entrega da A: não existem aí.
    let (st, v) = app
        .post(
            &format!("{b_path_com_ids_da_a}/{delivery}/redeliver"),
            Some(&b.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 404, "B reenviou uma entrega da A pelo seu caminho: {v}");
    // 3. Listar e ler: igual.
    for (path, quem, esperado) in [
        (a_path.clone(), &b.token, vec![403, 404]),
        (b_path_com_ids_da_a.clone(), &b.token, vec![404]),
        (format!("{a_path}/{delivery}"), &b.token, vec![403, 404]),
        (
            format!("{b_path_com_ids_da_a}/{delivery}"),
            &b.token,
            vec![404],
        ),
    ] {
        let (st, v) = app.get(&path, Some(quem)).await;
        assert!(
            esperado.contains(&st),
            "GET {path} com a sessão da B: {st} {v}"
        );
        assert!(
            !v.to_string().contains(&delivery),
            "a resposta à B menciona a entrega da A: {v}"
        );
    }

    // Nada foi enviado nem criado por causa das tentativas da B.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        rx.received().len(),
        1,
        "o destino da A recebeu um reenvio pedido pela B"
    );
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM webhook_deliveries")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(total, 1, "as tentativas da B criaram entregas");

    // Controlo: a A reenvia pelo mesmo caminho.
    let (st, v) = app
        .post(
            &format!("{a_path}/{delivery}/redeliver"),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 202, "{v}");
    wait_final(&app, &a, &hook, 2).await;
    assert_eq!(rx.received().len(), 2);
}

/// Insere `n` entregas falhadas e vencidas de `hook`, com `retry_at` a começar
/// `older_secs` atrás e a avançar um segundo por entrega.
async fn insert_due(app: &TestApp, org: &str, hook: &str, n: i32, older_secs: i32) {
    sqlx::query(
        "INSERT INTO webhook_deliveries (org_id, webhook_id, event, payload, attempt, status, retry_at)
         SELECT $1::uuid, $2::uuid, 'meeting.created', '{\"event\":\"meeting.created\"}'::jsonb,
                1, 'failed', now() - make_interval(secs => $4 - g)
           FROM generate_series(1, $3) AS g",
    )
    .bind(org)
    .bind(hook)
    .bind(n)
    .bind(older_secs)
    .execute(&app.db)
    .await
    .unwrap();
}

/// Uma organização com muitas repetições vencidas não deixa as das outras à
/// espera: o lote de 50 reparte-se por organização. A tem 60 repetições mais
/// antigas, B tem uma mais recente — ordenado só por `retry_at`, a de B ficava
/// fora do primeiro lote.
#[sqlx::test(migrations = "./migrations")]
async fn a_backlog_in_one_organization_does_not_starve_anothers_retries(db: sqlx::PgPool) {
    let app = spawn_app(db).await;
    let rx_a = Receiver::spawn().await;
    let rx_b = Receiver::spawn().await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let hook_a = new_hook(&app, &a, &rx_a.url).await;
    let hook_b = new_hook(&app, &b, &rx_b.url).await;
    insert_due(&app, a.org(), &hook_a, 60, 3600).await;
    insert_due(&app, b.org(), &hook_b, 1, 5).await;

    // Um passo: o lote de 50, repartido — a de B entra mesmo sendo a mais recente.
    let sent = delonix_server::webhook_retry_due(&app.state).await.unwrap();
    assert_eq!(sent, 50, "o lote enche-se");
    assert_eq!(
        rx_b.received().len(),
        1,
        "a repetição da organização B tinha de entrar no primeiro lote"
    );
    assert_eq!(rx_a.received().len(), 49);

    // Só A tem repetições por fazer: o passo seguinte leva-as todas, sem teto
    // por organização que atrase quem está sozinho.
    let sent = delonix_server::webhook_retry_due(&app.state).await.unwrap();
    assert_eq!(sent, 11);
    assert_eq!(rx_a.received().len(), 60);
    // O corpo e a organização de cada repetição são os da própria: nada de A
    // chegou ao receptor de B.
    assert_eq!(rx_b.received().len(), 1);
}
