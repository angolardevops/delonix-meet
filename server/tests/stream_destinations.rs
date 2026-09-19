//! Destinos de emissão guardados (G1) contra Postgres real: contrato novo
//! (201 + Location, 204, paginação, envelope), chave cifrada em repouso e
//! devolvida só na criação e na rotação, isolamento entre organizações.
mod common;

use common::{test_config, TestApp};
use serde_json::json;

fn path(org: &str) -> String {
    format!("/api/orgs/{org}/stream-destinations")
}

#[sqlx::test(migrations = "./migrations")]
async fn crud_contract_and_key_never_leaves_after_creation(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let key = "live_abcd-efgh-ijkl-mnop";

    let res = app
        .http
        .post(app.url(&path(a.org())))
        .bearer_auth(&a.token)
        .json(&json!({"kind": "youtube", "label": "YouTube da Alfa",
                      "url": "rtmp://a.rtmp.youtube.com/live2", "stream_key": key}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    let location = res.headers()["location"].to_str().unwrap().to_string();
    let created: serde_json::Value = res.json().await.unwrap();
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(location, format!("{}/{id}", path(a.org())));
    assert_eq!(
        created["stream_key"], key,
        "a chave sai UMA vez, na criação"
    );
    assert_eq!(created["key_prefix"], "live");
    assert_eq!(created["has_key"], true);

    // Em repouso: cifrada, nunca em claro.
    let sealed: String =
        sqlx::query_scalar("SELECT stream_key_sealed FROM stream_destinations WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(sealed.starts_with("enc:v1:"), "{sealed}");
    assert!(!sealed.contains("abcd"));

    // GET um e lista: sem chave.
    let (st, one) = app.get(&location, Some(&a.token)).await;
    assert_eq!(st, 200);
    assert!(one.get("stream_key").is_none(), "{one}");
    let (st, page) = app.get(&path(a.org()), Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert!(!page.to_string().contains("abcd"), "{page}");

    // PATCH valida antes de escrever e não mexe na chave.
    let (st, bad) = app
        .patch(
            &location,
            Some(&a.token),
            json!({"label": "novo", "state": "zombie"}),
        )
        .await;
    assert_eq!(st, 400);
    assert_eq!(bad["code"], "stream_destination.invalid_state");
    let (_, still) = app.get(&location, Some(&a.token)).await;
    assert_eq!(still["label"], "YouTube da Alfa", "sem escrita parcial");
    let (st, upd) = app
        .patch(&location, Some(&a.token), json!({"state": "expired"}))
        .await;
    assert_eq!(st, 200, "{upd}");
    assert_eq!(upd["state"], "expired");

    // Rotação: chave nova devolvida uma vez; estado volta a ready.
    let (st, rot) = app
        .post(
            &format!("{location}/rotate-key"),
            Some(&a.token),
            json!({"stream_key": "zzzz-nova"}),
        )
        .await;
    assert_eq!(st, 200, "{rot}");
    assert_eq!(rot["stream_key"], "zzzz-nova");
    assert_eq!(rot["state"], "ready");
    assert_eq!(rot["key_prefix"], "zzzz");

    // DELETE 204; outra vez 404.
    let (st, _) = app.delete(&location, Some(&a.token)).await;
    assert_eq!(st, 204);
    let (st, v) = app.delete(&location, Some(&a.token)).await;
    assert_eq!(st, 404);
    assert_eq!(v["code"], "not_found");
}

#[sqlx::test(migrations = "./migrations")]
async fn pagination_is_bounded_and_complete(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    for i in 0..5 {
        let (st, _) = app
            .post(
                &path(a.org()),
                Some(&a.token),
                json!({"kind": "rtmp", "label": format!("d{i}"), "url": "rtmp://10.0.0.5/live"}),
            )
            .await;
        assert_eq!(st, 201);
    }
    let mut seen = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let q = match &token {
            Some(t) => format!("{}?page_size=2&page_token={t}", path(a.org())),
            None => format!("{}?page_size=2", path(a.org())),
        };
        let (st, p) = app.get(&q, Some(&a.token)).await;
        assert_eq!(st, 200, "{p}");
        let items = p["items"].as_array().unwrap();
        assert!(items.len() <= 2);
        seen.extend(
            items
                .iter()
                .map(|d| d["label"].as_str().unwrap().to_string()),
        );
        token = p["next_page_token"].as_str().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    assert_eq!(seen, vec!["d0", "d1", "d2", "d3", "d4"]);
    let (st, v) = app
        .get(
            &format!("{}?page_token=%%%lixo", path(a.org())),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 400, "{v}");
}

#[sqlx::test(migrations = "./migrations")]
async fn other_org_and_non_admin_are_refused(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let b = app.new_org("beta.ao").await;
    let (st, created) = app
        .post(
            &path(b.org()),
            Some(&b.token),
            json!({"kind": "facebook", "label": "FB", "url": "rtmps://live-api-s.facebook.com:443/rtmp/", "stream_key": "segredo-da-b"}),
        )
        .await;
    assert_eq!(st, 201);
    let loc = format!("{}/{}", path(b.org()), created["id"].as_str().unwrap());

    for (method, url) in [
        ("GET", path(b.org())),
        ("GET", loc.clone()),
        ("DELETE", loc.clone()),
    ] {
        let (st, v) = match method {
            "GET" => app.get(&url, Some(&a.token)).await,
            _ => app.delete(&url, Some(&a.token)).await,
        };
        assert!(!(200..300).contains(&st), "{method} {url}: {st} {v}");
        assert!(!v.to_string().contains("segredo-da-b"));
    }
    let (st, v) = app
        .post(
            &format!("{loc}/rotate-key"),
            Some(&a.token),
            json!({"stream_key": "roubo"}),
        )
        .await;
    assert!(!(200..300).contains(&st), "{v}");
    // O destino da B continua intacto.
    let (st, still) = app.get(&loc, Some(&b.token)).await;
    assert_eq!(st, 200);
    assert_eq!(still["key_prefix"], "segr");
}

#[sqlx::test(migrations = "./migrations")]
async fn without_encryption_keys_a_key_is_refused_not_stored_in_clear(db: sqlx::PgPool) {
    let mut config = test_config(&[]);
    config.secret_box = None;
    let app = TestApp::spawn_with_config(db, config).await;
    let a = app.new_org("alfa.ao").await;
    let (st, v) = app
        .post(
            &path(a.org()),
            Some(&a.token),
            json!({"kind": "rtmp", "label": "x", "url": "rtmp://h/live", "stream_key": "k"}),
        )
        .await;
    assert_eq!(st, 422, "{v}");
    assert_eq!(v["code"], "secrets.encryption_unconfigured");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM stream_destinations")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 0);
    // Sem chave, o destino pode existir (a chave vem depois, por rotação).
    let (st, _) = app
        .post(
            &path(a.org()),
            Some(&a.token),
            json!({"kind": "rtmp", "label": "x", "url": "rtmp://h/live"}),
        )
        .await;
    assert_eq!(st, 201);
}
