//! Chaves de acesso (WebAuthn / passkeys) como SEGUNDO factor — ADR-0011 — e o
//! resumo «Segurança» da conta.
//!
//! Contrato (BFF, sessão, sempre a própria conta):
//! - `GET    /api/users/me/security`                          resumo: password, TOTP, chaves, exigência da org
//! - `GET    /api/users/me/passkeys`                          as minhas chaves
//! - `GET    /api/users/me/passkeys/{passkey_id}`             uma
//! - `POST   /api/users/me/passkeys/begin-registration`       método personalizado: opções para o browser (exige reautenticação)
//! - `POST   /api/users/me/passkeys`                          conclui o registo → `201` + `Location`
//! - `DELETE /api/users/me/passkeys/{passkey_id}`             remove (exige reautenticação; protege o último factor)
//!
//! Login (sem sessão, com o desafio de MFA do `/api/auth/login`):
//! - `POST /api/auth/login/mfa/passkey-options`  opções de autenticação para as chaves da conta
//! - `POST /api/auth/login/mfa/passkey`          conclui → sessão
//!
//! **Sem palavra-passe NÃO** (ADR-0011): a chave de acesso substitui o código
//! TOTP, não a password. Numa conta gerida pelo Odoo, a password é o que prova
//! que a pessoa continua activa no ERP; entrar só com a chave contornava isso.
//!
//! O estado de cada cerimónia vive na base (`webauthn_ceremonies`), é consumido
//! UMA vez e expira em [`CEREMONY_TTL_SECS`]: um desafio reutilizável seria um
//! replay, e guardá-lo em memória partia com duas réplicas.

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::{DomainError, ErrorKind};
use delonix_meet_domain::identity::factors::{self, Factor};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;
use webauthn_rs::prelude::{
    Passkey, PasskeyAuthentication, PasskeyRegistration, PublicKeyCredential,
    RegisterPublicKeyCredential, Webauthn, WebauthnBuilder,
};

use crate::{auth::AuthUser, error::ApiError, AppState};

pub const CEREMONY_TTL_SECS: i64 = 5 * 60;
pub const NAME_MAX: usize = 60;
/// Tecto de chaves por conta (a lista vai inteira no desafio de login).
pub const MAX_PASSKEYS: i64 = 20;

/// Constrói o `Webauthn` a partir da configuração. `None` sem RP ID e origem —
/// e aí as rotas respondem `passkeys.not_configured` em vez de inventar.
pub fn build(config: &crate::config::Config) -> Option<Arc<Webauthn>> {
    let rp_id = config.webauthn_rp_id.as_deref()?;
    let origin = config.webauthn_rp_origin.as_deref()?;
    let origin = match webauthn_rs::prelude::Url::parse(origin) {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(error = %e, "WEBAUTHN_RP_ORIGIN inválido — chaves de acesso desligadas");
            return None;
        }
    };
    match WebauthnBuilder::new(rp_id, &origin).and_then(|b| b.rp_name("Delonix Meet").build()) {
        Ok(w) => Some(Arc::new(w)),
        Err(e) => {
            tracing::error!(error = %e, "WEBAUTHN_RP_ID/ORIGIN incoerentes — chaves de acesso desligadas");
            None
        }
    }
}

fn webauthn(state: &AppState) -> Result<&Webauthn, ApiError> {
    state.webauthn.as_deref().ok_or_else(|| {
        DomainError::new(
            ErrorKind::Unavailable,
            "passkeys.not_configured",
            "as chaves de acesso não estão configuradas nesta instalação (WEBAUTHN_RP_ID / WEBAUTHN_RP_ORIGIN)",
        )
        .into()
    })
}

fn webauthn_error(code: &'static str, e: impl std::fmt::Display) -> ApiError {
    tracing::info!(error = %e, code, "WebAuthn recusado");
    DomainError::new(
        ErrorKind::Unauthenticated,
        code,
        "a chave de acesso não foi aceite",
    )
    .into()
}

// ---------------------------------------------------------------------------
//  Factores e cerimónias
// ---------------------------------------------------------------------------

pub(crate) async fn count(db: &sqlx::PgPool, user_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM user_passkeys WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(db)
        .await
}

async fn load_passkeys(db: &sqlx::PgPool, user_id: Uuid) -> Result<Vec<(Uuid, Passkey)>, ApiError> {
    let rows: Vec<(Uuid, serde_json::Value)> = sqlx::query_as(
        "SELECT id, credential FROM user_passkeys WHERE user_id = $1 ORDER BY created_at",
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;
    rows.into_iter()
        .map(|(id, v)| {
            serde_json::from_value::<Passkey>(v)
                .map(|p| (id, p))
                .map_err(ApiError::internal)
        })
        .collect()
}

async fn save_ceremony(
    state: &AppState,
    user_id: Uuid,
    kind: &str,
    ceremony_state: serde_json::Value,
) -> Result<(Uuid, DateTime<Utc>), ApiError> {
    Ok(sqlx::query_as(
        "INSERT INTO webauthn_ceremonies (user_id, kind, state, expires_at)
         VALUES ($1, $2, $3, now() + make_interval(secs => $4))
         RETURNING id, expires_at",
    )
    .bind(user_id)
    .bind(kind)
    .bind(ceremony_state)
    .bind(CEREMONY_TTL_SECS as f64)
    .fetch_one(&state.db)
    .await?)
}

/// Consome a cerimónia (uma vez só). Uma cerimónia de outra pessoa, de outro
/// tipo ou vencida é o mesmo que inexistente.
async fn take_ceremony(
    state: &AppState,
    id: Uuid,
    user_id: Uuid,
    kind: &str,
) -> Result<serde_json::Value, ApiError> {
    sqlx::query_scalar(
        "DELETE FROM webauthn_ceremonies
          WHERE id = $1 AND user_id = $2 AND kind = $3 AND expires_at > now()
          RETURNING state",
    )
    .bind(id)
    .bind(user_id)
    .bind(kind)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| {
        DomainError::not_found("passkeys.ceremony_not_found")
            .with_message("a cerimónia não existe, já foi usada ou expirou: comece de novo")
            .into()
    })
}

// ---------------------------------------------------------------------------
//  Tipos
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct PasskeyInfo {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PasskeyList {
    pub items: Vec<PasskeyInfo>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct WebauthnOptions {
    /// Id da cerimónia, a devolver no passo seguinte.
    pub ceremony_id: Uuid,
    /// `CredentialCreationOptions` / `CredentialRequestOptions` para
    /// `navigator.credentials.create/get` (campos binários em base64url).
    #[schema(value_type = Object)]
    pub options: serde_json::Value,
    pub expires_at: DateTime<Utc>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FinishRegistrationReq {
    pub ceremony_id: Uuid,
    /// Nome para a pessoa reconhecer a chave (1–60).
    pub name: String,
    /// O `PublicKeyCredential` devolvido por `navigator.credentials.create`.
    #[schema(value_type = Object)]
    pub credential: serde_json::Value,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PasskeyChallengeReq {
    /// O `mfa_token` do desafio devolvido por `/api/auth/login`.
    pub mfa_token: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PasskeyLoginReq {
    pub mfa_token: String,
    pub ceremony_id: Uuid,
    /// O `PublicKeyCredential` devolvido por `navigator.credentials.get`.
    #[schema(value_type = Object)]
    pub credential: serde_json::Value,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct TotpStatus {
    pub enabled: bool,
    pub pending: bool,
    pub backup_codes_left: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PasskeysStatus {
    pub count: i64,
    /// `available` | `not_configured` (sem RP ID/origem nesta instalação).
    pub availability: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SecurityOverview {
    /// Há pelo menos um segundo factor (TOTP ou chave de acesso).
    pub two_factor_enabled: bool,
    /// Uma organização activa da pessoa exige 2FA: o último factor não se remove.
    pub required_by_organization: bool,
    pub password: crate::account::PasswordStatus,
    pub totp: TotpStatus,
    pub passkeys: PasskeysStatus,
    /// Reautenticação recente NESTA sessão (para alterar factores).
    pub reauthenticated_until: Option<DateTime<Utc>>,
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        security,
        list,
        get_one,
        begin_registration,
        finish_registration,
        delete,
        login_options,
        login
    ),
    components(schemas(
        PasskeyInfo,
        PasskeyList,
        WebauthnOptions,
        FinishRegistrationReq,
        PasskeyChallengeReq,
        PasskeyLoginReq,
        TotpStatus,
        PasskeysStatus,
        SecurityOverview
    ))
)]
pub struct ApiDoc;

// ---------------------------------------------------------------------------
//  Endpoints da conta
// ---------------------------------------------------------------------------

/// «Segurança»: o estado da password, do TOTP e das chaves de acesso.
#[utoipa::path(
    get, path = "/api/users/me/security", tag = "security",
    security(("session" = [])),
    responses(
        (status = 200, body = SecurityOverview),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn security(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<SecurityOverview>, ApiError> {
    let (enabled_at, pending_row): (Option<DateTime<Utc>>, bool) =
        sqlx::query_as::<_, (Option<DateTime<Utc>>,)>(
            "SELECT enabled_at FROM user_mfa WHERE user_id = $1",
        )
        .bind(auth.user_id)
        .fetch_optional(&state.db)
        .await?
        .map(|(e,)| (e, true))
        .unwrap_or((None, false));
    let backup_codes_left: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_mfa_backup_codes WHERE user_id = $1 AND used_at IS NULL",
    )
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;
    let passkeys = count(&state.db, auth.user_id).await?;
    let odoo = crate::account::is_odoo_managed(&state, auth.user_id).await?;
    let changed_at: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT password_changed_at FROM users WHERE id = $1")
            .bind(auth.user_id)
            .fetch_one(&state.db)
            .await?;
    let reauth_until = auth.reauthenticated_at.and_then(|t| {
        let until = t + chrono::Duration::seconds(
            delonix_meet_domain::identity::session::REAUTH_WINDOW_SECS,
        );
        (until > Utc::now()).then_some(until)
    });
    Ok(Json(SecurityOverview {
        two_factor_enabled: enabled_at.is_some() || passkeys > 0,
        required_by_organization: crate::org::any_org_requires_mfa(&state, auth.user_id).await?,
        password: crate::account::PasswordStatus {
            managed_by_odoo: odoo,
            changed_at: if odoo { None } else { changed_at },
        },
        totp: TotpStatus {
            enabled: enabled_at.is_some(),
            pending: pending_row && enabled_at.is_none(),
            backup_codes_left,
        },
        passkeys: PasskeysStatus {
            count: passkeys,
            availability: if state.webauthn.is_some() {
                "available"
            } else {
                "not_configured"
            }
            .into(),
        },
        reauthenticated_until: reauth_until,
    }))
}

/// As minhas chaves de acesso (no máximo 20 por conta).
#[utoipa::path(
    get, path = "/api/users/me/passkeys", tag = "security",
    security(("session" = [])),
    responses(
        (status = 200, body = PasskeyList),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<PasskeyList>, ApiError> {
    let items = sqlx::query_as(
        "SELECT id, name, created_at, last_used_at FROM user_passkeys
          WHERE user_id = $1 ORDER BY created_at, id LIMIT $2",
    )
    .bind(auth.user_id)
    .bind(MAX_PASSKEYS)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(PasskeyList { items }))
}

/// Uma chave de acesso minha.
#[utoipa::path(
    get, path = "/api/users/me/passkeys/{passkey_id}", tag = "security",
    security(("session" = [])),
    params(("passkey_id" = Uuid, Path)),
    responses(
        (status = 200, body = PasskeyInfo),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "Não existe ou é de outra pessoa (`passkeys.not_found`).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_one(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(passkey_id): Path<Uuid>,
) -> Result<Json<PasskeyInfo>, ApiError> {
    sqlx::query_as(
        "SELECT id, name, created_at, last_used_at FROM user_passkeys WHERE id = $1 AND user_id = $2",
    )
    .bind(passkey_id)
    .bind(auth.user_id)
    .fetch_optional(&state.db)
    .await?
    .map(Json)
    .ok_or_else(|| DomainError::not_found("passkeys.not_found").into())
}

/// Método personalizado: começa o registo de uma chave de acesso. Devolve as
/// opções para `navigator.credentials.create`. Exige reautenticação recente.
#[utoipa::path(
    post, path = "/api/users/me/passkeys/begin-registration", tag = "security",
    security(("session" = [])),
    responses(
        (status = 200, body = WebauthnOptions),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, description = "Sem reautenticação recente (`auth.reauthentication_required`).", body = crate::openapi::ErrorBody),
        (status = 409, description = "Já tem 20 chaves (`passkeys.limit_reached`).", body = crate::openapi::ErrorBody),
        (status = 503, description = "`passkeys.not_configured`.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn begin_registration(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<WebauthnOptions>, ApiError> {
    let wa = webauthn(&state)?;
    crate::sessions::require_recent(&auth)?;
    let existing = load_passkeys(&state.db, auth.user_id).await?;
    if existing.len() as i64 >= MAX_PASSKEYS {
        return Err(DomainError::conflict(
            "passkeys.limit_reached",
            format!("no máximo {MAX_PASSKEYS} chaves de acesso por conta"),
        )
        .into());
    }
    let (email, name): (String, String) =
        sqlx::query_as("SELECT email, COALESCE(display_name, username) FROM users WHERE id = $1")
            .bind(auth.user_id)
            .fetch_one(&state.db)
            .await?;
    let exclude = existing.iter().map(|(_, p)| p.cred_id().clone()).collect();
    let (ccr, reg) = wa
        .start_passkey_registration(auth.user_id, &email, &name, Some(exclude))
        .map_err(ApiError::internal)?;
    let (ceremony_id, expires_at) = save_ceremony(
        &state,
        auth.user_id,
        "registration",
        serde_json::to_value(&reg).map_err(ApiError::internal)?,
    )
    .await?;
    Ok(Json(WebauthnOptions {
        ceremony_id,
        options: serde_json::to_value(&ccr).map_err(ApiError::internal)?,
        expires_at,
    }))
}

/// Conclui o registo com a resposta do autenticador. A cerimónia é consumida
/// (uma tentativa por cerimónia).
#[utoipa::path(
    post, path = "/api/users/me/passkeys", tag = "security",
    security(("session" = [])),
    request_body = FinishRegistrationReq,
    responses(
        (status = 201, body = PasskeyInfo, headers(("Location" = String))),
        (status = 400, description = "Nome inválido (`passkeys.invalid_name`).", body = crate::openapi::ErrorBody),
        (status = 401, description = "Resposta do autenticador recusada (`passkeys.registration_failed`).", body = crate::openapi::ErrorBody),
        (status = 403, description = "`auth.reauthentication_required`.", body = crate::openapi::ErrorBody),
        (status = 404, description = "Cerimónia inexistente, usada, vencida ou de outra pessoa (`passkeys.ceremony_not_found`).", body = crate::openapi::ErrorBody),
        (status = 409, description = "Esta credencial já está registada (`passkeys.already_registered`).", body = crate::openapi::ErrorBody),
        (status = 503, description = "`passkeys.not_configured`.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn finish_registration(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<FinishRegistrationReq>,
) -> Result<Response, ApiError> {
    let wa = webauthn(&state)?;
    crate::sessions::require_recent(&auth)?;
    let name = req.name.trim();
    if name.is_empty() || name.chars().count() > NAME_MAX || name.chars().any(|c| c.is_control()) {
        return Err(DomainError::invalid(
            "passkeys.invalid_name",
            format!("o nome da chave tem de ter 1-{NAME_MAX} caracteres"),
        )
        .into());
    }
    let reg_state: PasskeyRegistration = serde_json::from_value(
        take_ceremony(&state, req.ceremony_id, auth.user_id, "registration").await?,
    )
    .map_err(ApiError::internal)?;
    let credential: RegisterPublicKeyCredential = serde_json::from_value(req.credential)
        .map_err(|e| webauthn_error("passkeys.registration_failed", e))?;
    let passkey = wa
        .finish_passkey_registration(&credential, &reg_state)
        .map_err(|e| webauthn_error("passkeys.registration_failed", e))?;
    let credential_id = serde_json::to_value(passkey.cred_id())
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .ok_or_else(|| ApiError::internal("credential id"))?;
    let info: PasskeyInfo = sqlx::query_as(
        "INSERT INTO user_passkeys (user_id, credential_id, credential, name)
         VALUES ($1, $2, $3, $4)
         RETURNING id, name, created_at, last_used_at",
    )
    .bind(auth.user_id)
    .bind(&credential_id)
    .bind(serde_json::to_value(&passkey).map_err(ApiError::internal)?)
    .bind(name)
    .fetch_one(&state.db)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => DomainError::conflict(
            "passkeys.already_registered",
            "esta chave de acesso já está registada",
        )
        .into(),
        _ => ApiError::from(e),
    })?;
    crate::audit::log(
        &state.db,
        None,
        auth.user_id,
        "security.passkey_added",
        &info.id.to_string(),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        [(
            header::LOCATION,
            format!("/api/users/me/passkeys/{}", info.id),
        )],
        Json(info),
    )
        .into_response())
}

/// Remove uma chave de acesso. Exige reautenticação recente, e recusa remover
/// o último factor quando a organização exige 2FA.
#[utoipa::path(
    delete, path = "/api/users/me/passkeys/{passkey_id}", tag = "security",
    security(("session" = [])),
    params(("passkey_id" = Uuid, Path)),
    responses(
        (status = 204, description = "Removida."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, description = "`auth.reauthentication_required`.", body = crate::openapi::ErrorBody),
        (status = 404, description = "`passkeys.not_found`.", body = crate::openapi::ErrorBody),
        (status = 409, description = "É o último factor e a organização exige 2FA (`security.last_factor_required`).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(passkey_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    crate::sessions::require_recent(&auth)?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM user_passkeys WHERE id = $1 AND user_id = $2)",
    )
    .bind(passkey_id)
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;
    if !exists {
        return Err(DomainError::not_found("passkeys.not_found").into());
    }
    // Com uma só chave, remover ESTA é remover o factor «chave de acesso»;
    // com duas ou mais, o factor continua.
    let current = crate::mfa::factors(&state.db, auth.user_id).await?;
    if current.passkeys == 1 {
        factors::check_removal(
            crate::org::any_org_requires_mfa(&state, auth.user_id).await?,
            current,
            Factor::Passkey,
        )?;
    }
    sqlx::query("DELETE FROM user_passkeys WHERE id = $1 AND user_id = $2")
        .bind(passkey_id)
        .bind(auth.user_id)
        .execute(&state.db)
        .await?;
    crate::audit::log(
        &state.db,
        None,
        auth.user_id,
        "security.passkey_removed",
        &passkey_id.to_string(),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
//  Login: a chave de acesso como segundo factor
// ---------------------------------------------------------------------------

fn challenge_user(state: &AppState, mfa_token: &str) -> Result<Uuid, ApiError> {
    crate::auth::verify_jwt(&state.config.jwt_secret, mfa_token, "mfa")
        .map(|c| c.sub)
        .map_err(|_| ApiError::Unauthorized)
}

/// Segunda metade do login com chave de acesso, passo 1: as opções para
/// `navigator.credentials.get`, restritas às chaves da conta do desafio.
#[utoipa::path(
    post, path = "/api/auth/login/mfa/passkey-options", tag = "auth",
    request_body = PasskeyChallengeReq,
    responses(
        (status = 200, body = WebauthnOptions),
        (status = 401, description = "Desafio de MFA inválido ou expirado.", body = crate::openapi::ErrorBody),
        (status = 422, description = "A conta não tem chaves de acesso (`passkeys.none_registered`).", body = crate::openapi::ErrorBody),
        (status = 429, body = crate::openapi::ErrorBody),
        (status = 503, description = "`passkeys.not_configured`.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn login_options(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PasskeyChallengeReq>,
) -> Result<Json<WebauthnOptions>, ApiError> {
    let wa = webauthn(&state)?;
    let user_id = challenge_user(&state, &req.mfa_token)?;
    let creds: Vec<Passkey> = load_passkeys(&state.db, user_id)
        .await?
        .into_iter()
        .map(|(_, p)| p)
        .collect();
    if creds.is_empty() {
        return Err(DomainError::precondition(
            "passkeys.none_registered",
            "esta conta não tem chaves de acesso: use o código do autenticador",
        )
        .into());
    }
    let (rcr, auth_state) = wa
        .start_passkey_authentication(&creds)
        .map_err(ApiError::internal)?;
    let (ceremony_id, expires_at) = save_ceremony(
        &state,
        user_id,
        "authentication",
        serde_json::to_value(&auth_state).map_err(ApiError::internal)?,
    )
    .await?;
    Ok(Json(WebauthnOptions {
        ceremony_id,
        options: serde_json::to_value(&rcr).map_err(ApiError::internal)?,
        expires_at,
    }))
}

/// Passo 2: verifica a asserção e abre a sessão. O refresh token vai no
/// cabeçalho `Set-Cookie: dlx_refresh=…; HttpOnly; SameSite=Strict; Path=/api/auth`.
#[utoipa::path(
    post, path = "/api/auth/login/mfa/passkey", tag = "auth",
    request_body = PasskeyLoginReq,
    responses(
        (status = 200, description = "Sessão aberta. Define o cookie `dlx_refresh`.", body = crate::auth::AuthOk),
        (status = 401, description = "Desafio inválido, ou asserção recusada (`passkeys.authentication_failed`).", body = crate::openapi::ErrorBody),
        (status = 404, description = "`passkeys.ceremony_not_found`.", body = crate::openapi::ErrorBody),
        (status = 429, description = "Demasiadas tentativas nesta conta.", body = crate::openapi::ErrorBody),
        (status = 503, description = "`passkeys.not_configured`.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn login(
    State(state): State<Arc<AppState>>,
    ctx: crate::sessions::ClientCtx,
    Json(req): Json<PasskeyLoginReq>,
) -> Result<Response, ApiError> {
    let wa = webauthn(&state)?;
    let user_id = challenge_user(&state, &req.mfa_token)?;
    // O mesmo travão por conta do código TOTP.
    if !state.login_limiter.check(&format!("mfa:{user_id}")) {
        return Err(ApiError::TooManyRequests);
    }
    let auth_state: PasskeyAuthentication = serde_json::from_value(
        take_ceremony(&state, req.ceremony_id, user_id, "authentication").await?,
    )
    .map_err(ApiError::internal)?;
    let fail = |e: &dyn std::fmt::Display| webauthn_error("passkeys.authentication_failed", e);
    let credential: PublicKeyCredential =
        serde_json::from_value(req.credential).map_err(|e| fail(&e))?;
    let result = match wa.finish_passkey_authentication(&credential, &auth_state) {
        Ok(r) => r,
        Err(e) => {
            crate::audit::log(&state.db, None, user_id, "auth.passkey_failed", "").await;
            return Err(fail(&e));
        }
    };
    // Contador e «usada por último» da chave que respondeu.
    for (id, mut pk) in load_passkeys(&state.db, user_id).await? {
        if pk.cred_id() == result.cred_id() {
            pk.update_credential(&result);
            sqlx::query(
                "UPDATE user_passkeys SET credential = $3, last_used_at = now() WHERE id = $1 AND user_id = $2",
            )
            .bind(id)
            .bind(user_id)
            .bind(serde_json::to_value(&pk).map_err(ApiError::internal)?)
            .execute(&state.db)
            .await?;
        }
    }
    let user = crate::users::fetch_public(&state.db, user_id).await?;
    crate::audit::log(&state.db, None, user.id, "auth.login_passkey", &user.email).await;
    let pair =
        crate::auth::open_session(&state, user, crate::sessions::AuthMethod::Passkey, &ctx).await?;
    Ok(crate::auth::auth_ok_response(&state, pair))
}
