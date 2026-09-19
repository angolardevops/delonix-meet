use axum::{
    extract::{ConnectInfo, FromRequestParts, State},
    http::{header, request::Parts, HeaderMap},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use uuid::Uuid;

use crate::{error::ApiError, AppState};

// ---------- JWT ----------

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub typ: String, // "access" | "room"
    pub exp: i64,
    pub iat: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub topo: Option<String>,
    /// Room token: este utilizador é o dono da sala (anfitrião).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub owner: bool,
    /// Room token: a sala tem sala de espera ativa.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub wait: bool,
    /// Room token: este utilizador pode admitir convidados (anfitrião ou
    /// co-anfitrião de admissões persistido em `room_admitters`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub adm: bool,
    /// Room token: indica que este participante é um bot headless.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_bot: bool,
    /// Room token: como a pessoa chegou (`sso`|`password`|`guest`|`pstn`|`bot`),
    /// decidido no servidor ao emitir o token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// Room token: cargo (`org_members.title`) na organização do dono da sala.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Room token: sem entrada directa (nem dono, nem convite, nem
    /// co-anfitrião) — espera SEMPRE, com ou sem sala de espera ligada.
    /// Ausente num token antigo: vale o `wait`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lobby: Option<bool>,
    /// Room token: a sala de espera configurada na sala (BD).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wr: Option<bool>,
}

pub fn sign_jwt(secret: &str, claims: &Claims) -> Result<String, ApiError> {
    encode(
        &Header::default(),
        claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(ApiError::internal)
}

pub fn verify_jwt(secret: &str, token: &str, expected_typ: &str) -> Result<Claims, ApiError> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| ApiError::Unauthorized)?;
    if data.claims.typ != expected_typ {
        return Err(ApiError::Unauthorized);
    }
    Ok(data.claims)
}

pub fn access_token(state: &AppState, user_id: Uuid) -> Result<String, ApiError> {
    let now = Utc::now().timestamp();
    sign_jwt(
        &state.config.jwt_secret,
        &Claims {
            sub: user_id,
            typ: "access".into(),
            iat: now,
            exp: now + state.config.access_ttl_secs,
            room: None,
            name: None,
            topo: None,
            owner: false,
            wait: false,
            adm: false,
            is_bot: false,
            origin: None,
            title: None,
            lobby: None,
            wr: None,
        },
    )
}

// ---------- Passwords ----------

pub fn hash_password(password: &str) -> Result<String, ApiError> {
    delonix_meet_core::crypto::hash_password(password).map_err(ApiError::internal)
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    delonix_meet_core::crypto::verify_password(password, hash)
}

// ---------- Refresh tokens ----------

pub fn new_refresh_token() -> (String, String) {
    let token = delonix_meet_core::crypto::random_hex(32);
    (token.clone(), hash_refresh_token(&token))
}

pub fn hash_refresh_token(token: &str) -> String {
    delonix_meet_core::crypto::sha256_hex(token)
}

// ---------- Extractor ----------

/// O token de `Authorization: Bearer <token>`, se o cabeçalho existir e tiver
/// esse esquema. Os extractores chamam isto em vez de lerem o cabeçalho à mão
/// (ADR-0004 §5, regra 2).
pub(crate) fn bearer_token(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
}

/// Authenticated user, extracted from `Authorization: Bearer <access token>`.
pub struct AuthUser {
    pub user_id: Uuid,
}

impl FromRequestParts<Arc<AppState>> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer_token(&parts.headers).ok_or(ApiError::Unauthorized)?;
        let claims = verify_jwt(&state.config.jwt_secret, token, "access")?;
        Ok(AuthUser {
            user_id: claims.sub,
        })
    }
}

// ---------- Handlers ----------

#[derive(Deserialize, utoipa::ToSchema)]
pub struct RegisterReq {
    /// Nome da organização. Obrigatório em tenancy `multi` (o registo cria
    /// uma organização); opcional na edição pessoal e ao juntar-se à org
    /// única da instalação.
    #[serde(default)]
    pub org_name: String,
    pub email: String,
    #[serde(default)]
    pub username: String,
    pub password: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct LoginReq {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub user: crate::users::UserPublic,
}

/// Resposta de auth ao cliente: o access token vai no corpo (usado no header
/// Authorization); o refresh token NUNCA vai no corpo — vai num cookie
/// HttpOnly (inacessível a JS, imune a roubo por XSS).
#[derive(Serialize, utoipa::ToSchema)]
pub struct AuthOk {
    pub access_token: String,
    pub user: crate::users::UserPublic,
}

/// Desafio do segundo factor: a password foi aceite mas a conta tem MFA
/// activo, por isso ainda não há sessão. O `mfa_token` troca-se em
/// `/api/auth/login/mfa`.
#[derive(Serialize, utoipa::ToSchema)]
pub struct MfaChallenge {
    /// Sempre `true`.
    pub mfa_required: bool,
    /// JWT `typ: "mfa"`, válido 5 minutos; não abre mais nenhum endpoint.
    pub mfa_token: String,
}

/// Resposta do login: sessão aberta OU desafio de MFA (sem discriminador — o
/// cliente distingue pela presença de `mfa_required`).
#[derive(Serialize, utoipa::ToSchema)]
#[serde(untagged)]
pub enum LoginResponse {
    Session(AuthOk),
    MfaRequired(MfaChallenge),
}

const REFRESH_COOKIE: &str = "dlx_refresh";

fn refresh_cookie(token: &str, secure: bool, max_age: i64) -> String {
    let sec = if secure { "; Secure" } else { "" };
    // Path restrito a /api/auth: o cookie só é enviado para refresh/logout.
    format!("{REFRESH_COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/api/auth; Max-Age={max_age}{sec}")
}

/// Constrói a resposta de auth: define o cookie de refresh + devolve o access.
fn auth_ok(state: &AppState, pair: TokenPair) -> Response {
    let cookie = refresh_cookie(
        &pair.refresh_token,
        state.config.cookie_secure,
        state.config.refresh_ttl_secs,
    );
    (
        [(header::SET_COOKIE, cookie)],
        // `untagged`: serializa exactamente como o `AuthOk` sozinho.
        Json(LoginResponse::Session(AuthOk {
            access_token: pair.access_token,
            user: pair.user,
        })),
    )
        .into_response()
}

/// Lê o refresh token do cookie HttpOnly (`dlx_refresh`).
fn read_refresh_cookie(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|p| {
        p.trim()
            .strip_prefix(&format!("{REFRESH_COOKIE}="))
            .filter(|v| !v.is_empty())
            .map(String::from)
    })
}

/// Identidade de uma sessão de conta, vista em "A minha conta" ➜ sessões
/// activas. O refresh token roda a cada renovação (linha nova, `token_hash`
/// diferente), mas `id`/`started_at` nascem uma vez no login e viajam para
/// cada linha seguinte — é o que deixa a sessão "o portátil de casa"
/// reconhecível ao longo do tempo, em vez de desaparecer a cada refresh.
struct SessionMeta {
    id: Uuid,
    started_at: DateTime<Utc>,
    user_agent: Option<String>,
    ip: Option<String>,
}

impl SessionMeta {
    /// Sessão nova (login/registo/SSO): id e início gerados agora.
    fn fresh(headers: &HeaderMap, ip: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            started_at: Utc::now(),
            user_agent: request_user_agent(headers),
            ip: Some(ip),
        }
    }

    /// Continuação de uma sessão existente (refresh): mantém id/início,
    /// actualiza o agente/IP para reflectir o pedido actual.
    fn continued(id: Uuid, started_at: DateTime<Utc>, headers: &HeaderMap, ip: String) -> Self {
        Self {
            id,
            started_at,
            user_agent: request_user_agent(headers),
            ip: Some(ip),
        }
    }
}

fn request_user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.chars().take(300).collect())
}

async fn issue_tokens(
    state: &AppState,
    user: crate::users::UserPublic,
    session: SessionMeta,
) -> Result<TokenPair, ApiError> {
    let access = access_token(state, user.id)?;
    let (refresh, refresh_hash) = new_refresh_token();
    let expires = Utc::now() + chrono::Duration::seconds(state.config.refresh_ttl_secs);
    sqlx::query(
        "INSERT INTO refresh_tokens
            (token_hash, user_id, expires_at, session_id, session_started_at, user_agent, ip_address)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(&refresh_hash)
    .bind(user.id)
    .bind(expires)
    .bind(session.id)
    .bind(session.started_at)
    .bind(&session.user_agent)
    .bind(&session.ip)
    .execute(&state.db)
    .await?;
    Ok(TokenPair {
        access_token: access,
        refresh_token: refresh,
        user,
    })
}

/// Documentação OpenAPI das rotas deste módulo (`openapi.rs` junta-as).
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        register,
        login,
        mfa_login,
        refresh,
        logout,
        sso_check,
        sso_login,
        sso_callback
    ),
    components(schemas(
        RegisterReq,
        LoginReq,
        MfaReq,
        AuthOk,
        MfaChallenge,
        LoginResponse,
        SsoCheck
    ))
)]
pub struct ApiDoc;

/// Emite uma sessão NOVA (tokens + cookie de refresh) para `user`, com a
/// MESMA resposta que `register`/`login` devolvem. Usado por quem acaba de
/// aceitar um convite de organização (`org::accept_invite`): a conta acabada
/// de nascer entra logada, sem ter de fazer login a seguir.
pub(crate) async fn login_response(
    state: &AppState,
    user: crate::users::UserPublic,
    headers: &HeaderMap,
    ip: String,
) -> Result<Response, ApiError> {
    let session = SessionMeta::fresh(headers, ip);
    Ok(auth_ok(state, issue_tokens(state, user, session).await?))
}

/// Registo de conta. O QUE acontece decide-o a política da instalação
/// (`delonix_meet_domain::identity::registration`, ADR-0006 §2); aqui só se lê
/// o retrato da instalação e se executa o plano.
///
/// No perfil histórico (`saas` + `open` + `multi`) o registo cria uma
/// organização com o primeiro utilizador como admin, e o domínio do email passa
/// a ser o domínio da organização (único) — igual ao que sempre foi.
///
/// Abre sessão: O refresh token vai no cabeçalho `Set-Cookie: dlx_refresh=…; HttpOnly; SameSite=Strict; Path=/api/auth` — nunca no corpo.
#[utoipa::path(
    post, path = "/api/auth/register", tag = "auth",
    request_body = RegisterReq,
    responses(
        (status = 200, description = "Conta criada e sessão aberta. Define o cookie de refresh `dlx_refresh`.", body = AuthOk),
        (status = 400, description = "Email, password ou nome da organização inválidos (`registration.invalid_*`, `registration.corporate_email_required`).", body = crate::openapi::ErrorBody),
        (status = 403, description = "A política da instalação não admite este registo (`registration.closed`, `registration.invite_only`, `registration.domain_not_allowed`).", body = crate::openapi::ErrorBody),
        (status = 409, description = "Email/username já em uso, ou já existe organização para o domínio (`registration.domain_taken`).", body = crate::openapi::ErrorBody),
        (status = 429, description = "Limite de pedidos de autenticação por IP.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn register(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<RegisterReq>,
) -> Result<Response, ApiError> {
    use delonix_meet_domain::identity::registration::{
        self, RegistrationPlan, RegistrationRequest,
    };
    use delonix_meet_domain::identity::validation;

    let ip = crate::rate_limit::client_ip(&headers, addr.ip());
    let email = validation::normalize_email(&req.email);
    // Sem username explícito → deriva da parte local do email.
    let username = if req.username.trim().len() >= 2 {
        req.username.trim().to_string()
    } else {
        email.split('@').next().unwrap_or("admin").to_string()
    };
    let request = RegistrationRequest {
        email: email.clone(),
        org_name: Some(req.org_name.clone()).filter(|n| !n.trim().is_empty()),
        username: username.clone(),
        password: req.password.clone(),
    };
    let policy = state.config.registration_policy();

    // 1.ª decisão sem trinco: recusa cedo, antes do argon2 (caro), o que já se
    // sabe que não entra.
    let snapshot = installation_snapshot(&state.db, &email).await?;
    registration::plan(&policy, &request, &snapshot)?;
    let password_hash = hash_password(&req.password)?;

    // 2.ª decisão, a que conta: dentro da transação e com trinco, para que duas
    // contas a nascer ao mesmo tempo numa instalação vazia não sejam ambas «a
    // primeira».
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('delonix.registration'))")
        .execute(&mut *tx)
        .await?;
    let snapshot = installation_snapshot(&mut *tx, &email).await?;
    let plan = registration::plan(&policy, &request, &snapshot)?;

    let user: crate::users::UserPublic = sqlx::query_as(&format!(
        "INSERT INTO users (email, username, password_hash) VALUES ($1, $2, $3)
         RETURNING {}",
        crate::users::USER_PUBLIC_COLUMNS
    ))
    .bind(&email)
    .bind(&username)
    .bind(&password_hash)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            ApiError::Conflict("email ou nome de utilizador já em uso".into())
        }
        _ => e.into(),
    })?;

    let (org_id, action, target) = match plan {
        RegistrationPlan::CreateOrganization {
            name,
            email_domain,
            kind,
        } => {
            let base = crate::org::slugify(&name);
            // slug único: sufixo do domínio (histórico) ou aleatório (sem domínio).
            let slug = match &email_domain {
                Some(d) => format!("{base}-{}", d.replace('.', "-")),
                None => format!("{base}-{}", delonix_meet_core::crypto::random_hex(3)),
            };
            let (org_id,): (uuid::Uuid,) = sqlx::query_as(
                "INSERT INTO organizations (name, slug, created_by, email_domain, kind)
                 VALUES ($1, $2, $3, $4, $5) RETURNING id",
            )
            .bind(&name)
            .bind(&slug)
            .bind(user.id)
            .bind(email_domain.as_deref().unwrap_or(""))
            .bind(kind.as_str())
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| match &e {
                sqlx::Error::Database(db) if db.is_unique_violation() => {
                    ApiError::Conflict("já existe uma organização para este domínio".into())
                }
                _ => e.into(),
            })?;

            // Os dois papéis de sistema nascem com a organização — ver rbac.rs.
            let (admin_role_id,): (Uuid,) = sqlx::query_as(
                "INSERT INTO org_roles (org_id, name, is_system) VALUES ($1, 'Administrador', TRUE) RETURNING id",
            )
            .bind(org_id)
            .fetch_one(&mut *tx)
            .await?;
            sqlx::query(
                "INSERT INTO org_roles (org_id, name, is_system) VALUES ($1, 'Membro', TRUE)",
            )
            .bind(org_id)
            .execute(&mut *tx)
            .await?;
            crate::org::insert_member_with_role_tx(
                &mut tx,
                org_id,
                user.id,
                "admin",
                admin_role_id,
                "Administrador",
            )
            .await?;
            (org_id, "org.created", name)
        }
        RegistrationPlan::JoinOrganization { org_id, as_admin } => {
            let role = if as_admin { "admin" } else { "member" };
            crate::org::insert_member_tx(&mut tx, org_id, user.id, role, "").await?;
            (org_id, "member.registered", email.clone())
        }
    };
    tx.commit().await?;

    crate::audit::log(&state.db, Some(org_id), user.id, action, &target).await;
    let session = SessionMeta::fresh(&headers, ip);
    Ok(auth_ok(&state, issue_tokens(&state, user, session).await?))
}

/// O que a política de registo precisa de saber sobre a instalação.
async fn installation_snapshot<'e, E>(
    db: E,
    email: &str,
) -> Result<delonix_meet_domain::identity::registration::InstallationSnapshot, ApiError>
where
    E: sqlx::PgExecutor<'e>,
{
    let domain = email.split('@').nth(1).unwrap_or("");
    // As contas técnicas (`…@delonix.internal`, ex.: a de provisionamento) não
    // fazem de uma instalação vazia uma instalação com utilizadores.
    let (has_users, domain_taken, single_org): (bool, bool, Option<uuid::Uuid>) = sqlx::query_as(
        "SELECT
            EXISTS (SELECT 1 FROM users WHERE email NOT LIKE '%@delonix.internal'),
            EXISTS (SELECT 1 FROM organizations WHERE email_domain = $1 AND email_domain <> ''),
            (SELECT id FROM organizations ORDER BY created_at, id LIMIT 1)",
    )
    .bind(domain)
    .fetch_one(db)
    .await?;
    Ok(
        delonix_meet_domain::identity::registration::InstallationSnapshot {
            has_users,
            domain_taken,
            single_org,
        },
    )
}

/// Login por email e password.
///
/// Sem MFA, abre sessão: O refresh token vai no cabeçalho `Set-Cookie: dlx_refresh=…; HttpOnly; SameSite=Strict; Path=/api/auth` — nunca no corpo. Com MFA activo devolve só o desafio
/// (`MfaChallenge`) e nenhum cookie; os tokens saem em `/api/auth/login/mfa`. Sem
/// conta local, tenta o primeiro login pelo Odoo da plataforma.
#[utoipa::path(
    post, path = "/api/auth/login", tag = "auth",
    request_body = LoginReq,
    responses(
        (status = 200, description = "Sessão aberta (`AuthOk` + cookie `dlx_refresh`) ou desafio de MFA (`MfaChallenge`, sem cookie).", body = LoginResponse),
        (status = 400, description = "A organização do domínio exige SSO exclusivo.", body = crate::openapi::ErrorBody),
        (status = 401, description = "Credenciais inválidas.", body = crate::openapi::ErrorBody),
        (status = 429, description = "Demasiadas tentativas (por IP ou por conta).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn login(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<LoginReq>,
) -> Result<Response, ApiError> {
    let ip = crate::rate_limit::client_ip(&headers, addr.ip());
    let email = req.email.trim().to_lowercase();

    // Anti-brute-force por conta (complementa o limite por IP): trava após
    // demasiadas tentativas na mesma conta, mesmo vindas de vários IPs. É a
    // PRIMEIRA coisa que responde (R132): nenhuma resposta específica da conta
    // ou do domínio sai antes dele, senão essas respostas ficam sem travão.
    if !state.login_limiter.check(&format!("acct:{email}")) {
        return Err(ApiError::TooManyRequests);
    }

    // Bloquear login por password se a organização exige SSO exclusivo.
    if is_sso_enforced(&state.db, &email).await {
        return Err(ApiError::BadRequest(
            "Esta organização exige login via SSO — usa o botão «Entrar com SSO»".into(),
        ));
    }
    let row: Option<(Uuid, String, String, String, chrono::DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, email, username, password_hash, created_at FROM users WHERE email = $1",
    )
    .bind(&email)
    .fetch_optional(&state.db)
    .await?;

    // Verify against a dummy hash when the user doesn't exist so timing
    // doesn't leak account existence.
    const DUMMY: &str = "$argon2id$v=19$m=19456,t=2,p=1$YWFhYWFhYWFhYWFhYWFhYQ$m6vRnxkbG10eB0QdjqfLd8Y6M3holKAAvfeFXTiXBdU";

    let Some((id, user_email, username, password_hash, created_at)) = row else {
        // Sem conta local: antes de recusar, tenta o Odoo da plataforma. É o
        // que faz o PRIMEIRO login criar a organização e trazer os colegas —
        // sem isto, alguém teria de provisionar antes de alguém poder entrar.
        // Devolve `None` quando o login por Odoo está desligado ou as
        // credenciais não servem, e aí o 401 mantém-se.
        if let Some(user) = crate::odoo_sso::try_first_login(&state, &email, &req.password).await {
            crate::audit::log(&state.db, None, user.id, "auth.login_odoo", &user.email).await;
            let session = SessionMeta::fresh(&headers, ip);
            return Ok(auth_ok(&state, issue_tokens(&state, user, session).await?));
        }
        let _ = verify_password(&req.password, DUMMY);
        return Err(ApiError::Unauthorized);
    };

    // Integração Odoo: se a org do utilizador tem integração activa, validar
    // contra o Odoo primeiro. Em modo offline (Odoo inacessível), usa o hash
    // Argon2 guardado na última autenticação online bem-sucedida.
    let odoo_cfg = crate::odoo::org_odoo_config(&state.db, &email).await;
    let authenticated = if let Some((org_id, odoo_url, odoo_db)) = odoo_cfg {
        // Usa o mesmo cliente do primeiro login (odoo_sso): além do uid, dá a
        // SESSÃO, e é ela que permite reler o directório. Sem isso, os
        // colegas admitidos no Odoo depois do primeiro login nunca chegavam
        // aqui — a sincronização era um evento único, não um estado.
        match crate::odoo_sso::login(&state.outbound, &odoo_url, &odoo_db, &email, &req.password)
            .await
        {
            Ok(Some(session)) => {
                // Online: guarda o hash para o modo offline seguinte.
                if let Ok(h) = hash_password(&req.password) {
                    let _ = sqlx::query(
                        "UPDATE users SET password_hash = $1, odoo_uid = $2 WHERE id = $3",
                    )
                    .bind(&h)
                    .bind(session.uid)
                    .bind(id)
                    .execute(&state.db)
                    .await;
                }
                // Re-sincroniza o directório se estiver velho (>1h). Corre em
                // segundo plano: o login não paga a leitura.
                crate::odoo_sso::spawn_directory_sync(
                    state.clone(),
                    org_id,
                    odoo_url.clone(),
                    session,
                    false,
                );
                true
            }
            Ok(None) => {
                // Senha alterada/revogada no Odoo — rejeitar mesmo que o hash
                // local ainda coincida. O Odoo é a fonte de verdade.
                let _ = verify_password(&req.password, DUMMY);
                false
            }
            Err(_) => {
                // Odoo inacessível: vale o hash Argon2 da última autenticação
                // online. É o que mantém as reuniões a funcionar quando é o
                // ERP que está em baixo.
                !password_hash.is_empty() && verify_password(&req.password, &password_hash)
            }
        }
    } else {
        verify_password(&req.password, &password_hash)
    };

    if authenticated {
        let user = crate::users::UserPublic {
            id,
            email: user_email,
            username,
            created_at,
            locale: "pt".into(),
        };
        // Segundo factor: com MFA activo, a password sozinha NÃO produz sessão.
        // Devolve-se um desafio de curta duração, e os tokens só saem no
        // `/api/auth/login/mfa`. É o ponto todo do segundo factor — se a password
        // bastasse para obter o access token, o resto era teatro.
        if crate::mfa::activo(&state.db, user.id).await? {
            crate::audit::log(&state.db, None, user.id, "auth.mfa_challenge", &user.email).await;
            return Ok(Json(LoginResponse::MfaRequired(MfaChallenge {
                mfa_required: true,
                mfa_token: mfa_challenge_token(&state, user.id)?,
            }))
            .into_response());
        }
        crate::audit::log(&state.db, None, user.id, "auth.login", &user.email).await;
        let session = SessionMeta::fresh(&headers, ip);
        Ok(auth_ok(&state, issue_tokens(&state, user, session).await?))
    } else {
        Err(ApiError::Unauthorized)
    }
}

/// Token de DESAFIO do segundo factor.
///
/// JWT separado, `typ: "mfa"`, válido 5 minutos. Não serve para mais nada: o
/// `verify_jwt` do resto da API exige `typ: "access"`, por isso este token não
/// abre um único endpoint. Prova só que a password foi aceite.
fn mfa_challenge_token(state: &AppState, user_id: Uuid) -> Result<String, ApiError> {
    let now = Utc::now().timestamp();
    sign_jwt(
        &state.config.jwt_secret,
        &Claims {
            sub: user_id,
            typ: "mfa".into(),
            iat: now,
            exp: now + 5 * 60,
            room: None,
            name: None,
            topo: None,
            owner: false,
            wait: false,
            adm: false,
            is_bot: false,
            origin: None,
            title: None,
            lobby: None,
            wr: None,
        },
    )
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct MfaReq {
    pub mfa_token: String,
    pub code: String,
}

/// Segunda metade do login: troca o desafio + código pelos tokens de sessão.
///
/// O código pode ser TOTP ou de recuperação. O refresh token vai no cabeçalho `Set-Cookie: dlx_refresh=…; HttpOnly; SameSite=Strict; Path=/api/auth` — nunca no corpo.
#[utoipa::path(
    post, path = "/api/auth/login/mfa", tag = "auth",
    request_body = MfaReq,
    responses(
        (status = 200, description = "Sessão aberta. Define o cookie de refresh `dlx_refresh`.", body = AuthOk),
        (status = 401, description = "Desafio inválido/expirado ou código errado/já usado.", body = crate::openapi::ErrorBody),
        (status = 404, description = "A conta do desafio já não existe.", body = crate::openapi::ErrorBody),
        (status = 429, description = "Demasiadas tentativas (por IP ou por conta).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn mfa_login(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<MfaReq>,
) -> Result<Response, ApiError> {
    let ip = crate::rate_limit::client_ip(&headers, addr.ip());
    // O `verify_jwt` exige o `typ` esperado: um access token NÃO serve de
    // desafio, nem o desafio serve de access token. É a mesma chave a assinar
    // os dois, e sem esta verificação seriam intermutáveis.
    let claims = verify_jwt(&state.config.jwt_secret, &req.mfa_token, "mfa")
        .map_err(|_| ApiError::Unauthorized)?;
    let user_id = claims.sub;

    // Anti-força-bruta: seis dígitos são um milhão de hipóteses, e sem travão
    // uma rede rápida percorre-as em minutos. O limitador é POR CONTA, não por
    // IP — distribuir as tentativas por vários IPs não deve ajudar.
    if !state.login_limiter.check(&format!("mfa:{user_id}")) {
        return Err(ApiError::TooManyRequests);
    }
    if !crate::mfa::consome_codigo(&state, user_id, &req.code).await? {
        crate::audit::log(&state.db, None, user_id, "auth.mfa_failed", "").await;
        return Err(ApiError::Unauthorized);
    }
    let user = crate::users::fetch_public(&state.db, user_id).await?;
    crate::audit::log(&state.db, None, user.id, "auth.login_mfa", &user.email).await;
    let session = SessionMeta::fresh(&headers, ip);
    Ok(auth_ok(&state, issue_tokens(&state, user, session).await?))
}

/// Roda a sessão: consome o refresh token do cookie `dlx_refresh` (revoga-o)
/// e emite um par novo.
///
/// Autentica-se pelo cookie HttpOnly, não por header. O refresh token vai no cabeçalho `Set-Cookie: dlx_refresh=…; HttpOnly; SameSite=Strict; Path=/api/auth` — nunca no corpo.
#[utoipa::path(
    post, path = "/api/auth/refresh", tag = "auth",
    responses(
        (status = 200, description = "Access token novo. Define o cookie `dlx_refresh` rodado.", body = AuthOk),
        (status = 401, description = "Cookie ausente, revogado ou expirado.", body = crate::openapi::ErrorBody),
        (status = 404, description = "A conta do token já não existe.", body = crate::openapi::ErrorBody),
        (status = 429, description = "Limite de pedidos de autenticação por IP.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn refresh(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let ip = crate::rate_limit::client_ip(&headers, addr.ip());
    // O refresh token vem do cookie HttpOnly (não do corpo — imune a XSS).
    let token = read_refresh_cookie(&headers).ok_or(ApiError::Unauthorized)?;
    let hash = hash_refresh_token(&token);
    let row: Option<(Uuid, Uuid, DateTime<Utc>)> = sqlx::query_as(
        "SELECT user_id, session_id, session_started_at FROM refresh_tokens
         WHERE token_hash = $1 AND NOT revoked AND expires_at > now()",
    )
    .bind(&hash)
    .fetch_optional(&state.db)
    .await?;
    let (user_id, session_id, session_started_at) = row.ok_or(ApiError::Unauthorized)?;

    // Rotate: revoke the used token, issue a fresh pair.
    sqlx::query("UPDATE refresh_tokens SET revoked = TRUE WHERE token_hash = $1")
        .bind(&hash)
        .execute(&state.db)
        .await?;

    let user = crate::users::fetch_public(&state.db, user_id).await?;
    let session = SessionMeta::continued(session_id, session_started_at, &headers, ip);
    Ok(auth_ok(&state, issue_tokens(&state, user, session).await?))
}

/// Termina a sessão: revoga o refresh token (se presente) e limpa o cookie.
///
/// Responde sempre 200, com ou sem cookie; o `Set-Cookie` devolvido expira o
/// `dlx_refresh` (`Max-Age=0`).
#[utoipa::path(
    post, path = "/api/auth/logout", tag = "auth",
    responses(
        (status = 204, description = "Sessão terminada. Limpa o cookie `dlx_refresh` (e revoga-o, se vier)."),
        (status = 429, description = "Limite de pedidos de autenticação por IP.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn logout(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if let Some(token) = read_refresh_cookie(&headers) {
        let hash = hash_refresh_token(&token);
        let _ = sqlx::query("UPDATE refresh_tokens SET revoked = TRUE WHERE token_hash = $1")
            .bind(&hash)
            .execute(&state.db)
            .await;
    }
    let clear = refresh_cookie("", state.config.cookie_secure, 0);
    Ok((
        axum::http::StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, clear)],
    )
        .into_response())
}

// ---------- SSO / OIDC ----------

use dashmap::DashMap;
use std::time::Instant;

/// Estado PKCE pendente: guardado em memória entre o redirect e o callback.
/// TTL de 10 minutos — limpo no consumo ou por expiração passiva.
struct PkceEntry {
    verifier: String,
    org_id: Uuid,
    nonce: String,
    created: Instant,
}

/// Cache global de estados OIDC pendentes (anti-CSRF `state` → PKCE verifier).
/// Vive em memória: OK para single-node; em multi-node, migrar para Redis.
static SSO_PENDING: std::sync::LazyLock<DashMap<String, PkceEntry>> =
    std::sync::LazyLock::new(DashMap::new);

/// Resultado da verificação de SSO para um domínio de email.
#[derive(Serialize, utoipa::ToSchema)]
pub struct SsoCheck {
    pub sso_enabled: bool,
    /// Se true, o login por password está bloqueado para este domínio.
    pub enforce_sso: bool,
}

/// `GET /api/auth/sso/discovery?domain=example.com`
/// O frontend chama isto ao preencher o email para decidir se mostra o campo
/// de password ou redireciona para o IdP.
#[utoipa::path(
    get, path = "/api/auth/sso/discovery", tag = "auth",
    params(("domain" = Option<String>, Query, description = "Domínio de email (ex.: `example.com`). Vazio ou omisso ⇒ tudo `false`.")),
    responses(
        (status = 200, body = SsoCheck),
        (status = 429, description = "Limite de pedidos de autenticação por IP.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn sso_check(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<SsoCheck>, ApiError> {
    let domain = params
        .get("domain")
        .map(|d| d.trim().to_lowercase())
        .unwrap_or_default();
    if domain.is_empty() {
        return Ok(Json(SsoCheck {
            sso_enabled: false,
            enforce_sso: false,
        }));
    }

    let row: Option<(bool,)> = sqlx::query_as(
        "SELECT s.enforce_sso FROM org_sso_configs s
         JOIN organizations o ON o.id = s.org_id
         WHERE o.email_domain = $1",
    )
    .bind(&domain)
    .fetch_optional(&state.db)
    .await?;

    match row {
        Some((enforce,)) => Ok(Json(SsoCheck {
            sso_enabled: true,
            enforce_sso: enforce,
        })),
        None => Ok(Json(SsoCheck {
            sso_enabled: false,
            enforce_sso: false,
        })),
    }
}

/// Erro da descoberta OIDC. Um emissor recusado pela guarda de saída é um erro
/// de CONFIGURAÇÃO da organização (400, com razão), não uma avaria do servidor.
fn oidc_discovery_error(
    e: openidconnect::DiscoveryError<crate::net_guard::OidcHttpError>,
) -> ApiError {
    match e {
        openidconnect::DiscoveryError::Request(crate::net_guard::OidcHttpError::Blocked(why)) => {
            ApiError::BadRequest(format!("emissor OIDC recusado pela guarda de saída: {why}"))
        }
        e => ApiError::Internal(format!("OIDC discovery: {e}")),
    }
}

/// `GET /api/auth/sso/authorize?domain=example.com`
/// Descobre o IdP OIDC da organização, gera state+PKCE e redireciona (302)
/// o browser do utilizador para o IdP (Google/Microsoft/Okta).
#[utoipa::path(
    get, path = "/api/auth/sso/authorize", tag = "auth",
    params(("domain" = String, Query, description = "Domínio de email da organização.")),
    responses(
        (status = 302, description = "Redirecção (`Location`) para o endpoint de autorização do IdP, com state + PKCE."),
        (status = 400, description = "`domain` em falta, ou o emissor OIDC da organização aponta para um endereço interno (guarda de saída).", body = crate::openapi::ErrorBody),
        (status = 404, description = "Nenhuma organização com SSO configurado para o domínio.", body = crate::openapi::ErrorBody),
        (status = 429, description = "Limite de pedidos de autenticação por IP.", body = crate::openapi::ErrorBody),
        (status = 500, description = "Issuer inválido ou discovery OIDC falhou.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn sso_login(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let domain = params
        .get("domain")
        .map(|d| d.trim().to_lowercase())
        .ok_or_else(|| ApiError::BadRequest("domain is required".into()))?;

    let sso: Option<(Uuid, String, String, String)> = sqlx::query_as(
        "SELECT s.org_id, s.issuer_url, s.client_id, s.client_secret
         FROM org_sso_configs s
         JOIN organizations o ON o.id = s.org_id
         WHERE o.email_domain = $1",
    )
    .bind(&domain)
    .fetch_optional(&state.db)
    .await?;

    let (org_id, issuer_url, client_id, client_secret) = sso.ok_or(ApiError::NotFound)?;
    // Guardado cifrado (S5); o herdado em claro passa como está.
    let client_secret = crate::secrets_at_rest::open(
        &state.config,
        &client_secret,
        &crate::org::sso_client_secret_aad(org_id),
    )?;

    // OIDC Discovery (cached pelo crate; contacta <issuer>/.well-known/openid-configuration).
    use openidconnect::{
        core::CoreClient, AuthenticationFlow, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce,
        RedirectUrl, Scope,
    };

    let issuer = IssuerUrl::new(issuer_url.clone())
        .map_err(|e| ApiError::Internal(format!("issuer URL inválido: {e}")))?;

    // Sem redirects, com timeout e guarda anti-SSRF em cada pedido do fluxo
    // (descoberta, JWKS, troca do código) — ver `net_guard::OidcHttp`.
    let http_client = state.outbound.oidc();

    let provider_metadata =
        openidconnect::core::CoreProviderMetadata::discover_async(issuer, &http_client)
            .await
            .map_err(oidc_discovery_error)?;

    // O callback URL é relativo ao host que serviu o pedido.
    let callback_url = state
        .config
        .cors_origins
        .first()
        .map(|o| format!("{o}/api/auth/sso/callback"))
        .unwrap_or_else(|| "http://localhost:8180/api/auth/sso/callback".to_string());

    let client = CoreClient::from_provider_metadata(
        provider_metadata,
        ClientId::new(client_id),
        Some(ClientSecret::new(client_secret)),
    )
    .set_redirect_uri(
        RedirectUrl::new(callback_url)
            .map_err(|e| ApiError::Internal(format!("redirect URL inválido: {e}")))?,
    );

    let (pkce_challenge, pkce_verifier) = openidconnect::PkceCodeChallenge::new_random_sha256();
    let (auth_url, csrf_token, nonce) = client
        .authorize_url(
            AuthenticationFlow::<openidconnect::core::CoreResponseType>::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .add_scope(Scope::new("email".to_string()))
        .add_scope(Scope::new("profile".to_string()))
        .set_pkce_challenge(pkce_challenge)
        .url();

    // Guardar o verifier PKCE para validar no callback.
    SSO_PENDING.insert(
        csrf_token.secret().clone(),
        PkceEntry {
            verifier: pkce_verifier.secret().clone(),
            org_id,
            nonce: nonce.secret().clone(),
            created: Instant::now(),
        },
    );

    // Limpar entradas expiradas (> 10 min) passivamente.
    SSO_PENDING.retain(|_, v| v.created.elapsed().as_secs() < 600);

    tracing::info!(%domain, %org_id, "SSO login redirect → IdP");

    Ok((
        [(header::LOCATION, auth_url.to_string())],
        axum::http::StatusCode::FOUND,
    )
        .into_response())
}

/// `GET /api/auth/sso/callback?code=...&state=...`
/// Recebe o código de autorização do IdP, troca-o pelo id_token, e faz
/// Just-in-Time Provisioning se o utilizador não existir.
///
/// Abre sessão e redirecciona para o frontend com o access token no fragmento
/// (`/#/sso-complete?token=…`). O refresh token vai no cabeçalho `Set-Cookie: dlx_refresh=…; HttpOnly; SameSite=Strict; Path=/api/auth` — nunca no corpo.
#[utoipa::path(
    get, path = "/api/auth/sso/callback", tag = "auth",
    params(
        ("code" = String, Query, description = "Código de autorização do IdP."),
        ("state" = String, Query, description = "State anti-CSRF emitido por `/api/auth/sso/authorize` (uso único, 10 min)."),
    ),
    responses(
        (status = 302, description = "Redirecção para o frontend com o access token no fragmento. Define o cookie `dlx_refresh`."),
        (status = 400, description = "`code`/`state` em falta, ou o IdP não devolveu email.", body = crate::openapi::ErrorBody),
        (status = 401, description = "State desconhecido, já consumido ou expirado.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Regra de pertença (R130): `sso.account_not_in_org` — a conta existe mas não é membro activo desta organização; `sso.email_domain_mismatch` — conta nova de um domínio que não é o da organização.", body = crate::openapi::ErrorBody),
        (status = 404, description = "A configuração SSO da organização foi removida entretanto.", body = crate::openapi::ErrorBody),
        (status = 409, description = "Provisionamento JIT colidiu com email/username existente.", body = crate::openapi::ErrorBody),
        (status = 429, description = "Limite de pedidos de autenticação por IP.", body = crate::openapi::ErrorBody),
        (status = 500, description = "Discovery, troca de código ou verificação do id_token falhou.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn sso_callback(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let ip = crate::rate_limit::client_ip(&headers, addr.ip());
    let code = params
        .get("code")
        .ok_or_else(|| ApiError::BadRequest("code is required".into()))?;
    let csrf_state = params
        .get("state")
        .ok_or_else(|| ApiError::BadRequest("state is required".into()))?;

    // Recuperar e consumir o PKCE entry (one-time use).
    let (_, entry) = SSO_PENDING
        .remove(csrf_state)
        .ok_or(ApiError::Unauthorized)?;

    // Expirado?
    if entry.created.elapsed().as_secs() > 600 {
        return Err(ApiError::Unauthorized);
    }

    // Recarregar config do IdP da org.
    let sso: (String, String, String) = sqlx::query_as(
        "SELECT s.issuer_url, s.client_id, s.client_secret
         FROM org_sso_configs s WHERE s.org_id = $1",
    )
    .bind(entry.org_id)
    .fetch_one(&state.db)
    .await?;
    let (issuer_url, client_id, client_secret) = sso;
    let client_secret = crate::secrets_at_rest::open(
        &state.config,
        &client_secret,
        &crate::org::sso_client_secret_aad(entry.org_id),
    )?;

    use openidconnect::{
        core::CoreClient, AuthorizationCode, ClientId, ClientSecret, IssuerUrl, Nonce,
        PkceCodeVerifier, RedirectUrl, TokenResponse,
    };

    let issuer =
        IssuerUrl::new(issuer_url).map_err(|e| ApiError::Internal(format!("issuer URL: {e}")))?;

    // Sem redirects, com timeout e guarda anti-SSRF em cada pedido do fluxo
    // (descoberta, JWKS, troca do código) — ver `net_guard::OidcHttp`.
    let http_client = state.outbound.oidc();

    let provider_metadata =
        openidconnect::core::CoreProviderMetadata::discover_async(issuer, &http_client)
            .await
            .map_err(oidc_discovery_error)?;

    let callback_url = state
        .config
        .cors_origins
        .first()
        .map(|o| format!("{o}/api/auth/sso/callback"))
        .unwrap_or_else(|| "http://localhost:8180/api/auth/sso/callback".to_string());

    let client = CoreClient::from_provider_metadata(
        provider_metadata,
        ClientId::new(client_id),
        Some(ClientSecret::new(client_secret)),
    )
    .set_redirect_uri(
        RedirectUrl::new(callback_url)
            .map_err(|e| ApiError::Internal(format!("redirect URL: {e}")))?,
    );

    // Trocar o authorization code pelo token set.
    let token_response = client
        .exchange_code(AuthorizationCode::new(code.clone()))
        .map_err(|e| ApiError::Internal(format!("oidc config: {e}")))?
        .set_pkce_verifier(PkceCodeVerifier::new(entry.verifier))
        .request_async(&http_client)
        .await
        .map_err(|e| ApiError::Internal(format!("token exchange: {e}")))?;

    // Verificar o id_token (assinatura + nonce + issuer + audience).
    let id_token = token_response
        .id_token()
        .ok_or_else(|| ApiError::Internal("IdP não devolveu id_token".into()))?;

    let verifier = client.id_token_verifier();
    let claims = id_token
        .claims(&verifier, &Nonce::new(entry.nonce))
        .map_err(|e| ApiError::Internal(format!("id_token verification: {e}")))?;

    // Extrair email e nome do id_token.
    let email = claims
        .email()
        .map(|e| e.to_string().to_lowercase())
        .ok_or_else(|| {
            ApiError::BadRequest("O IdP não devolveu o email — verifica os scopes".into())
        })?;
    let name = claims
        .name()
        .and_then(|n| n.get(None).map(|v| v.to_string()))
        .unwrap_or_else(|| email.split('@').next().unwrap_or("user").to_string());

    let sso_provider = claims.issuer().to_string();
    let sso_subject = claims.subject().to_string();

    // Just-in-Time Provisioning: procurar ou criar o utilizador — mas só dentro
    // da regra de pertença (R130). O IdP é escolhido pelo administrador da org
    // e pode afirmar o email que quiser: o email não é prova de pertença.
    let existing: Option<crate::users::UserPublic> = sqlx::query_as(&format!(
        "SELECT {} FROM users WHERE email = $1",
        crate::users::USER_PUBLIC_COLUMNS
    ))
    .bind(&email)
    .fetch_optional(&state.db)
    .await?;

    let account = match &existing {
        Some(u) => {
            // Pertença decide-se em org.rs (catraca, regra 1): `role_in_org`
            // já filtra `archived_at`.
            let active = crate::org::role_in_org(&state, entry.org_id, u.id)
                .await?
                .is_some();
            Some(active)
        }
        None => None,
    };
    let org_domain: String =
        sqlx::query_scalar("SELECT email_domain FROM organizations WHERE id = $1")
            .bind(entry.org_id)
            .fetch_one(&state.db)
            .await?;

    match sso_login_decision(&email, &org_domain, account) {
        SsoLoginDecision::LogIn | SsoLoginDecision::Provision => {}
        SsoLoginDecision::Refuse(code) => {
            tracing::warn!(%email, org_id = %entry.org_id, code, "SSO recusado: fora da regra de pertença");
            crate::audit::log(
                &state.db,
                Some(entry.org_id),
                existing.as_ref().map(|u| u.id).unwrap_or(Uuid::nil()),
                "auth.sso_refused",
                &email,
            )
            .await;
            let message = match code {
                SSO_ACCOUNT_NOT_IN_ORG => {
                    "Esta conta não é membro activo desta organização — o SSO dela não a abre."
                }
                _ => "O email devolvido pelo IdP não é do domínio desta organização.",
            };
            return Err(ApiError::Domain(
                delonix_meet_core::DomainError::forbidden(code).with_message(message),
            ));
        }
    }

    let user = match existing {
        Some(u) => {
            // Atualizar os campos SSO se estiverem vazios (migração de conta local → SSO).
            if u.id != Uuid::nil() {
                let _ = sqlx::query(
                    "UPDATE users SET sso_provider = $1, sso_subject = $2
                     WHERE id = $3 AND sso_provider = ''",
                )
                .bind(&sso_provider)
                .bind(&sso_subject)
                .bind(u.id)
                .execute(&state.db)
                .await;
            }
            u
        }
        None => {
            // JIT: criar conta + adicionar como membro da org.
            let dummy_hash = hash_password(&Uuid::new_v4().to_string())?;
            let mut tx = state.db.begin().await?;
            let new_user: crate::users::UserPublic = sqlx::query_as(&format!(
                "INSERT INTO users (email, username, password_hash, sso_provider, sso_subject)
                 VALUES ($1, $2, $3, $4, $5)
                 RETURNING {}",
                crate::users::USER_PUBLIC_COLUMNS
            ))
            .bind(&email)
            .bind(&name)
            .bind(&dummy_hash)
            .bind(&sso_provider)
            .bind(&sso_subject)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| match &e {
                sqlx::Error::Database(db) if db.is_unique_violation() => {
                    ApiError::Conflict("email ou username já em uso".into())
                }
                _ => e.into(),
            })?;

            // Adicionar como membro da org (role = member; admins são promovidos manualmente).
            let membro_role_id: Option<(Uuid,)> = sqlx::query_as(
                "SELECT id FROM org_roles WHERE org_id = $1 AND is_system = TRUE AND name = 'Membro'",
            )
            .bind(entry.org_id)
            .fetch_optional(&mut *tx)
            .await?;
            sqlx::query(
                "INSERT INTO org_members (org_id, user_id, role, role_id, title)
                 VALUES ($1, $2, 'member', $3, '')
                 ON CONFLICT DO NOTHING",
            )
            .bind(entry.org_id)
            .bind(new_user.id)
            .bind(membro_role_id.map(|(id,)| id))
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;
            tracing::info!(%email, org_id = %entry.org_id, "SSO JIT provisioned new user");
            new_user
        }
    };

    // Emitir tokens nativos do Delonix e redirecionar para o frontend.
    let session = SessionMeta::fresh(&headers, ip);
    let pair = issue_tokens(&state, user, session).await?;
    let cookie = refresh_cookie(
        &pair.refresh_token,
        state.config.cookie_secure,
        state.config.refresh_ttl_secs,
    );

    // Redirecionar para o frontend com o access_token como fragment (nunca na query
    // string, para não aparecer em logs do servidor). O frontend lê o hash fragment.
    let redirect_url = state
        .config
        .cors_origins
        .first()
        .map(|o| format!("{o}/#/sso-complete?token={}", pair.access_token))
        .unwrap_or_else(|| {
            format!(
                "https://localhost:5173/#/sso-complete?token={}",
                pair.access_token
            )
        });

    tracing::info!(email = %pair.user.email, "SSO login success");

    Ok((
        [
            (header::LOCATION, redirect_url),
            (header::SET_COOKIE, cookie),
        ],
        axum::http::StatusCode::FOUND,
    )
        .into_response())
}

/// Código estável: a conta existe mas não é membro ACTIVO da org do IdP.
pub(crate) const SSO_ACCOUNT_NOT_IN_ORG: &str = "sso.account_not_in_org";
/// Código estável: conta nova de um domínio que não é o da org do IdP.
pub(crate) const SSO_EMAIL_DOMAIN_MISMATCH: &str = "sso.email_domain_mismatch";

/// O que o callback SSO pode fazer com o email que o IdP afirmou.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SsoLoginDecision {
    /// Conta existente e membro activo da org: abre sessão.
    LogIn,
    /// Sem conta, email do domínio da org: cria a conta como membro.
    Provision,
    /// Recusa, com o código estável.
    Refuse(&'static str),
}

/// Regra de pertença do SSO (R130, família R25/R122) — a mais restritiva.
///
/// O IdP de uma organização é configurado pelo administrador DELA, por isso o
/// email que devolve só vale dentro da organização:
/// - conta existente → só se for membro ACTIVO desta org (`account =
///   Some(true)`). Membro de outra org, arquivado, ou órfã: recusa. Nunca se
///   junta à org pelo SSO — isso seria o próprio ataque.
/// - conta nova → só se o domínio do email for o `email_domain` da org, e a
///   org tiver domínio (uma org sem domínio não cria contas por SSO).
///
/// `account`: `None` = não há conta com este email; `Some(activo)`.
pub(crate) fn sso_login_decision(
    email: &str,
    org_domain: &str,
    account: Option<bool>,
) -> SsoLoginDecision {
    match account {
        Some(true) => SsoLoginDecision::LogIn,
        Some(false) => SsoLoginDecision::Refuse(SSO_ACCOUNT_NOT_IN_ORG),
        None => {
            let org_domain = org_domain.trim().to_lowercase();
            let email_domain = match email.rsplit_once('@') {
                Some((local, domain)) if !local.is_empty() => domain.to_lowercase(),
                _ => String::new(),
            };
            if !org_domain.is_empty() && email_domain == org_domain {
                SsoLoginDecision::Provision
            } else {
                SsoLoginDecision::Refuse(SSO_EMAIL_DOMAIN_MISMATCH)
            }
        }
    }
}

/// `GET /api/auth/sso/enforce?domain=...`
/// O handler de login local consulta isto para bloquear password quando
/// a org exige SSO exclusivo.
pub async fn is_sso_enforced(db: &sqlx::PgPool, email: &str) -> bool {
    let domain = email.split('@').nth(1).unwrap_or("");
    if domain.is_empty() {
        return false;
    }
    let row: Option<(bool,)> = sqlx::query_as(
        "SELECT s.enforce_sso FROM org_sso_configs s
         JOIN organizations o ON o.id = s.org_id
         WHERE o.email_domain = $1 AND s.enforce_sso = TRUE",
    )
    .bind(domain)
    .fetch_optional(db)
    .await
    .unwrap_or(None);
    row.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R130 — a regra de pertença do SSO, caso a caso.
    #[test]
    fn sso_login_decision_is_the_most_restrictive_rule() {
        use SsoLoginDecision::*;
        // Membro activo entra; qualquer outra conta existente é recusada,
        // mesmo com o email no domínio da org.
        assert_eq!(
            sso_login_decision("ana@alfa.ao", "alfa.ao", Some(true)),
            LogIn
        );
        assert_eq!(
            sso_login_decision("ana@alfa.ao", "alfa.ao", Some(false)),
            Refuse(SSO_ACCOUNT_NOT_IN_ORG)
        );
        assert_eq!(
            sso_login_decision("admin@beta.ao", "alfa.ao", Some(false)),
            Refuse(SSO_ACCOUNT_NOT_IN_ORG)
        );
        // Conta nova: só do domínio da org, e só se a org tiver domínio.
        assert_eq!(
            sso_login_decision("nova@alfa.ao", "alfa.ao", None),
            Provision
        );
        assert_eq!(
            sso_login_decision("nova@alfa.ao", "Alfa.AO ", None),
            Provision
        );
        for (email, dom) in [
            ("nova@beta.ao", "alfa.ao"),
            ("nova@sub.alfa.ao", "alfa.ao"),
            ("nova@alfa.ao", ""),
            ("@alfa.ao", "alfa.ao"),
            ("sem-arroba", "alfa.ao"),
            ("x@evil.ao@alfa.ao", "evil.ao"),
        ] {
            assert_eq!(
                sso_login_decision(email, dom, None),
                Refuse(SSO_EMAIL_DOMAIN_MISMATCH),
                "{email} em {dom:?}"
            );
        }
    }

    /// As respostas de login passaram de `json!` a tipos (OpenAPI): o JSON
    /// tem de continuar igual ao que o web já lê.
    #[test]
    fn login_response_serializa_como_antes() {
        let user = crate::users::UserPublic {
            id: Uuid::nil(),
            email: "a@b.c".into(),
            username: "a".into(),
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            locale: "pt".into(),
        };
        let sessao = serde_json::to_value(LoginResponse::Session(AuthOk {
            access_token: "t".into(),
            user,
        }))
        .unwrap();
        assert_eq!(
            sessao,
            serde_json::json!({
                "access_token": "t",
                "user": {"id": Uuid::nil(), "email": "a@b.c", "username": "a",
                         "created_at": "1970-01-01T00:00:00Z", "locale": "pt"},
            })
        );
        let desafio = serde_json::to_value(LoginResponse::MfaRequired(MfaChallenge {
            mfa_required: true,
            mfa_token: "m".into(),
        }))
        .unwrap();
        assert_eq!(
            desafio,
            serde_json::json!({ "mfa_required": true, "mfa_token": "m" })
        );
    }

    #[test]
    fn password_hash_roundtrip() {
        let hash = hash_password("s3cret-password").unwrap();
        assert!(verify_password("s3cret-password", &hash));
        assert!(!verify_password("wrong", &hash));
    }

    #[test]
    fn jwt_roundtrip_and_type_check() {
        let secret = "test-secret";
        let now = Utc::now().timestamp();
        let user = Uuid::new_v4();
        let token = sign_jwt(
            secret,
            &Claims {
                sub: user,
                typ: "access".into(),
                iat: now,
                exp: now + 60,
                room: None,
                name: None,
                topo: None,
                owner: false,
                wait: false,
                adm: false,
                is_bot: false,
                origin: None,
                title: None,
                lobby: None,
                wr: None,
            },
        )
        .unwrap();
        let claims = verify_jwt(secret, &token, "access").unwrap();
        assert_eq!(claims.sub, user);
        // Wrong expected type must fail (an access token is not a room token).
        assert!(verify_jwt(secret, &token, "room").is_err());
        // Tampered secret must fail.
        assert!(verify_jwt("other-secret", &token, "access").is_err());
    }

    #[test]
    fn expired_jwt_rejected() {
        let secret = "test-secret";
        let now = Utc::now().timestamp();
        let token = sign_jwt(
            secret,
            &Claims {
                sub: Uuid::new_v4(),
                typ: "access".into(),
                iat: now - 3600,
                exp: now - 120,
                room: None,
                name: None,
                topo: None,
                owner: false,
                wait: false,
                adm: false,
                is_bot: false,
                origin: None,
                title: None,
                lobby: None,
                wr: None,
            },
        )
        .unwrap();
        assert!(verify_jwt(secret, &token, "access").is_err());
    }

    #[test]
    fn refresh_token_hash_is_stable_and_opaque() {
        let (token, hash) = new_refresh_token();
        assert_eq!(hash, hash_refresh_token(&token));
        assert_ne!(token, hash);
        assert_eq!(token.len(), 64);
    }
}
