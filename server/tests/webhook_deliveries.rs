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
    assert_eq!(st, 200);
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
