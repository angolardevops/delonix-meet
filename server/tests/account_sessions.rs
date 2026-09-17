//! «Dispositivos e sessões» contra Postgres e servidor reais (R200–R202).
//!
//! O que se prova: terminar uma sessão corta-a JÁ — o refresh token deixa de
//! funcionar, o access token deixa de abrir a API, e os WebSockets dessa sessão
//! (`/rtc` e o `/ws` da sala) fecham; as outras sessões continuam. E ninguém
//! vê nem termina as sessões de outra pessoa, nem o administrador da org.
mod common;

use common::{jwt_claims, TestApp, PASSWORD};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::time::Duration;

const MAC_CHROME: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";
const IPHONE: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1";

/// Uma sessão aberta num «dispositivo»: access token e cookie de refresh.
struct Device {
    token: String,
    cookie: String,
    sid: String,
}

async fn login_as(app: &TestApp, email: &str, ua: &str) -> Device {
    let res = app
        .raw(
            reqwest::Method::POST,
            "/api/auth/login",
            &[("user-agent", ua)],
            Some(json!({"email": email, "password": PASSWORD})),
        )
        .await;
    assert_eq!(res.status, 200, "login: {}", res.text);
    let token = res.json()["access_token"].as_str().unwrap().to_string();
    let cookie = res
        .set_cookies()
        .into_iter()
        .find(|c| c.starts_with("dlx_refresh="))
        .map(|c| c.split(';').next().unwrap().to_string())
        .expect("cookie de refresh");
    let sid = jwt_claims(&token)["sid"]
        .as_str()
        .expect("o access token leva o sid")
        .to_string();
    Device { token, cookie, sid }
}

async fn refresh(app: &TestApp, cookie: &str) -> (u16, Value) {
    let r = app
        .raw(
            reqwest::Method::POST,
            "/api/auth/refresh",
            &[("cookie", cookie)],
            None,
        )
        .await;
    (r.status, r.json())
}

/// Espera que o WebSocket feche (Close, erro ou fim do stream).
async fn assert_ws_closes<S>(ws: &mut S, what: &str)
where
    S: futures_util::Stream<
            Item = Result<
                tokio_tungstenite::tungstenite::Message,
                tokio_tungstenite::tungstenite::Error,
            >,
        > + Unpin,
{
    let closed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match ws.next().await {
                None | Some(Err(_)) => return true,
                Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => return true,
                Some(Ok(_)) => continue,
            }
        }
    })
    .await;
    assert_eq!(closed, Ok(true), "{what}: o WebSocket devia fechar");
}

/// R200 — terminar uma sessão corta o refresh, o access token e os WebSockets
/// dela; a sessão de onde se termina continua a funcionar.
#[sqlx::test(migrations = "./migrations")]
async fn revoking_a_session_kills_refresh_access_and_websockets(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let admin = app.new_org("alfa.test").await;
    let mac = login_as(&app, &admin.email, MAC_CHROME).await;
    let phone = login_as(&app, &admin.email, IPHONE).await;
    assert_ne!(mac.sid, phone.sid);

    // A lista mostra as duas (e a do `new_org`), com o dispositivo e a actual.
    let (st, list) = app.get("/api/users/me/sessions", Some(&mac.token)).await;
    assert_eq!(st, 200, "{list}");
    let items = list["items"].as_array().unwrap();
    let by_id = |sid: &str| items.iter().find(|s| s["id"] == sid).cloned().unwrap();
    let m = by_id(&mac.sid);
    assert_eq!(m["current"], true);
    assert_eq!(
        (m["os"].as_str(), m["browser"].as_str()),
        (Some("macOS"), Some("Chrome"))
    );
    assert_eq!(m["ip_masked"], "127.0.0.x");
    assert!(m["location"].is_null(), "sem GeoIP local não há cidade");
    let p = by_id(&phone.sid);
    assert_eq!(p["current"], false);
    assert_eq!(p["device_type"], "mobile");
    assert!(
        !list.to_string().contains("127.0.0.1"),
        "o IP inteiro nunca sai"
    );

    // O telemóvel tem o /rtc aberto e está numa sala.
    let ws_base = app.base.replacen("http://", "ws://", 1);
    let (mut rtc, _) =
        tokio_tungstenite::connect_async(format!("{ws_base}/rtc?token={}", phone.token))
            .await
            .expect("/rtc");
    let first = tokio::time::timeout(Duration::from_secs(5), rtc.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(first.to_text().unwrap().contains("presence"));
    let (st, room) = app
        .post(
            "/api/rooms",
            Some(&phone.token),
            json!({"name": "Sala", "topology": "mesh"}),
        )
        .await;
    assert_eq!(st, 200, "{room}");
    let (st, join) = app
        .post(
            &format!("/api/rooms/{}/join", room["code"].as_str().unwrap()),
            Some(&phone.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{join}");
    let room_token = join["room_token"].as_str().unwrap().to_string();
    assert_eq!(
        jwt_claims(&room_token)["sid"],
        phone.sid.as_str(),
        "o room token herda a sessão"
    );
    let (mut sala, _) =
        tokio_tungstenite::connect_async(format!("{ws_base}/ws?token={room_token}"))
            .await
            .expect("/ws");
    tokio::time::timeout(Duration::from_secs(5), sala.next())
        .await
        .expect("/ws responde")
        .unwrap()
        .unwrap();

    // Termina o telemóvel a partir do Mac.
    let (st, body) = app
        .delete(
            &format!("/api/users/me/sessions/{}", phone.sid),
            Some(&mac.token),
        )
        .await;
    assert_eq!(st, 204, "{body}");

    // 1. os WebSockets dele fecham
    assert_ws_closes(&mut rtc, "/rtc do telemóvel").await;
    assert_ws_closes(&mut sala, "/ws da sala do telemóvel").await;
    // 2. o access token dele deixa de abrir a API já
    let (st, body) = app.get("/api/users/me", Some(&phone.token)).await;
    assert_eq!(st, 401, "{body}");
    assert_eq!(body["code"], "auth.session_revoked");
    // 3. o refresh dele deixa de funcionar
    let (st, _) = refresh(&app, &phone.cookie).await;
    assert_eq!(st, 401, "refresh de uma sessão terminada");
    // 4. nem o room token ainda válido reabre o /ws, nem o access reabre o /rtc
    assert!(
        tokio_tungstenite::connect_async(format!("{ws_base}/ws?token={room_token}"))
            .await
            .is_err()
    );
    assert!(
        tokio_tungstenite::connect_async(format!("{ws_base}/rtc?token={}", phone.token))
            .await
            .is_err()
    );

    // O Mac continua: API, refresh (que roda dentro da MESMA sessão).
    let (st, _) = app.get("/api/users/me", Some(&mac.token)).await;
    assert_eq!(st, 200);
    let (st, renewed) = refresh(&app, &mac.cookie).await;
    assert_eq!(st, 200, "{renewed}");
    assert_eq!(
        jwt_claims(renewed["access_token"].as_str().unwrap())["sid"],
        mac.sid.as_str()
    );
    let (_, list) = app.get("/api/users/me/sessions", Some(&mac.token)).await;
    assert!(list["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["id"] != phone.sid.as_str()));

    // Terminar outra vez a mesma: 404, não «ok».
    let (st, body) = app
        .delete(
            &format!("/api/users/me/sessions/{}", phone.sid),
            Some(&mac.token),
        )
        .await;
    assert_eq!(
        (st, body["code"].as_str()),
        (404, Some("sessions.not_found"))
    );

    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_logs WHERE action = 'session.revoked' AND target = $1",
    )
    .bind(&phone.sid)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(n, 1, "auditado");
}

/// R201 — «terminar todas as outras» deixa só a actual.
#[sqlx::test(migrations = "./migrations")]
async fn revoke_others_keeps_only_the_current_one(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let admin = app.new_org("alfa.test").await;
    let a = login_as(&app, &admin.email, MAC_CHROME).await;
    let b = login_as(&app, &admin.email, IPHONE).await;
    let c = login_as(&app, &admin.email, "curl/8").await;

    let (st, body) = app
        .post(
            "/api/users/me/sessions/revoke-others",
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    // b, c, e as duas do `new_org` (registo e login)
    assert_eq!(body["revoked"], 4, "{body}");
    for d in [&b, &c] {
        let (st, _) = app.get("/api/users/me", Some(&d.token)).await;
        assert_eq!(st, 401);
        assert_eq!(refresh(&app, &d.cookie).await.0, 401);
    }
    let (st, list) = app.get("/api/users/me/sessions", Some(&a.token)).await;
    assert_eq!(st, 200);
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{list}");
    assert_eq!(items[0]["id"], a.sid.as_str());

    // Sair (logout) termina a sessão, não só o cookie.
    let r = app
        .raw(
            reqwest::Method::POST,
            "/api/auth/logout",
            &[("cookie", &a.cookie)],
            None,
        )
        .await;
    assert_eq!(r.status, 200);
    let (st, body) = app.get("/api/users/me", Some(&a.token)).await;
    assert_eq!(
        (st, body["code"].as_str()),
        (401, Some("auth.session_revoked"))
    );
}

/// R202 — ninguém lê nem termina sessões de outra pessoa: nem um colega, nem
/// o administrador da organização. A resposta é a de um id inexistente.
#[sqlx::test(migrations = "./migrations")]
async fn sessions_of_others_are_not_found(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let admin = app.new_org("alfa.test").await;
    let member = app.add_member(&admin, "bento", "member").await;
    let outsider = app.new_org("beta.test").await;
    let victim = login_as(&app, &member.email, IPHONE).await;

    for (who, token) in [
        ("admin da mesma org", admin.token.as_str()),
        ("outra org", outsider.token.as_str()),
    ] {
        let path = format!("/api/users/me/sessions/{}", victim.sid);
        let (st, body) = app.get(&path, Some(token)).await;
        assert_eq!(
            (st, body["code"].as_str()),
            (404, Some("sessions.not_found")),
            "{who}"
        );
        let (st, body) = app.delete(&path, Some(token)).await;
        assert_eq!(
            (st, body["code"].as_str()),
            (404, Some("sessions.not_found")),
            "{who}"
        );
        let (_, list) = app.get("/api/users/me/sessions", Some(token)).await;
        assert!(
            !list.to_string().contains(&victim.sid),
            "{who}: a lista é só da própria pessoa"
        );
    }
    let (st, _) = app.get("/api/users/me", Some(&victim.token)).await;
    assert_eq!(st, 200, "a vítima continua com a sessão");
    let (st, _) = app.get("/api/users/me/sessions", None).await;
    assert_eq!(st, 401);
}

/// R203 — reautenticação: a password certa abre a janela, a errada não, e o
/// travão conta as falhas.
#[sqlx::test(migrations = "./migrations")]
async fn reauthentication_needs_the_real_password(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let admin = app.new_org("alfa.test").await;
    let d = login_as(&app, &admin.email, MAC_CHROME).await;

    let (st, body) = app
        .post("/api/users/me/reauthentication", Some(&d.token), json!({}))
        .await;
    assert_eq!(
        (st, body["code"].as_str()),
        (400, Some("reauthentication.missing_proof"))
    );
    let (st, body) = app
        .post(
            "/api/users/me/reauthentication",
            Some(&d.token),
            json!({"password": "errada-errada"}),
        )
        .await;
    assert_eq!(
        (st, body["code"].as_str()),
        (401, Some("reauthentication.failed"))
    );
    let (st, body) = app
        .post(
            "/api/users/me/reauthentication",
            Some(&d.token),
            json!({"password": PASSWORD}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    assert!(body["valid_until"].is_string());
    for _ in 0..5 {
        app.post(
            "/api/users/me/reauthentication",
            Some(&d.token),
            json!({"password": "x-errada-x"}),
        )
        .await;
    }
    let (st, _) = app
        .post(
            "/api/users/me/reauthentication",
            Some(&d.token),
            json!({"password": PASSWORD}),
        )
        .await;
    assert_eq!(st, 429, "bloqueado também com a certa durante a janela");
}
