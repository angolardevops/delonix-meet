//! Chaves de acesso (WebAuthn) como segundo factor, contra Postgres e servidor
//! reais, com um autenticador em SOFTWARE que assina de verdade (R208).
//!
//! O que se prova: registar exige reautenticação recente; o login com password
//! passa a pedir o segundo factor e a chave abre a sessão; a cerimónia serve
//! uma vez; a chave de outra pessoa não se vê nem se remove; o último factor
//! não sai quando a organização exige 2FA; sem configuração a API diz
//! `not_configured` em vez de fingir.
mod common;

use common::{jwt_claims, TestApp, PASSWORD};
use serde_json::{json, Value};
use webauthn_authenticator_rs::{softpasskey::SoftPasskey, WebauthnAuthenticator};
use webauthn_rs::prelude::{CreationChallengeResponse, RequestChallengeResponse, Url};

const ORIGIN: &str = "http://localhost";
const CFG: &[(&str, &str)] = &[
    ("WEBAUTHN_RP_ID", "localhost"),
    ("WEBAUTHN_RP_ORIGIN", ORIGIN),
];

fn origin() -> Url {
    Url::parse(ORIGIN).unwrap()
}

async fn reauth(app: &TestApp, token: &str) {
    let (st, b) = app
        .post(
            "/api/users/me/reauthentication",
            Some(token),
            json!({"password": PASSWORD}),
        )
        .await;
    assert_eq!(st, 200, "reautenticação: {b}");
}

/// Regista uma chave com o autenticador `auth` e devolve o JSON criado.
async fn register(
    app: &TestApp,
    token: &str,
    auth: &mut WebauthnAuthenticator<SoftPasskey>,
    name: &str,
) -> Value {
    let (st, begin) = app
        .post(
            "/api/users/me/passkeys/begin-registration",
            Some(token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "begin: {begin}");
    let ccr: CreationChallengeResponse =
        serde_json::from_value(begin["options"].clone()).expect("opções de criação");
    let credential = auth
        .do_registration(origin(), ccr)
        .expect("o autenticador regista");
    let (st, created) = app
        .post(
            "/api/users/me/passkeys",
            Some(token),
            json!({"ceremony_id": begin["ceremony_id"], "name": name,
                   "credential": serde_json::to_value(&credential).unwrap()}),
        )
        .await;
    assert_eq!(st, 201, "finish: {created}");
    created
}

async fn password_login(app: &TestApp, email: &str) -> Value {
    let (st, b) = app
        .post(
            "/api/auth/login",
            None,
            json!({"email": email, "password": PASSWORD}),
        )
        .await;
    assert_eq!(st, 200, "{b}");
    b
}

/// R208 — registo, login com a chave, cerimónia de uso único, isolamento e
/// último factor.
#[sqlx::test(migrations = "./migrations")]
async fn passkey_registration_login_and_last_factor(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, CFG).await;
    let a = app.new_org("alfa.test").await;
    let other = app.new_org("beta.test").await;
    let mut key = WebauthnAuthenticator::new(SoftPasskey::new(true));

    // Sem sessão: 401. Sem reautenticação recente: 403 com código estável.
    // (a sessão do login tem reautenticação implícita; força-se a antiga)
    sqlx::query("UPDATE user_sessions SET reauthenticated_at = now() - interval '1 hour'")
        .execute(&app.db)
        .await
        .unwrap();
    let (st, _) = app
        .post("/api/users/me/passkeys/begin-registration", None, json!({}))
        .await;
    assert_eq!(st, 401);
    let (st, e) = app
        .post(
            "/api/users/me/passkeys/begin-registration",
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (403, Some("auth.reauthentication_required")),
        "{e}"
    );

    reauth(&app, &a.token).await;
    let created = register(&app, &a.token, &mut key, "Portátil").await;
    let pk_id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["name"], "Portátil");

    let (_, sec) = app.get("/api/users/me/security", Some(&a.token)).await;
    assert_eq!(sec["two_factor_enabled"], true, "{sec}");
    assert_eq!(sec["passkeys"]["count"], 1);
    assert_eq!(sec["passkeys"]["availability"], "available");
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_logs WHERE action = 'security.passkey_added'",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(n, 1, "auditado");

    // A cerimónia de registo não serve duas vezes.
    let (_, begin) = app
        .post(
            "/api/users/me/passkeys/begin-registration",
            Some(&a.token),
            json!({}),
        )
        .await;
    let mut other_key = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let cred = other_key
        .do_registration(
            origin(),
            serde_json::from_value(begin["options"].clone()).unwrap(),
        )
        .unwrap();
    let body = json!({"ceremony_id": begin["ceremony_id"], "name": "Segunda",
                      "credential": serde_json::to_value(&cred).unwrap()});
    let (st, _) = app
        .post("/api/users/me/passkeys", Some(&a.token), body.clone())
        .await;
    assert_eq!(st, 201);
    let (st, e) = app
        .post("/api/users/me/passkeys", Some(&a.token), body)
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (404, Some("passkeys.ceremony_not_found"))
    );

    // Outra pessoa: nem vê, nem remove, nem usa a cerimónia.
    reauth(&app, &other.token).await;
    let (st, e) = app
        .get(
            &format!("/api/users/me/passkeys/{pk_id}"),
            Some(&other.token),
        )
        .await;
    assert_eq!((st, e["code"].as_str()), (404, Some("passkeys.not_found")));
    let (st, _) = app
        .delete(
            &format!("/api/users/me/passkeys/{pk_id}"),
            Some(&other.token),
        )
        .await;
    assert_eq!(st, 404);
    let (_, list) = app.get("/api/users/me/passkeys", Some(&other.token)).await;
    assert_eq!(list["items"].as_array().unwrap().len(), 0);

    // Login: a password já não basta; o desafio anuncia a chave.
    let challenge = password_login(&app, &a.email).await;
    assert_eq!(challenge["mfa_required"], true, "{challenge}");
    assert_eq!(challenge["methods"], json!(["passkey"]));
    let mfa_token = challenge["mfa_token"].as_str().unwrap();
    let (st, opts) = app
        .post(
            "/api/auth/login/mfa/passkey-options",
            None,
            json!({"mfa_token": mfa_token}),
        )
        .await;
    assert_eq!(st, 200, "{opts}");
    let rcr: RequestChallengeResponse = serde_json::from_value(opts["options"].clone()).unwrap();
    let assertion = key
        .do_authentication(origin(), rcr)
        .expect("o autenticador assina");
    let login_body = json!({"mfa_token": mfa_token, "ceremony_id": opts["ceremony_id"],
                            "credential": serde_json::to_value(&assertion).unwrap()});
    let (st, session) = app
        .post("/api/auth/login/mfa/passkey", None, login_body.clone())
        .await;
    assert_eq!(st, 200, "{session}");
    let token = session["access_token"].as_str().unwrap().to_string();
    let sid = jwt_claims(&token)["sid"].as_str().unwrap().to_string();
    let method: String =
        sqlx::query_scalar("SELECT auth_method FROM user_sessions WHERE id = $1::uuid")
            .bind(&sid)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(method, "passkey");
    let (st, _) = app.get("/api/users/me", Some(&token)).await;
    assert_eq!(st, 200);
    // Replay da mesma asserção: a cerimónia já foi consumida.
    let (st, e) = app
        .post("/api/auth/login/mfa/passkey", None, login_body)
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (404, Some("passkeys.ceremony_not_found"))
    );
    // Uma asserção de outra cerimónia não entra noutra.
    let (_, opts2) = app
        .post(
            "/api/auth/login/mfa/passkey-options",
            None,
            json!({"mfa_token": mfa_token}),
        )
        .await;
    let (st, e) = app
        .post(
            "/api/auth/login/mfa/passkey",
            None,
            json!({"mfa_token": mfa_token, "ceremony_id": opts2["ceremony_id"],
                   "credential": serde_json::to_value(&assertion).unwrap()}),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (401, Some("passkeys.authentication_failed")),
        "{e}"
    );
    // O desafio não vale como access token.
    let (st, _) = app.get("/api/users/me", Some(mfa_token)).await;
    assert_eq!(st, 401);

    // Último factor: a org exige 2FA. Com duas chaves, remove-se uma; a
    // última não sai.
    sqlx::query("UPDATE organizations SET require_mfa = TRUE WHERE id = $1::uuid")
        .bind(a.org())
        .execute(&app.db)
        .await
        .unwrap();
    reauth(&app, &token).await;
    let (st, _) = app
        .delete(&format!("/api/users/me/passkeys/{pk_id}"), Some(&token))
        .await;
    assert_eq!(st, 204, "a segunda chave continua");
    let (_, list) = app.get("/api/users/me/passkeys", Some(&token)).await;
    let last = list["items"][0]["id"].as_str().unwrap().to_string();
    let (st, e) = app
        .delete(&format!("/api/users/me/passkeys/{last}"), Some(&token))
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (409, Some("security.last_factor_required")),
        "{e}"
    );
    let (_, sec) = app.get("/api/users/me/security", Some(&token)).await;
    assert_eq!(sec["required_by_organization"], true);
}

/// R208 — o TOTP também é «último factor», e regenerar códigos exige
/// reautenticação e devolve códigos que funcionam.
#[sqlx::test(migrations = "./migrations")]
async fn totp_last_factor_and_regenerated_codes(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let t = Some(a.token.as_str());

    let (st, e) = app
        .post("/api/users/me/mfa/backup-codes/regenerate", t, json!({}))
        .await;
    assert_eq!((st, e["code"].as_str()), (409, Some("mfa.not_enabled")));

    // Activa o TOTP com um código calculado a partir do segredo.
    let (_, enrol) = app.post("/api/users/me/mfa/enroll", t, json!({})).await;
    let secret = enrol["secret"].as_str().unwrap();
    let code = totp_now(secret, 0);
    let (st, act) = app
        .post("/api/users/me/mfa/activate", t, json!({"code": code}))
        .await;
    assert_eq!(st, 200, "{act}");

    sqlx::query("UPDATE user_sessions SET reauthenticated_at = now() - interval '1 hour'")
        .execute(&app.db)
        .await
        .unwrap();
    let (st, e) = app
        .post("/api/users/me/mfa/backup-codes/regenerate", t, json!({}))
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (403, Some("auth.reauthentication_required"))
    );
    reauth(&app, &a.token).await;
    let (st, codes) = app
        .post("/api/users/me/mfa/backup-codes/regenerate", t, json!({}))
        .await;
    assert_eq!(st, 200, "{codes}");
    let new_code = codes["backup_codes"][0].as_str().unwrap().to_string();
    let old_code = act["backup_codes"][0].as_str().unwrap();
    assert_ne!(new_code, old_code);

    // O login pede TOTP; um código ANTIGO já não serve, o novo sim.
    let challenge = password_login(&app, &a.email).await;
    assert_eq!(challenge["methods"], json!(["totp"]));
    let mfa = challenge["mfa_token"].as_str().unwrap();
    let (st, _) = app
        .post(
            "/api/auth/login/mfa",
            None,
            json!({"mfa_token": mfa, "code": old_code}),
        )
        .await;
    assert_eq!(st, 401, "os códigos antigos morreram");
    let (st, _) = app
        .post(
            "/api/auth/login/mfa",
            None,
            json!({"mfa_token": mfa, "code": new_code}),
        )
        .await;
    assert_eq!(st, 200);

    // Org exige 2FA e o TOTP é o único: não sai (e o código não se gasta).
    sqlx::query("UPDATE organizations SET require_mfa = TRUE WHERE id = $1::uuid")
        .bind(a.org())
        .execute(&app.db)
        .await
        .unwrap();
    let (st, e) = app
        .post(
            "/api/users/me/mfa/disable",
            t,
            json!({"code": codes["backup_codes"][1]}),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (409, Some("security.last_factor_required"))
    );
    let (_, sec) = app.get("/api/users/me/security", t).await;
    assert_eq!(sec["totp"]["enabled"], true);
    assert_eq!(
        sec["totp"]["backup_codes_left"], 9,
        "a recusa não gastou o código: {sec}"
    );
}

/// Sem `WEBAUTHN_RP_ID`/`_ORIGIN` a API diz `not_configured` (503), não finge.
#[sqlx::test(migrations = "./migrations")]
async fn passkeys_not_configured_is_honest(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let (st, e) = app
        .post(
            "/api/users/me/passkeys/begin-registration",
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (503, Some("passkeys.not_configured"))
    );
    let (_, sec) = app.get("/api/users/me/security", Some(&a.token)).await;
    assert_eq!(sec["passkeys"]["availability"], "not_configured");
}

/// TOTP (RFC 6238, SHA-1, 30 s, 6 dígitos) calculado no teste, sem o código
/// do servidor.
fn totp_now(secret_b32: &str, offset_steps: i64) -> String {
    use hmac::{Hmac, Mac};
    const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut bits = 0u32;
    let mut buf = 0u32;
    let mut key = Vec::new();
    for ch in secret_b32.bytes() {
        let v = ALPHA.iter().position(|a| *a == ch).unwrap() as u32;
        buf = (buf << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            key.push((buf >> bits) as u8);
        }
    }
    let step = (chrono::Utc::now().timestamp() / 30 + offset_steps) as u64;
    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(&key).unwrap();
    mac.update(&step.to_be_bytes());
    let h = mac.finalize().into_bytes();
    let o = (h[19] & 0x0f) as usize;
    let n = u32::from_be_bytes([h[o] & 0x7f, h[o + 1], h[o + 2], h[o + 3]]) % 1_000_000;
    format!("{n:06}")
}
