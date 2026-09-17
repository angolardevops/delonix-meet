//! Contrato transversal: envelope de erro, `X-Request-Id`, recusas do axum.
mod common;

use common::TestApp;

#[sqlx::test(migrations = "./migrations")]
async fn error_envelope_carries_code_and_request_id(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let res = app
        .http
        .get(app.url("/api/users/me"))
        .header("x-request-id", "rastreio-123")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
    assert_eq!(res.headers()["x-request-id"], "rastreio-123");
    let v: serde_json::Value = res.json().await.unwrap();
    assert_eq!(v["error"], "unauthorized"); // o texto que o web já lê
    assert_eq!(v["code"], "auth.unauthenticated");
    assert_eq!(v["request_id"], "rastreio-123");
}

#[sqlx::test(migrations = "./migrations")]
async fn unsafe_request_id_is_replaced(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let res = app
        .http
        .get(app.url("/health"))
        .header("x-request-id", "a b;<script>")
        .send()
        .await
        .unwrap();
    let id = res.headers()["x-request-id"].to_str().unwrap().to_string();
    assert_eq!(id.len(), 32, "gerado: {id}");
    assert_eq!(res.text().await.unwrap(), "ok"); // sonda continua em texto
}

#[sqlx::test(migrations = "./migrations")]
async fn axum_rejections_use_the_envelope(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    // JSON mal formado no login: recusa do extractor, não do handler.
    let res = app
        .http
        .post(app.url("/api/auth/login"))
        .header("content-type", "application/json")
        .body("{nao-e-json")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400);
    let v: serde_json::Value = res.json().await.unwrap();
    assert_eq!(v["code"], "invalid_argument");
    assert!(v["request_id"].is_string());

    // Rota inexistente.
    let (st, v) = app.get("/api/nao-existe", None).await;
    assert_eq!(st, 404);
    assert_eq!(v["code"], "not_found");
}

/// Reorganização de rotas de 2026-09-16 (`docs/reference/api-routes.md`): os
/// caminhos antigos saíram SEM aliases. Cada um tem de dar 404 com o envelope —
/// e com uma sessão válida (e chave/segredo onde a superfície os pedia), para
/// que o 404 seja da rota e não uma recusa de autenticação disfarçada.
#[sqlx::test(migrations = "./migrations")]
async fn old_paths_are_gone_without_aliases(db: sqlx::PgPool) {
    use reqwest::Method as M;
    use serde_json::json;
    let secret = "segredo-de-plataforma-de-teste";
    let app = TestApp::spawn_with(db, &[("PROVISIONING_SECRET", secret)]).await;
    let a = app.new_org("alfa.test").await;
    let org = a.org().to_string();
    let (_, key) = app.api_key(&a).await;
    let room = app.new_room(&a, "sala").await;
    let code = room["code"].as_str().unwrap().to_string();
    let meeting = app.new_meeting(&a, "reunião", &[]).await;
    let mid = meeting["id"].as_str().unwrap().to_string();
    let rec = app
        .insert_recording(room["id"].as_str().unwrap(), &a.user_id)
        .await;
    let session = format!("Bearer {}", a.token);
    let api_key = format!("Bearer {key}");

    // Controlo positivo: as rotas NOVAS equivalentes respondem com as mesmas
    // credenciais — o 404 de baixo não é da conta nem do servidor.
    for (method, path, auth) in [
        (M::GET, "/api/v1/organization".to_string(), &api_key),
        (M::GET, "/api/users?q=admin".to_string(), &session),
        (M::GET, "/api/ice-servers".to_string(), &session),
        (M::GET, format!("/api/orgs/{org}/members"), &session),
        (M::GET, format!("/api/recordings/{rec}"), &session),
        (
            M::GET,
            format!("/api/meetings/{mid}/calendar.ics"),
            &session,
        ),
        (M::GET, format!("/api/rooms/{code}"), &session),
    ] {
        let r = app
            .raw(method.clone(), &path, &[("Authorization", auth)], None)
            .await;
        assert_eq!(r.status, 200, "{method} {path} (nova): {}", r.text);
    }

    let x = common::INVENTED_ID;
    let old: Vec<(M, String, &str)> = vec![
        (M::POST, "/api/v1/admin/orgs".into(), "secret"),
        (M::GET, "/api/v1/platform/storage".into(), "session"),
        (M::POST, "/api/v1/integration/odoo/provision".into(), "key"),
        (M::GET, "/api/v1/org".into(), "key"),
        (M::POST, format!("/api/v1/rooms/{code}/join-bot"), "key"),
        (M::GET, format!("/api/v1/meetings/{mid}/notes"), "key"),
        (M::PATCH, format!("/api/action-items/{x}"), "session"),
        (M::GET, "/api/users/search?q=admin".into(), "session"),
        (M::GET, "/api/ice".into(), "session"),
        (M::GET, "/api/share/x".into(), "none"),
        (M::GET, "/api/share/x/download".into(), "none"),
        (M::GET, "/api/whiteboards/shared/x".into(), "none"),
        (M::GET, "/api/quarantine/analytics".into(), "session"),
        (M::POST, "/api/voice/rooms".into(), "session"),
        (M::POST, "/api/voice/ivr/validate".into(), "none"),
        (M::PUT, "/api/sms/agent/devices".into(), "none"),
        (M::POST, "/api/auth/mfa".into(), "none"),
        (M::GET, "/api/auth/sso/check".into(), "none"),
        (M::POST, "/api/missed-calls/ack".into(), "session"),
        (M::POST, "/api/translate".into(), "session"),
        (M::POST, format!("/api/orgs/{org}/settings"), "session"),
        (M::GET, format!("/api/orgs/{org}/employees"), "session"),
        (M::GET, format!("/api/orgs/{org}/audit"), "session"),
        (M::GET, format!("/api/recordings/{rec}/metadata"), "session"),
        (M::GET, format!("/api/meetings/{mid}/ics"), "session"),
        (M::POST, format!("/api/meetings/{mid}/respond"), "session"),
        (M::GET, format!("/api/rooms/{code}/chat"), "session"),
        (M::GET, format!("/api/rooms/{code}/notes"), "session"),
    ];
    for (method, path, cred) in old {
        let headers: Vec<(&str, &str)> = match cred {
            "session" => vec![("Authorization", session.as_str())],
            "key" => vec![("Authorization", api_key.as_str())],
            "secret" => vec![("X-Provisioning-Secret", secret)],
            _ => vec![],
        };
        let r = app
            .raw(method.clone(), &path, &headers, Some(json!({})))
            .await;
        assert_eq!(r.status, 404, "{method} {path} (antiga): {}", r.text);
        assert_eq!(r.json()["code"], "not_found", "{method} {path}");
    }

    // Onde só mudou o MÉTODO (singletons passaram a `PUT`), o caminho existe e
    // o método antigo é 405 — não 404, e muito menos 200.
    for path in [
        format!("/api/meetings/{mid}/minutes"),
        format!("/api/rooms/{code}/minutes"),
        format!("/api/recordings/{rec}/public-link"),
    ] {
        let r = app
            .raw(
                M::POST,
                &path,
                &[("Authorization", session.as_str())],
                Some(json!({})),
            )
            .await;
        assert_eq!(r.status, 405, "POST {path}: {}", r.text);
    }
}
