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
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

// ---------- helpers ----------

pub fn sha256_hex_pub(s: &str) -> String {
    crate::crypto::sha256_hex(s)
}

fn sha256_hex(s: &str) -> String {
    sha256_hex_pub(s)
}

pub fn gen_token_pub() -> String {
    gen_token()
}

fn gen_token() -> String {
    crate::crypto::random_token("dlxo_")
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
            .or_else(|| crate::auth::bearer_token(&parts.headers).map(str::to_string))
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
            // `org_api_keys` NÃO tem coluna de revogação — revogar é apagar a
            // linha (ver apikeys::revoke). Filtrar por `revoked_at IS NULL`
            // rebentava com "column does not exist" (500) e deixava TODO o
            // caminho /api/v1/integration/odoo/* inacessível com chave dlx_.
            sqlx::query_scalar("SELECT org_id FROM org_api_keys WHERE key_hash = $1")
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

    // Activa a integração no mesmo passo: o extractor exige
    // `odoo_enabled = TRUE` para tokens `dlxo_`, portanto rodar a chave numa
    // org ainda desactivada produzia um token que nunca autenticava.
    sqlx::query(
        "UPDATE organizations
         SET odoo_token_hash = $1, odoo_token_prefix = $2, odoo_enabled = TRUE
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
    /// `hr.employee.mobile_phone` / `work_phone`. Aditivos à v1: ausentes não
    /// mexem no número; `false`/`""` apagam o que veio do Odoo; um número
    /// editado à mão no Delonix nunca é sobrescrito (migração 0049).
    #[serde(default)]
    pub mobile_phone: Option<serde_json::Value>,
    #[serde(default)]
    pub work_phone: Option<serde_json::Value>,
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
    /// Entradas do directório que NÃO foram aplicadas, com a razão. Aditivo à
    /// v1: um integrador antigo ignora o campo, e continua a receber `created`
    /// e `updated` com o mesmo significado.
    pub skipped: Vec<SkippedUser>,
    /// Membros sincronizados cujo telefone do Odoo NÃO foi gravado (ex.: número
    /// fora de Angola, que o encaminhamento de SMS não serve). Aditivo à v1.
    pub phones_rejected: Vec<SkippedUser>,
}

#[derive(Serialize)]
pub struct SkippedUser {
    pub email: String,
    pub reason: String,
}

/// `POST /api/v1/integration/odoo/provision`
/// Chamado pelo módulo nk_delonix_meet para provisionar utilizadores.
///
/// Cada entrada passa por `odoo_sso::upsert_member` — a MESMA regra de
/// autoridade que o login por conta Odoo já aplicava (R25): uma conta gerida
/// por outra organização, ou uma conta local, NUNCA é reclamada por uma
/// sincronização de directório.
///
/// Esta função tinha a sua própria cópia do «liga por email», sem essa regra:
/// qualquer org com uma chave `dlx_` listava o endereço de alguém de outra
/// empresa, reescrevia-lhe o nome, marcava a conta como gerida e punha-a na
/// sua org — até como admin. Auditoria 2026-09-16, S2, provado ao vivo antes
/// desta correcção. Duas cópias de uma regra de acesso acabam por divergir; por
/// isso a cópia saiu, em vez de ser remendada.
pub async fn provision(
    State(state): State<Arc<AppState>>,
    odoo: OdooTokenAuth,
    Json(req): Json<ProvisionReq>,
) -> Result<Json<ProvisionResult>, ApiError> {
    let org_id = odoo.org_id;
    let mut created = 0usize;
    let mut updated = 0usize;
    let mut skipped = Vec::new();
    let mut phones_rejected = Vec::new();
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
        let existed: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE email = $1)")
                .bind(&email)
                .fetch_one(&state.db)
                .await?;
        let admin = email == admin_email || u.is_admin;

        let user_id = match crate::odoo_sso::upsert_member(
            &state,
            org_id,
            &email,
            u.name.trim(),
            u.odoo_uid,
            admin,
        )
        .await
        {
            Ok(id) => id,
            // Recusa de AUTORIDADE: a conta não é desta org. Salta a entrada e
            // diz porquê — falhar o lote inteiro deixava o directório legítimo
            // por sincronizar por causa de um único endereço.
            Err(ApiError::Conflict(reason)) => {
                skipped.push(SkippedUser { email, reason });
                continue;
            }
            Err(e) => return Err(e),
        };

        let phone = crate::sms::phone_from_directory(
            &crate::sms::DirectoryField::from_json(u.mobile_phone.as_ref()),
            &crate::sms::DirectoryField::from_json(u.work_phone.as_ref()),
        );
        match phone {
            crate::sms::DirectoryPhone::Untouched => {}
            crate::sms::DirectoryPhone::Set(p) => {
                crate::org::sync_member_phone_from_directory(&state, org_id, user_id, p.as_deref())
                    .await?;
            }
            crate::sms::DirectoryPhone::Rejected(reason) => phones_rejected.push(SkippedUser {
                email: email.clone(),
                reason,
            }),
        }

        if existed {
            // O nome acompanha o Odoo, mas só numa conta que ESTA org gere (o
            // `upsert_member` acabou de o garantir) e sem roubar um nome de
            // utilizador que já é de outra pessoa.
            sqlx::query(
                "UPDATE users SET username = $1
                 WHERE id = $2 AND odoo_org_id = $3 AND username <> $1
                   AND NOT EXISTS (SELECT 1 FROM users WHERE username = $1 AND id <> $2)",
            )
            .bind(u.name.trim())
            .bind(user_id)
            .bind(org_id)
            .execute(&state.db)
            .await?;
            updated += 1;
        } else {
            created += 1;
        }
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
        &format!(
            "created={created} updated={updated} skipped={}",
            skipped.len()
        ),
    )
    .await;

    Ok(Json(ProvisionResult {
        org_id,
        created,
        updated,
        skipped,
        phones_rejected,
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

// ---------- descoberta da config Odoo de um utilizador ----------
//
// NOTA: o cliente de autenticação vive em `odoo_sso.rs`. Havia aqui um
// `odoo_authenticate` que só devolvia o uid; foi substituído porque o login
// precisa da SESSÃO (cookie) para depois reler o directório de utilizadores —
// com só o uid, a sincronização era um evento único em vez de um estado.

/// Devolve (org_id, odoo_url, odoo_db) se o utilizador pertence a uma org
/// com integração Odoo activa e URL/BD configuradas.
pub async fn org_odoo_config(db: &sqlx::PgPool, email: &str) -> Option<(Uuid, String, String)> {
    // A autoridade é a org que GERE a conta (`users.odoo_org_id`), não "uma
    // qualquer org a que o email pertença".
    //
    // A forma anterior juntava por `org_members` com `LIMIT 1` e SEM `ORDER BY`:
    // para quem estivesse em mais do que uma org com Odoo, saía uma arbitrária —
    // e podia sair diferente entre dois logins seguidos. Como é esta função que
    // decide CONTRA QUE ODOO a password é validada, isso era escolher a
    // autoridade de autenticação por sorteio. Com a sincronização de directório
    // a poder reclamar contas por email (fechado em `odoo_sso::upsert_member`),
    // as duas coisas juntas davam tomada de conta.
    //
    // `odoo_org_id` NULL = conta local: não devolve nada, e o `auth::login` cai
    // no caminho da password local, que é o correcto para ela.
    sqlx::query_as::<_, (Uuid, String, String)>(
        "SELECT o.id, o.odoo_url, o.odoo_db
         FROM organizations o
         JOIN users u ON u.odoo_org_id = o.id
         WHERE u.email = $1
           AND o.odoo_enabled = TRUE
           AND o.odoo_url IS NOT NULL
           AND o.odoo_db IS NOT NULL",
    )
    .bind(email)
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
}
