//! Ataques à IDENTIDADE (SSO, MFA, login), com controlo positivo (R51/R94):
//! cada recusa vem acompanhada da prova de que o caminho legítimo continua a
//! funcionar — senão o teste mediria uma avaria, não a regra.
//!
//! O SSO corre de ponta a ponta contra um IdP OIDC FALSO servido neste
//! processo (discovery, JWKS e token endpoint, id_token RS256 assinado com uma
//! chave gerada em memória). É o mesmo caminho de código que um IdP real
//! percorre: discovery, troca do código, verificação da assinatura, do nonce,
//! do issuer e da audiência. O que NÃO se prova aqui é a interoperabilidade
//! com um IdP real (Google, Entra, Okta).
mod common;

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{extract::State, routing::get, routing::post, Json, Router};
use common::{jwt_claims, TestApp, PASSWORD};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
//  IdP OIDC falso
// ---------------------------------------------------------------------------

const CLIENT_ID: &str = "delonix-meet-teste";

#[derive(Default)]
struct IdpAnswer {
    /// Email que o IdP vai afirmar no próximo id_token.
    email: String,
    /// Nonce do pedido de autorização em curso (lido do `Location`).
    nonce: String,
}

struct FakeIdp {
    issuer: String,
    answer: Arc<Mutex<IdpAnswer>>,
}

#[derive(Clone)]
struct IdpState {
    issuer: String,
    jwk: Value,
    signing_der: Arc<Vec<u8>>,
    answer: Arc<Mutex<IdpAnswer>>,
}

async fn idp_discovery(State(s): State<IdpState>) -> Json<Value> {
    Json(json!({
        "issuer": s.issuer,
        "authorization_endpoint": format!("{}/authorize", s.issuer),
        "token_endpoint": format!("{}/token", s.issuer),
        "jwks_uri": format!("{}/jwks", s.issuer),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],
    }))
}

async fn idp_jwks(State(s): State<IdpState>) -> Json<Value> {
    Json(json!({ "keys": [s.jwk] }))
}

async fn idp_token(State(s): State<IdpState>) -> Json<Value> {
    let (email, nonce) = {
        let a = s.answer.lock().unwrap();
        (a.email.clone(), a.nonce.clone())
    };
    let now = chrono::Utc::now().timestamp();
    let claims = json!({
        "iss": s.issuer,
        "sub": format!("sub-{email}"),
        "aud": CLIENT_ID,
        "iat": now,
        "exp": now + 300,
        "nonce": nonce,
        "email": email,
        "email_verified": true,
        "name": format!("Pessoa {email}"),
    });
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some("k1".into());
    let key = jsonwebtoken::EncodingKey::from_rsa_der(&s.signing_der);
    let id_token = jsonwebtoken::encode(&header, &claims, &key).unwrap();
    Json(json!({
        "access_token": "at-falso",
        "token_type": "Bearer",
        "expires_in": 300,
        "id_token": id_token,
    }))
}

impl FakeIdp {
    async fn start() -> Self {
        use base64::Engine;
        use rsa::{pkcs1::EncodeRsaPrivateKey, traits::PublicKeyParts};

        let key = rsa::RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap();
        let b64 = |b: Vec<u8>| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b);
        let jwk = json!({
            "kty": "RSA", "use": "sig", "alg": "RS256", "kid": "k1",
            "n": b64(key.n().to_bytes_be()),
            "e": b64(key.e().to_bytes_be()),
        });
        let signing_der = Arc::new(key.to_pkcs1_der().unwrap().as_bytes().to_vec());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        let issuer = format!("http://{addr}");
        let answer = Arc::new(Mutex::new(IdpAnswer::default()));
        let state = IdpState {
            issuer: issuer.clone(),
            jwk,
            signing_der,
            answer: answer.clone(),
        };
        let app = Router::new()
            .route("/.well-known/openid-configuration", get(idp_discovery))
            .route("/jwks", get(idp_jwks))
            .route("/token", post(idp_token))
            .with_state(state);
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { issuer, answer }
    }
}

/// Um administrador liga o IdP à SUA organização pelo caminho normal
/// (`PUT /api/orgs/{id}/sso`) e depois o issuer é apontado para o IdP falso.
///
/// O `PUT` exige `https://`; o IdP falso é `http://`. Trocar o issuer
/// directamente na base NÃO dá ao atacante nada que ele não tenha: quem
/// administra a org já escolhe o issuer que quiser, e servir um IdP em https é
/// trivial. A exigência de https não é fronteira de segurança.
async fn wire_sso(app: &TestApp, admin: &common::Account, idp: &FakeIdp) {
    let (st, body) = app
        .put(
            &format!("/api/orgs/{}/sso", admin.org()),
            Some(&admin.token),
            json!({"issuer_url": "https://idp.exemplo.test", "client_id": CLIENT_ID,
                   "client_secret": "segredo", "enforce_sso": false}),
        )
        .await;
    assert_eq!(st, 200, "o admin configura o SSO da sua org: {body}");
    sqlx::query("UPDATE org_sso_configs SET issuer_url = $1 WHERE org_id = $2::uuid")
        .bind(&idp.issuer)
        .bind(admin.org())
        .execute(&app.db)
        .await
        .unwrap();
}

/// Corre o fluxo OIDC completo (login → IdP → callback) com o IdP a afirmar
/// `email`. Devolve o estado do callback, o `sub` da sessão aberta (se houve)
/// e o corpo (se não houve).
async fn sso_flow(
    app: &TestApp,
    idp: &FakeIdp,
    domain: &str,
    email: &str,
) -> (u16, Option<String>, Value) {
    let r = app
        .raw(
            reqwest::Method::GET,
            &format!("/api/auth/sso/authorize?domain={domain}"),
            &[],
            None,
        )
        .await;
    assert_eq!(r.status, 302, "sso/login: {}", r.text);
    let location = url::Url::parse(&r.header("location").unwrap()).unwrap();
    assert!(location.as_str().starts_with(&idp.issuer), "{location}");
    let q = |k: &str| {
        location
            .query_pairs()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.to_string())
            .unwrap()
    };
    {
        let mut a = idp.answer.lock().unwrap();
        a.email = email.to_string();
        a.nonce = q("nonce");
    }
    let r = app
        .raw(
            reqwest::Method::GET,
            &format!(
                "/api/auth/sso/callback?code=codigo-falso&state={}",
                q("state")
            ),
            &[],
            None,
        )
        .await;
    let sub = if r.status == 302 {
        let loc = r.header("location").unwrap();
        let token = loc.split("token=").nth(1).expect("token no fragmento");
        Some(jwt_claims(token)["sub"].as_str().unwrap().to_string())
    } else {
        None
    };
    (r.status, sub, r.json())
}

async fn user_id_by_email(app: &TestApp, email: &str) -> Option<String> {
    sqlx::query_scalar::<_, uuid::Uuid>("SELECT id FROM users WHERE email = $1")
        .bind(email)
        .fetch_optional(&app.db)
        .await
        .unwrap()
        .map(|u| u.to_string())
}

/// R130 — O callback OIDC abria sessão em QUALQUER conta cujo email o IdP da
/// organização afirmasse. O administrador de uma org controla o seu IdP
/// (`PUT /api/orgs/{id}/sso`), por isso bastava afirmar `admin@vitima` para
/// entrar como o administrador de OUTRA organização.
#[sqlx::test(migrations = "./migrations")]
async fn sso_callback_refuses_account_of_another_org(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let vitima = app.new_org("vitima.test").await;
    let atacante = app.new_org("atacante.test").await;
    let idp = FakeIdp::start().await;
    wire_sso(&app, &atacante, &idp).await;

    // Controlo positivo: um membro activo da org entra pelo SSO dela.
    let (st, sub, body) = sso_flow(&app, &idp, "atacante.test", &atacante.email).await;
    assert_eq!(st, 302, "membro activo entra por SSO: {body}");
    assert_eq!(sub.as_deref(), Some(atacante.user_id.as_str()));

    // O ataque: o IdP do atacante afirma o email do admin da vítima.
    let (st, sub, body) = sso_flow(&app, &idp, "atacante.test", &vitima.email).await;
    assert_eq!(
        (st, sub.as_deref()),
        (403, None),
        "tomada de conta: sessão aberta como {sub:?} (vítima = {}): {body}",
        vitima.user_id
    );
    assert_eq!(body["code"], "sso.account_not_in_org", "{body}");

    // E não foi juntado à org do atacante pelo caminho.
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM org_members WHERE org_id = $1::uuid AND user_id = $2::uuid",
    )
    .bind(atacante.org())
    .bind(&vitima.user_id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(n, 0, "a vítima foi juntada à org do atacante");
}

/// R130 — O JIT criava conta para QUALQUER email, de qualquer domínio, e
/// juntava-a à org. Uma org só pode criar contas do seu próprio domínio.
#[sqlx::test(migrations = "./migrations")]
async fn sso_jit_only_creates_accounts_of_the_org_domain(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let admin = app.new_org("gama.test").await;
    let idp = FakeIdp::start().await;
    wire_sso(&app, &admin, &idp).await;

    // Controlo positivo: email NOVO do domínio da org → conta criada, membro.
    let (st, sub, body) = sso_flow(&app, &idp, "gama.test", "nova@gama.test").await;
    assert_eq!(st, 302, "JIT do próprio domínio: {body}");
    let criada = user_id_by_email(&app, "nova@gama.test").await;
    assert_eq!(sub, criada);
    let role: Option<String> = sqlx::query_scalar(
        "SELECT role FROM org_members WHERE org_id = $1::uuid AND user_id = $2::uuid
         AND archived_at IS NULL",
    )
    .bind(admin.org())
    .bind(criada.as_deref().unwrap())
    .fetch_optional(&app.db)
    .await
    .unwrap();
    assert_eq!(role.as_deref(), Some("member"));

    // O ataque: email novo de OUTRO domínio.
    let (st, sub, body) = sso_flow(&app, &idp, "gama.test", "alguem@delta.test").await;
    assert_eq!(
        (st, sub.as_deref()),
        (403, None),
        "JIT fora do domínio: {body}"
    );
    assert_eq!(body["code"], "sso.email_domain_mismatch", "{body}");
    assert_eq!(
        user_id_by_email(&app, "alguem@delta.test").await,
        None,
        "a conta de outro domínio foi criada"
    );
}

/// R130 — Quem saiu da org (membro arquivado) não volta a entrar pelo SSO
/// dela, mesmo que o IdP ainda o tenha.
#[sqlx::test(migrations = "./migrations")]
async fn sso_refuses_archived_member(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let admin = app.new_org("epsilon.test").await;
    let saiu = app.add_member(&admin, "saiu", "member").await;
    let fica = app.add_member(&admin, "fica", "member").await;
    let idp = FakeIdp::start().await;
    wire_sso(&app, &admin, &idp).await;
    app.archive_member(admin.org(), &saiu.user_id).await;

    // Controlo positivo: o colega que ficou entra.
    let (st, sub, body) = sso_flow(&app, &idp, "epsilon.test", &fica.email).await;
    assert_eq!(st, 302, "{body}");
    assert_eq!(sub.as_deref(), Some(fica.user_id.as_str()));

    let (st, sub, body) = sso_flow(&app, &idp, "epsilon.test", &saiu.email).await;
    assert_eq!(
        (st, sub.as_deref()),
        (403, None),
        "arquivado entrou: {body}"
    );
    assert_eq!(body["code"], "sso.account_not_in_org", "{body}");
}

// ---------------------------------------------------------------------------
//  MFA — força bruta do código
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

/// Gerador independente (RFC 6238, SHA-1, 6 dígitos) — não usa o do servidor,
/// para o teste não herdar um defeito dele.
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
    let seed = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
    assert_eq!(totp_at_step(seed, 59 / 30), "287082");
    assert_eq!(totp_at_step(seed, 1_234_567_890 / 30), "005924");
}

/// Um código de 6 dígitos que NÃO é válido em nenhum passo aceite (±1).
fn wrong_code(secret: &str) -> String {
    let s = current_step();
    let validos: Vec<String> = (s - 2..=s + 2).map(|p| totp_at_step(secret, p)).collect();
    (0..1_000_000u32)
        .map(|n| format!("{n:06}"))
        .find(|c| !validos.contains(c))
        .unwrap()
}

async fn enrol(app: &TestApp, token: &str) -> String {
    let (st, body) = app
        .post("/api/users/me/mfa/enroll", Some(token), json!({}))
        .await;
    assert_eq!(st, 200, "{body}");
    body["secret"].as_str().unwrap().to_string()
}

/// Activa o MFA e devolve (segredo, códigos de recuperação).
async fn enable_mfa(app: &TestApp, token: &str) -> (String, Vec<String>) {
    let secret = enrol(app, token).await;
    let (st, act) = app
        .post(
            "/api/users/me/mfa/activate",
            Some(token),
            json!({"code": totp_at_step(&secret, current_step())}),
        )
        .await;
    assert_eq!(st, 200, "{act}");
    let backup = act["backup_codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_string())
        .collect();
    (secret, backup)
}

/// R131 — `POST /api/users/me/mfa/activate` aceitava tentativas ilimitadas.
/// O risco aqui é menor do que no `disable` (quem tem a sessão pode reinscrever
/// e receber um segredo seu), mas um verificador de códigos sem travão é um
/// oráculo, e o limite é o mesmo nos dois. Tem de travar também o código CERTO
/// enquanto dura — senão é decoração.
#[sqlx::test(migrations = "./migrations")]
async fn mfa_activate_locks_after_five_failures(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let alvo = app.new_org("mfa-activar.test").await;
    let secret = enrol(&app, &alvo.token).await;
    let errado = wrong_code(&secret);

    for i in 1..=5 {
        let (st, body) = app
            .post(
                "/api/users/me/mfa/activate",
                Some(&alvo.token),
                json!({"code": errado}),
            )
            .await;
        assert_eq!(st, 401, "falha {i}: {body}");
    }
    let (st, body) = app
        .post(
            "/api/users/me/mfa/activate",
            Some(&alvo.token),
            json!({"code": errado}),
        )
        .await;
    assert_eq!(
        st, 429,
        "a sexta tentativa errada devia ser travada: {body}"
    );

    let (st, body) = app
        .post(
            "/api/users/me/mfa/activate",
            Some(&alvo.token),
            json!({"code": totp_at_step(&secret, current_step())}),
        )
        .await;
    assert_eq!(st, 429, "o código CERTO passou durante o bloqueio: {body}");

    // Controlo positivo: noutra conta, 4 falhas não bloqueiam e o código certo
    // activa. Só as falhas contam, e só acima do limite.
    let ok = app.new_org("mfa-activar-ok.test").await;
    let secret = enrol(&app, &ok.token).await;
    let errado = wrong_code(&secret);
    for _ in 0..4 {
        let (st, _) = app
            .post(
                "/api/users/me/mfa/activate",
                Some(&ok.token),
                json!({"code": errado}),
            )
            .await;
        assert_eq!(st, 401);
    }
    let (st, body) = app
        .post(
            "/api/users/me/mfa/activate",
            Some(&ok.token),
            json!({"code": totp_at_step(&secret, current_step())}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
}

/// R131 — `POST /api/users/me/mfa/disable` aceitava tentativas ilimitadas:
/// com uma sessão roubada, adivinhar o código desligava o segundo factor.
#[sqlx::test(migrations = "./migrations")]
async fn mfa_disable_locks_after_five_failures(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let alvo = app.new_org("mfa-desligar.test").await;
    let (secret, backup) = enable_mfa(&app, &alvo.token).await;
    let errado = wrong_code(&secret);

    for i in 1..=5 {
        let (st, body) = app
            .post(
                "/api/users/me/mfa/disable",
                Some(&alvo.token),
                json!({"code": errado}),
            )
            .await;
        assert_eq!(st, 401, "falha {i}: {body}");
    }
    let (st, body) = app
        .post(
            "/api/users/me/mfa/disable",
            Some(&alvo.token),
            json!({"code": errado}),
        )
        .await;
    assert_eq!(
        st, 429,
        "a sexta tentativa errada devia ser travada: {body}"
    );
    // O código de recuperação é válido — e mesmo assim não passa no bloqueio.
    let (st, body) = app
        .post(
            "/api/users/me/mfa/disable",
            Some(&alvo.token),
            json!({"code": backup[0]}),
        )
        .await;
    assert_eq!(
        st, 429,
        "um código VÁLIDO passou durante o bloqueio: {body}"
    );
    let (_, e) = app.get("/api/users/me/mfa", Some(&alvo.token)).await;
    assert_eq!(e["enabled"], true, "o MFA foi desligado: {e}");

    // Controlo positivo: noutra conta, o código de recuperação desliga.
    let ok = app.new_org("mfa-desligar-ok.test").await;
    let (_, backup) = enable_mfa(&app, &ok.token).await;
    let (st, body) = app
        .post(
            "/api/users/me/mfa/disable",
            Some(&ok.token),
            json!({"code": backup[0]}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
}

/// O passo MFA do login (`/api/auth/login/mfa`) JÁ tinha travão por conta (8 em
/// 5 min, `login_limiter`). Este teste não corrige nada: guarda que o travão
/// continua lá e que trava também o código válido.
#[sqlx::test(migrations = "./migrations")]
async fn mfa_login_step_is_limited_per_account(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let acc = app.new_org("mfa-login.test").await;
    let (secret, backup) = enable_mfa(&app, &acc.token).await;
    let errado = wrong_code(&secret);

    let challenge = || async {
        let (st, body) = app
            .post(
                "/api/auth/login",
                None,
                json!({"email": acc.email, "password": PASSWORD}),
            )
            .await;
        assert_eq!(st, 200, "{body}");
        body["mfa_token"].as_str().unwrap().to_string()
    };

    // Controlo positivo: o código de recuperação abre sessão.
    let ch = challenge().await;
    let (st, body) = app
        .post(
            "/api/auth/login/mfa",
            None,
            json!({"mfa_token": ch, "code": backup[0]}),
        )
        .await;
    assert_eq!(st, 200, "{body}");

    let ch = challenge().await;
    let mut ultimo = 0;
    for _ in 0..10 {
        let (st, _) = app
            .post(
                "/api/auth/login/mfa",
                None,
                json!({"mfa_token": ch, "code": errado}),
            )
            .await;
        ultimo = st;
        if st == 429 {
            break;
        }
        assert_eq!(st, 401);
    }
    assert_eq!(ultimo, 429, "o passo MFA do login não trava");
    let (st, body) = app
        .post(
            "/api/auth/login/mfa",
            None,
            json!({"mfa_token": ch, "code": backup[1]}),
        )
        .await;
    assert_eq!(
        st, 429,
        "um código VÁLIDO passou durante o bloqueio: {body}"
    );
}

// ---------------------------------------------------------------------------
//  Login — SSO exclusivo antes do travão por conta
// ---------------------------------------------------------------------------

/// R132 — Num domínio com SSO exclusivo, o login por password respondia 400
/// ANTES do travão por conta: essas contas não tinham limite nenhum, e a
/// resposta não passava pelo mesmo caminho das outras.
#[sqlx::test(migrations = "./migrations")]
async fn login_rate_limit_applies_to_sso_enforced_accounts(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let admin = app.new_org("zeta.test").await; // gasta 1 login da conta
    let (st, body) = app
        .put(
            &format!("/api/orgs/{}/sso", admin.org()),
            Some(&admin.token),
            json!({"issuer_url": "https://idp.zeta.test", "client_id": "cid",
                   "client_secret": "s", "enforce_sso": true}),
        )
        .await;
    assert_eq!(st, 200, "{body}");

    // Controlo positivo: a recusa do SSO exclusivo continua a ser dita.
    let (st, body) = app
        .post(
            "/api/auth/login",
            None,
            json!({"email": admin.email, "password": PASSWORD}),
        )
        .await;
    assert_eq!(st, 400, "{body}");

    let mut ultimo = 0;
    for _ in 0..10 {
        let (st, _) = app
            .post(
                "/api/auth/login",
                None,
                json!({"email": admin.email, "password": "errada-123456"}),
            )
            .await;
        ultimo = st;
        if st == 429 {
            break;
        }
    }
    assert_eq!(
        ultimo, 429,
        "conta de domínio com SSO exclusivo sem travão por conta"
    );
}
