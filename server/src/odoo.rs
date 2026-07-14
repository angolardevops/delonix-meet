//! Integração Odoo ↔ Delonix Meet (módulo nk_delonix_meet).
//!
//! Fluxo de activação:
//!   1. Admin da org activa a integração no painel e configura URL + BD Odoo.
//!   2. Gera um token de integração (`dlxo_<hex>`) e copia-o para o módulo
//!      nk_delonix_meet no Odoo.
//!   3. O módulo Odoo provisiona utilizadores via
//!      `POST /api/v1/integration/odoo/provision` (token no header).
//!   4. No login, o Delonix Meet valida a senha contra o Odoo (modo online)
//!      ou usa o hash Argon2 em cache (modo offline). Uma alteração de senha
//!      no Odoo sincroniza automaticamente no próximo login online.

use axum::{
    extract::{FromRequestParts, Path, State},
    http::request::Parts,
    Json,
};
use chrono::{DateTime, Utc};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

// ---------- helpers ----------

pub fn sha256_hex_pub(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

fn sha256_hex(s: &str) -> String {
    sha256_hex_pub(s)
}

pub fn gen_token_pub() -> String {
    gen_token()
}

fn gen_token() -> String {
    let mut b = [0u8; 32];
    OsRng.fill_bytes(&mut b);
    format!("dlxo_{}", hex::encode(b))
}

// ---------- extractor — token de integração Odoo ----------

/// Extractor para endpoints `/api/v1/integration/odoo/*`.
/// Autentica pelo header `Authorization: Bearer dlxo_...`
/// ou `X-Integration-Token: dlxo_...`.
pub struct OdooTokenAuth {
    pub org_id: Uuid,
}

impl FromRequestParts<Arc<AppState>> for OdooTokenAuth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, ApiError> {
        let raw = parts
            .headers
            .get("x-integration-token")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .or_else(|| {
                parts
                    .headers
                    .get(axum::http::header::AUTHORIZATION)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|h| h.strip_prefix("Bearer "))
                    .map(|s| s.to_string())
            })
            .ok_or(ApiError::Unauthorized)?;

        let hash = sha256_hex(&raw);

        // Aceitar dlxo_ (token de integração Odoo) OU dlx_ (API key da org).
        // O fluxo de auto-provisão via /admin/orgs gera uma dlx_ key que o
        // módulo nk_delonix_meet usa diretamente sem passo extra de token.
        let org_id: Option<Uuid> = if raw.starts_with("dlxo_") {
            sqlx::query_scalar(
                "SELECT id FROM organizations
                 WHERE odoo_token_hash = $1 AND odoo_enabled = TRUE",
            )
            .bind(&hash)
            .fetch_optional(&state.db)
            .await?
        } else if raw.starts_with("dlx_") {
            sqlx::query_scalar(
                "SELECT org_id FROM org_api_keys
                 WHERE key_hash = $1 AND revoked_at IS NULL",
            )
            .bind(&hash)
            .fetch_optional(&state.db)
            .await?
        } else {
            return Err(ApiError::Unauthorized);
        };

        let org_id = org_id.ok_or(ApiError::Unauthorized)?;
        Ok(OdooTokenAuth { org_id })
    }
}

// ---------- DTO da configuração ----------

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct OdooConfig {
    pub org_id: Uuid,
    pub odoo_enabled: bool,
    pub odoo_url: Option<String>,
    pub odoo_db: Option<String>,
    pub odoo_token_prefix: Option<String>,
    pub odoo_admin_id: Option<Uuid>,
    pub odoo_synced_at: Option<DateTime<Utc>>,
    pub hide_org_creation: bool,
    pub hide_sso_button: bool,
}

#[derive(Deserialize)]
pub struct OdooConfigReq {
    pub odoo_enabled: bool,
    pub odoo_url: Option<String>,
    pub odoo_db: Option<String>,
    pub hide_org_creation: bool,
    pub hide_sso_button: bool,
}

// ---------- handlers BFF (sessão admin) ----------

/// `GET /api/orgs/{org_id}/integration/odoo`
pub async fn get_config(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<OdooConfig>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let cfg: OdooConfig = sqlx::query_as(
        "SELECT id AS org_id, odoo_enabled, odoo_url, odoo_db, odoo_token_prefix,
                odoo_admin_id, odoo_synced_at, hide_org_creation, hide_sso_button
         FROM organizations WHERE id = $1",
    )
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(cfg))
}

/// `PUT /api/orgs/{org_id}/integration/odoo`
pub async fn save_config(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<OdooConfigReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;

    let url = req
        .odoo_url
        .as_deref()
        .map(|u| u.trim_end_matches('/').to_string());

    sqlx::query(
        "UPDATE organizations
         SET odoo_enabled = $1, odoo_url = $2, odoo_db = $3,
             hide_org_creation = $4, hide_sso_button = $5,
             odoo_admin_id = COALESCE(odoo_admin_id, $6)
         WHERE id = $7",
    )
    .bind(req.odoo_enabled)
    .bind(url)
    .bind(req.odoo_db)
    .bind(req.hide_org_creation)
    .bind(req.hide_sso_button)
    .bind(auth.user_id)
    .bind(org_id)
    .execute(&state.db)
    .await?;

    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "odoo.config_saved",
        "",
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `POST /api/orgs/{org_id}/integration/odoo/token` — gera/rota token
pub async fn rotate_token(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;

    let token = gen_token();
    let prefix = token[..12].to_string(); // "dlxo_XXXXXX"
    let hash = sha256_hex(&token);

    sqlx::query(
        "UPDATE organizations
         SET odoo_token_hash = $1, odoo_token_prefix = $2
         WHERE id = $3",
    )
    .bind(&hash)
    .bind(&prefix)
    .bind(org_id)
    .execute(&state.db)
    .await?;

    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "odoo.token_rotated",
        "",
    )
    .await;
    Ok(Json(
        serde_json::json!({ "token": token, "prefix": prefix }),
    ))
}

// ---------- provisioning (Odoo → Delonix Meet) ----------

#[derive(Deserialize)]
pub struct OdooUserEntry {
    pub odoo_uid: i32,
    pub name: String,
    pub email: String,
    #[serde(default)]
    pub is_admin: bool,
}

#[derive(Deserialize)]
pub struct ProvisionReq {
    /// Nome da empresa Odoo (actualiza o nome da org se a org ainda tem o nome
    /// padrão ou se force_name=true).
    pub company: String,
    /// Email do utilizador que fez a integração — será admin da org.
    pub admin_email: String,
    #[serde(default)]
    pub users: Vec<OdooUserEntry>,
}

#[derive(Serialize)]
pub struct ProvisionResult {
    pub org_id: Uuid,
    pub created: usize,
    pub updated: usize,
}

/// `POST /api/v1/integration/odoo/provision`
/// Chamado pelo módulo nk_delonix_meet para provisionar utilizadores.
pub async fn provision(
    State(state): State<Arc<AppState>>,
    odoo: OdooTokenAuth,
    Json(req): Json<ProvisionReq>,
) -> Result<Json<ProvisionResult>, ApiError> {
    let org_id = odoo.org_id;
    let mut created = 0usize;
    let mut updated = 0usize;
    let admin_email = req.admin_email.trim().to_lowercase();

    // Actualizar nome da org para o nome da empresa Odoo
    if !req.company.trim().is_empty() {
        let _ = sqlx::query("UPDATE organizations SET name = $1 WHERE id = $2 AND name != $1")
            .bind(req.company.trim())
            .bind(org_id)
            .execute(&state.db)
            .await;
    }

    for u in &req.users {
        let email = u.email.trim().to_lowercase();
        let existing: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM users WHERE email = $1")
            .bind(&email)
            .fetch_optional(&state.db)
            .await?;

        let user_id = if let Some((uid,)) = existing {
            sqlx::query(
                "UPDATE users SET username = $1, odoo_uid = $2, odoo_managed = TRUE
                 WHERE id = $3",
            )
            .bind(u.name.trim())
            .bind(u.odoo_uid)
            .bind(uid)
            .execute(&state.db)
            .await?;
            updated += 1;
            uid
        } else {
            // Utilizador criado sem password — requer login online via Odoo
            // para obter o hash em cache antes de poder usar modo offline.
            let (uid,): (Uuid,) = sqlx::query_as(
                "INSERT INTO users (email, username, password_hash, odoo_uid, odoo_managed)
                 VALUES ($1, $2, '', $3, TRUE) RETURNING id",
            )
            .bind(&email)
            .bind(u.name.trim())
            .bind(u.odoo_uid)
            .fetch_one(&state.db)
            .await
            .map_err(|e| match &e {
                sqlx::Error::Database(db) if db.is_unique_violation() => {
                    ApiError::Conflict(format!("email {email} já em uso"))
                }
                _ => e.into(),
            })?;
            created += 1;
            uid
        };

        // O integrador e qualquer is_admin=true ficam como admin na org
        let role = if email == admin_email || u.is_admin {
            "admin"
        } else {
            "member"
        };
        sqlx::query(
            "INSERT INTO org_members (org_id, user_id, role)
             VALUES ($1, $2, $3)
             ON CONFLICT (org_id, user_id) DO UPDATE SET role = EXCLUDED.role",
        )
        .bind(org_id)
        .bind(user_id)
        .bind(role)
        .execute(&state.db)
        .await?;
    }

    sqlx::query("UPDATE organizations SET odoo_synced_at = now() WHERE id = $1")
        .bind(org_id)
        .execute(&state.db)
        .await?;

    crate::audit::log(
        &state.db,
        Some(org_id),
        Uuid::nil(),
        "odoo.provision",
        &format!("created={created} updated={updated}"),
    )
    .await;

    Ok(Json(ProvisionResult {
        org_id,
        created,
        updated,
    }))
}

/// `GET /api/v1/integration/odoo/users` — lista utilizadores para o Odoo
pub async fn list_users(
    State(state): State<Arc<AppState>>,
    odoo: OdooTokenAuth,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    let rows = sqlx::query_as::<_, (Uuid, String, String, Option<i32>, String)>(
        "SELECT u.id, u.email, u.username, u.odoo_uid, m.role
         FROM users u
         JOIN org_members m ON m.user_id = u.id
         WHERE m.org_id = $1
         ORDER BY u.username",
    )
    .bind(odoo.org_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(
        rows.into_iter()
            .map(|(id, email, username, odoo_uid, role)| {
                serde_json::json!({
                    "id": id,
                    "email": email,
                    "username": username,
                    "odoo_uid": odoo_uid,
                    "role": role,
                })
            })
            .collect(),
    ))
}

// ---------- configurações públicas da plataforma ----------

/// `GET /api/public/settings` — sem autenticação; usado na página de login.
/// Agrega flags de todas as orgs com integração Odoo activa.
pub async fn public_settings(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let row: Option<(Option<bool>, Option<bool>)> = sqlx::query_as(
        "SELECT BOOL_OR(hide_org_creation), BOOL_OR(hide_sso_button)
         FROM organizations WHERE odoo_enabled = TRUE",
    )
    .fetch_optional(&state.db)
    .await?;

    let (hide_org, hide_sso) = row
        .map(|(a, b)| (a.unwrap_or(false), b.unwrap_or(false)))
        .unwrap_or((false, false));

    Ok(Json(serde_json::json!({
        "hide_org_creation": hide_org,
        "hide_sso_button": hide_sso,
    })))
}

// ---------- validação de credenciais Odoo (usada em auth::login) ----------

pub enum OdooAuthResult {
    /// Autenticação válida; uid do utilizador no Odoo.
    Ok(i32),
    /// Credenciais inválidas (Odoo respondeu uid=false).
    InvalidCredentials,
    /// Odoo inacessível — usar hash local (modo offline).
    Offline,
}

/// Valida email/password contra o endpoint JSON-RPC do Odoo.
/// Timeout de 4 s para não bloquear logins quando o Odoo está lento.
pub async fn odoo_authenticate(
    client: &reqwest::Client,
    odoo_url: &str,
    odoo_db: &str,
    email: &str,
    password: &str,
) -> OdooAuthResult {
    let url = format!("{odoo_url}/web/session/authenticate");
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "call",
        "id": 1,
        "params": {
            "db": odoo_db,
            "login": email,
            "password": password
        }
    });

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(4),
        client.post(&url).json(&body).send(),
    )
    .await;

    match result {
        Err(_) | Ok(Err(_)) => OdooAuthResult::Offline,
        Ok(Ok(resp)) => match resp.json::<serde_json::Value>().await {
            Err(_) => OdooAuthResult::Offline,
            Ok(json) => match json["result"]["uid"].as_i64() {
                Some(uid) if uid > 0 => OdooAuthResult::Ok(uid as i32),
                _ => OdooAuthResult::InvalidCredentials,
            },
        },
    }
}

/// Devolve (org_id, odoo_url, odoo_db) se o utilizador pertence a uma org
/// com integração Odoo activa e URL/BD configuradas.
pub async fn org_odoo_config(db: &sqlx::PgPool, email: &str) -> Option<(Uuid, String, String)> {
    sqlx::query_as::<_, (Uuid, String, String)>(
        "SELECT o.id, o.odoo_url, o.odoo_db
         FROM organizations o
         JOIN org_members m ON m.org_id = o.id
         JOIN users u ON u.id = m.user_id
         WHERE u.email = $1
           AND o.odoo_enabled = TRUE
           AND o.odoo_url IS NOT NULL
           AND o.odoo_db IS NOT NULL
         LIMIT 1",
    )
    .bind(email)
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
}
