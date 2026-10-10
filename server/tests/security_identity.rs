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
    let app = // O IdP falso vive em 127.0.0.1: declarado como destino de saída, como
    // um IdP on-prem (guarda anti-SSRF, R180).
    TestApp::spawn_with(db, &[("OUTBOUND_ALLOW_HOSTS", "127.0.0.1")]).await;
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

/// C1 (revisão de segurança 2026-10-09) — um CONVIDADO EXTERNO de uma
/// organização não é "membro" dela para efeitos de SSO. `role_in_org` lê a
/// coluna legada `org_members.role`, que colapsa 'member' e
/// 'external_guest' no mesmo texto ('member') -- por isso o callback
/// achava a vítima "activa" na org do atacante e abria-lhe a conta REAL
/// (admin da sua própria org), sem nunca ter sido convidada para lá além
/// de uma reunião. É o mesmo ataque do teste anterior, mas pela porta dos
/// convites legítimos entre empresas, que esse teste não cobria.
#[sqlx::test(migrations = "./migrations")]
async fn sso_callback_refuses_external_guest_of_the_idp_org(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, &[("OUTBOUND_ALLOW_HOSTS", "127.0.0.1")]).await;
    let vitima = app.new_org("vitima2.test").await; // admin da SUA própria org
    let atacante = app.new_org("atacante2.test").await;
    let idp = FakeIdp::start().await;
    wire_sso(&app, &atacante, &idp).await;

    // A vítima é só CONVIDADA EXTERNA da org do atacante (parceria entre
    // empresas, reunião conjunta) -- nunca pediu para entrar lá como membro.
    sqlx::query("SELECT seed_system_roles($1::uuid)")
        .bind(atacante.org())
        .execute(&app.db)
        .await
        .unwrap();
    let guest_role_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM org_roles WHERE org_id = $1::uuid AND system_key = 'external_guest'",
    )
    .bind(atacante.org())
    .fetch_one(&app.db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO org_members (org_id, user_id, role, role_id, title)
         VALUES ($1::uuid, $2::uuid, 'member', $3, 'Convidado')",
    )
    .bind(atacante.org())
    .bind(&vitima.user_id)
    .bind(guest_role_id)
    .execute(&app.db)
    .await
    .unwrap();

    // Controlo positivo: o admin do atacante continua a entrar pelo SSO
    // da sua própria org -- a correcção não parte o caminho legítimo.
    let (st, sub, body) = sso_flow(&app, &idp, "atacante2.test", &atacante.email).await;
    assert_eq!(st, 302, "membro real da org ainda entra por SSO: {body}");
    assert_eq!(sub.as_deref(), Some(atacante.user_id.as_str()));

    // O ataque: o IdP do atacante (que ele controla) afirma o email da
    // vítima -- que "existe" em org_members, mas só como convidada.
    let (st, sub, body) = sso_flow(&app, &idp, "atacante2.test", &vitima.email).await;
    assert_eq!(
        (st, sub.as_deref()),
        (403, None),
        "convidado externo abriu a conta real da vítima: {body}"
    );
    assert_eq!(body["code"], "sso.account_not_in_org", "{body}");
}

/// A1 (revisão de segurança 2026-10-09) — depois do primeiro login SSO, a
/// conta fica presa ao (issuer, subject) que a autenticou. Antes desta
/// correcção, qualquer IdP que afirmasse o MESMO email reabria a conta --
/// mesmo um segundo IdP, de outra organização ou reconfigurado sem aviso.
#[sqlx::test(migrations = "./migrations")]
async fn sso_callback_refuses_identity_not_already_bound(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, &[("OUTBOUND_ALLOW_HOSTS", "127.0.0.1")]).await;
    let admin = app.new_org("zeta.test").await;
    let idp1 = FakeIdp::start().await;
    wire_sso(&app, &admin, &idp1).await;

    // Primeiro login: liga a conta a (issuer de idp1, sub).
    let (st, sub, body) = sso_flow(&app, &idp1, "zeta.test", &admin.email).await;
    assert_eq!(st, 302, "primeiro login SSO: {body}");
    assert_eq!(sub.as_deref(), Some(admin.user_id.as_str()));

    // Um SEGUNDO IdP -- a org trocou de fornecedor, ou alguém comprometeu a
    // configuração -- afirma o MESMO email.
    let idp2 = FakeIdp::start().await;
    wire_sso(&app, &admin, &idp2).await;
    let (st, sub, body) = sso_flow(&app, &idp2, "zeta.test", &admin.email).await;
    assert_eq!(
        (st, sub.as_deref()),
        (403, None),
        "segundo IdP reabriu a conta só pelo email: {body}"
    );
    assert_eq!(body["code"], "sso.identity_mismatch", "{body}");
}

/// R130 — O JIT criava conta para QUALQUER email, de qualquer domínio, e
/// juntava-a à org. Uma org só pode criar contas do seu próprio domínio.
#[sqlx::test(migrations = "./migrations")]
async fn sso_jit_only_creates_accounts_of_the_org_domain(db: sqlx::PgPool) {
    let app = // O IdP falso vive em 127.0.0.1: declarado como destino de saída, como
    // um IdP on-prem (guarda anti-SSRF, R180).
    TestApp::spawn_with(db, &[("OUTBOUND_ALLOW_HOSTS", "127.0.0.1")]).await;
    let admin = app.new_org("gama.test").await;
    let idp = FakeIdp::start().await;
    wire_sso(&app, &admin, &idp).await;

    // A3, raiz: domínio certo mas AINDA sem prova de posse por DNS TXT --
    // o JIT recusa-se, não cria a conta.
    let (st, sub, body) = sso_flow(&app, &idp, "gama.test", "nova@gama.test").await;
    assert_eq!(
        (st, sub.as_deref()),
        (403, None),
        "JIT antes de provar o domínio: {body}"
    );
    assert_eq!(body["code"], "sso.domain_not_verified", "{body}");
    assert_eq!(
        user_id_by_email(&app, "nova@gama.test").await,
        None,
        "conta criada sem o domínio provado"
    );

    app.verify_domain(admin.org()).await;

    // Controlo positivo: email NOVO do domínio da org, já provado → conta
    // criada, membro.
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
    let app = // O IdP falso vive em 127.0.0.1: declarado como destino de saída, como
    // um IdP on-prem (guarda anti-SSRF, R180).
    TestApp::spawn_with(db, &[("OUTBOUND_ALLOW_HOSTS", "127.0.0.1")]).await;
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

/// A2 (revisão de segurança, 2026-10-09) — o `state` anti-CSRF vivia na
/// memória de UM processo. A produção corre várias réplicas sem afinidade
/// de sessão: um `authorize` numa réplica e o `callback` a cair noutra
/// (o caso normal, não um ataque) respondia 401 sem motivo nenhum visível
/// para quem está a entrar. Duas instâncias de `TestApp` sobre a MESMA
/// base simulam exactamente isso.
#[sqlx::test(migrations = "./migrations")]
async fn sso_callback_works_across_replicas_without_sticky_sessions(db: sqlx::PgPool) {
    let app_a = TestApp::spawn_with(db.clone(), &[("OUTBOUND_ALLOW_HOSTS", "127.0.0.1")]).await;
    let admin = app_a.new_org("multireplica.test").await;
    let idp = FakeIdp::start().await;
    wire_sso(&app_a, &admin, &idp).await;

    // Uma SEGUNDA "réplica", mesma base de dados, porta diferente.
    let app_b = TestApp::spawn_with(db, &[("OUTBOUND_ALLOW_HOSTS", "127.0.0.1")]).await;

    // authorize na réplica A.
    let r = app_a
        .raw(
            reqwest::Method::GET,
            "/api/auth/sso/authorize?domain=multireplica.test",
            &[],
            None,
        )
        .await;
    assert_eq!(r.status, 302, "sso/authorize em A: {}", r.text);
    let location = url::Url::parse(&r.header("location").unwrap()).unwrap();
    let q = |k: &str| {
        location
            .query_pairs()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.to_string())
            .unwrap()
    };
    {
        let mut a = idp.answer.lock().unwrap();
        a.email = admin.email.clone();
        a.nonce = q("nonce");
    }

    // callback na réplica B -- é o que um `DashMap` por processo nunca
    // conseguia ver.
    let r = app_b
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
    assert_eq!(
        r.status, 302,
        "callback na réplica B não viu o state emitido pela réplica A: {}",
        r.text
    );
    let loc = r.header("location").unwrap();
    assert!(loc.contains("token="), "{loc}");

    // O state é de uso único: repetir o MESMO callback (em qualquer réplica)
    // já não encontra nada -- prova que ficou mesmo em Postgres, não
    // duplicado num DashMap por processo que o primeiro consumo não limpou.
    let r = app_a
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
    assert_eq!(
        r.status, 401,
        "state reutilizável entre réplicas: {}",
        r.text
    );
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
    assert_eq!(st, 204, "{body}");
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
    app.verify_domain(admin.org()).await;

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

/// A3 (revisão de segurança, 2026-10-09) — `enforce_sso` bloqueava o login
/// por password de QUALQUER conta cujo email terminasse no domínio
/// reivindicado, mesmo sem ela ter nada a ver com a organização que o
/// impôs. Isto transformava um domínio de email em dono de toda a gente
/// que o usasse: bastava uma org registar-se com um email desse domínio
/// (o registo não o verifica -- aberto conhecido) e ligar o enforce_sso
/// para trancar o login por password de uma conta de OUTRA organização,
/// sem relação nenhuma com a que o activou.
#[sqlx::test(migrations = "./migrations")]
async fn enforce_sso_only_blocks_members_of_the_enforcing_org(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;

    // A org "squat" reivindica o domínio (regista-se com um email dele) e
    // liga enforce_sso -- exactamente o que um atacante faria.
    let squat = app.new_org("squat.test").await;
    let (st, body) = app
        .put(
            &format!("/api/orgs/{}/sso", squat.org()),
            Some(&squat.token),
            json!({"issuer_url": "https://idp.squat.test", "client_id": "cid",
                   "client_secret": "segredo", "enforce_sso": true}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    // Este teste prova o gate de PERTENÇA (is_full_member), não o de posse
    // do domínio (esse tem teste próprio) -- domínio já provado, para os
    // dois não se confundirem.
    app.verify_domain(squat.org()).await;

    // A VÍTIMA é membro de uma organização COMPLETAMENTE DIFERENTE, sem
    // SSO nenhum. `POST /members` recusa um email fora do domínio da
    // própria org (prova em separado, noutro teste) -- por isso o email
    // "estranho" entra como entraria numa conta LEGADA ou convertida antes
    // da regra actual, directo na base: o que importa aqui é o estado da
    // conta no momento do login, não o caminho que a lá pôs.
    let outra = app.new_org("outra-empresa.test").await;
    let vitima = app.add_member(&outra, "funcionaria", "member").await;
    let vitima_email = "funcionaria@squat.test";
    sqlx::query("UPDATE users SET email = $1 WHERE id = $2::uuid")
        .bind(vitima_email)
        .bind(&vitima.user_id)
        .execute(&app.db)
        .await
        .unwrap();

    // Controlo positivo: o próprio squat continua travado (a correcção não
    // parte o caso legítimo -- é o mesmo teste que
    // sso_enforced_blocks_password_login em organization.rs).
    let (st, body) = app
        .post(
            "/api/auth/login",
            None,
            json!({"email": squat.email, "password": PASSWORD}),
        )
        .await;
    assert_eq!(
        st, 400,
        "membro real da org que impõe SSO devia ficar travado: {body}"
    );

    // O ataque: a vítima, que nunca teve nada a ver com "squat", continua a
    // conseguir entrar com a password da SUA organização.
    let (st, body) = app
        .post(
            "/api/auth/login",
            None,
            json!({"email": vitima_email, "password": PASSWORD}),
        )
        .await;
    assert_eq!(
        st, 200,
        "conta de outra organização ficou bloqueada por um domínio que não é dela: {body}"
    );
}

/// A4 (revisão de segurança, 2026-10-09) — o SSO emitia sessão directa mesmo
/// para uma conta com um segundo factor LOCAL activo. O IdP ser "o" factor de
/// quem administra a organização não é o mesmo que ser o segundo factor
/// DESTA conta: quem activou TOTP aqui quer os dois, e o SSO não lho podia
/// retirar em silêncio -- sobretudo depois de C1/A1 (identidade federada já
/// ter sido o caminho de uma tomada de conta nesta mesma revisão).
#[sqlx::test(migrations = "./migrations")]
async fn sso_callback_challenges_local_mfa_instead_of_issuing_a_session(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, &[("OUTBOUND_ALLOW_HOSTS", "127.0.0.1")]).await;
    let admin = app.new_org("mfa-sso.test").await;
    let (secret, _) = enable_mfa(&app, &admin.token).await;
    let idp = FakeIdp::start().await;
    wire_sso(&app, &admin, &idp).await;

    let r = app
        .raw(
            reqwest::Method::GET,
            "/api/auth/sso/authorize?domain=mfa-sso.test",
            &[],
            None,
        )
        .await;
    assert_eq!(r.status, 302, "{}", r.text);
    let location = url::Url::parse(&r.header("location").unwrap()).unwrap();
    let q = |k: &str| {
        location
            .query_pairs()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.to_string())
            .unwrap()
    };
    {
        let mut a = idp.answer.lock().unwrap();
        a.email = admin.email.clone();
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
    assert_eq!(r.status, 302, "{}", r.text);
    let loc = r.header("location").unwrap();
    // Nem sessão (sem cookie `dlx_refresh`) nem access token no fragmento --
    // só o desafio, pelo mesmo caminho do login por password.
    assert!(
        loc.contains("/#/sso-mfa?mfa_token="),
        "SSO com MFA local devia desafiar, não abrir sessão: {loc}"
    );
    assert!(
        !r.headers.contains_key(reqwest::header::SET_COOKIE),
        "cookie de sessão definido antes do segundo factor: {:?}",
        r.headers
    );
    let mfa_token = loc.split("mfa_token=").nth(1).unwrap().to_string();

    // O desafio é REAL: completa-se pelo mesmo endpoint do login por
    // password, com o código TOTP desta conta. Passo SEGUINTE ao do
    // `enable_mfa`: a ACTIVAÇÃO já consome o passo actual (`last_step`,
    // anti-replay), e usar o mesmo aqui falhava por repetição, não pela
    // correcção em teste.
    let (st, body) = app
        .post(
            "/api/auth/login/mfa",
            None,
            json!({"mfa_token": mfa_token, "code": totp_at_step(&secret, current_step() + 1)}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["user"]["id"], admin.user_id);
}

// ---------------------------------------------------------------------------
//  Achados do laboratório da Frente C (Odoo 16 + Meet reais, 2026-10-10):
//  o endereço público, o fornecedor em baixo, a recusa que volta ao login, e
//  a entrada que fica na trilha.
// ---------------------------------------------------------------------------

const PUBLICO: &str = "https://meet.publico.test";

/// authorize → IdP → callback, devolvendo o estado e o `Location` do callback.
/// Ao contrário do `sso_flow`, não interpreta: o que se mede aqui é o destino.
async fn ida_e_volta(app: &TestApp, idp: &FakeIdp, domain: &str, email: &str) -> (u16, String) {
    let r = app
        .raw(
            reqwest::Method::GET,
            &format!("/api/auth/sso/authorize?domain={domain}"),
            &[],
            None,
        )
        .await;
    assert_eq!(r.status, 302, "authorize: {}", r.text);
    let location = url::Url::parse(&r.header("location").unwrap()).unwrap();
    let q = |k: &str| {
        location
            .query_pairs()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.to_string())
            .unwrap()
    };
    // O redirect_uri registado no IdP sai do PUBLIC_URL — não do CORS nem de
    // localhost.
    assert_eq!(
        q("redirect_uri"),
        format!("{PUBLICO}/api/auth/sso/callback")
    );
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
    (r.status, r.header("location").unwrap_or_default())
}

async fn na_trilha(app: &TestApp, org: &str, action: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE org_id = $1::uuid AND action = $2")
        .bind(org)
        .bind(action)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

/// A recusa volta ao LOGIN com o código (era JSON cru no ecrã), o sucesso volta
/// ao endereço público, e a entrada e a conta criada ficam na trilha (só a
/// recusa ficava).
#[sqlx::test(migrations = "./migrations")]
async fn sso_volta_ao_login_com_o_codigo_e_a_entrada_fica_na_trilha(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(
        db,
        &[
            ("OUTBOUND_ALLOW_HOSTS", "127.0.0.1"),
            ("PUBLIC_URL", PUBLICO),
        ],
    )
    .await;
    let admin = app.new_org("eta.test").await;
    let idp = FakeIdp::start().await;
    wire_sso(&app, &admin, &idp).await;

    let (st, loc) = ida_e_volta(&app, &idp, "eta.test", "nova@eta.test").await;
    assert_eq!(
        (st, loc.as_str()),
        (
            302,
            format!("{PUBLICO}/#/login?sso_error=sso.domain_not_verified").as_str()
        ),
        "a recusa tem de voltar ao login com o código"
    );
    assert_eq!(na_trilha(&app, admin.org(), "auth.sso_refused").await, 1);

    app.verify_domain(admin.org()).await;
    let (st, loc) = ida_e_volta(&app, &idp, "eta.test", "nova@eta.test").await;
    assert_eq!(st, 302);
    assert!(
        loc.starts_with(&format!("{PUBLICO}/#/sso-complete?token=")),
        "o sucesso volta ao endereço público: {loc}"
    );
    assert_eq!(
        na_trilha(&app, admin.org(), "auth.sso_provisioned").await,
        1
    );
    assert_eq!(na_trilha(&app, admin.org(), "auth.sso_login").await, 1);

    // Uma segunda entrada da MESMA conta: entra outra vez, não cria outra.
    let (st, _) = ida_e_volta(&app, &idp, "eta.test", "nova@eta.test").await;
    assert_eq!(st, 302);
    assert_eq!(
        na_trilha(&app, admin.org(), "auth.sso_provisioned").await,
        1
    );
    assert_eq!(na_trilha(&app, admin.org(), "auth.sso_login").await, 2);
}

/// O IdP em baixo é do LADO DELE: 503 com código próprio (era 500 «internal
/// error»), e com o endereço público configurado volta ao login com esse código.
#[sqlx::test(migrations = "./migrations")]
async fn sso_com_o_fornecedor_em_baixo_diz_que_e_dele(db: sqlx::PgPool) {
    let sem_url = TestApp::spawn_with(db.clone(), &[("OUTBOUND_ALLOW_HOSTS", "127.0.0.1")]).await;
    let admin = sem_url.new_org("teta.test").await;
    let idp = FakeIdp::start().await;
    wire_sso(&sem_url, &admin, &idp).await;
    // Um destino permitido onde nada responde.
    sqlx::query(
        "UPDATE org_sso_configs SET issuer_url = 'http://127.0.0.1:1' WHERE org_id = $1::uuid",
    )
    .bind(admin.org())
    .execute(&sem_url.db)
    .await
    .unwrap();

    let (st, body) = sem_url
        .get("/api/auth/sso/authorize?domain=teta.test", None)
        .await;
    assert_eq!(st, 503, "{body}");
    assert_eq!(body["code"], "sso.provider_unavailable", "{body}");

    let com_url = TestApp::spawn_with(
        db,
        &[
            ("OUTBOUND_ALLOW_HOSTS", "127.0.0.1"),
            ("PUBLIC_URL", PUBLICO),
        ],
    )
    .await;
    let r = com_url
        .raw(
            reqwest::Method::GET,
            "/api/auth/sso/authorize?domain=teta.test",
            &[],
            None,
        )
        .await;
    assert_eq!(r.status, 302, "{}", r.text);
    assert_eq!(
        r.header("location").unwrap(),
        format!("{PUBLICO}/#/login?sso_error=sso.provider_unavailable")
    );
}

/// Em produção, sem PUBLIC_URL nem CORS_ORIGINS, o SSO RECUSA — até aqui
/// mandava o browser para `localhost` em silêncio. E recusa antes de ir ao IdP.
#[sqlx::test(migrations = "./migrations")]
async fn em_producao_sem_endereco_publico_o_sso_recusa(db: sqlx::PgPool) {
    let mut config = common::test_config(&[("OUTBOUND_ALLOW_HOSTS", "127.0.0.1")]);
    config.allow_insecure = false;
    let app = TestApp::spawn_with_config(db, config).await;
    let admin = app.new_org("iota.test").await;
    let idp = FakeIdp::start().await;
    wire_sso(&app, &admin, &idp).await;

    let (st, body) = app
        .get("/api/auth/sso/authorize?domain=iota.test", None)
        .await;
    assert_eq!(st, 422, "{body}");
    assert_eq!(body["code"], "sso.public_url_missing", "{body}");
}
