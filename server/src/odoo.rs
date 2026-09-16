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

use delonix_meet_core::crypto::sha256_hex;

/// Token de integração Odoo: `dlxo_` + 256 bits.
fn gen_token() -> String {
    delonix_meet_core::crypto::prefixed_token("dlxo_")
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

        // Só o token de integração `dlxo_`. A chave `dlx_` do inquilino também
        // era aceite aqui (R142): uma credencial emitida para a superfície v1
        // da org abria o provisionamento de directório, que reescreve membros e
        // papéis — um público e um poder que a chave nunca declarou. O módulo
        // `nk_delonix_meet` usa a `dlx_` só em `/api/v1/meetings` e
        // `/api/v1/admin/orgs`; estas rotas são do token `dlxo_`.
        if !raw.starts_with("dlxo_") {
            return Err(ApiError::Unauthorized);
        }
        let hash = sha256_hex(&raw);
        let org_id: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM organizations
             WHERE odoo_token_hash = $1 AND odoo_enabled = TRUE",
        )
        .bind(&hash)
        .fetch_optional(&state.db)
        .await?;

        let org_id = org_id.ok_or(ApiError::Unauthorized)?;
        Ok(OdooTokenAuth { org_id })
    }
}

// ---------- documentação OpenAPI ----------

/// Rotas da BFF deste módulo: configuração da integração (admin) e as
/// configurações públicas da página de login.
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(get_config, save_config, rotate_token, public_settings),
    components(schemas(
        OdooConfig,
        OdooConfigReq,
        OdooTokenResp,
        PublicSettings,
        PublicCapabilities
    ))
)]
pub struct ApiDoc;

/// Rotas da API pública v1 deste módulo, usadas pelo módulo Odoo
/// `nk_delonix_meet`.
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(provision, list_users),
    components(schemas(
        ProvisionReq,
        OdooUserEntry,
        ProvisionResult,
        SkippedUser,
        OdooDirectoryUser
    ))
)]
pub struct V1ApiDoc;

// ---------- DTO da configuração ----------

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct OdooConfig {
    pub org_id: Uuid,
    pub odoo_enabled: bool,
    pub odoo_url: Option<String>,
    pub odoo_db: Option<String>,
    /// Primeiros 12 caracteres do token (`dlxo_…`); o token nunca volta a sair.
    pub odoo_token_prefix: Option<String>,
    pub odoo_admin_id: Option<Uuid>,
    pub odoo_synced_at: Option<DateTime<Utc>>,
    pub hide_org_creation: bool,
    pub hide_sso_button: bool,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct OdooConfigReq {
    pub odoo_enabled: bool,
    /// Guardado sem `/` finais. Não é validado.
    pub odoo_url: Option<String>,
    pub odoo_db: Option<String>,
    pub hide_org_creation: bool,
    pub hide_sso_button: bool,
}

// ---------- handlers BFF (sessão admin) ----------

/// `GET /api/orgs/{org_id}/integration/odoo`
///
/// Configuração da integração Odoo da organização (admin).
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/integration/odoo", tag = "odoo",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses(
        (status = 200, body = OdooConfig),
        (status = 401, description = "Sessão inválida OU membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "Não é membro da organização.", body = crate::openapi::ErrorBody),
    )
)]
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
///
/// Grava a configuração (admin). Na primeira gravação o autenticado fica
/// registado como `odoo_admin_id`.
#[utoipa::path(
    put, path = "/api/orgs/{org_id}/integration/odoo", tag = "odoo",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = OdooConfigReq,
    responses(
        (status = 200, description = "`{\"ok\": true}` (forma herdada)"),
        (status = 401, description = "Sessão inválida OU membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "Não é membro da organização.", body = crate::openapi::ErrorBody),
    )
)]
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

/// Token de integração acabado de gerar. É a única vez que sai em claro.
#[derive(Serialize, utoipa::ToSchema)]
pub struct OdooTokenResp {
    /// `dlxo_<hex>`, para colar no módulo `nk_delonix_meet`.
    pub token: String,
    /// Primeiros 12 caracteres, o que fica visível na configuração.
    pub prefix: String,
}

/// `POST /api/orgs/{org_id}/integration/odoo/token` — gera/rota token
///
/// Invalida o token anterior e ACTIVA a integração (`odoo_enabled = true`).
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/integration/odoo/token", tag = "odoo",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses(
        (status = 200, body = OdooTokenResp),
        (status = 401, description = "Sessão inválida OU membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "Não é membro da organização.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn rotate_token(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<OdooTokenResp>, ApiError> {
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
    Ok(Json(OdooTokenResp { token, prefix }))
}

// ---------- provisioning (Odoo → Delonix Meet) ----------

#[derive(Deserialize, utoipa::ToSchema)]
pub struct OdooUserEntry {
    pub odoo_uid: i32,
    pub name: String,
    pub email: String,
    #[serde(default)]
    pub is_admin: bool,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ProvisionReq {
    /// Nome da empresa Odoo (actualiza o nome da org se a org ainda tem o nome
    /// padrão ou se force_name=true).
    pub company: String,
    /// Email do utilizador que fez a integração — será admin da org.
    pub admin_email: String,
    #[serde(default)]
    pub users: Vec<OdooUserEntry>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ProvisionResult {
    pub org_id: Uuid,
    pub created: usize,
    pub updated: usize,
    /// Entradas do directório que NÃO foram aplicadas, com a razão. Aditivo à
    /// v1: um integrador antigo ignora o campo, e continua a receber `created`
    /// e `updated` com o mesmo significado.
    pub skipped: Vec<SkippedUser>,
}

#[derive(Serialize, utoipa::ToSchema)]
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
#[utoipa::path(
    post, path = "/api/v1/integration/odoo/provision", tag = "odoo",
    description = "Provisiona o directório de utilizadores do Odoo na organização do token.\n\n\
Autentica SÓ pelo token de integração `dlxo_` (a chave `dlx_` da organização recebe `401`), \
em `Authorization: Bearer …` ou `X-Integration-Token: …`. Uma entrada cuja conta pertence a \
outra organização ou é local não é aplicada e vem em `skipped`.",
    security(("api_key" = [])),
    request_body = ProvisionReq,
    responses(
        (status = 200, body = ProvisionResult),
        (status = 401, description = "Token em falta, desconhecido, integração desactivada, ou chave `dlx_` (não é o token desta rota).", body = crate::openapi::ErrorBody),
        (status = 429, description = "Rate-limit da v1 por IP.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn provision(
    State(state): State<Arc<AppState>>,
    odoo: OdooTokenAuth,
    Json(req): Json<ProvisionReq>,
) -> Result<Json<ProvisionResult>, ApiError> {
    let org_id = odoo.org_id;
    let mut created = 0usize;
    let mut updated = 0usize;
    let mut skipped = Vec::new();
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
    }))
}

/// Membro da organização, como o Odoo o vê.
#[derive(Serialize, utoipa::ToSchema)]
pub struct OdooDirectoryUser {
    pub id: Uuid,
    pub email: String,
    pub username: String,
    /// `null` se a conta não veio do Odoo.
    pub odoo_uid: Option<i32>,
    /// Papel na organização (`admin` | `member`).
    pub role: String,
}

/// `GET /api/v1/integration/odoo/users` — lista utilizadores para o Odoo
#[utoipa::path(
    get, path = "/api/v1/integration/odoo/users", tag = "odoo",
    description = "Membros ACTIVOS da organização do token (os arquivados não saem), por `username`.\n\n\
Autentica SÓ pelo token de integração `dlxo_` (a chave `dlx_` da organização recebe `401`), \
em `Authorization: Bearer …` ou `X-Integration-Token: …`.",
    security(("api_key" = [])),
    responses(
        (status = 200, body = Vec<OdooDirectoryUser>),
        (status = 401, description = "Token em falta, desconhecido, integração desactivada, ou chave `dlx_` (não é o token desta rota).", body = crate::openapi::ErrorBody),
        (status = 429, description = "Rate-limit da v1 por IP.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_users(
    State(state): State<Arc<AppState>>,
    odoo: OdooTokenAuth,
) -> Result<Json<Vec<OdooDirectoryUser>>, ApiError> {
    // Só membros ACTIVOS (R143, a S3 do R121 aplicada ao directório): quem
    // saiu da empresa não volta ao Odoo como membro. O filtro entra na query
    // que já existia — um helper em `org.rs` com email/username/odoo_uid é o
    // destino (ADR-0004 §6 passo 3), não uma segunda cópia aqui.
    let rows = sqlx::query_as::<_, (Uuid, String, String, Option<i32>, String)>(
        "SELECT u.id, u.email, u.username, u.odoo_uid, m.role
         FROM users u
         JOIN org_members m ON m.user_id = u.id
         WHERE m.org_id = $1 AND m.archived_at IS NULL
         ORDER BY u.username",
    )
    .bind(odoo.org_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(
        rows.into_iter()
            .map(|(id, email, username, odoo_uid, role)| OdooDirectoryUser {
                id,
                email,
                username,
                odoo_uid,
                role,
            })
            .collect(),
    ))
}

// ---------- configurações públicas da plataforma ----------

/// Configurações públicas da instalação.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PublicSettings {
    /// Esconder «criar organização»: alguma org Odoo o pediu, ou tenancy `single`.
    pub hide_org_creation: bool,
    pub hide_sso_button: bool,
    /// `saas` | `enterprise` | `personal`.
    #[schema(value_type = String)]
    pub edition: delonix_meet_core::edition::Edition,
    /// `open` | `domain` | `invite` | `closed`.
    #[schema(value_type = String)]
    pub registration_mode: delonix_meet_core::edition::RegistrationMode,
    /// `registration_mode` é `open` ou `domain`.
    pub registration_open: bool,
    /// `multi` | `single`.
    #[schema(value_type = String)]
    pub tenancy_mode: delonix_meet_core::edition::TenancyMode,
    pub capabilities: PublicCapabilities,
}

/// O que ESTA instalação faz — cada flag lê a configuração que a liga.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PublicCapabilities {
    pub odoo_login: bool,
    pub ai: bool,
    pub pstn_dial_in: bool,
    pub livestream: bool,
    pub multi_organization: bool,
    pub operator_surface: bool,
}

/// `GET /api/public/settings` — sem autenticação; usado na página de login.
/// Agrega flags de todas as orgs com integração Odoo activa.
#[utoipa::path(
    get, path = "/api/public/settings", tag = "odoo",
    responses(
        (status = 200, body = PublicSettings),
    )
)]
pub async fn public_settings(
    State(state): State<Arc<AppState>>,
) -> Result<Json<PublicSettings>, ApiError> {
    let row: Option<(Option<bool>, Option<bool>)> = sqlx::query_as(
        "SELECT BOOL_OR(hide_org_creation), BOOL_OR(hide_sso_button)
         FROM organizations WHERE odoo_enabled = TRUE",
    )
    .fetch_optional(&state.db)
    .await?;

    let (hide_org, hide_sso) = row
        .map(|(a, b)| (a.unwrap_or(false), b.unwrap_or(false)))
        .unwrap_or((false, false));

    use delonix_meet_core::edition::{RegistrationMode, TenancyMode};
    let c = &state.config;
    let registration_open = matches!(
        c.registration_mode,
        RegistrationMode::Open | RegistrationMode::Domain
    );
    // `capabilities` diz ao frontend o que ESTA instalação faz, para não
    // mostrar um botão para uma capacidade desligada. Cada linha lê a
    // configuração que liga a capacidade — nenhuma é afirmada sem código por
    // trás (check-capability-claims.sh).
    Ok(Json(PublicSettings {
        hide_org_creation: hide_org || c.tenancy_mode == TenancyMode::Single,
        hide_sso_button: hide_sso,
        edition: c.edition,
        registration_mode: c.registration_mode,
        registration_open,
        tenancy_mode: c.tenancy_mode,
        capabilities: PublicCapabilities {
            odoo_login: c.platform_odoo_url.is_some() && c.platform_odoo_db.is_some(),
            ai: c.ollama_url.is_some(),
            pstn_dial_in: !c.voice_internal_secret.is_empty(),
            livestream: c.max_directos > 0,
            multi_organization: c.tenancy_mode == TenancyMode::Multi,
            operator_surface: c.edition.operator_surface(),
        },
    }))
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
