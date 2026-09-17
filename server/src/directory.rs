//! Utilizadores e convites (ADR-0008 §7 e §11) — adaptador HTTP + Postgres do
//! ecrã «Utilizadores e convites». Reutiliza os membros (`org_members`): as
//! leituras e escritas da pertença estão em `org.rs`.
//!
//! - `GET    /api/orgs/{org_id}/users`                         directório (membros + convites pendentes)
//! - `GET    /api/search/schemas/users`                        descrição da pesquisa (contrato de `pesquisa.md`)
//! - `POST   /api/orgs/{org_id}/users/bulk-actions`            acções em massa, resultado por item
//! - `POST   /api/orgs/{org_id}/users/imports`                 importar CSV (idempotente)
//! - `GET|POST /api/orgs/{org_id}/departments`, `GET|PATCH|DELETE …/{department_id}`
//! - `GET|POST /api/orgs/{org_id}/invitations`, `GET|DELETE …/{invitation_id}`, `POST …/{invitation_id}/resend`
//! - `POST   /api/invitations/accept`                          aceitar (o token é a credencial)
//! - `GET    /api/orgs/{org_id}/seats`, `POST /api/orgs/{org_id}/seats/release`
//! - `PUT    /api/operator/v1/organizations/{org_id}/seats`    tecto (operador)
//! - `GET    /api/orgs/{org_id}/provisioning`                  estado do aprovisionamento Odoo
//! - `GET|PUT /api/orgs/{org_id}/entry-rules`                  regras de entrada

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Duration, Utc};
use delonix_meet_core::{page::PageRequest, DomainError};
use delonix_meet_domain::identity::authorization::{
    Capability, Decision, ResourceScope, SystemRole,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::ApiError,
    org::{DirectoryCursor, DirectoryEntry, DirectoryFilter, DirectoryOrder},
    AppState,
};

// ---------------------------------------------------------------------------
//  Âmbito de quem gere contas
// ---------------------------------------------------------------------------

/// Quem gere contas em toda a org, ou só no seu departamento (papel de
/// departamento). `Some(d)` = restrito ao departamento `d`.
async fn accounts_scope(
    state: &AppState,
    org_id: Uuid,
    actor: Uuid,
) -> Result<Option<Uuid>, ApiError> {
    match crate::org::decide(
        state,
        org_id,
        actor,
        Capability::AdminManageAccounts,
        ResourceScope::Organization,
    )
    .await?
    {
        None => Err(ApiError::NotFound),
        Some((_, Decision::Allow)) => Ok(None),
        Some((grant, _)) => {
            if let Some(d) = grant.department_id {
                if let Some((_, Decision::Allow)) = crate::org::decide(
                    state,
                    org_id,
                    actor,
                    Capability::AdminManageAccounts,
                    ResourceScope::Department { department_id: d },
                )
                .await?
                {
                    return Ok(Some(d));
                }
            }
            // Sem nenhum dos dois: o caminho normal (403 ou pedido de aprovação).
            crate::org::require_capability(
                state,
                org_id,
                actor,
                Capability::AdminManageAccounts,
                ResourceScope::Organization,
            )
            .await?;
            Ok(None)
        }
    }
}

fn outside_scope() -> ApiError {
    DomainError::forbidden("authz.missing_capability")
        .with_message("fora do departamento que gere")
        .with_field("capability", Capability::AdminManageAccounts.as_str())
        .into()
}

// ---------------------------------------------------------------------------
//  Directório
// ---------------------------------------------------------------------------

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct UsersQuery {
    /// Nome, correio, papel ou departamento (termos em E, sem maiúsculas).
    pub q: Option<String>,
    /// Filtros pré-definidos separados por vírgulas (ver `GET /api/search/schemas/users`).
    pub filters: Option<String>,
    /// O domínio JSON da pesquisa profunda — ainda não ligado nesta linha (`search.filter_unsupported`).
    pub filter: Option<String>,
    /// `department` | `role` | `status` | `origin`.
    pub group_by: Option<String>,
    /// `name` (omissão) | `-name` | `-last_access_at` | `last_access_at`.
    pub order_by: Option<String>,
    pub department_id: Option<Uuid>,
    pub role_id: Option<Uuid>,
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DirectoryGroup {
    pub key: Option<String>,
    pub label: Option<String>,
    pub count: i64,
    /// Parâmetros a juntar para abrir o grupo (`department_id`, `role_id`, ou um filtro).
    #[schema(value_type = Object)]
    pub open: serde_json::Value,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DirectoryCounts {
    pub active: i64,
    pub invited: i64,
    pub suspended: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DirectoryPage {
    pub items: Vec<DirectoryEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
    /// Total do conjunto filtrado (exacto).
    pub total: i64,
    pub total_kind: &'static str,
    /// Contagens da org (ou do departamento de quem pede, se só gere esse).
    pub counts: DirectoryCounts,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<DirectoryGroup>>,
    /// `null` = toda a org; um id = só o departamento que quem pede gere.
    pub restricted_to_department: Option<Uuid>,
}

/// Filtros pré-definidos: (nome, rótulo, grupo).
const NAMED_FILTERS: &[(&str, &str, &str)] = &[
    ("active", "Activos", "status"),
    ("invited", "Convidados", "status"),
    ("suspended", "Suspensos", "status"),
    ("inactive_30_days", "Sem entrar há 30 dias", "activity"),
    ("inactive_60_days", "Sem entrar há 60 dias", "activity"),
    ("inactive_90_days", "Sem entrar há 90 dias", "activity"),
    ("never_signed_in", "Nunca entraram", "activity"),
    ("origin_odoo_sso", "Odoo SSO", "origin"),
    ("origin_invitation", "Convite", "origin"),
    ("origin_code", "Código", "origin"),
    ("origin_manual", "Criados na consola", "origin"),
    ("external", "Convidados externos", "kind"),
    ("without_department", "Sem departamento", "department"),
];

fn search_err(code: &'static str, msg: &str, field: &str) -> ApiError {
    DomainError::invalid(code, msg)
        .with_field(field, msg)
        .into()
}

fn build_filter(q: &UsersQuery) -> Result<DirectoryFilter, ApiError> {
    if q.filter.as_deref().is_some_and(|f| !f.trim().is_empty()) {
        return Err(search_err(
            "search.filter_unsupported",
            "o domínio `filter` chega com o motor de pesquisa (ADR-0007); use `filters` e `q`",
            "filter",
        ));
    }
    let mut f = DirectoryFilter::default();
    if let Some(text) = q.q.as_deref() {
        let text = text.trim();
        if text.chars().count() > 200 {
            return Err(search_err(
                "search.invalid_query",
                "q tem no máximo 200 caracteres",
                "q",
            ));
        }
        f.terms = text
            .split_whitespace()
            .take(8)
            .map(|t| t.to_lowercase())
            .collect();
    }
    let mut inactive: Vec<i32> = Vec::new();
    for name in q
        .filters
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        match name {
            "active" | "invited" | "suspended" => f.statuses.push(name.into()),
            "inactive_30_days" => inactive.push(30),
            "inactive_60_days" => inactive.push(60),
            "inactive_90_days" => inactive.push(90),
            "never_signed_in" => f.never_signed_in = true,
            "origin_odoo_sso" => f.origins.push("odoo_sso".into()),
            "origin_invitation" => f.origins.push("invitation".into()),
            "origin_code" => f.origins.push("code".into()),
            "origin_manual" => f.origins.extend([
                "manual".into(),
                "registration".into(),
                "sso".into(),
                "api".into(),
            ]),
            "external" => f.external_only = true,
            "without_department" => f.without_department = true,
            _ => {
                return Err(search_err(
                    "search.unknown_filter",
                    &format!("filtro desconhecido: {name}"),
                    "filters",
                ))
            }
        }
    }
    // Filtros do mesmo grupo juntam-se com OU: o menor número de dias é o mais largo.
    f.inactive_days = inactive.into_iter().min();
    f.department_id = q.department_id;
    f.role_id = q.role_id;
    Ok(f)
}

fn order_of(raw: Option<&str>) -> Result<DirectoryOrder, ApiError> {
    Ok(match raw.unwrap_or("name") {
        "name" => DirectoryOrder::NameAsc,
        "-name" => DirectoryOrder::NameDesc,
        "-last_access_at" => DirectoryOrder::LastAccessDesc,
        "last_access_at" => DirectoryOrder::LastAccessAsc,
        other => {
            return Err(search_err(
                "search.invalid_order_by",
                &format!("ordem desconhecida: {other}"),
                "order_by",
            ))
        }
    })
}

fn fingerprint(q: &UsersQuery) -> String {
    delonix_meet_core::crypto::sha256_hex(format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}",
        q.q, q.filters, q.order_by, q.department_id, q.role_id
    ))[..16]
        .to_string()
}

/// Directório de pessoas da organização: membros (activos e suspensos) e
/// convites pendentes, com pesquisa, filtros pré-definidos e agrupamento.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/users", tag = "directory",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), UsersQuery),
    responses(
        (status = 200, body = DirectoryPage),
        (status = 400, body = crate::openapi::ErrorBody, description = "`search.unknown_filter`, `search.invalid_group_by`, `search.invalid_order_by`, `search.invalid_query`, `search.filter_unsupported`, `search.page_token_mismatch`, `page.invalid_token`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "sem `admin.manage_accounts`"),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_users(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Query(q): Query<UsersQuery>,
) -> Result<Json<DirectoryPage>, ApiError> {
    let restricted = accounts_scope(&state, org_id, auth.user_id).await?;
    let mut f = build_filter(&q)?;
    if let Some(d) = restricted {
        if f.department_id.is_some_and(|x| x != d) {
            return Err(outside_scope());
        }
        f.department_id = Some(d);
    }
    let order = order_of(q.order_by.as_deref())?;
    let group_field = match q.group_by.as_deref() {
        None | Some("") => None,
        Some(g @ ("department" | "role" | "status" | "origin")) => Some(g),
        Some(other) => {
            return Err(search_err(
                "search.invalid_group_by",
                &format!("agrupamento desconhecido: {other}"),
                "group_by",
            ))
        }
    };
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token.clone(),
    };
    let size = page.size();
    let fp = fingerprint(&q);
    let cursor: Option<DirectoryCursor> = page.cursor()?;
    if cursor.as_ref().is_some_and(|c| c.fp != fp) {
        return Err(search_err(
            "search.page_token_mismatch",
            "o page_token é de outra pesquisa",
            "page_token",
        ));
    }
    let mut rows = crate::org::directory_page(
        &state.db,
        org_id,
        &f,
        order,
        cursor.as_ref(),
        size as i64 + 1,
    )
    .await?;
    let next_page_token = if rows.len() > size as usize {
        rows.truncate(size as usize);
        rows.last().map(|last| {
            delonix_meet_core::page::encode_cursor(&DirectoryCursor {
                text: last.name.to_lowercase(),
                at: last.last_access_at.unwrap_or(DateTime::<Utc>::MIN_UTC),
                id: last.id,
                fp: fp.clone(),
            })
        })
    } else {
        None
    };
    let (total, active, invited, suspended) =
        crate::org::directory_counts(&state.db, org_id, &f).await?;
    let groups = match group_field {
        None => None,
        Some(field) => Some(
            crate::org::directory_groups(&state.db, org_id, &f, field)
                .await?
                .into_iter()
                .map(|(key, label, count)| {
                    let open = match (field, &key) {
                        ("department", Some(k)) => serde_json::json!({"department_id": k}),
                        ("department", None) => {
                            serde_json::json!({"filters": "without_department"})
                        }
                        ("role", k) => serde_json::json!({"role_id": k}),
                        ("status", Some(k)) => serde_json::json!({"filters": k}),
                        ("origin", Some(k)) => {
                            serde_json::json!({"filters": format!("origin_{k}")})
                        }
                        _ => serde_json::Value::Null,
                    };
                    DirectoryGroup {
                        key,
                        label,
                        count,
                        open,
                    }
                })
                .collect(),
        ),
    };
    Ok(Json(DirectoryPage {
        items: rows,
        next_page_token,
        total,
        total_kind: "exact",
        counts: DirectoryCounts {
            active,
            invited,
            suspended,
        },
        groups,
        restricted_to_department: restricted,
    }))
}

/// Descrição da pesquisa do directório, na forma de `GET /api/search/schemas/{resource}`
/// (`docs/reference/pesquisa.md` §3). Quando o motor da ADR-0007 chegar a esta
/// linha, este recurso regista-se nele e esta rota sai.
#[utoipa::path(
    get, path = "/api/search/schemas/users", tag = "directory",
    security(("session" = [])),
    responses((status = 200, description = "schema do recurso `users`"), (status = 401, body = crate::openapi::ErrorBody))
)]
pub async fn users_schema(_auth: AuthUser) -> Json<serde_json::Value> {
    let enum_field = |name: &str, label: &str, options: serde_json::Value| {
        serde_json::json!({"name": name, "label": label, "type": "enum",
            "operators": ["eq", "ne", "in", "not_in"], "filterable": true, "sortable": false,
            "groupable": true, "options": options})
    };
    Json(serde_json::json!({
        "resource": "users",
        "label": "Utilizadores e convites",
        "collection": "/api/orgs/{org_id}/users",
        "org_scoped": true,
        "timezone": "Africa/Luanda",
        "text_search": {"fields": ["name", "email", "role", "department"], "typo_tolerant": false},
        "engine": {"filter_domain": false, "note": "`filter` chega com o motor da ADR-0007; hoje: q, filters, group_by, order_by"},
        "fields": [
            {"name": "name", "label": "Pessoa", "type": "text", "operators": [], "filterable": false, "sortable": true, "groupable": false},
            {"name": "email", "label": "Correio", "type": "text", "operators": [], "filterable": false, "sortable": false, "groupable": false},
            {"name": "role", "label": "Papel", "type": "ref", "operators": ["eq"], "filterable": true, "sortable": false, "groupable": true, "param": "role_id"},
            {"name": "department", "label": "Departamento", "type": "ref", "operators": ["eq", "is_not_set"], "filterable": true, "sortable": false, "groupable": true, "param": "department_id"},
            enum_field("origin", "Origem", serde_json::json!([
                {"value": "odoo_sso", "label": "Odoo SSO"}, {"value": "invitation", "label": "Convite"},
                {"value": "code", "label": "Código"}, {"value": "manual", "label": "Consola"},
                {"value": "registration", "label": "Registo"}, {"value": "sso", "label": "SSO"},
                {"value": "api", "label": "API"}])),
            enum_field("status", "Estado", serde_json::json!([
                {"value": "active", "label": "Activo"}, {"value": "invited", "label": "Convidado"},
                {"value": "suspended", "label": "Suspenso"}])),
            {"name": "last_access_at", "label": "Último acesso", "type": "datetime", "operators": [], "filterable": false, "sortable": true, "groupable": false}
        ],
        "filters": NAMED_FILTERS.iter().map(|(n, l, g)| serde_json::json!({"name": n, "label": l, "group": g})).collect::<Vec<_>>(),
        "group_by": [
            {"value": "department", "label": "Departamento"}, {"value": "role", "label": "Papel"},
            {"value": "status", "label": "Estado"}, {"value": "origin", "label": "Origem"}
        ],
        "default_order": ["name"],
        "order_by": ["name", "-name", "-last_access_at", "last_access_at"],
        "relevance_default": false
    }))
}

// ---------------------------------------------------------------------------
//  Convites
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Invitation {
    pub id: Uuid,
    pub org_id: Uuid,
    pub email: String,
    pub role_id: Uuid,
    pub role_name: String,
    pub department_id: Option<Uuid>,
    pub department_name: Option<String>,
    /// Convite de convidado externo (papel `external_guest`).
    pub external: bool,
    /// `link` | `code`.
    pub delivery: String,
    /// Primeiros caracteres do token, para o reconhecer (nunca o token).
    pub token_prefix: String,
    /// `pending` | `accepted` | `revoked` | `expired`.
    pub status: String,
    pub expires_at: DateTime<Utc>,
    pub invited_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub resent_at: Option<DateTime<Utc>>,
    pub accepted_at: Option<DateTime<Utc>>,
}

const INVITATION_COLUMNS: &str = "i.id, i.org_id, i.email, i.role_id, r.name AS role_name, i.department_id, \
     d.name AS department_name, i.external, i.delivery, i.token_prefix, \
     CASE WHEN i.status = 'pending' AND i.expires_at <= now() THEN 'expired' ELSE i.status END AS status, \
     i.expires_at, i.invited_by, i.created_at, i.resent_at, i.accepted_at";
const INVITATION_FROM: &str =
    "org_invitations i JOIN org_roles r ON r.id = i.role_id LEFT JOIN departments d ON d.id = i.department_id";

/// O convite e o token — só na criação e no reenvio.
#[derive(Serialize, utoipa::ToSchema)]
pub struct InvitationWithToken {
    #[serde(flatten)]
    pub invitation: Invitation,
    /// A credencial do convite, mostrada esta única vez. Não há envio de correio
    /// no servidor: a UI entrega-a (link ou código).
    pub token: String,
    /// Sempre `manual`: o servidor não envia correio (medido: zero SMTP).
    pub delivery_channel: &'static str,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateInvitationReq {
    pub email: String,
    pub role_id: Uuid,
    #[serde(default)]
    pub department_id: Option<Uuid>,
    /// `link` (omissão) | `code`.
    #[serde(default)]
    pub delivery: Option<String>,
    /// Omissão: 168 h; convidado externo: `external_guest_ttl_hours` das regras.
    #[serde(default)]
    pub expires_in_hours: Option<i64>,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct InvitationQuery {
    /// `pending` (omissão) | `accepted` | `revoked` | `expired` | `all`.
    pub status: Option<String>,
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct InvitationPage {
    pub items: Vec<Invitation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct AcceptInvitationReq {
    pub token: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct AcceptedInvitation {
    pub org_id: Uuid,
    pub role_id: Uuid,
    pub access_expires_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize)]
struct AtCursor {
    at: DateTime<Utc>,
    id: Uuid,
}

const DEFAULT_INVITE_HOURS: i64 = 168;
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

fn new_token(delivery: &str) -> String {
    if delivery == "code" {
        let bytes: [u8; 10] = delonix_meet_core::crypto::random_bytes();
        bytes
            .iter()
            .map(|b| CODE_ALPHABET[*b as usize % CODE_ALPHABET.len()] as char)
            .collect()
    } else {
        delonix_meet_core::crypto::prefixed_token("dlxi_")
    }
}

fn token_hash(token: &str) -> String {
    delonix_meet_core::crypto::sha256_hex(token.trim().to_uppercase_if_code())
}

trait CodeNorm {
    fn to_uppercase_if_code(&self) -> String;
}
impl CodeNorm for str {
    /// Os códigos curtos escrevem-se à mão: sem maiúsculas nem hífenes.
    fn to_uppercase_if_code(&self) -> String {
        if self.starts_with("dlxi_") {
            self.to_string()
        } else {
            self.replace('-', "").to_uppercase()
        }
    }
}

async fn load_invitation(state: &AppState, org_id: Uuid, id: Uuid) -> Result<Invitation, ApiError> {
    sqlx::query_as::<_, Invitation>(&format!(
        "SELECT {INVITATION_COLUMNS} FROM {INVITATION_FROM} WHERE i.org_id = $1 AND i.id = $2"
    ))
    .bind(org_id)
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| DomainError::not_found("invitation.not_found").into())
}

#[derive(sqlx::FromRow)]
struct EntryRulesRow {
    create_account_on_first_login: bool,
    approved_domains: Vec<String>,
    suspend_on_odoo_exit: bool,
    external_guest_ttl_hours: i32,
    updated_at: Option<DateTime<Utc>>,
}

pub(crate) async fn entry_rules_of(
    db: &sqlx::PgPool,
    org_id: Uuid,
) -> Result<EntryRules, ApiError> {
    let row: Option<EntryRulesRow> = sqlx::query_as(
        "SELECT create_account_on_first_login, approved_domains, suspend_on_odoo_exit,
                external_guest_ttl_hours, updated_at
           FROM org_entry_rules WHERE org_id = $1",
    )
    .bind(org_id)
    .fetch_optional(db)
    .await?;
    let r = row.unwrap_or(EntryRulesRow {
        create_account_on_first_login: true,
        approved_domains: Vec::new(),
        suspend_on_odoo_exit: false,
        external_guest_ttl_hours: 24,
        updated_at: None,
    });
    Ok(EntryRules {
        create_account_on_first_login: r.create_account_on_first_login,
        approved_domains: r.approved_domains,
        suspend_on_odoo_exit: r.suspend_on_odoo_exit,
        external_guest_ttl_hours: r.external_guest_ttl_hours,
        attendance_log: AttendanceLog {
            enabled: false,
            available: false,
            reason: "não existe integração com hr.attendance nesta instalação",
        },
        updated_at: r.updated_at,
    })
}

/// O correio pode ser convidado como pessoa da org (não externa)?
pub(crate) fn domain_allowed(email: &str, approved: &[String], org_domain: &str) -> bool {
    let domain = email.rsplit('@').next().unwrap_or("").to_lowercase();
    if approved.is_empty() && org_domain.is_empty() {
        return true;
    }
    approved.iter().any(|d| d.eq_ignore_ascii_case(&domain))
        || (!org_domain.is_empty() && org_domain == domain)
}

struct NewInvitation<'a> {
    email: &'a str,
    role_id: Uuid,
    department_id: Option<Uuid>,
    delivery: &'a str,
    expires_in_hours: Option<i64>,
}

/// Cria um convite com as regras todas; devolve (id, token). Usado pelo POST e
/// pela importação.
async fn create_invitation_inner(
    state: &AppState,
    org_id: Uuid,
    actor: Uuid,
    restricted: Option<Uuid>,
    req: NewInvitation<'_>,
) -> Result<(Uuid, String), ApiError> {
    let email = delonix_meet_domain::identity::validation::normalize_email(req.email);
    delonix_meet_domain::identity::validation::validate_email(&email).map_err(|m| {
        ApiError::from(
            DomainError::invalid("invitation.invalid_email", m)
                .with_field("email", "correio válido"),
        )
    })?;
    if !matches!(req.delivery, "link" | "code") {
        return Err(
            DomainError::invalid("invitation.invalid_delivery", "delivery é link ou code")
                .with_field("delivery", "link | code")
                .into(),
        );
    }
    if let Some(d) = restricted {
        if req.department_id != Some(d) {
            return Err(outside_scope());
        }
    }
    let role_key: Option<Option<String>> =
        sqlx::query_scalar("SELECT system_key FROM org_roles WHERE org_id = $1 AND id = $2")
            .bind(org_id)
            .bind(req.role_id)
            .fetch_optional(&state.db)
            .await?;
    let Some(role_key) = role_key else {
        return Err(DomainError::not_found("role.not_found").into());
    };
    if role_key.as_deref() == Some(SystemRole::Owner.key()) {
        return Err(DomainError::precondition(
            "invitation.owner_forbidden",
            "não se convida alguém como Proprietário — atribui-se depois de entrar",
        )
        .into());
    }
    crate::roles::ensure_can_assign(state, org_id, actor, req.role_id).await?;
    if let Some(d) = req.department_id {
        let ok: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM departments WHERE org_id = $1 AND id = $2)",
        )
        .bind(org_id)
        .bind(d)
        .fetch_one(&state.db)
        .await?;
        if !ok {
            return Err(DomainError::not_found("department.not_found").into());
        }
    }
    let external = role_key.as_deref() == Some(SystemRole::ExternalGuest.key());
    let rules = entry_rules_of(&state.db, org_id).await?;
    if !external {
        let org_domain: String =
            sqlx::query_scalar("SELECT email_domain FROM organizations WHERE id = $1")
                .bind(org_id)
                .fetch_one(&state.db)
                .await?;
        if !domain_allowed(&email, &rules.approved_domains, &org_domain) {
            return Err(DomainError::precondition(
                "invitation.domain_not_approved",
                "o domínio do correio não está nos domínios aprovados — convide como convidado externo",
            )
            .with_field("email", "domínio aprovado")
            .into());
        }
    }
    if let Some((_, active, _, _, _)) =
        crate::org::member_by_email(&state.db, org_id, &email).await?
    {
        if active {
            return Err(DomainError::conflict(
                "invitation.already_member",
                "a pessoa já é membro activo",
            )
            .into());
        }
    }
    if external {
        // Limite mensal do papel de quem convida (ADR-0008 §8).
        let actor_role = crate::org::member_subject(&state.db, org_id, actor)
            .await?
            .map(|s| s.0)
            .ok_or(ApiError::NotFound)?;
        let limit: Option<i32> = sqlx::query_scalar(
            "SELECT eff_max_external_guests_per_month FROM org_roles WHERE id = $1",
        )
        .bind(actor_role)
        .fetch_one(&state.db)
        .await?;
        if let Some(max) = limit {
            let used: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM org_invitations
                  WHERE org_id = $1 AND invited_by = $2 AND external
                    AND created_at >= (date_trunc('month', now() AT TIME ZONE 'Africa/Luanda') AT TIME ZONE 'Africa/Luanda')",
            )
            .bind(org_id)
            .bind(actor)
            .fetch_one(&state.db)
            .await?;
            if used >= max as i64 {
                return Err(DomainError::precondition(
                    "role.external_guest_limit_reached",
                    format!("o seu papel permite {max} convidados externos por mês"),
                )
                .into());
            }
        }
    }
    let hours = match req.expires_in_hours {
        Some(h) if !(1..=24 * 30).contains(&h) => {
            return Err(DomainError::invalid(
                "invitation.invalid_expiry",
                "expires_in_hours é 1–720",
            )
            .with_field("expires_in_hours", "1–720")
            .into())
        }
        Some(h) => h,
        None if external => rules.external_guest_ttl_hours as i64,
        None => DEFAULT_INVITE_HOURS,
    };
    // Um convite pendente mas já expirado não bloqueia um novo (o sweeper é de hora a hora).
    sqlx::query(
        "UPDATE org_invitations SET status = 'expired'
          WHERE org_id = $1 AND lower(email) = $2 AND status = 'pending' AND expires_at <= now()",
    )
    .bind(org_id)
    .bind(&email)
    .execute(&state.db)
    .await?;
    let token = new_token(req.delivery);
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO org_invitations (org_id, email, role_id, department_id, external, delivery,
                                      token_hash, token_prefix, expires_at, invited_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now() + make_interval(hours => $9), $10)
         RETURNING id",
    )
    .bind(org_id)
    .bind(&email)
    .bind(req.role_id)
    .bind(req.department_id)
    .bind(external)
    .bind(req.delivery)
    .bind(token_hash(&token))
    .bind(token.chars().take(8).collect::<String>())
    .bind(hours as i32)
    .bind(actor)
    .fetch_one(&state.db)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => DomainError::conflict(
            "invitation.already_pending",
            "já há um convite pendente para este correio — reenvie-o",
        )
        .into(),
        _ => ApiError::from(e),
    })?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        actor,
        "invitation.created",
        &serde_json::json!({"invitation_id": id, "email": email, "role_id": req.role_id,
                            "department_id": req.department_id, "external": external,
                            "delivery": req.delivery, "expires_in_hours": hours})
        .to_string(),
    )
    .await;
    Ok((id, token))
}

/// Convida uma pessoa. Devolve o token UMA vez.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/invitations", tag = "directory",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = CreateInvitationReq,
    responses(
        (status = 201, body = InvitationWithToken, headers(("Location" = String))),
        (status = 400, body = crate::openapi::ErrorBody, description = "`invitation.invalid_email`, `invitation.invalid_delivery`, `invitation.invalid_expiry`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "sem `admin.manage_accounts`, fora do departamento, `authz.escalation`"),
        (status = 404, body = crate::openapi::ErrorBody, description = "org, `role.not_found`, `department.not_found`"),
        (status = 409, body = crate::openapi::ErrorBody, description = "`invitation.already_member`, `invitation.already_pending`"),
        (status = 422, body = crate::openapi::ErrorBody, description = "`invitation.owner_forbidden`, `invitation.domain_not_approved`, `role.external_guest_limit_reached`"),
    )
)]
pub async fn create_invitation(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateInvitationReq>,
) -> Result<Response, ApiError> {
    let restricted = accounts_scope(&state, org_id, auth.user_id).await?;
    let delivery = req.delivery.as_deref().unwrap_or("link");
    let (id, token) = create_invitation_inner(
        &state,
        org_id,
        auth.user_id,
        restricted,
        NewInvitation {
            email: &req.email,
            role_id: req.role_id,
            department_id: req.department_id,
            delivery,
            expires_in_hours: req.expires_in_hours,
        },
    )
    .await?;
    let invitation = load_invitation(&state, org_id, id).await?;
    Ok((
        StatusCode::CREATED,
        [(
            header::LOCATION,
            format!("/api/orgs/{org_id}/invitations/{id}"),
        )],
        Json(InvitationWithToken {
            invitation,
            token,
            delivery_channel: "manual",
        }),
    )
        .into_response())
}

/// Convites da organização (omissão: pendentes).
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/invitations", tag = "directory",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), InvitationQuery),
    responses(
        (status = 200, body = InvitationPage),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_invitations(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Query(q): Query<InvitationQuery>,
) -> Result<Json<InvitationPage>, ApiError> {
    let restricted = accounts_scope(&state, org_id, auth.user_id).await?;
    let status = q.status.unwrap_or_else(|| "pending".into());
    if !matches!(
        status.as_str(),
        "pending" | "accepted" | "revoked" | "expired" | "all"
    ) {
        return Err(
            DomainError::invalid("invitation.invalid_status", "estado desconhecido")
                .with_field("status", "pending | accepted | revoked | expired | all")
                .into(),
        );
    }
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<AtCursor> = page.cursor()?;
    let rows: Vec<Invitation> = sqlx::query_as(&format!(
        "SELECT * FROM (SELECT {INVITATION_COLUMNS} FROM {INVITATION_FROM} WHERE i.org_id = $1
                          AND ($5::uuid IS NULL OR i.department_id = $5)) x
          WHERE ($2 = 'all' OR x.status = $2)
            AND ($3::timestamptz IS NULL OR (x.created_at, x.id) > ($3, $4))
          ORDER BY x.created_at, x.id LIMIT $6"
    ))
    .bind(org_id)
    .bind(&status)
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(restricted)
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = delonix_meet_core::page::Page::from_overfetch(rows, size, |r| AtCursor {
        at: r.created_at,
        id: r.id,
    });
    Ok(Json(InvitationPage {
        items: p.items,
        next_page_token: p.next_page_token,
    }))
}

async fn invitation_in_scope(
    state: &AppState,
    org_id: Uuid,
    actor: Uuid,
    id: Uuid,
) -> Result<Invitation, ApiError> {
    let restricted = accounts_scope(state, org_id, actor).await?;
    let inv = load_invitation(state, org_id, id).await?;
    if restricted.is_some_and(|d| inv.department_id != Some(d)) {
        return Err(outside_scope());
    }
    Ok(inv)
}

/// Um convite.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/invitations/{invitation_id}", tag = "directory",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("invitation_id" = Uuid, Path)),
    responses((status = 200, body = Invitation), (status = 401, body = crate::openapi::ErrorBody),
              (status = 403, body = crate::openapi::ErrorBody), (status = 404, body = crate::openapi::ErrorBody))
)]
pub async fn get_invitation(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Invitation>, ApiError> {
    Ok(Json(
        invitation_in_scope(&state, org_id, auth.user_id, id).await?,
    ))
}

async fn revoke_inner(
    state: &AppState,
    org_id: Uuid,
    actor: Uuid,
    id: Uuid,
) -> Result<(), ApiError> {
    let n = sqlx::query(
        "UPDATE org_invitations SET status = 'revoked' WHERE org_id = $1 AND id = $2 AND status = 'pending'",
    )
    .bind(org_id)
    .bind(id)
    .execute(&state.db)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(DomainError::conflict(
            "invitation.not_pending",
            "o convite já não está pendente",
        )
        .into());
    }
    crate::audit::log(
        &state.db,
        Some(org_id),
        actor,
        "invitation.revoked",
        &serde_json::json!({"invitation_id": id}).to_string(),
    )
    .await;
    Ok(())
}

/// Revoga um convite pendente.
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/invitations/{invitation_id}", tag = "directory",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("invitation_id" = Uuid, Path)),
    responses((status = 204), (status = 401, body = crate::openapi::ErrorBody),
              (status = 403, body = crate::openapi::ErrorBody), (status = 404, body = crate::openapi::ErrorBody),
              (status = 409, body = crate::openapi::ErrorBody, description = "`invitation.not_pending`"))
)]
pub async fn revoke_invitation(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    invitation_in_scope(&state, org_id, auth.user_id, id).await?;
    revoke_inner(&state, org_id, auth.user_id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn resend_inner(
    state: &AppState,
    org_id: Uuid,
    actor: Uuid,
    inv: &Invitation,
) -> Result<String, ApiError> {
    if !matches!(inv.status.as_str(), "pending" | "expired") {
        return Err(DomainError::conflict(
            "invitation.not_pending",
            "só se reenvia um convite pendente ou expirado",
        )
        .into());
    }
    let hours = if inv.external {
        entry_rules_of(&state.db, org_id)
            .await?
            .external_guest_ttl_hours as i64
    } else {
        DEFAULT_INVITE_HOURS
    };
    let token = new_token(&inv.delivery);
    let n = sqlx::query(
        "UPDATE org_invitations
            SET token_hash = $3, token_prefix = $4, expires_at = now() + make_interval(hours => $5),
                status = 'pending', resent_at = now()
          WHERE org_id = $1 AND id = $2 AND status = 'pending'",
    )
    .bind(org_id)
    .bind(inv.id)
    .bind(token_hash(&token))
    .bind(token.chars().take(8).collect::<String>())
    .bind(hours as i32)
    .execute(&state.db)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(DomainError::conflict(
            "invitation.not_pending",
            "o convite já não está pendente",
        )
        .into());
    }
    crate::audit::log(
        &state.db,
        Some(org_id),
        actor,
        "invitation.resent",
        &serde_json::json!({"invitation_id": inv.id, "expires_in_hours": hours}).to_string(),
    )
    .await;
    Ok(token)
}

/// Reenvia: roda o token (o anterior deixa de servir) e renova a validade.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/invitations/{invitation_id}/resend", tag = "directory",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("invitation_id" = Uuid, Path)),
    responses((status = 200, body = InvitationWithToken), (status = 401, body = crate::openapi::ErrorBody),
              (status = 403, body = crate::openapi::ErrorBody), (status = 404, body = crate::openapi::ErrorBody),
              (status = 409, body = crate::openapi::ErrorBody, description = "`invitation.not_pending`"))
)]
pub async fn resend_invitation(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<InvitationWithToken>, ApiError> {
    let inv = invitation_in_scope(&state, org_id, auth.user_id, id).await?;
    let token = resend_inner(&state, org_id, auth.user_id, &inv).await?;
    Ok(Json(InvitationWithToken {
        invitation: load_invitation(&state, org_id, id).await?,
        token,
        delivery_channel: "manual",
    }))
}

#[derive(sqlx::FromRow)]
struct PendingInvitation {
    id: Uuid,
    org_id: Uuid,
    email: String,
    role_id: Uuid,
    department_id: Option<Uuid>,
    external: bool,
    delivery: String,
    expires_at: DateTime<Utc>,
    token_hash: String,
}

/// Aceita um convite. O token é a credencial (uso único, com expiração); o
/// correio da sessão tem de ser o do convite como defesa em profundidade.
#[utoipa::path(
    post, path = "/api/invitations/accept", tag = "directory",
    security(("session" = [])),
    request_body = AcceptInvitationReq,
    responses(
        (status = 200, body = AcceptedInvitation),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`invitation.email_mismatch`"),
        (status = 404, body = crate::openapi::ErrorBody, description = "`invitation.not_found` (inexistente, revogado ou já usado)"),
        (status = 409, body = crate::openapi::ErrorBody, description = "`invitation.already_member`, `invitation.foreign_org`, `role.last_owner`"),
        (status = 422, body = crate::openapi::ErrorBody, description = "`invitation.expired`, `seats.limit_reached`"),
        (status = 429, body = crate::openapi::ErrorBody),
    )
)]
pub async fn accept_invitation(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<AcceptInvitationReq>,
) -> Result<Json<AcceptedInvitation>, ApiError> {
    let hash = token_hash(&req.token);
    let not_found = || ApiError::from(DomainError::not_found("invitation.not_found"));
    let mut tx = state.db.begin().await?;
    // Uso único: a linha fica presa até ao fim da transacção.
    let row: Option<PendingInvitation> = sqlx::query_as(
        "SELECT id, org_id, email, role_id, department_id, external, delivery, expires_at, token_hash
           FROM org_invitations WHERE token_hash = $1 AND status = 'pending' FOR UPDATE",
    )
    .bind(&hash)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(PendingInvitation {
        id,
        org_id,
        email,
        role_id,
        department_id,
        external,
        delivery,
        expires_at,
        token_hash: stored,
    }) = row
    else {
        return Err(not_found());
    };
    if !delonix_meet_core::crypto::ct_eq(stored.as_bytes(), hash.as_bytes()) {
        return Err(not_found());
    }
    if expires_at <= Utc::now() {
        sqlx::query("UPDATE org_invitations SET status = 'expired' WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Err(DomainError::precondition(
            "invitation.expired",
            "o convite expirou — peça que o reenviem",
        )
        .into());
    }
    let my_email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(auth.user_id)
        .fetch_one(&mut *tx)
        .await?;
    if !my_email.eq_ignore_ascii_case(&email) {
        return Err(DomainError::forbidden("invitation.email_mismatch")
            .with_message("o convite é para outro correio")
            .into());
    }
    let access_expires_at = if external {
        let ttl = entry_rules_of(&state.db, org_id)
            .await?
            .external_guest_ttl_hours as i64;
        Some(Utc::now() + Duration::hours(ttl))
    } else {
        None
    };
    let origin = if delivery == "code" {
        "code"
    } else {
        "invitation"
    };
    crate::org::activate_membership_tx(
        &mut tx,
        org_id,
        auth.user_id,
        role_id,
        department_id,
        origin,
        access_expires_at,
    )
    .await?;
    sqlx::query(
        "UPDATE org_invitations SET status = 'accepted', accepted_by = $2, accepted_at = now()
          WHERE id = $1 AND status = 'pending'",
    )
    .bind(id)
    .bind(auth.user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit()
        .await
        .map_err(crate::org::map_member_write_error)?;
    crate::audit::log(&state.db, Some(org_id), auth.user_id, "invitation.accepted",
        &serde_json::json!({"invitation_id": id, "role_id": role_id, "department_id": department_id,
                            "access_expires_at": access_expires_at}).to_string()).await;
    Ok(Json(AcceptedInvitation {
        org_id,
        role_id,
        access_expires_at,
    }))
}

/// Sweeper: convites pendentes fora de prazo passam a `expired`.
pub(crate) async fn expire_invitations(state: &AppState) -> Result<u64, ApiError> {
    Ok(sqlx::query("UPDATE org_invitations SET status = 'expired' WHERE status = 'pending' AND expires_at <= now()")
        .execute(&state.db)
        .await?
        .rows_affected())
}

// ---------------------------------------------------------------------------
//  Acções em massa
// ---------------------------------------------------------------------------

#[derive(Deserialize, utoipa::ToSchema)]
pub struct BulkActionReq {
    /// `change_role` | `change_department` | `resend_invitation` | `revoke_invitation` | `suspend` | `reactivate`.
    pub action: String,
    #[serde(default)]
    pub user_ids: Vec<Uuid>,
    #[serde(default)]
    pub invitation_ids: Vec<Uuid>,
    /// `change_role`.
    #[serde(default)]
    pub role_id: Option<Uuid>,
    /// `change_department`; `null` tira o departamento.
    #[serde(default)]
    pub department_id: Option<Uuid>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct BulkItemResult {
    /// `member` | `invitation`.
    pub kind: &'static str,
    pub id: Uuid,
    pub ok: bool,
    /// Houve mudança (um item já no estado pedido dá `ok` sem mudança).
    pub changed: bool,
    pub code: Option<String>,
    pub message: Option<String>,
    /// Só em `resend_invitation`: o token novo, esta única vez.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct BulkActionResult {
    pub action: String,
    pub succeeded: usize,
    pub failed: usize,
    pub results: Vec<BulkItemResult>,
}

const BULK_MAX: usize = 200;

fn item_err(kind: &'static str, id: Uuid, e: ApiError) -> BulkItemResult {
    let (code, message) = match &e {
        ApiError::Domain(d) => (d.code.to_string(), d.message.clone()),
        ApiError::NotFound => ("not_found".into(), "não encontrado".into()),
        ApiError::Forbidden => ("permission_denied".into(), "sem permissão".into()),
        ApiError::Conflict(m) => ("conflict".into(), m.clone()),
        ApiError::BadRequest(m) => ("invalid_argument".into(), m.clone()),
        other => ("internal".into(), {
            tracing::error!(error = %other, "acção em massa");
            "erro interno".into()
        }),
    };
    BulkItemResult {
        kind,
        id,
        ok: false,
        changed: false,
        code: Some(code),
        message: Some(message),
        token: None,
    }
}

fn item_ok(kind: &'static str, id: Uuid, changed: bool, token: Option<String>) -> BulkItemResult {
    BulkItemResult {
        kind,
        id,
        ok: true,
        changed,
        code: None,
        message: None,
        token,
    }
}

async fn member_action(
    state: &AppState,
    org_id: Uuid,
    actor: Uuid,
    restricted: Option<Uuid>,
    req: &BulkActionReq,
    user_id: Uuid,
) -> Result<bool, ApiError> {
    let subject = crate::org::member_subject(&state.db, org_id, user_id).await?;
    let mut tx = state.db.begin().await?;
    let st = crate::org::member_state(&mut *tx, org_id, user_id)
        .await?
        .ok_or_else(|| ApiError::from(DomainError::not_found("member.not_found")))?;
    drop(tx);
    if restricted.is_some() && st.department_id != restricted {
        return Err(outside_scope());
    }
    let _ = subject;
    match req.action.as_str() {
        "change_role" => {
            let role = req.role_id.ok_or_else(|| {
                ApiError::from(
                    DomainError::invalid("bulk.role_required", "change_role exige role_id")
                        .with_field("role_id", "uuid"),
                )
            })?;
            Ok(
                crate::roles::assign_role_as(state, org_id, actor, user_id, role)
                    .await?
                    .0,
            )
        }
        "change_department" => {
            if let Some(d) = req.department_id {
                if restricted.is_some_and(|r| r != d) {
                    return Err(outside_scope());
                }
            }
            let mut tx = state.db.begin().await?;
            let changed =
                crate::org::set_member_department_tx(&mut tx, org_id, user_id, req.department_id)
                    .await?;
            tx.commit().await?;
            if changed {
                crate::audit::log(&state.db, Some(org_id), actor, "member.department_changed",
                    &serde_json::json!({"user_id": user_id, "before": st.department_id, "after": req.department_id}).to_string()).await;
            }
            Ok(changed)
        }
        "suspend" => {
            if user_id == actor {
                return Err(DomainError::precondition(
                    "member.cannot_suspend_self",
                    "não pode suspender o próprio acesso",
                )
                .into());
            }
            let mut tx = state.db.begin().await?;
            let changed =
                crate::org::archive_member_tx(&mut tx, org_id, user_id, "suspended", Some(actor))
                    .await?;
            tx.commit()
                .await
                .map_err(crate::org::map_member_write_error)?;
            if changed {
                crate::audit::log(
                    &state.db,
                    Some(org_id),
                    actor,
                    "member.suspended",
                    &serde_json::json!({"user_id": user_id, "role_id": st.role_id}).to_string(),
                )
                .await;
            }
            Ok(changed)
        }
        "reactivate" => {
            let mut tx = state.db.begin().await?;
            let changed = crate::org::reactivate_member_tx(&mut tx, org_id, user_id).await?;
            tx.commit()
                .await
                .map_err(crate::org::map_member_write_error)?;
            if changed {
                crate::audit::log(
                    &state.db,
                    Some(org_id),
                    actor,
                    "member.reactivated",
                    &serde_json::json!({"user_id": user_id, "reason_before": st.archived_reason})
                        .to_string(),
                )
                .await;
            }
            Ok(changed)
        }
        _ => Err(DomainError::precondition(
            "bulk.not_applicable",
            "esta acção não se aplica a membros",
        )
        .into()),
    }
}

async fn invitation_action(
    state: &AppState,
    org_id: Uuid,
    actor: Uuid,
    req: &BulkActionReq,
    id: Uuid,
) -> Result<(bool, Option<String>), ApiError> {
    let inv = invitation_in_scope(state, org_id, actor, id).await?;
    match req.action.as_str() {
        "resend_invitation" => Ok((true, Some(resend_inner(state, org_id, actor, &inv).await?))),
        "revoke_invitation" => {
            revoke_inner(state, org_id, actor, id).await?;
            Ok((true, None))
        }
        "change_role" | "change_department" => {
            if inv.status != "pending" {
                return Err(DomainError::conflict(
                    "invitation.not_pending",
                    "o convite já não está pendente",
                )
                .into());
            }
            let (role, dept) = if req.action == "change_role" {
                let role = req.role_id.ok_or_else(|| {
                    ApiError::from(
                        DomainError::invalid("bulk.role_required", "change_role exige role_id")
                            .with_field("role_id", "uuid"),
                    )
                })?;
                crate::roles::ensure_can_assign(state, org_id, actor, role).await?;
                (role, inv.department_id)
            } else {
                (inv.role_id, req.department_id)
            };
            let n = sqlx::query(
                "UPDATE org_invitations SET role_id = $3, department_id = $4
                  WHERE org_id = $1 AND id = $2 AND status = 'pending'
                    AND (role_id <> $3 OR department_id IS DISTINCT FROM $4)
                    AND EXISTS (SELECT 1 FROM org_roles WHERE org_id = $1 AND id = $3
                                 AND system_key IS DISTINCT FROM 'owner')",
            )
            .bind(org_id)
            .bind(id)
            .bind(role)
            .bind(dept)
            .execute(&state.db)
            .await
            .map_err(|e| match &e {
                sqlx::Error::Database(db) if db.is_foreign_key_violation() => {
                    ApiError::from(DomainError::not_found("department.not_found"))
                }
                _ => ApiError::from(e),
            })?
            .rows_affected();
            if n > 0 {
                crate::audit::log(&state.db, Some(org_id), actor, "invitation.updated",
                    &serde_json::json!({"invitation_id": id, "before": {"role_id": inv.role_id, "department_id": inv.department_id},
                                        "after": {"role_id": role, "department_id": dept}}).to_string()).await;
            }
            Ok((n > 0, None))
        }
        _ => Err(DomainError::precondition(
            "bulk.not_applicable",
            "esta acção não se aplica a convites",
        )
        .into()),
    }
}

/// Acções em massa: cada item é atómico por si e tem o seu resultado.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/users/bulk-actions", tag = "directory",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = BulkActionReq,
    responses(
        (status = 200, body = BulkActionResult, description = "resultado por item (os erros de um item não falham os outros)"),
        (status = 400, body = crate::openapi::ErrorBody, description = "`bulk.invalid_action`, `bulk.empty`, `bulk.too_many`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn bulk_actions(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<BulkActionReq>,
) -> Result<Json<BulkActionResult>, ApiError> {
    let restricted = accounts_scope(&state, org_id, auth.user_id).await?;
    if !matches!(
        req.action.as_str(),
        "change_role"
            | "change_department"
            | "resend_invitation"
            | "revoke_invitation"
            | "suspend"
            | "reactivate"
    ) {
        return Err(DomainError::invalid("bulk.invalid_action", "acção desconhecida")
            .with_field("action", "change_role | change_department | resend_invitation | revoke_invitation | suspend | reactivate")
            .into());
    }
    let n = req.user_ids.len() + req.invitation_ids.len();
    if n == 0 {
        return Err(DomainError::invalid("bulk.empty", "sem itens")
            .with_field("user_ids", "≥ 1 item")
            .into());
    }
    if n > BULK_MAX {
        return Err(
            DomainError::invalid("bulk.too_many", format!("no máximo {BULK_MAX} itens"))
                .with_field("user_ids", "≤ 200 itens")
                .into(),
        );
    }
    let mut results = Vec::with_capacity(n);
    let users: BTreeSet<Uuid> = req.user_ids.iter().copied().collect();
    for u in users {
        results.push(
            match member_action(&state, org_id, auth.user_id, restricted, &req, u).await {
                Ok(changed) => item_ok("member", u, changed, None),
                Err(e) => item_err("member", u, e),
            },
        );
    }
    let invites: BTreeSet<Uuid> = req.invitation_ids.iter().copied().collect();
    for i in invites {
        results.push(
            match invitation_action(&state, org_id, auth.user_id, &req, i).await {
                Ok((changed, token)) => item_ok("invitation", i, changed, token),
                Err(e) => item_err("invitation", i, e),
            },
        );
    }
    let succeeded = results.iter().filter(|r| r.ok).count();
    Ok(Json(BulkActionResult {
        action: req.action,
        succeeded,
        failed: results.len() - succeeded,
        results,
    }))
}

// ---------------------------------------------------------------------------
//  Importar CSV
// ---------------------------------------------------------------------------

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ImportReq {
    /// CSV UTF-8 com cabeçalho: `email` obrigatório; `role` (nome ou chave),
    /// `department` (nome) e `delivery` (`link`|`code`) opcionais.
    pub csv: String,
    /// Valida e diz o que faria, sem escrever.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ImportLine {
    /// Número da linha no ficheiro (o cabeçalho é a 1).
    pub line: usize,
    pub email: String,
    /// `invited` | `updated` | `unchanged` | `error` (em `dry_run`: `would_invite`, `would_update`).
    pub outcome: String,
    pub code: Option<String>,
    pub message: Option<String>,
    pub invitation_id: Option<Uuid>,
    /// Token do convite criado, esta única vez.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ImportReport {
    pub dry_run: bool,
    pub lines: Vec<ImportLine>,
    pub invited: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub errors: usize,
}

const IMPORT_MAX_LINES: usize = 1000;

/// Separa uma linha de CSV (aspas duplas; `;` também serve de separador se não houver vírgula).
fn split_csv_line(line: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            c if c == sep && !quoted => out.push(std::mem::take(&mut cur).trim().to_string()),
            c => cur.push(c),
        }
    }
    out.push(cur.trim().to_string());
    out
}

/// Importa pessoas. Idempotente: membro activo com o mesmo papel e departamento
/// fica igual; convite pendente para o mesmo correio é actualizado, nunca
/// duplicado; ninguém é criado como conta — cria-se um convite.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/users/imports", tag = "directory",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = ImportReq,
    responses(
        (status = 200, body = ImportReport, description = "relatório linha a linha"),
        (status = 400, body = crate::openapi::ErrorBody, description = "`import.missing_email_column`, `import.too_many_lines`, `import.empty`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn import_users(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<ImportReq>,
) -> Result<Json<ImportReport>, ApiError> {
    let restricted = accounts_scope(&state, org_id, auth.user_id).await?;
    let text = req.csv.trim_start_matches('\u{feff}');
    let mut lines = text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty());
    let Some((_, header)) = lines.next() else {
        return Err(DomainError::invalid("import.empty", "CSV vazio")
            .with_field("csv", "cabeçalho + linhas")
            .into());
    };
    let sep = if header.contains(',') { ',' } else { ';' };
    let cols: Vec<String> = split_csv_line(header, sep)
        .into_iter()
        .map(|c| c.to_lowercase())
        .collect();
    let col = |name: &str| cols.iter().position(|c| c == name);
    let Some(email_col) = col("email").or_else(|| col("correio")) else {
        return Err(
            DomainError::invalid("import.missing_email_column", "falta a coluna email")
                .with_field("csv", "cabeçalho com email")
                .into(),
        );
    };
    let role_col = col("role").or_else(|| col("papel"));
    let dept_col = col("department").or_else(|| col("departamento"));
    let delivery_col = col("delivery");
    let body: Vec<(usize, &str)> = lines.collect();
    if body.len() > IMPORT_MAX_LINES {
        return Err(DomainError::invalid(
            "import.too_many_lines",
            format!("no máximo {IMPORT_MAX_LINES} linhas"),
        )
        .with_field("csv", "≤ 1000 linhas")
        .into());
    }
    let roles: Vec<(Uuid, Option<String>, String)> =
        sqlx::query_as("SELECT id, system_key, name FROM org_roles WHERE org_id = $1")
            .bind(org_id)
            .fetch_all(&state.db)
            .await?;
    let member_role = roles
        .iter()
        .find(|r| r.1.as_deref() == Some("member"))
        .map(|r| r.0)
        .unwrap_or_default();
    let departments: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, name FROM departments WHERE org_id = $1")
            .bind(org_id)
            .fetch_all(&state.db)
            .await?;
    let mut seen = BTreeSet::new();
    let mut report = ImportReport {
        dry_run: req.dry_run,
        lines: Vec::new(),
        invited: 0,
        updated: 0,
        unchanged: 0,
        errors: 0,
    };
    for (idx, raw) in body {
        let cells = split_csv_line(raw, sep);
        let get = |i: Option<usize>| {
            i.and_then(|i| cells.get(i))
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
        };
        let email = delonix_meet_domain::identity::validation::normalize_email(
            get(Some(email_col)).unwrap_or(""),
        );
        let mut line = ImportLine {
            line: idx + 1,
            email: email.clone(),
            outcome: "error".into(),
            code: None,
            message: None,
            invitation_id: None,
            token: None,
        };
        let outcome: Result<(String, Option<Uuid>, Option<String>), ApiError> = async {
            delonix_meet_domain::identity::validation::validate_email(&email)
                .map_err(|m| ApiError::from(DomainError::invalid("import.invalid_email", m)))?;
            if !seen.insert(email.clone()) {
                return Err(DomainError::invalid("import.duplicate_in_file", "correio repetido no ficheiro").into());
            }
            let role_id = match get(role_col) {
                None => member_role,
                Some(r) => roles
                    .iter()
                    .find(|x| x.1.as_deref() == Some(r) || x.2.eq_ignore_ascii_case(r))
                    .map(|x| x.0)
                    .ok_or_else(|| ApiError::from(DomainError::invalid("import.unknown_role", format!("papel desconhecido: {r}"))))?,
            };
            let department_id = match get(dept_col) {
                None => None,
                Some(d) => Some(
                    departments
                        .iter()
                        .find(|x| x.1.eq_ignore_ascii_case(d))
                        .map(|x| x.0)
                        .ok_or_else(|| ApiError::from(DomainError::invalid("import.unknown_department", format!("departamento desconhecido: {d}"))))?,
                ),
            };
            let delivery = get(delivery_col).unwrap_or("link");
            if let Some((user_id, active, reason, cur_role, cur_dept)) =
                crate::org::member_by_email(&state.db, org_id, &email).await?
            {
                if !active {
                    return Err(DomainError::precondition(
                        "import.member_suspended",
                        format!("membro suspenso ({}) — reactive-o", reason.unwrap_or_default()),
                    )
                    .into());
                }
                if restricted.is_some() && (cur_dept != restricted || (dept_col.is_some() && department_id != restricted)) {
                    return Err(outside_scope());
                }
                let role_change = role_col.is_some() && get(role_col).is_some() && cur_role != role_id;
                let dept_change = dept_col.is_some() && department_id != cur_dept;
                if !role_change && !dept_change {
                    return Ok(("unchanged".to_string(), None, None));
                }
                if req.dry_run {
                    if role_change {
                        crate::roles::ensure_can_assign(&state, org_id, auth.user_id, role_id).await?;
                    }
                    return Ok(("would_update".to_string(), None, None));
                }
                if role_change {
                    crate::roles::assign_role_as(&state, org_id, auth.user_id, user_id, role_id).await?;
                }
                if dept_change {
                    let mut tx = state.db.begin().await?;
                    crate::org::set_member_department_tx(&mut tx, org_id, user_id, department_id).await?;
                    tx.commit().await?;
                    crate::audit::log(&state.db, Some(org_id), auth.user_id, "member.department_changed",
                        &serde_json::json!({"user_id": user_id, "before": cur_dept, "after": department_id, "via": "import"}).to_string()).await;
                }
                return Ok(("updated".to_string(), None, None));
            }
            let pending: Option<(Uuid, Uuid, Option<Uuid>)> = sqlx::query_as(
                "SELECT id, role_id, department_id FROM org_invitations
                  WHERE org_id = $1 AND lower(email) = $2 AND status = 'pending' AND expires_at > now()",
            )
            .bind(org_id)
            .bind(&email)
            .fetch_optional(&state.db)
            .await?;
            if let Some((inv_id, inv_role, inv_dept)) = pending {
                let wants_role = if role_col.is_some() && get(role_col).is_some() { role_id } else { inv_role };
                let wants_dept = if dept_col.is_some() { department_id } else { inv_dept };
                if wants_role == inv_role && wants_dept == inv_dept {
                    return Ok(("unchanged".to_string(), Some(inv_id), None));
                }
                if req.dry_run {
                    return Ok(("would_update".to_string(), Some(inv_id), None));
                }
                let upd = BulkActionReq {
                    action: if wants_role != inv_role { "change_role".into() } else { "change_department".into() },
                    user_ids: vec![],
                    invitation_ids: vec![inv_id],
                    role_id: Some(wants_role),
                    department_id: wants_dept,
                };
                invitation_action(&state, org_id, auth.user_id, &upd, inv_id).await?;
                if wants_role != inv_role && wants_dept != inv_dept {
                    let upd2 = BulkActionReq { action: "change_department".into(), ..upd };
                    invitation_action(&state, org_id, auth.user_id, &upd2, inv_id).await?;
                }
                return Ok(("updated".to_string(), Some(inv_id), None));
            }
            if req.dry_run {
                crate::roles::ensure_can_assign(&state, org_id, auth.user_id, role_id).await?;
                return Ok(("would_invite".to_string(), None, None));
            }
            let (id, token) = create_invitation_inner(
                &state,
                org_id,
                auth.user_id,
                restricted,
                NewInvitation {
                    email: &email,
                    role_id,
                    department_id,
                    delivery,
                    expires_in_hours: None,
                },
            )
            .await?;
            Ok(("invited".to_string(), Some(id), Some(token)))
        }
        .await;
        match outcome {
            Ok((o, inv, token)) => {
                match o.as_str() {
                    "invited" | "would_invite" => report.invited += 1,
                    "updated" | "would_update" => report.updated += 1,
                    _ => report.unchanged += 1,
                }
                line.outcome = o;
                line.invitation_id = inv;
                line.token = token;
            }
            Err(e) => {
                let r = item_err("member", Uuid::nil(), e);
                line.code = r.code;
                line.message = r.message;
                report.errors += 1;
            }
        }
        report.lines.push(line);
    }
    if !req.dry_run {
        crate::audit::log(
            &state.db,
            Some(org_id),
            auth.user_id,
            "users.imported",
            &serde_json::json!({"invited": report.invited, "updated": report.updated,
                                "unchanged": report.unchanged, "errors": report.errors})
            .to_string(),
        )
        .await;
    }
    Ok(Json(report))
}

// ---------------------------------------------------------------------------
//  Departamentos
// ---------------------------------------------------------------------------

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Department {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    /// `manual` | `odoo` (este não se edita à mão).
    pub source: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[sqlx(default)]
    pub member_count: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DepartmentPage {
    pub items: Vec<Department>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct DepartmentReq {
    pub name: String,
}

#[derive(Serialize, Deserialize)]
struct NameCursor {
    name: String,
    id: Uuid,
}

fn department_name(raw: &str) -> Result<String, ApiError> {
    let n = raw.trim();
    if n.is_empty() || n.chars().count() > 80 {
        return Err(
            DomainError::invalid("department.invalid_name", "nome com 1–80 caracteres")
                .with_field("name", "1–80")
                .into(),
        );
    }
    Ok(n.to_string())
}

async fn load_department(state: &AppState, org_id: Uuid, id: Uuid) -> Result<Department, ApiError> {
    let mut d: Department = sqlx::query_as(
        "SELECT id, org_id, name, source, created_at, updated_at FROM departments WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id)
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| ApiError::from(DomainError::not_found("department.not_found")))?;
    d.member_count = crate::org::department_member_counts(&state.db, org_id)
        .await?
        .get(&id)
        .copied()
        .unwrap_or(0);
    Ok(d)
}

/// Departamentos da org (qualquer membro activo lê).
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/departments", tag = "directory",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), crate::roles::PageQuery),
    responses((status = 200, body = DepartmentPage), (status = 401, body = crate::openapi::ErrorBody),
              (status = 404, body = crate::openapi::ErrorBody))
)]
pub async fn list_departments(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Query(q): Query<crate::roles::PageQuery>,
) -> Result<Json<DepartmentPage>, ApiError> {
    crate::org::require_member_pub(&state, org_id, auth.user_id).await?;
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<NameCursor> = page.cursor()?;
    let mut rows: Vec<Department> = sqlx::query_as(
        "SELECT id, org_id, name, source, created_at, updated_at FROM departments
          WHERE org_id = $1 AND ($2::text IS NULL OR (lower(name), id) > ($2, $3))
          ORDER BY lower(name), id LIMIT $4",
    )
    .bind(org_id)
    .bind(cursor.as_ref().map(|c| c.name.clone()))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let counts = crate::org::department_member_counts(&state.db, org_id).await?;
    for d in rows.iter_mut() {
        d.member_count = counts.get(&d.id).copied().unwrap_or(0);
    }
    let p = delonix_meet_core::page::Page::from_overfetch(rows, size, |d| NameCursor {
        name: d.name.to_lowercase(),
        id: d.id,
    });
    Ok(Json(DepartmentPage {
        items: p.items,
        next_page_token: p.next_page_token,
    }))
}

/// Cria um departamento manual.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/departments", tag = "directory",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = DepartmentReq,
    responses((status = 201, body = Department, headers(("Location" = String))),
              (status = 400, body = crate::openapi::ErrorBody), (status = 401, body = crate::openapi::ErrorBody),
              (status = 403, body = crate::openapi::ErrorBody), (status = 404, body = crate::openapi::ErrorBody),
              (status = 409, body = crate::openapi::ErrorBody, description = "`department.duplicate_name`"))
)]
pub async fn create_department(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<DepartmentReq>,
) -> Result<Response, ApiError> {
    crate::org::require_capability(
        &state,
        org_id,
        auth.user_id,
        Capability::AdminManageAccounts,
        ResourceScope::Organization,
    )
    .await?;
    let name = department_name(&req.name)?;
    let id: Uuid =
        sqlx::query_scalar("INSERT INTO departments (org_id, name) VALUES ($1, $2) RETURNING id")
            .bind(org_id)
            .bind(&name)
            .fetch_one(&state.db)
            .await
            .map_err(|e| match &e {
                sqlx::Error::Database(db) if db.is_unique_violation() => {
                    ApiError::from(DomainError::conflict(
                        "department.duplicate_name",
                        "já existe um departamento com esse nome",
                    ))
                }
                _ => ApiError::from(e),
            })?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "department.created",
        &serde_json::json!({"department_id": id, "name": name}).to_string(),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        [(
            header::LOCATION,
            format!("/api/orgs/{org_id}/departments/{id}"),
        )],
        Json(load_department(&state, org_id, id).await?),
    )
        .into_response())
}

/// Um departamento.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/departments/{department_id}", tag = "directory",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("department_id" = Uuid, Path)),
    responses((status = 200, body = Department), (status = 401, body = crate::openapi::ErrorBody),
              (status = 404, body = crate::openapi::ErrorBody))
)]
pub async fn get_department(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Department>, ApiError> {
    crate::org::require_member_pub(&state, org_id, auth.user_id).await?;
    Ok(Json(load_department(&state, org_id, id).await?))
}

/// Muda o nome de um departamento manual.
#[utoipa::path(
    patch, path = "/api/orgs/{org_id}/departments/{department_id}", tag = "directory",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("department_id" = Uuid, Path)),
    request_body = DepartmentReq,
    responses((status = 200, body = Department), (status = 400, body = crate::openapi::ErrorBody),
              (status = 401, body = crate::openapi::ErrorBody), (status = 403, body = crate::openapi::ErrorBody),
              (status = 404, body = crate::openapi::ErrorBody),
              (status = 409, body = crate::openapi::ErrorBody, description = "`department.managed_by_odoo`, `department.duplicate_name`"))
)]
pub async fn update_department(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
    Json(req): Json<DepartmentReq>,
) -> Result<Json<Department>, ApiError> {
    crate::org::require_capability(
        &state,
        org_id,
        auth.user_id,
        Capability::AdminManageAccounts,
        ResourceScope::Organization,
    )
    .await?;
    let before = load_department(&state, org_id, id).await?;
    if before.source == "odoo" {
        return Err(DomainError::conflict(
            "department.managed_by_odoo",
            "vem do Odoo — muda-se lá",
        )
        .into());
    }
    let name = department_name(&req.name)?;
    sqlx::query(
        "UPDATE departments SET name = $3, updated_at = now() WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id)
    .bind(id)
    .bind(&name)
    .execute(&state.db)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            ApiError::from(DomainError::conflict(
                "department.duplicate_name",
                "já existe um departamento com esse nome",
            ))
        }
        _ => ApiError::from(e),
    })?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "department.renamed",
        &serde_json::json!({"department_id": id, "before": before.name, "after": name}).to_string(),
    )
    .await;
    Ok(Json(load_department(&state, org_id, id).await?))
}

/// Apaga um departamento manual (as pessoas ficam sem departamento).
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/departments/{department_id}", tag = "directory",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("department_id" = Uuid, Path)),
    responses((status = 204), (status = 401, body = crate::openapi::ErrorBody),
              (status = 403, body = crate::openapi::ErrorBody), (status = 404, body = crate::openapi::ErrorBody),
              (status = 409, body = crate::openapi::ErrorBody, description = "`department.managed_by_odoo`, `department.in_use_by_role`"))
)]
pub async fn delete_department(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    crate::org::require_capability(
        &state,
        org_id,
        auth.user_id,
        Capability::AdminManageAccounts,
        ResourceScope::Organization,
    )
    .await?;
    let before = load_department(&state, org_id, id).await?;
    if before.source == "odoo" {
        return Err(DomainError::conflict(
            "department.managed_by_odoo",
            "vem do Odoo — sai quando sair lá",
        )
        .into());
    }
    let pinned: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM org_roles WHERE org_id = $1 AND scope_department_id = $2)",
    )
    .bind(org_id)
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    if pinned {
        return Err(DomainError::conflict(
            "department.in_use_by_role",
            "há papéis com âmbito neste departamento",
        )
        .into());
    }
    let mut tx = state.db.begin().await?;
    let cleared = crate::org::clear_department_tx(&mut tx, org_id, id).await?;
    sqlx::query(
        "UPDATE org_invitations SET department_id = NULL WHERE org_id = $1 AND department_id = $2",
    )
    .bind(org_id)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM departments WHERE org_id = $1 AND id = $2")
        .bind(org_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "department.deleted",
        &serde_json::json!({"department_id": id, "name": before.name, "members_cleared": cleared})
            .to_string(),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
//  Lugares
// ---------------------------------------------------------------------------

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SeatsQuery {
    /// Omissão 60.
    pub inactive_days: Option<i64>,
}

/// Lugares contratados vs usados, actividade do mês e inactivos.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/seats", tag = "directory",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), SeatsQuery),
    responses((status = 200, body = crate::org::SeatSummary), (status = 400, body = crate::openapi::ErrorBody),
              (status = 401, body = crate::openapi::ErrorBody), (status = 403, body = crate::openapi::ErrorBody),
              (status = 404, body = crate::openapi::ErrorBody))
)]
pub async fn seats(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Query(q): Query<SeatsQuery>,
) -> Result<Json<crate::org::SeatSummary>, ApiError> {
    crate::org::require_capability(
        &state,
        org_id,
        auth.user_id,
        Capability::AdminManageAccounts,
        ResourceScope::Organization,
    )
    .await?;
    let days = delonix_meet_domain::organization::seats::validate_inactive_days(
        q.inactive_days.unwrap_or(60),
    )?;
    Ok(Json(crate::org::seat_summary(&state, org_id, days).await?))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ReleaseSeatsReq {
    pub inactive_days: i64,
    /// Só lista quem seria suspenso.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ReleaseSeatsResult {
    pub dry_run: bool,
    pub candidates: Vec<Uuid>,
    pub results: Vec<BulkItemResult>,
    pub seats: crate::org::SeatSummary,
}

/// Liberta lugares suspendendo (razão `inactive`) quem não entra há N dias.
/// Nunca o dono nem quem pede.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/seats/release", tag = "directory",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = ReleaseSeatsReq,
    responses((status = 200, body = ReleaseSeatsResult), (status = 400, body = crate::openapi::ErrorBody),
              (status = 401, body = crate::openapi::ErrorBody), (status = 403, body = crate::openapi::ErrorBody),
              (status = 404, body = crate::openapi::ErrorBody))
)]
pub async fn release_seats(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<ReleaseSeatsReq>,
) -> Result<Json<ReleaseSeatsResult>, ApiError> {
    crate::org::require_capability(
        &state,
        org_id,
        auth.user_id,
        Capability::AdminManageAccounts,
        ResourceScope::Organization,
    )
    .await?;
    let days = delonix_meet_domain::organization::seats::validate_inactive_days(req.inactive_days)?;
    let candidates: Vec<Uuid> = crate::org::inactive_occupants(&state.db, org_id, days)
        .await?
        .into_iter()
        .filter(|u| *u != auth.user_id)
        .collect();
    let mut results = Vec::new();
    if !req.dry_run {
        for u in &candidates {
            let mut tx = state.db.begin().await?;
            let r = match crate::org::archive_member_tx(
                &mut tx,
                org_id,
                *u,
                "inactive",
                Some(auth.user_id),
            )
            .await
            {
                Ok(changed) => match tx.commit().await {
                    Ok(()) => {
                        if changed {
                            crate::audit::log(&state.db, Some(org_id), auth.user_id, "member.suspended",
                                &serde_json::json!({"user_id": u, "reason": "inactive", "inactive_days": days}).to_string()).await;
                        }
                        item_ok("member", *u, changed, None)
                    }
                    Err(e) => item_err("member", *u, crate::org::map_member_write_error(e)),
                },
                Err(e) => item_err("member", *u, e),
            };
            results.push(r);
        }
    }
    Ok(Json(ReleaseSeatsResult {
        dry_run: req.dry_run,
        candidates,
        results,
        seats: crate::org::seat_summary(&state, org_id, days).await?,
    }))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct SeatLimitReq {
    /// `null` = sem tecto.
    pub max_seats: Option<i64>,
}

/// Tecto de lugares de uma organização — só o operador da plataforma.
#[utoipa::path(
    put, path = "/api/operator/v1/organizations/{org_id}/seats", tag = "platform",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = SeatLimitReq,
    responses((status = 200, body = crate::org::SeatSummary), (status = 400, body = crate::openapi::ErrorBody),
              (status = 401, body = crate::openapi::ErrorBody),
              (status = 403, body = crate::openapi::ErrorBody, description = "não é administrador da plataforma"),
              (status = 404, body = crate::openapi::ErrorBody), (status = 429, body = crate::openapi::ErrorBody))
)]
pub async fn operator_set_seats(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<SeatLimitReq>,
) -> Result<Json<crate::org::SeatSummary>, ApiError> {
    crate::storage::require_platform_admin(&state, auth.user_id)?;
    let limit = delonix_meet_domain::organization::seats::validate_limit(req.max_seats)?;
    let n = sqlx::query("UPDATE organizations SET max_seats = $2 WHERE id = $1")
        .bind(org_id)
        .bind(limit)
        .execute(&state.db)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(ApiError::NotFound);
    }
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "org.seat_limit_set",
        &serde_json::json!({"max_seats": limit}).to_string(),
    )
    .await;
    Ok(Json(crate::org::seat_summary(&state, org_id, 60).await?))
}

// ---------------------------------------------------------------------------
//  Aprovisionamento e regras de entrada
// ---------------------------------------------------------------------------

#[derive(Serialize, utoipa::ToSchema)]
pub struct AttendanceLog {
    pub enabled: bool,
    /// `false` enquanto não houver integração com `hr.attendance`.
    pub available: bool,
    pub reason: &'static str,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct EntryRules {
    /// Criar conta na primeira entrada pelo Odoo (só para `approved_domains`, se houver).
    pub create_account_on_first_login: bool,
    pub approved_domains: Vec<String>,
    /// Suspender (razão `odoo_exit`) quem deixa de estar activo no Odoo.
    pub suspend_on_odoo_exit: bool,
    pub external_guest_ttl_hours: i32,
    pub attendance_log: AttendanceLog,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct EntryRulesReq {
    pub create_account_on_first_login: bool,
    pub approved_domains: Vec<String>,
    pub suspend_on_odoo_exit: bool,
    pub external_guest_ttl_hours: i32,
    /// Recusado a `true` enquanto a integração não existir.
    #[serde(default)]
    pub attendance_log_enabled: bool,
}

/// Regras de entrada.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/entry-rules", tag = "directory",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses((status = 200, body = EntryRules), (status = 401, body = crate::openapi::ErrorBody),
              (status = 403, body = crate::openapi::ErrorBody), (status = 404, body = crate::openapi::ErrorBody))
)]
pub async fn get_entry_rules(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<EntryRules>, ApiError> {
    crate::org::require_capability(
        &state,
        org_id,
        auth.user_id,
        Capability::AdminManageAccounts,
        ResourceScope::Organization,
    )
    .await?;
    Ok(Json(entry_rules_of(&state.db, org_id).await?))
}

/// Substitui as regras de entrada.
#[utoipa::path(
    put, path = "/api/orgs/{org_id}/entry-rules", tag = "directory",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = EntryRulesReq,
    responses((status = 200, body = EntryRules),
              (status = 400, body = crate::openapi::ErrorBody, description = "`entry_rules.invalid_domain`, `entry_rules.invalid_ttl`"),
              (status = 401, body = crate::openapi::ErrorBody), (status = 403, body = crate::openapi::ErrorBody),
              (status = 404, body = crate::openapi::ErrorBody),
              (status = 422, body = crate::openapi::ErrorBody, description = "`entry_rules.attendance_unavailable`"))
)]
pub async fn put_entry_rules(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<EntryRulesReq>,
) -> Result<Json<EntryRules>, ApiError> {
    crate::org::require_capability(
        &state,
        org_id,
        auth.user_id,
        Capability::AdminManageAccounts,
        ResourceScope::Organization,
    )
    .await?;
    if req.attendance_log_enabled {
        return Err(DomainError::precondition(
            "entry_rules.attendance_unavailable",
            "o registo de presenças só se liga quando existir a integração com hr.attendance",
        )
        .into());
    }
    if !(1..=8760).contains(&req.external_guest_ttl_hours) {
        return Err(DomainError::invalid(
            "entry_rules.invalid_ttl",
            "external_guest_ttl_hours é 1–8760",
        )
        .with_field("external_guest_ttl_hours", "1–8760")
        .into());
    }
    let mut domains = BTreeSet::new();
    for d in &req.approved_domains {
        let d = d.trim().trim_start_matches('@').to_lowercase();
        let ok = d.len() <= 253
            && d.contains('.')
            && d.split('.')
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
        if !ok {
            return Err(DomainError::invalid(
                "entry_rules.invalid_domain",
                format!("domínio inválido: {d}"),
            )
            .with_field("approved_domains", "domínios como empresa.co.ao")
            .into());
        }
        domains.insert(d);
    }
    if domains.len() > 50 {
        return Err(
            DomainError::invalid("entry_rules.invalid_domain", "no máximo 50 domínios")
                .with_field("approved_domains", "≤ 50")
                .into(),
        );
    }
    let before = entry_rules_of(&state.db, org_id).await?;
    let domains: Vec<String> = domains.into_iter().collect();
    sqlx::query(
        "INSERT INTO org_entry_rules (org_id, create_account_on_first_login, approved_domains,
                                      suspend_on_odoo_exit, external_guest_ttl_hours, updated_at, updated_by)
         VALUES ($1, $2, $3, $4, $5, now(), $6)
         ON CONFLICT (org_id) DO UPDATE SET
            create_account_on_first_login = EXCLUDED.create_account_on_first_login,
            approved_domains = EXCLUDED.approved_domains,
            suspend_on_odoo_exit = EXCLUDED.suspend_on_odoo_exit,
            external_guest_ttl_hours = EXCLUDED.external_guest_ttl_hours,
            updated_at = now(), updated_by = EXCLUDED.updated_by",
    )
    .bind(org_id)
    .bind(req.create_account_on_first_login)
    .bind(&domains)
    .bind(req.suspend_on_odoo_exit)
    .bind(req.external_guest_ttl_hours)
    .bind(auth.user_id)
    .execute(&state.db)
    .await?;
    crate::audit::log(&state.db, Some(org_id), auth.user_id, "entry_rules.updated",
        &serde_json::json!({
            "before": {"create_account_on_first_login": before.create_account_on_first_login,
                       "approved_domains": before.approved_domains, "suspend_on_odoo_exit": before.suspend_on_odoo_exit,
                       "external_guest_ttl_hours": before.external_guest_ttl_hours},
            "after": {"create_account_on_first_login": req.create_account_on_first_login,
                      "approved_domains": domains, "suspend_on_odoo_exit": req.suspend_on_odoo_exit,
                      "external_guest_ttl_hours": req.external_guest_ttl_hours}}).to_string()).await;
    Ok(Json(entry_rules_of(&state.db, org_id).await?))
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct OdooProvisioning {
    /// A integração Odoo desta org está ligada.
    pub enabled: bool,
    /// A leitura periódica do directório corre no login, quando a última tem mais de isto.
    pub sync_max_age_seconds: i64,
    pub last_synced_at: Option<DateTime<Utc>>,
    /// Resultado da última corrida (`created`, `updated`, `skipped`, `suspended`, `role_changes`,
    /// `conflicts`, `groups_read`); `null` = nenhuma desde a 0062.
    #[schema(value_type = Object)]
    pub last_result: Option<serde_json::Value>,
    pub pending_role_conflicts: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct Provisioning {
    pub odoo: OdooProvisioning,
    pub entry_rules: EntryRules,
}

/// Estado do aprovisionamento (Odoo) e regras de entrada.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/provisioning", tag = "directory",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses((status = 200, body = Provisioning), (status = 401, body = crate::openapi::ErrorBody),
              (status = 403, body = crate::openapi::ErrorBody), (status = 404, body = crate::openapi::ErrorBody))
)]
pub async fn provisioning(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Provisioning>, ApiError> {
    crate::org::require_capability(
        &state,
        org_id,
        auth.user_id,
        Capability::AdminManageAccounts,
        ResourceScope::Organization,
    )
    .await?;
    let (enabled, last_synced_at, last_result): (
        bool,
        Option<DateTime<Utc>>,
        Option<serde_json::Value>,
    ) = sqlx::query_as(
        "SELECT odoo_enabled, odoo_synced_at, odoo_last_sync FROM organizations WHERE id = $1",
    )
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;
    let pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM role_conflicts WHERE org_id = $1 AND status = 'pending'",
    )
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(Provisioning {
        odoo: OdooProvisioning {
            enabled,
            sync_max_age_seconds: crate::odoo_sso::SYNC_MAX_AGE_SECS,
            last_synced_at,
            last_result,
            pending_role_conflicts: pending,
        },
        entry_rules: entry_rules_of(&state.db, org_id).await?,
    }))
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        list_users,
        users_schema,
        bulk_actions,
        import_users,
        create_invitation,
        list_invitations,
        get_invitation,
        revoke_invitation,
        resend_invitation,
        accept_invitation,
        list_departments,
        create_department,
        get_department,
        update_department,
        delete_department,
        seats,
        release_seats,
        operator_set_seats,
        get_entry_rules,
        put_entry_rules,
        provisioning,
    ),
    components(schemas(
        DirectoryEntry,
        DirectoryGroup,
        DirectoryCounts,
        DirectoryPage,
        Invitation,
        InvitationWithToken,
        CreateInvitationReq,
        InvitationPage,
        AcceptInvitationReq,
        AcceptedInvitation,
        BulkActionReq,
        BulkItemResult,
        BulkActionResult,
        ImportReq,
        ImportLine,
        ImportReport,
        Department,
        DepartmentPage,
        DepartmentReq,
        ReleaseSeatsReq,
        ReleaseSeatsResult,
        SeatLimitReq,
        AttendanceLog,
        EntryRules,
        EntryRulesReq,
        OdooProvisioning,
        Provisioning,
        crate::org::SeatSummary,
    ))
)]
pub struct ApiDoc;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_line_split_handles_quotes_and_separators() {
        assert_eq!(
            split_csv_line("a@b.co, \"Silva, Ana\" ,Membro", ','),
            vec!["a@b.co", "Silva, Ana", "Membro"]
        );
        assert_eq!(
            split_csv_line("x;\"diz \"\"olá\"\"\";", ';'),
            vec!["x", "diz \"olá\"", ""]
        );
    }

    #[test]
    fn domain_rule() {
        assert!(
            domain_allowed("a@x.ao", &[], ""),
            "sem regra: qualquer domínio"
        );
        assert!(domain_allowed("a@x.ao", &[], "x.ao"));
        assert!(!domain_allowed("a@y.ao", &[], "x.ao"));
        assert!(domain_allowed("a@y.ao", &["y.ao".into()], "x.ao"));
        assert!(!domain_allowed("a@z.ao", &["y.ao".into()], ""));
    }

    #[test]
    fn codes_are_normalised_and_links_are_not() {
        assert_eq!(token_hash("ab-cd"), token_hash("ABCD"));
        assert_ne!(token_hash("dlxi_ab"), token_hash("dlxi_AB"));
        let code = new_token("code");
        assert_eq!(code.len(), 10);
        assert!(code.bytes().all(|b| CODE_ALPHABET.contains(&b)));
        assert!(new_token("link").starts_with("dlxi_"));
    }

    #[test]
    fn filters_and_orders() {
        let q = |filters: &str| UsersQuery {
            q: Some("  Ana  emissão ".into()),
            filters: Some(filters.into()),
            filter: None,
            group_by: None,
            order_by: None,
            department_id: None,
            role_id: None,
            page_size: None,
            page_token: None,
        };
        let f = build_filter(&q("inactive_90_days,inactive_60_days,active,origin_code")).unwrap();
        assert_eq!(f.inactive_days, Some(60));
        assert_eq!(f.terms, vec!["ana", "emissão"]);
        assert_eq!(f.statuses, vec!["active"]);
        assert_eq!(f.origins, vec!["code"]);
        assert!(build_filter(&q("nope")).is_err());
        let mut with_domain = q("");
        with_domain.filter = Some("[]".into());
        match build_filter(&with_domain) {
            Err(ApiError::Domain(e)) => assert_eq!(e.code, "search.filter_unsupported"),
            _ => panic!("filter devia ser recusado"),
        }
        assert!(order_of(Some("-last_access_at")).is_ok());
        assert!(order_of(Some("email")).is_err());
    }
}
