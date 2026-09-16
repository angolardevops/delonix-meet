//! Ataques à IDENTIDADE, com controlo positivo (R51/R94):
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
use common::{jwt_claims, TestApp};
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
            &format!("/api/auth/sso/login?domain={domain}"),
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
