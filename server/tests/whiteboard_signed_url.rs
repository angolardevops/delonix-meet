//! PNG de quadro por URL assinado (G11) contra Postgres real: carrega SEM
//! sessão, assinatura adulterada ou expirada dá 404, quem não vê o quadro não
//! emite URL, e o PNG com sessão fica como estava.
mod common;

use base64::Engine;
use common::{Account, TestApp, INVENTED_ID, PNG_1X1};
use delonix_meet_domain::content::whiteboard as rules;
use serde_json::json;

async fn board(app: &TestApp, who: &Account) -> String {
    let (st, v) = app
        .post(
            "/api/whiteboards",
            Some(&who.token),
            json!({"title": "quadro", "png_base64": PNG_1X1}),
        )
        .await;
    assert_eq!(st, 200, "{v}");
    v["id"].as_str().unwrap().to_string()
}

async fn get_raw(app: &TestApp, path: &str, token: Option<&str>) -> (u16, Option<String>, Vec<u8>) {
    let mut rb = app.http.get(app.url(path));
    if let Some(t) = token {
        rb = rb.bearer_auth(t);
    }
    let res = rb.send().await.unwrap();
    let st = res.status().as_u16();
    let ct = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    (st, ct, res.bytes().await.unwrap().to_vec())
}

/// O URL assinado aponta para a rota da imagem (`/image`).
fn on_image_route(url: &str) -> String {
    url.to_string()
}

fn png_bytes() -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(PNG_1X1)
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn signed_url_loads_without_session(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let id = board(&app, &a).await;

    let (st, s) = app
        .post(
            &format!("/api/whiteboards/{id}/signed-url"),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{s}");
    let emitted = s["url"].as_str().unwrap().to_string();
    let url = on_image_route(&emitted);
    assert!(
        url.starts_with(&format!("/api/whiteboards/{id}/image?exp=")),
        "{url}"
    );
    // A rota antiga não existe (sem aliases).
    let old = url.replacen("/image?", "/png?", 1);
    let (st, _, _) = get_raw(&app, &old, None).await;
    assert_eq!(st, 404, "a rota antiga `/png` saiu do router");
    let expires = chrono::DateTime::parse_from_rfc3339(s["expires_at"].as_str().unwrap()).unwrap();
    let ttl = expires.timestamp() - chrono::Utc::now().timestamp();
    assert!(
        (1..=rules::SIGNED_URL_TTL_SECS).contains(&ttl),
        "≤ 15 min: {ttl}"
    );

    let (st, ct, body) = get_raw(&app, &url, None).await;
    assert_eq!(st, 200, "sem sessão, com assinatura");
    assert_eq!(ct.as_deref(), Some("image/png"));
    assert_eq!(body, png_bytes());
}

#[sqlx::test(migrations = "./migrations")]
async fn tampered_expired_or_foreign_signatures_are_404(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let id = board(&app, &a).await;
    let other = board(&app, &a).await;
    let (_, s) = app
        .post(
            &format!("/api/whiteboards/{id}/signed-url"),
            Some(&a.token),
            json!({}),
        )
        .await;
    let url = on_image_route(s["url"].as_str().unwrap());
    let (exp, sig) = {
        let q = url.split_once('?').unwrap().1;
        let mut exp = "";
        let mut sig = "";
        for kv in q.split('&') {
            match kv.split_once('=') {
                Some(("exp", v)) => exp = v,
                Some(("sig", v)) => sig = v,
                _ => {}
            }
        }
        (exp.to_string(), sig.to_string())
    };
    let flipped = {
        let mut c: Vec<char> = sig.chars().collect();
        c[0] = if c[0] == '0' { '1' } else { '0' };
        c.into_iter().collect::<String>()
    };
    let exp_n: i64 = exp.parse().unwrap();

    let cases = [
        format!("/api/whiteboards/{id}/image?exp={exp}&sig={flipped}"),
        format!("/api/whiteboards/{id}/image?exp={}&sig={sig}", exp_n + 1),
        format!("/api/whiteboards/{other}/image?exp={exp}&sig={sig}"),
        format!("/api/whiteboards/{id}/image?exp={exp}"),
        format!("/api/whiteboards/{id}/image?sig={sig}"),
        format!("/api/whiteboards/{id}/image?exp=amanha&sig={sig}"),
        format!("/api/whiteboards/nao-e-uuid/image?exp={exp}&sig={sig}"),
    ];
    for path in &cases {
        let (st, _, body) = get_raw(&app, path, None).await;
        assert_eq!(st, 404, "{path}");
        assert_ne!(body, png_bytes(), "{path}");
    }

    // Expirado: assinado com a chave verdadeira, mas o prazo já passou.
    let key =
        delonix_meet_core::crypto::derive_key(&app.state.config.jwt_secret, rules::KEY_PURPOSE);
    let past = chrono::Utc::now().timestamp() - 5;
    let expired = on_image_route(&rules::signed_path(&key, id.parse().unwrap(), past));
    let (st, _, _) = get_raw(&app, &expired, None).await;
    assert_eq!(st, 404, "expirado");
    // Controlo positivo com a mesma chave: o 404 acima é do prazo, não da chave.
    let fresh = on_image_route(&rules::signed_path(
        &key,
        id.parse().unwrap(),
        rules::expiry_from(chrono::Utc::now().timestamp()),
    ));
    let (st, _, _) = get_raw(&app, &fresh, None).await;
    assert_eq!(st, 200);

    // Apagado o quadro, o URL ainda válido deixa de servir.
    let (st, _) = app
        .delete(&format!("/api/whiteboards/{id}"), Some(&a.token))
        .await;
    assert_eq!(st, 204);
    let (st, _, _) = get_raw(&app, &url, None).await;
    assert_eq!(st, 404);
}

#[sqlx::test(migrations = "./migrations")]
async fn only_who_can_view_mints_and_session_png_is_unchanged(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let carla = app.add_member(&a, "carla", "member").await;
    let id = board(&app, &a).await;

    let (st, v) = app
        .post(
            &format!("/api/whiteboards/{id}/signed-url"),
            Some(&b.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 404, "outra org não emite: {v}");
    assert!(v.get("url").is_none());
    let (st, _) = app
        .post(
            &format!("/api/whiteboards/{INVENTED_ID}/signed-url"),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 404);
    let (st, _) = app
        .post(
            &format!("/api/whiteboards/{id}/signed-url"),
            None,
            json!({}),
        )
        .await;
    assert_eq!(st, 401);
    let (st, v) = app
        .post(
            &format!("/api/whiteboards/{id}/signed-url"),
            Some(&carla.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "colega da org vê, e por isso emite: {v}");

    // PNG com sessão: como sempre.
    let png = format!("/api/whiteboards/{id}/image");
    let (st, ct, body) = get_raw(&app, &png, Some(&a.token)).await;
    assert_eq!((st, ct.as_deref()), (200, Some("image/png")));
    assert_eq!(body, png_bytes());
    let (st, _, _) = get_raw(&app, &png, Some(&b.token)).await;
    assert_eq!(st, 404, "outra org");
    let (st, _, _) = get_raw(&app, &png, None).await;
    assert_eq!(st, 401, "sem sessão e sem assinatura");
    let (st, _, _) = get_raw(
        &app,
        &format!("/api/whiteboards/{INVENTED_ID}/image"),
        Some(&a.token),
    )
    .await;
    assert_eq!(st, 404);
}
