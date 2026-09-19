//! `GET/POST /api/net-probe` — sondagem de rede pré-entrada.
mod common;

use common::TestApp;

#[sqlx::test(migrations = "./migrations")]
async fn download_returns_the_requested_bytes_uncached(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;

    let res = app
        .http
        .get(app.url("/api/net-probe?bytes=1024"))
        .bearer_auth(&a.token)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    assert_eq!(res.headers()["content-type"], "application/octet-stream");
    assert_eq!(res.headers()["cache-control"], "no-store");
    let body = res.bytes().await.unwrap();
    assert_eq!(body.len(), 1024);

    // Sem `bytes`: usa a omissão (256 KiB), não 0.
    let res = app
        .http
        .get(app.url("/api/net-probe"))
        .bearer_auth(&a.token)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    assert_eq!(res.bytes().await.unwrap().len(), 256 * 1024);
}

#[sqlx::test(migrations = "./migrations")]
async fn download_rejects_zero_and_above_the_ceiling(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;

    for bytes in [0usize, 4 * 1024 * 1024 + 1] {
        let (st, v) = app
            .get(&format!("/api/net-probe?bytes={bytes}"), Some(&a.token))
            .await;
        assert_eq!(st, 400, "{bytes}: {v}");
        assert_eq!(v["code"], "net_probe.invalid_bytes");
    }

    // O tecto exacto ainda passa.
    let res = app
        .http
        .get(app.url(&format!("/api/net-probe?bytes={}", 4 * 1024 * 1024)))
        .bearer_auth(&a.token)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
}

#[sqlx::test(migrations = "./migrations")]
async fn upload_counts_bytes_and_times_the_read(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;

    let payload = vec![7u8; 65_536];
    let res = app
        .http
        .post(app.url("/api/net-probe"))
        .bearer_auth(&a.token)
        .header("content-type", "application/octet-stream")
        .body(payload.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let v: serde_json::Value = res.json().await.unwrap();
    assert_eq!(v["bytes"], payload.len());
    assert!(v["server_ms"].as_u64().unwrap() >= 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn probe_is_rate_limited_per_account(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;

    // 30 sondagens de A esgotam a conta dela; a 31.ª é 429.
    for i in 0..30 {
        let (st, v) = app.get("/api/net-probe?bytes=16", Some(&a.token)).await;
        assert_eq!(st, 200, "pedido {i}: {v}");
    }
    let res = app
        .http
        .get(app.url("/api/net-probe?bytes=16"))
        .bearer_auth(&a.token)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 429);
    assert_eq!(res.headers()["retry-after"], "60");

    // B tem a sua própria conta — não herda o travão de A.
    let res = app
        .http
        .get(app.url("/api/net-probe?bytes=16"))
        .bearer_auth(&b.token)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200, "B não é A");
}
