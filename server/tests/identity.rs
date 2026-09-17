//! Caracterização do contexto de IDENTIDADE: registo, login, refresh/logout
//! por cookie, perfil, pesquisa de utilizadores e MFA (TOTP).
//!
//! Fixa o comportamento OBSERVÁVEL de hoje (códigos de estado e campos-chave)
//! para que um refactor que o mude falhe aqui. Onde o comportamento actual é
//! um defeito conhecido, o teste chama-se `..._current_behavior_...` e leva um
//! comentário `// DÍVIDA:`.
mod common;

use common::{jwt_claims, TestApp, PASSWORD};
use hmac::{Hmac, Mac};
use serde_json::json;

// ---------------------------------------------------------------------------
//  Registo
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn register_creates_org_admin_and_returns_access_token(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let res = app
        .raw(
            reqwest::Method::POST,
            "/api/auth/register",
            &[],
            Some(json!({"org_name": "Org Alfa", "email": "Ana@Alfa.test",
                        "username": "ana", "password": PASSWORD})),
        )
        .await;
    assert_eq!(res.status, 200, "{}", res.text);
    let body = res.json();
    assert!(body["access_token"].as_str().is_some());
    // O refresh token NUNCA vai no corpo — só no cookie HttpOnly.
    assert!(body.get("refresh_token").is_none(), "{body}");
    // Email normalizado (trim + minúsculas).
    assert_eq!(body["user"]["email"], "ana@alfa.test");
    assert_eq!(body["user"]["username"], "ana");
    assert_eq!(body["user"]["locale"], "pt");
    let cookies = res.set_cookies();
    assert!(
        cookies.iter().any(|c| c.starts_with("dlx_refresh=")),
        "{cookies:?}"
    );

    // O autor é admin da org nova, cujo domínio é o do email.
    let token = body["access_token"].as_str().unwrap();
    let (st, orgs) = app.get("/api/orgs", Some(token)).await;
    assert_eq!(st, 200);
    assert_eq!(orgs.as_array().unwrap().len(), 1);
    assert_eq!(orgs[0]["role"], "admin");
    assert_eq!(orgs[0]["name"], "Org Alfa");
    assert_eq!(orgs[0]["slug"], "org-alfa-alfa-test");
    assert_eq!(orgs[0]["member_count"], 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn register_without_username_derives_it_from_email(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let (st, body) = app
        .post(
            "/api/auth/register",
            None,
            json!({"org_name": "Org Beta", "email": "joana@beta.test", "password": PASSWORD}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["user"]["username"], "joana");
}

#[sqlx::test(migrations = "./migrations")]
async fn register_duplicate_email_is_409_by_domain_rule(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    app.new_org("alfa.test").await;
    // A regra «uma org por domínio» corre ANTES da unicidade do email: um
    // email repetido recebe a mensagem do domínio, não a de «email em uso».
    let (st, body) = app
        .post(
            "/api/auth/register",
            None,
            json!({"org_name": "Outra", "email": "admin@alfa.test",
                   "username": "outro", "password": PASSWORD}),
        )
        .await;
    assert_eq!(st, 409, "{body}");
    assert!(
        body["error"].as_str().unwrap().contains("alfa.test"),
        "{body}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn register_second_user_same_domain_is_refused(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    app.new_org("alfa.test").await;
    let (st, body) = app
        .post(
            "/api/auth/register",
            None,
            json!({"org_name": "Segunda", "email": "bruno@alfa.test",
                   "username": "bruno", "password": PASSWORD}),
        )
        .await;
    assert_eq!(st, 409, "{body}");
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("já tem uma organização"));
    // E não se criou conta nenhuma.
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE email = 'bruno@alfa.test'")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn register_validation_errors_are_400(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let cases = [
        // password < 8
        json!({"org_name": "Org", "email": "a@gama.test", "password": "curta12"}),
        // password > 128
        json!({"org_name": "Org", "email": "a@gama.test", "password": "a".repeat(129)}),
        // email sem @
        json!({"org_name": "Org", "email": "sem-arroba.test", "password": PASSWORD}),
        // domínio sem ponto (não corporativo)
        json!({"org_name": "Org", "email": "a@localhost", "password": PASSWORD}),
        // nome da org < 2
        json!({"org_name": "O", "email": "a@gama.test", "password": PASSWORD}),
        // nome da org > 80
        json!({"org_name": "O".repeat(81), "email": "a@gama.test", "password": PASSWORD}),
    ];
    for c in cases {
        let (st, body) = app.post("/api/auth/register", None, c.clone()).await;
        assert_eq!(st, 400, "caso {c}: {body}");
        assert!(body["error"].is_string());
    }
    // Fronteiras aceites: 8 e 128 caracteres.
    let (st, body) = app
        .post(
            "/api/auth/register",
            None,
            json!({"org_name": "Org Oito", "email": "oito@oito.test", "password": "12345678"}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    let (st, body) = app
        .post(
            "/api/auth/register",
            None,
            json!({"org_name": "Org Cento", "email": "cento@cento.test", "password": "b".repeat(128)}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
}

#[sqlx::test(migrations = "./migrations")]
async fn register_username_collision_across_domains_is_409(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    // O username deriva da parte local do email e é ÚNICO em toda a
    // plataforma: `ana@alfa.test` e `ana@beta.test` colidem, apesar de serem
    // organizações diferentes.
    let (st, _) = app
        .post(
            "/api/auth/register",
            None,
            json!({"org_name": "Org Alfa", "email": "ana@alfa.test", "password": PASSWORD}),
        )
        .await;
    assert_eq!(st, 200);
    let (st, body) = app
        .post(
            "/api/auth/register",
            None,
            json!({"org_name": "Org Beta", "email": "ana@beta.test", "password": PASSWORD}),
        )
        .await;
    assert_eq!(st, 409, "{body}");
    assert_eq!(body["error"], "email ou nome de utilizador já em uso");
    // A transação desfez-se: o domínio beta.test continua livre.
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM organizations WHERE email_domain = 'beta.test'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(n, 0);
}

// ---------------------------------------------------------------------------
//  Login
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn login_ok_sets_http_only_refresh_cookie(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let admin = app.new_org("alfa.test").await;
    let res = app
        .raw(
            reqwest::Method::POST,
            "/api/auth/login",
            &[],
            Some(json!({"email": "  ADMIN@alfa.TEST ", "password": PASSWORD})),
        )
        .await;
    assert_eq!(res.status, 200, "{}", res.text);
    let body = res.json();
    assert_eq!(body["user"]["id"], admin.user_id.as_str());
    assert_eq!(body["user"]["email"], "admin@alfa.test");
    assert!(body.get("refresh_token").is_none());
    let claims = jwt_claims(body["access_token"].as_str().unwrap());
    assert_eq!(claims["typ"], "access");
    assert_eq!(claims["sub"], admin.user_id.as_str());

    let cookie = res
        .set_cookies()
        .into_iter()
        .find(|c| c.starts_with("dlx_refresh="))
        .expect("cookie de refresh");
    assert!(cookie.contains("HttpOnly"), "{cookie}");
    assert!(cookie.contains("SameSite=Strict"), "{cookie}");
    assert!(cookie.contains("Path=/api/auth"), "{cookie}");
    // COOKIE_INSECURE=1 no harness: sem `Secure`.
    assert!(!cookie.contains("Secure"), "{cookie}");
}

#[sqlx::test(migrations = "./migrations")]
async fn login_wrong_password_and_unknown_email_are_401(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    app.new_org("alfa.test").await;
    let (st, body) = app
        .post(
            "/api/auth/login",
            None,
            json!({"email": "admin@alfa.test", "password": "errada-123456"}),
        )
        .await;
    assert_eq!(st, 401);
    assert_eq!(body["error"], "unauthorized");
    let (st, body) = app
        .post(
            "/api/auth/login",
            None,
            json!({"email": "ninguem@alfa.test", "password": PASSWORD}),
        )
        .await;
    assert_eq!(st, 401);
    assert_eq!(body["error"], "unauthorized");
}

#[sqlx::test(migrations = "./migrations")]
async fn login_is_rate_limited_per_account_after_eight_attempts(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    app.new_org("alfa.test").await; // new_org já gasta 1 login
    let mut last = 0;
    for _ in 0..8 {
        let (st, _) = app
            .post(
                "/api/auth/login",
                None,
                json!({"email": "admin@alfa.test", "password": "errada-123456"}),
            )
            .await;
        last = st;
        if st == 429 {
            break;
        }
    }
    // 8 tentativas por conta em 5 min: a nona (contando a do new_org) é 429,
    // mesmo com a password CERTA.
    assert_eq!(last, 429);
    let (st, _) = app
        .post(
            "/api/auth/login",
            None,
            json!({"email": "admin@alfa.test", "password": PASSWORD}),
        )
        .await;
    assert_eq!(st, 429);
}

#[sqlx::test(migrations = "./migrations")]
async fn tampered_empty_and_garbage_tokens_are_401(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let admin = app.new_org("alfa.test").await;
    let mut parts = admin.token.split('.');
    let (h, p) = (parts.next().unwrap(), parts.next().unwrap());
    let forged = format!("{h}.{p}.assinaturaFalsa");
    for t in [forged.as_str(), "", "nao-e-um-jwt"] {
        let (st, _) = app.get("/api/users/me", Some(t)).await;
        assert_eq!(st, 401, "token {t:?}");
    }
    let (st, _) = app.get("/api/users/me", None).await;
    assert_eq!(st, 401);
}

// ---------------------------------------------------------------------------
//  Refresh / logout (cookie)
// ---------------------------------------------------------------------------

fn refresh_cookie_pair(cookies: &[String]) -> String {
    cookies
        .iter()
        .find(|c| c.starts_with("dlx_refresh="))
        .expect("cookie dlx_refresh")
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

#[sqlx::test(migrations = "./migrations")]
async fn refresh_rotates_the_cookie_and_revokes_the_old_one(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    app.new_org("alfa.test").await;
    let login = app
        .raw(
            reqwest::Method::POST,
            "/api/auth/login",
            &[],
            Some(json!({"email": "admin@alfa.test", "password": PASSWORD})),
        )
        .await;
    let c1 = refresh_cookie_pair(&login.set_cookies());

    // Sem cookie: 401.
    let r = app
        .raw(reqwest::Method::POST, "/api/auth/refresh", &[], None)
        .await;
    assert_eq!(r.status, 401);
    // Um token de ACESSO no Authorization não serve de refresh.
    let tok = login.json()["access_token"].as_str().unwrap().to_string();
    let r = app
        .raw(
            reqwest::Method::POST,
            "/api/auth/refresh",
            &[("Authorization", &format!("Bearer {tok}"))],
            None,
        )
        .await;
    assert_eq!(r.status, 401);

    let r = app
        .raw(
            reqwest::Method::POST,
            "/api/auth/refresh",
            &[("Cookie", &c1)],
            None,
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.text);
    assert!(r.json()["access_token"].as_str().is_some());
    assert_eq!(r.json()["user"]["email"], "admin@alfa.test");
    let c2 = refresh_cookie_pair(&r.set_cookies());
    assert_ne!(c1, c2, "o refresh roda o token");

    // O antigo foi revogado na rotação.
    let r = app
        .raw(
            reqwest::Method::POST,
            "/api/auth/refresh",
            &[("Cookie", &c1)],
            None,
        )
        .await;
    assert_eq!(r.status, 401);
    // O novo serve.
    let r = app
        .raw(
            reqwest::Method::POST,
            "/api/auth/refresh",
            &[("Cookie", &c2)],
            None,
        )
        .await;
    assert_eq!(r.status, 200);
}

#[sqlx::test(migrations = "./migrations")]
async fn logout_clears_cookie_and_revokes_refresh(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    app.new_org("alfa.test").await;
    let login = app
        .raw(
            reqwest::Method::POST,
            "/api/auth/login",
            &[],
            Some(json!({"email": "admin@alfa.test", "password": PASSWORD})),
        )
        .await;
    let c = refresh_cookie_pair(&login.set_cookies());
    let r = app
        .raw(
            reqwest::Method::POST,
            "/api/auth/logout",
            &[("Cookie", &c)],
            None,
        )
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(r.json()["ok"], true);
    let cleared = r
        .set_cookies()
        .into_iter()
        .find(|x| x.starts_with("dlx_refresh="))
        .expect("cookie de limpeza");
    assert!(cleared.starts_with("dlx_refresh=;"), "{cleared}");
    assert!(cleared.contains("Max-Age=0"), "{cleared}");

    let r = app
        .raw(
            reqwest::Method::POST,
            "/api/auth/refresh",
            &[("Cookie", &c)],
            None,
        )
        .await;
    assert_eq!(r.status, 401);

    // Logout sem cookie também responde 200 (idempotente).
    let r = app
        .raw(reqwest::Method::POST, "/api/auth/logout", &[], None)
        .await;
    assert_eq!(r.status, 200);
}

// ---------------------------------------------------------------------------
//  /api/users/me
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn users_me_get_and_patch(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let admin = app.new_org("alfa.test").await;
    let (st, me) = app.get("/api/users/me", Some(&admin.token)).await;
    assert_eq!(st, 200);
    assert_eq!(me["id"], admin.user_id.as_str());
    assert_eq!(me["username"], "admin-alfa.test");
    assert_eq!(me["locale"], "pt");
    assert!(me.get("password_hash").is_none());

    let (st, me) = app
        .patch(
            "/api/users/me",
            Some(&admin.token),
            json!({"username": "  Ana Nova  ", "locale": "en"}),
        )
        .await;
    assert_eq!(st, 200, "{me}");
    assert_eq!(me["username"], "Ana Nova");
    assert_eq!(me["locale"], "en");

    // Locale desconhecido é IGNORADO em silêncio (200, fica o anterior).
    let (st, me) = app
        .patch("/api/users/me", Some(&admin.token), json!({"locale": "xx"}))
        .await;
    assert_eq!(st, 200);
    assert_eq!(me["locale"], "en");

    // Validações.
    for bad in [
        json!({"username": "   "}),
        json!({"username": "u".repeat(41)}),
        json!({"password": "curta"}),
        json!({"password": "p".repeat(129)}),
    ] {
        let (st, body) = app
            .patch("/api/users/me", Some(&admin.token), bad.clone())
            .await;
        assert_eq!(st, 400, "{bad}: {body}");
    }

    // Mudar a password: a nova passa a valer, a antiga deixa de valer.
    let (st, _) = app
        .patch(
            "/api/users/me",
            Some(&admin.token),
            json!({"password": "OutraPasswordForte9"}),
        )
        .await;
    assert_eq!(st, 200);
    let (st, _) = app
        .post(
            "/api/auth/login",
            None,
            json!({"email": admin.email, "password": PASSWORD}),
        )
        .await;
    assert_eq!(st, 401);
    let (st, _) = app
        .post(
            "/api/auth/login",
            None,
            json!({"email": admin.email, "password": "OutraPasswordForte9"}),
        )
        .await;
    assert_eq!(st, 200);

    // Sem sessão.
    let (st, _) = app
        .patch("/api/users/me", None, json!({"username": "x"}))
        .await;
    assert_eq!(st, 401);
}

// ---------------------------------------------------------------------------
//  /api/users
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn users_search_is_scoped_to_shared_orgs(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let carla = app.add_member(&a, "carla", "member").await;

    // Termo curto: lista vazia, não erro.
    let (st, body) = app.get("/api/users?q=a", Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(body, json!([]));

    // Colega da mesma org: encontrado (por email ou username).
    let (st, body) = app.get("/api/users?q=carla", Some(&a.token)).await;
    assert_eq!(st, 200, "{body}");
    let ids: Vec<&str> = body
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec![carla.user_id.as_str()]);
    assert_eq!(body[0]["locale"], "pt");

    // O próprio nunca aparece.
    let (_, body) = app.get("/api/users?q=admin", Some(&a.token)).await;
    assert!(!body.to_string().contains(&a.user_id), "{body}");

    // Utilizadores de OUTRA org: nunca aparecem.
    let (st, body) = app.get("/api/users?q=beta", Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(body, json!([]), "vazou o directório da org B");
    let (_, body) = app.get("/api/users?q=admin", Some(&a.token)).await;
    assert!(!body.to_string().contains(&b.user_id), "{body}");

    // Sem sessão: 401. Sem `q`: 400 do extractor de query.
    let (st, _) = app.get("/api/users?q=carla", None).await;
    assert_eq!(st, 401);
    let (st, _) = app.get("/api/users", Some(&a.token)).await;
    assert_eq!(st, 400);
}

// ---------------------------------------------------------------------------
//  MFA (TOTP, RFC 6238)
// ---------------------------------------------------------------------------

const B32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn base32_decode(s: &str) -> Vec<u8> {
    let (mut buf, mut bits, mut out) = (0u32, 0u32, Vec::new());
    for c in s.chars().filter(|c| *c != '=') {
        let v = B32
            .iter()
            .position(|&a| a == c.to_ascii_uppercase() as u8)
            .expect("base32") as u32;
        buf = (buf << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    out
}

/// Gerador independente (RFC 6238, SHA-1, 6 dígitos) para o passo `step`.
fn totp_at_step(secret_b32: &str, step: u64) -> String {
    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(&base32_decode(secret_b32)).unwrap();
    mac.update(&step.to_be_bytes());
    let tag = mac.finalize().into_bytes();
    let off = (tag[19] & 0x0f) as usize;
    let bin = ((tag[off] as u32 & 0x7f) << 24)
        | ((tag[off + 1] as u32) << 16)
        | ((tag[off + 2] as u32) << 8)
        | tag[off + 3] as u32;
    format!("{:06}", bin % 1_000_000)
}

fn current_step() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        / 30
}

#[test]
fn test_totp_generator_matches_rfc6238_vectors() {
    // "12345678901234567890" em base32.
    let seed = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
    assert_eq!(totp_at_step(seed, 59 / 30), "287082");
    assert_eq!(totp_at_step(seed, 1_111_111_109 / 30), "081804");
    assert_eq!(totp_at_step(seed, 1_234_567_890 / 30), "005924");
}

#[sqlx::test(migrations = "./migrations")]
async fn mfa_enrol_activate_login_backup_and_disable(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let acc = app.new_org("mfa.test").await;
    let tok = acc.token.clone();

    let (st, e) = app.get("/api/users/me/mfa", Some(&tok)).await;
    assert_eq!(st, 200);
    assert_eq!(
        e,
        json!({"enabled": false, "pending": false, "backup_codes_left": 0})
    );

    // Activar sem inscrição: 400.
    let (st, _) = app
        .post(
            "/api/users/me/mfa/activate",
            Some(&tok),
            json!({"code": "000000"}),
        )
        .await;
    assert_eq!(st, 400);

    let (st, insc) = app
        .post("/api/users/me/mfa/enroll", Some(&tok), json!({}))
        .await;
    assert_eq!(st, 200, "{insc}");
    let secret = insc["secret"].as_str().unwrap().to_string();
    assert_eq!(secret.len(), 32, "160 bits em base32");
    let uri = insc["otpauth_uri"].as_str().unwrap();
    assert!(uri.starts_with("otpauth://totp/Delonix%20Meet:admin%40mfa.test?"));
    assert!(uri.contains(&format!("secret={secret}")));

    let (_, e) = app.get("/api/users/me/mfa", Some(&tok)).await;
    assert_eq!(e["pending"], true);
    assert_eq!(e["enabled"], false);

    // Pendente não tranca a conta.
    let (st, l) = app
        .post(
            "/api/auth/login",
            None,
            json!({"email": acc.email, "password": PASSWORD}),
        )
        .await;
    assert_eq!(st, 200);
    assert!(l["access_token"].is_string());

    // Código errado não activa.
    let (st, _) = app
        .post(
            "/api/users/me/mfa/activate",
            Some(&tok),
            json!({"code": "000000"}),
        )
        .await;
    assert_eq!(st, 401);

    let step = current_step();
    let (st, act) = app
        .post(
            "/api/users/me/mfa/activate",
            Some(&tok),
            json!({"code": totp_at_step(&secret, step)}),
        )
        .await;
    assert_eq!(st, 200, "{act}");
    let backup: Vec<String> = act["backup_codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_string())
        .collect();
    assert_eq!(backup.len(), 10);
    assert!(backup.iter().all(|c| c.len() == 11 && &c[5..6] == "-"));

    let (_, e) = app.get("/api/users/me/mfa", Some(&tok)).await;
    assert_eq!(
        e,
        json!({"enabled": true, "pending": false, "backup_codes_left": 10})
    );

    // Reinscrever com MFA activo: 400.
    let (st, _) = app
        .post("/api/users/me/mfa/enroll", Some(&tok), json!({}))
        .await;
    assert_eq!(st, 400);

    // Login: a password sozinha devolve DESAFIO, não sessão.
    let challenge = |app: &TestApp| {
        let email = acc.email.clone();
        let app_base = app.base.clone();
        let http = app.http.clone();
        async move {
            let r = http
                .post(format!("{app_base}/api/auth/login"))
                .json(&json!({"email": email, "password": PASSWORD}))
                .send()
                .await
                .unwrap();
            assert_eq!(r.status(), 200);
            let v: serde_json::Value = r.json().await.unwrap();
            assert_eq!(v["mfa_required"], true, "{v}");
            assert!(v.get("access_token").is_none(), "{v}");
            v["mfa_token"].as_str().unwrap().to_string()
        }
    };
    let ch1 = challenge(&app).await;
    assert_eq!(jwt_claims(&ch1)["typ"], "mfa");

    // O desafio não abre a API.
    let (st, _) = app.get("/api/users/me", Some(&ch1)).await;
    assert_eq!(st, 401);
    // Um access token não serve de desafio.
    let (st, _) = app
        .post(
            "/api/auth/login/mfa",
            None,
            json!({"mfa_token": tok, "code": totp_at_step(&secret, step + 1)}),
        )
        .await;
    assert_eq!(st, 401);
    // Código errado.
    let (st, _) = app
        .post(
            "/api/auth/login/mfa",
            None,
            json!({"mfa_token": ch1, "code": "000000"}),
        )
        .await;
    assert_eq!(st, 401);
    // O código usado para ACTIVAR não serve para entrar (anti-replay, R117).
    let (st, _) = app
        .post(
            "/api/auth/login/mfa",
            None,
            json!({"mfa_token": ch1, "code": totp_at_step(&secret, step)}),
        )
        .await;
    assert_eq!(st, 401);

    // O código do passo SEGUINTE (dentro do skew de ±1) entra — sem esperar.
    let next = totp_at_step(&secret, step + 1);
    let r = app
        .raw(
            reqwest::Method::POST,
            "/api/auth/login/mfa",
            &[],
            Some(json!({"mfa_token": ch1, "code": next})),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.text);
    assert!(r.json()["access_token"].is_string());
    assert!(r
        .set_cookies()
        .iter()
        .any(|c| c.starts_with("dlx_refresh=")));

    // O MESMO código não serve segunda vez.
    let ch2 = challenge(&app).await;
    let (st, _) = app
        .post(
            "/api/auth/login/mfa",
            None,
            json!({"mfa_token": ch2, "code": next}),
        )
        .await;
    assert_eq!(st, 401);

    // Código de recuperação entra uma vez.
    let (st, rec) = app
        .post(
            "/api/auth/login/mfa",
            None,
            json!({"mfa_token": ch2, "code": backup[0].to_lowercase()}),
        )
        .await;
    assert_eq!(
        st, 200,
        "o código de recuperação é normalizado para maiúsculas"
    );
    let tok2 = rec["access_token"].as_str().unwrap().to_string();
    let ch3 = challenge(&app).await;
    let (st, _) = app
        .post(
            "/api/auth/login/mfa",
            None,
            json!({"mfa_token": ch3, "code": backup[0]}),
        )
        .await;
    assert_eq!(st, 401);
    let (_, e) = app.get("/api/users/me/mfa", Some(&tok2)).await;
    assert_eq!(e["backup_codes_left"], 9);

    // Desactivar exige código válido.
    let (st, _) = app
        .post(
            "/api/users/me/mfa/disable",
            Some(&tok2),
            json!({"code": "000000"}),
        )
        .await;
    assert_eq!(st, 401);
    let (st, d) = app
        .post(
            "/api/users/me/mfa/disable",
            Some(&tok2),
            json!({"code": backup[1]}),
        )
        .await;
    assert_eq!(st, 200);
    assert_eq!(d, json!({"ok": true}));
    let (_, e) = app.get("/api/users/me/mfa", Some(&tok2)).await;
    assert_eq!(
        e,
        json!({"enabled": false, "pending": false, "backup_codes_left": 0})
    );

    // Depois de desactivar, a password volta a bastar.
    let (st, l) = app
        .post(
            "/api/auth/login",
            None,
            json!({"email": acc.email, "password": PASSWORD}),
        )
        .await;
    assert_eq!(st, 200);
    assert!(l["access_token"].is_string());
}

#[sqlx::test(migrations = "./migrations")]
async fn mfa_endpoints_require_session(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let (st, _) = app.get("/api/users/me/mfa", None).await;
    assert_eq!(st, 401);
    let (st, _) = app.post("/api/users/me/mfa/enroll", None, json!({})).await;
    assert_eq!(st, 401);
    let (st, _) = app
        .post(
            "/api/auth/login/mfa",
            None,
            json!({"mfa_token": "lixo", "code": "123456"}),
        )
        .await;
    assert_eq!(st, 401);
}
