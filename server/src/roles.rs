//! Papéis e permissões (ADR-0008) — adaptador HTTP + Postgres do ecrã
//! «Papéis e permissões». A regra está em
//! `delonix_meet_domain::identity::authorization`; a imposição e tudo o que
//! toca `org_members` está em `org.rs`.
//!
//! - `GET    /api/capabilities`                                    catálogo estático
//! - `GET    /api/orgs/{org_id}/roles`                             lista (com contagem de pessoas)
//! - `POST   /api/orgs/{org_id}/roles`                             `201` + `Location`
//! - `GET    /api/orgs/{org_id}/roles/{role_id}`
//! - `PATCH  /api/orgs/{org_id}/roles/{role_id}`                   nome, descrição, herança, âmbito, limites, grupo Odoo
//! - `DELETE /api/orgs/{org_id}/roles/{role_id}?reassign_to=`      `204`
//! - `POST   /api/orgs/{org_id}/roles/{role_id}/duplicate`         `201`
//! - `GET    /api/orgs/{org_id}/roles/{role_id}/capabilities`      a coluna da matriz
//! - `PUT    /api/orgs/{org_id}/roles/{role_id}/capabilities`      substitui a coluna
//! - `GET    /api/orgs/{org_id}/permission-matrix[?format=csv]`    a matriz inteira / CSV
//! - `POST   /api/orgs/{org_id}/authorization/evaluations`         simular (só leitura)
//! - `GET    /api/orgs/{org_id}/members/me/capabilities`           as minhas
//! - `PUT    /api/orgs/{org_id}/members/{user_id}/role`            atribuir papel
//! - `…/sod-rules`, `…/sod-violations`, `…/sod-rules/{rule_id}/risk-acceptances`
//! - `…/role-conflicts`, `…/role-conflicts/{conflict_id}/resolve`
//!
//! Escritas de papéis recalculam `org_role_effective_capabilities` na mesma
//! transacção (`recompute_effective_tx`).

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::{
    page::{Page, PageRequest},
    DomainError,
};
use delonix_meet_domain::identity::authorization::{
    self as authz, Capability, CapabilityValue, Decision, ProposedRole, ResourceScope, RoleDef,
    RoleLimits, RoleScope, RoleSet, Subject, SystemRole,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

// ---------------------------------------------------------------------------
//  Store
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
struct RoleRow {
    id: Uuid,
    system_key: Option<String>,
    name: String,
    description: String,
    inherits_from: Option<Uuid>,
    scope: String,
    scope_department_id: Option<Uuid>,
    department_name: Option<String>,
    max_simultaneous_destinations: Option<i32>,
    max_external_guests_per_month: Option<i32>,
    eff_max_simultaneous_destinations: Option<i32>,
    eff_max_external_guests_per_month: Option<i32>,
    odoo_group: Option<String>,
    updated_at: DateTime<Utc>,
    updated_by_name: Option<String>,
}

const ROLE_COLUMNS: &str = "r.id, r.system_key, r.name, r.description, r.inherits_from, r.scope, \
     r.scope_department_id, d.name AS department_name, r.max_simultaneous_destinations, \
     r.max_external_guests_per_month, r.eff_max_simultaneous_destinations, \
     r.eff_max_external_guests_per_month, r.odoo_group, r.updated_at, uu.username AS updated_by_name";
const ROLE_FROM: &str = "org_roles r LEFT JOIN departments d ON d.id = r.scope_department_id \
     LEFT JOIN users uu ON uu.id = r.updated_by";

async fn role_rows<'e, E>(db: E, org_id: Uuid) -> Result<Vec<RoleRow>, ApiError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    Ok(sqlx::query_as(&format!(
        "SELECT {ROLE_COLUMNS} FROM {ROLE_FROM} WHERE r.org_id = $1
          ORDER BY CASE r.system_key WHEN 'owner' THEN 0 WHEN 'admin' THEN 1 ELSE 2 END,
                   (r.system_key IS NOT NULL AND r.system_key <> 'member' AND r.system_key <> 'external_guest'),
                   r.system_key NULLS FIRST, lower(r.name), r.id"
    ))
    .bind(org_id)
    .fetch_all(db)
    .await?)
}

fn row_to_def(r: &RoleRow, values: HashMap<Capability, CapabilityValue>) -> RoleDef {
    RoleDef {
        id: r.id,
        name: r.name.clone(),
        system: r.system_key.as_deref().and_then(SystemRole::parse),
        inherits_from: r.inherits_from,
        scope: if r.scope == "department" {
            RoleScope::Department {
                department_id: r.scope_department_id,
            }
        } else {
            RoleScope::Organization
        },
        values,
        limits: RoleLimits {
            max_simultaneous_destinations: r.max_simultaneous_destinations,
            max_external_guests_per_month: r.max_external_guests_per_month,
        },
    }
}

/// Os papéis da org com as matrizes, prontos para a policy.
async fn load_role_set(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
) -> Result<(RoleSet, Vec<RoleRow>), ApiError> {
    let rows = role_rows(&mut **tx, org_id).await?;
    let caps: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT c.role_id, c.capability, c.value FROM org_role_capabilities c
           JOIN org_roles r ON r.id = c.role_id WHERE r.org_id = $1",
    )
    .bind(org_id)
    .fetch_all(&mut **tx)
    .await?;
    let mut by_role: HashMap<Uuid, HashMap<Capability, CapabilityValue>> = HashMap::new();
    for (role, cap, value) in caps {
        // Um código que saiu do catálogo é ignorado (a migração que o tira limpa-o).
        if let (Ok(c), Ok(v)) = (Capability::parse(&cap), CapabilityValue::parse(&value)) {
            by_role.entry(role).or_default().insert(c, v);
        }
    }
    let defs = rows
        .iter()
        .map(|r| row_to_def(r, by_role.remove(&r.id).unwrap_or_default()))
        .collect();
    Ok((RoleSet::new(defs)?, rows))
}

/// Os papéis da org para a policy, dentro de uma transacção de quem chama.
pub(crate) async fn role_set_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
) -> Result<RoleSet, ApiError> {
    Ok(load_role_set(tx, org_id).await?.0)
}

async fn load_role_set_db(
    state: &AppState,
    org_id: Uuid,
) -> Result<(RoleSet, Vec<RoleRow>), ApiError> {
    let mut tx = state.db.begin().await?;
    let out = load_role_set(&mut tx, org_id).await?;
    tx.commit().await?;
    Ok(out)
}

fn decision_str(d: Decision) -> &'static str {
    match d {
        Decision::Allow => "allow",
        Decision::Deny => "deny",
        Decision::RequiresApproval => "requires_approval",
    }
}

/// Recalcula as decisões efectivas e os limites efectivos de TODOS os papéis
/// da org, com a policy do domínio, na transacção da escrita (ADR-0008 §4).
pub(crate) async fn recompute_effective_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
) -> Result<(), ApiError> {
    let (set, rows) = load_role_set(tx, org_id).await?;
    let mut role_ids = Vec::new();
    let mut caps = Vec::new();
    let mut org_d = Vec::new();
    let mut dept_d = Vec::new();
    for r in &rows {
        for (cap, eff) in authz::effective_for_role(&set, r.id) {
            role_ids.push(r.id);
            caps.push(cap.as_str().to_string());
            org_d.push(decision_str(eff.organization).to_string());
            dept_d.push(decision_str(eff.own_department).to_string());
        }
        let limits = authz::effective_limits(&set, r.id);
        sqlx::query(
            "UPDATE org_roles SET eff_max_simultaneous_destinations = $2,
                                  eff_max_external_guests_per_month = $3
              WHERE id = $1",
        )
        .bind(r.id)
        .bind(limits.max_simultaneous_destinations)
        .bind(limits.max_external_guests_per_month)
        .execute(&mut **tx)
        .await?;
    }
    sqlx::query(
        "DELETE FROM org_role_effective_capabilities
          WHERE role_id IN (SELECT id FROM org_roles WHERE org_id = $1)",
    )
    .bind(org_id)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO org_role_effective_capabilities (role_id, capability, org_decision, dept_decision)
         SELECT * FROM unnest($1::uuid[], $2::text[], $3::text[], $4::text[])",
    )
    .bind(&role_ids)
    .bind(&caps)
    .bind(&org_d)
    .bind(&dept_d)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Sujeito de quem pede (ou 404).
async fn actor_subject(state: &AppState, org_id: Uuid, user: Uuid) -> Result<Subject, ApiError> {
    let (role_id, department_id) = crate::org::member_subject(&state.db, org_id, user)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Subject {
        role_id,
        department_id,
    })
}

/// Sem escalada ao atribuir `role_id` (ADR-0008 §5).
pub(crate) async fn ensure_can_assign(
    state: &AppState,
    org_id: Uuid,
    actor: Uuid,
    role_id: Uuid,
) -> Result<(), ApiError> {
    let subject = actor_subject(state, org_id, actor).await?;
    let (set, _) = load_role_set_db(state, org_id).await?;
    authz::validate_assignment(&set, role_id, subject)?;
    Ok(())
}

pub(crate) async fn ensure_can_assign_system(
    state: &AppState,
    org_id: Uuid,
    actor: Uuid,
    role: SystemRole,
) -> Result<(), ApiError> {
    let role_id: Uuid =
        sqlx::query_scalar("SELECT id FROM org_roles WHERE org_id = $1 AND system_key = $2")
            .bind(org_id)
            .bind(role.key())
            .fetch_one(&state.db)
            .await?;
    ensure_can_assign(state, org_id, actor, role_id).await
}

async fn require_roles(
    state: &AppState,
    org_id: Uuid,
    user: Uuid,
) -> Result<crate::org::Grant, ApiError> {
    crate::org::require_capability(
        state,
        org_id,
        user,
        Capability::AdminManageRoles,
        ResourceScope::Organization,
    )
    .await
}

// ---------------------------------------------------------------------------
//  DTOs
// ---------------------------------------------------------------------------

#[derive(Serialize, utoipa::ToSchema)]
pub struct CapabilityCatalog {
    pub catalog_version: u32,
    pub values: Vec<&'static str>,
    pub items: Vec<CapabilityItem>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CapabilityItem {
    pub code: &'static str,
    pub group: &'static str,
    pub group_label: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    /// Se `false`, o servidor ainda não a impõe e a matriz só aceita o valor por omissão.
    pub enforced: bool,
    pub enforced_at: Vec<&'static str>,
    /// Só papéis de sistema.
    pub system_only: bool,
    pub approval_supported: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RoleScopeDto {
    /// `organization` | `department`.
    pub kind: String,
    pub department_id: Option<Uuid>,
    pub department_name: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct LimitDto {
    /// O valor escrito no papel (`null` = herda ou sem limite).
    pub value: Option<i32>,
    /// O valor que vale (herança resolvida). `null` = sem limite.
    pub effective: Option<i32>,
    pub enforced: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RoleLimitsDto {
    pub max_simultaneous_destinations: LimitDto,
    pub max_external_guests_per_month: LimitDto,
    /// Não imposto: nunca gravável (`role.limit_not_enforced`).
    pub max_session_minutes: LimitDto,
    /// Não imposto: nunca gravável (`role.limit_not_enforced`).
    pub max_resolution_p: LimitDto,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct OdooSyncDto {
    pub group: String,
    /// Pessoas activas que o Odoo diz estarem no grupo.
    pub total: i64,
    /// Dessas, as que têm este papel.
    pub synced: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct Role {
    pub id: Uuid,
    /// `owner` | `admin` | `member` | `external_guest`; `null` num personalizado.
    pub key: Option<String>,
    pub name: String,
    pub description: String,
    /// Papel de sistema: não se altera nem se apaga.
    pub system: bool,
    pub inherits_from: Option<Uuid>,
    pub scope: RoleScopeDto,
    pub limits: RoleLimitsDto,
    pub odoo_group: Option<String>,
    /// Só quando há `odoo_group`.
    pub odoo_sync: Option<OdooSyncDto>,
    pub member_count: i64,
    pub updated_at: DateTime<Utc>,
    pub updated_by_name: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RolePage {
    pub items: Vec<Role>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RoleWithWarnings {
    #[serde(flatten)]
    pub role: Role,
    /// Regras de segregação de funções que este papel, sozinho, já viola.
    pub warnings: Vec<SodWarning>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SodWarning {
    pub code: &'static str,
    pub rule_id: Uuid,
    pub rule_name: String,
}

#[derive(Deserialize, Default, utoipa::ToSchema)]
pub struct ScopeReq {
    /// `organization` | `department`.
    pub kind: String,
    #[serde(default)]
    pub department_id: Option<Uuid>,
}

#[derive(Deserialize, Default, utoipa::ToSchema)]
pub struct LimitsReq {
    #[serde(default)]
    pub max_simultaneous_destinations: Option<i32>,
    #[serde(default)]
    pub max_external_guests_per_month: Option<i32>,
    /// Recusado se não for `null` (ainda não imposto).
    #[serde(default)]
    pub max_session_minutes: Option<i32>,
    /// Recusado se não for `null` (ainda não imposto).
    #[serde(default)]
    pub max_resolution_p: Option<i32>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateRoleReq {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Por omissão, o `member` de sistema.
    #[serde(default)]
    pub inherits_from: Option<Uuid>,
    #[serde(default)]
    pub scope: Option<ScopeReq>,
    #[serde(default)]
    pub limits: Option<LimitsReq>,
    #[serde(default)]
    pub odoo_group: Option<String>,
    /// Código → `allow` | `deny` | `inherit` | `requires_approval`. Ausente = `inherit`.
    #[serde(default)]
    #[schema(value_type = Object)]
    pub capabilities: BTreeMap<String, String>,
}

#[derive(Deserialize, Default, utoipa::ToSchema)]
pub struct UpdateRoleReq {
    pub name: Option<String>,
    pub description: Option<String>,
    pub inherits_from: Option<Uuid>,
    pub scope: Option<ScopeReq>,
    /// Substitui os limites todos.
    pub limits: Option<LimitsReq>,
    /// `""` retira o grupo.
    pub odoo_group: Option<String>,
}

#[derive(Deserialize, Default, utoipa::ToSchema)]
pub struct DuplicateRoleReq {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct DeleteRoleQuery {
    /// Papel para onde passam as pessoas (obrigatório se o papel tiver pessoas).
    pub reassign_to: Option<Uuid>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RoleCapabilityRow {
    pub capability: &'static str,
    /// O valor gravado neste papel.
    pub value: &'static str,
    /// Decisão efectiva para um recurso da organização.
    pub organization: &'static str,
    /// Decisão efectiva para um recurso do departamento da pessoa.
    pub own_department: &'static str,
    /// Não se pode mudar (papel de sistema, não imposta, ou só de sistema).
    pub locked: bool,
    /// `system_role` | `not_enforced` | `system_only` | `null`.
    pub locked_reason: Option<&'static str>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RoleCapabilities {
    pub role_id: Uuid,
    pub catalog_version: u32,
    pub items: Vec<RoleCapabilityRow>,
    pub warnings: Vec<SodWarning>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct PutCapabilitiesReq {
    /// A coluna inteira: código → valor. Ausente = `inherit`.
    #[schema(value_type = Object)]
    pub values: BTreeMap<String, String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct MatrixRole {
    pub id: Uuid,
    pub key: Option<String>,
    pub name: String,
    pub member_count: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct MatrixRow {
    pub capability: &'static str,
    pub group: &'static str,
    pub group_label: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub enforced: bool,
    /// `role_id` → valor gravado.
    #[schema(value_type = Object)]
    pub values: BTreeMap<Uuid, &'static str>,
    /// `role_id` → decisão efectiva (âmbito organização).
    #[schema(value_type = Object)]
    pub effective: BTreeMap<Uuid, &'static str>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PermissionMatrix {
    pub catalog_version: u32,
    pub roles: Vec<MatrixRole>,
    pub rows: Vec<MatrixRow>,
    pub member_total: i64,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct MatrixQuery {
    /// `json` (omissão) | `csv`.
    pub format: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct EvaluationReq {
    pub user_id: Uuid,
    /// Uma capacidade; ausente = todas.
    #[serde(default)]
    pub capability: Option<String>,
    /// Âmbito do recurso; ausente = organização.
    #[serde(default)]
    pub department_id: Option<Uuid>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RoleRef {
    pub id: Uuid,
    pub name: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct EvaluationItem {
    pub capability: &'static str,
    /// `allow` | `deny` | `requires_approval`.
    pub decision: &'static str,
    /// `role_value` | `inherited` | `out_of_scope_member` | `not_granted`.
    pub reason: &'static str,
    pub via_role: Option<RoleRef>,
    pub chain: Vec<RoleRef>,
    pub enforced: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct EvaluationResult {
    pub user_id: Uuid,
    pub role: RoleRef,
    pub member_department_id: Option<Uuid>,
    /// `organization` | `department`.
    pub resource_scope: &'static str,
    pub resource_department_id: Option<Uuid>,
    pub items: Vec<EvaluationItem>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct MyCapability {
    pub capability: String,
    pub organization: String,
    pub own_department: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct MyCapabilities {
    pub role_id: Uuid,
    pub role_key: Option<String>,
    pub department_id: Option<Uuid>,
    pub catalog_version: u32,
    pub items: Vec<MyCapability>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct AssignRoleReq {
    pub role_id: Uuid,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RoleAssignment {
    pub user_id: Uuid,
    pub role_id: Uuid,
    pub role_name: String,
    pub changed: bool,
}

#[derive(Serialize, Deserialize)]
struct NameCursor {
    name: String,
    id: Uuid,
}

// ---------------------------------------------------------------------------
//  Montagem
// ---------------------------------------------------------------------------

fn value_str(v: CapabilityValue) -> &'static str {
    v.as_str()
}

async fn role_dto(
    state: &AppState,
    org_id: Uuid,
    r: &RoleRow,
    counts: &HashMap<Uuid, i64>,
) -> Result<Role, ApiError> {
    let odoo_sync = match &r.odoo_group {
        Some(g) => {
            let (total, synced) =
                crate::org::odoo_group_sync_counts(&state.db, org_id, g, r.id).await?;
            Some(OdooSyncDto {
                group: g.clone(),
                total,
                synced,
            })
        }
        None => None,
    };
    let not_enforced = || LimitDto {
        value: None,
        effective: None,
        enforced: false,
    };
    Ok(Role {
        id: r.id,
        key: r.system_key.clone(),
        name: r.name.clone(),
        description: r.description.clone(),
        system: r.system_key.is_some(),
        inherits_from: r.inherits_from,
        scope: RoleScopeDto {
            kind: r.scope.clone(),
            department_id: r.scope_department_id,
            department_name: r.department_name.clone(),
        },
        limits: RoleLimitsDto {
            max_simultaneous_destinations: LimitDto {
                value: r.max_simultaneous_destinations,
                effective: r.eff_max_simultaneous_destinations,
                enforced: true,
            },
            max_external_guests_per_month: LimitDto {
                value: r.max_external_guests_per_month,
                effective: r.eff_max_external_guests_per_month,
                enforced: true,
            },
            max_session_minutes: not_enforced(),
            max_resolution_p: not_enforced(),
        },
        odoo_group: r.odoo_group.clone(),
        odoo_sync,
        member_count: counts.get(&r.id).copied().unwrap_or(0),
        updated_at: r.updated_at,
        updated_by_name: r.updated_by_name.clone(),
    })
}

async fn load_one_role(state: &AppState, org_id: Uuid, role_id: Uuid) -> Result<Role, ApiError> {
    let row: Option<RoleRow> = sqlx::query_as(&format!(
        "SELECT {ROLE_COLUMNS} FROM {ROLE_FROM} WHERE r.org_id = $1 AND r.id = $2"
    ))
    .bind(org_id)
    .bind(role_id)
    .fetch_optional(&state.db)
    .await?;
    let row = row.ok_or_else(|| ApiError::from(DomainError::not_found("role.not_found")))?;
    let counts = crate::org::member_counts_by_role(&state.db, org_id).await?;
    role_dto(state, org_id, &row, &counts).await
}

fn parse_values(
    raw: &BTreeMap<String, String>,
) -> Result<HashMap<Capability, CapabilityValue>, ApiError> {
    let mut out = HashMap::new();
    for (k, v) in raw {
        out.insert(Capability::parse(k)?, CapabilityValue::parse(v)?);
    }
    Ok(out)
}

fn validate_limits(l: &LimitsReq) -> Result<(), ApiError> {
    for (field, v) in [
        ("max_session_minutes", l.max_session_minutes),
        ("max_resolution_p", l.max_resolution_p),
    ] {
        if v.is_some() {
            return Err(DomainError::precondition(
                "role.limit_not_enforced",
                format!("{field} ainda não é imposto pelo servidor e não se grava"),
            )
            .with_field(field, "null")
            .into());
        }
    }
    authz::validate_limit(
        "max_simultaneous_destinations",
        l.max_simultaneous_destinations,
    )?;
    authz::validate_limit(
        "max_external_guests_per_month",
        l.max_external_guests_per_month,
    )?;
    Ok(())
}

async fn check_scope(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    scope: &ScopeReq,
) -> Result<(String, Option<Uuid>), ApiError> {
    match scope.kind.as_str() {
        "organization" => Ok(("organization".into(), None)),
        "department" => {
            let Some(d) = scope.department_id else {
                return Err(DomainError::invalid(
                    "role.department_required",
                    "um papel de departamento indica o departamento",
                )
                .with_field("scope.department_id", "uuid")
                .into());
            };
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM departments WHERE org_id = $1 AND id = $2)",
            )
            .bind(org_id)
            .bind(d)
            .fetch_one(&mut **tx)
            .await?;
            if !exists {
                return Err(DomainError::not_found("department.not_found").into());
            }
            Ok(("department".into(), Some(d)))
        }
        _ => Err(
            DomainError::invalid("role.invalid_scope", "âmbito desconhecido")
                .with_field("scope.kind", "organization | department")
                .into(),
        ),
    }
}

fn unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.is_unique_violation())
}

/// A org não pode ter um grupo Odoo que dê um papel com capacidades que quem o
/// mapeia não tem (ADR-0008 §5).
fn check_group_mapping(
    set: &RoleSet,
    role_id: Uuid,
    actor: Subject,
    group: Option<&str>,
) -> Result<Option<String>, ApiError> {
    let Some(g) = group.map(str::trim).filter(|g| !g.is_empty()) else {
        return Ok(None);
    };
    let g = authz::validate_odoo_group(g)?;
    if set.get(role_id).and_then(|r| r.system) == Some(SystemRole::Owner) {
        return Err(DomainError::precondition(
            "role.odoo_group_owner_forbidden",
            "um grupo do Odoo nunca dá o papel de Proprietário",
        )
        .into());
    }
    authz::validate_assignment(set, role_id, actor)?;
    Ok(Some(g))
}

async fn sod_warnings(
    state: &AppState,
    org_id: Uuid,
    set: &RoleSet,
    role_id: Uuid,
) -> Result<Vec<SodWarning>, ApiError> {
    let rules = load_sod_rules(&state.db, org_id).await?;
    Ok(rules
        .into_iter()
        .filter(|(rule, _)| {
            authz::violates(
                set,
                Subject {
                    role_id,
                    department_id: None,
                },
                rule,
            )
        })
        .map(|(rule, name)| SodWarning {
            code: "sod.role_grants_combination",
            rule_id: rule.id,
            rule_name: name,
        })
        .collect())
}

fn audit_json(v: serde_json::Value) -> String {
    v.to_string()
}

// ---------------------------------------------------------------------------
//  Handlers — catálogo, papéis, matriz
// ---------------------------------------------------------------------------

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        catalog,
        list_roles,
        create_role,
        get_role,
        update_role,
        delete_role,
        duplicate_role,
        get_capabilities,
        put_capabilities,
        matrix,
        evaluate,
        my_capabilities,
        assign_role,
        list_sod_rules,
        create_sod_rule,
        get_sod_rule,
        update_sod_rule,
        delete_sod_rule,
        sod_violations,
        accept_risk,
        list_role_conflicts,
        resolve_role_conflict,
    ),
    components(schemas(
        CapabilityCatalog,
        CapabilityItem,
        Role,
        RolePage,
        RoleWithWarnings,
        RoleScopeDto,
        LimitDto,
        RoleLimitsDto,
        OdooSyncDto,
        SodWarning,
        ScopeReq,
        LimitsReq,
        CreateRoleReq,
        UpdateRoleReq,
        DuplicateRoleReq,
        RoleCapabilityRow,
        RoleCapabilities,
        PutCapabilitiesReq,
        MatrixRole,
        MatrixRow,
        PermissionMatrix,
        EvaluationReq,
        RoleRef,
        EvaluationItem,
        EvaluationResult,
        MyCapability,
        MyCapabilities,
        AssignRoleReq,
        RoleAssignment,
        SodRule,
        SodRulePage,
        SodRuleReq,
        UpdateSodRuleReq,
        SodViolation,
        SodViolationPage,
        RiskAcceptance,
        RiskAcceptanceReq,
        RoleConflict,
        RoleConflictPage,
        ResolveConflictReq,
    ))
)]
pub struct ApiDoc;

/// O catálogo fechado de capacidades (estático, igual para todas as orgs).
#[utoipa::path(
    get, path = "/api/capabilities", tag = "authorization",
    security(("session" = [])),
    responses((status = 200, body = CapabilityCatalog), (status = 401, body = crate::openapi::ErrorBody))
)]
pub async fn catalog(_auth: AuthUser) -> Json<CapabilityCatalog> {
    Json(CapabilityCatalog {
        catalog_version: authz::CATALOG_VERSION,
        values: vec!["allow", "deny", "inherit", "requires_approval"],
        items: Capability::ALL
            .into_iter()
            .map(|c| {
                let i = c.info();
                CapabilityItem {
                    code: i.code,
                    group: i.group,
                    group_label: i.group_label,
                    label: i.label,
                    hint: i.hint,
                    enforced: i.enforced,
                    enforced_at: i.enforced_at.to_vec(),
                    system_only: i.system_only,
                    approval_supported: i.approval_supported,
                }
            })
            .collect(),
    })
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageQuery {
    /// 1-100, omissão 50.
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
}

/// Papéis da org: os de sistema primeiro, depois os personalizados por nome.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/roles", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), PageQuery),
    responses(
        (status = 200, body = RolePage),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "sem `admin.manage_roles`"),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_roles(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Query(q): Query<PageQuery>,
) -> Result<Json<RolePage>, ApiError> {
    require_roles(&state, org_id, auth.user_id).await?;
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size() as usize;
    let skip: Option<NameCursor> = page.cursor()?;
    let rows = role_rows(&state.db, org_id).await?;
    let counts = crate::org::member_counts_by_role(&state.db, org_id).await?;
    // A ordem é a de `role_rows` (estável, com o id como desempate); o cursor
    // guarda o último id mostrado. Uma org tem poucos papéis.
    let start = match &skip {
        None => 0,
        Some(c) => rows
            .iter()
            .position(|r| r.id == c.id)
            .map(|i| i + 1)
            .unwrap_or(rows.len()),
    };
    let mut items = Vec::new();
    for r in rows.iter().skip(start).take(size) {
        items.push(role_dto(&state, org_id, r, &counts).await?);
    }
    let next_page_token = (start + size < rows.len()).then(|| {
        let last = &rows[start + size - 1];
        delonix_meet_core::page::encode_cursor(&NameCursor {
            name: last.name.clone(),
            id: last.id,
        })
    });
    Ok(Json(RolePage {
        items,
        next_page_token,
    }))
}

/// Cria um papel personalizado.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/roles", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = CreateRoleReq,
    responses(
        (status = 201, body = RoleWithWarnings, headers(("Location" = String))),
        (status = 400, body = crate::openapi::ErrorBody, description = "`role.invalid_name`, `authz.unknown_capability`, `authz.invalid_value`, `role.invalid_scope`, `role.inheritance_cycle`, `role.parent_not_found`, `role.invalid_odoo_group`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "sem `admin.manage_roles`, ou `authz.escalation`"),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "`role.duplicate_name` ou `role.duplicate_odoo_group`"),
        (status = 422, body = crate::openapi::ErrorBody, description = "`authz.capability_not_enforced`, `authz.system_only_capability`, `authz.approval_not_supported`, `role.limit_not_enforced`, `role.odoo_group_owner_forbidden`"),
    )
)]
pub async fn create_role(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateRoleReq>,
) -> Result<Response, ApiError> {
    require_roles(&state, org_id, auth.user_id).await?;
    let actor = actor_subject(&state, org_id, auth.user_id).await?;
    let name = authz::validate_role_name(&req.name)?;
    let values = parse_values(&req.capabilities)?;
    let limits = req.limits.unwrap_or_default();
    validate_limits(&limits)?;
    let id = Uuid::new_v4();
    let mut tx = state.db.begin().await?;
    let (set, _) = load_role_set(&mut tx, org_id).await?;
    let parent = req.inherits_from.unwrap_or(set.member_role());
    let actor_allowed = authz::allowed_in_org(&set, actor);
    authz::validate_role_write(
        &set,
        &ProposedRole {
            id,
            inherits_from: Some(parent),
            values: values.clone(),
        },
        &actor_allowed,
    )?;
    let (scope, scope_dept) = check_scope(
        &mut tx,
        org_id,
        req.scope.as_ref().unwrap_or(&ScopeReq {
            kind: "organization".into(),
            department_id: None,
        }),
    )
    .await?;
    let res = sqlx::query(
        "INSERT INTO org_roles (id, org_id, name, description, inherits_from, scope, scope_department_id,
                                max_simultaneous_destinations, max_external_guests_per_month, updated_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(id)
    .bind(org_id)
    .bind(&name)
    .bind(req.description.trim())
    .bind(parent)
    .bind(&scope)
    .bind(scope_dept)
    .bind(limits.max_simultaneous_destinations)
    .bind(limits.max_external_guests_per_month)
    .bind(auth.user_id)
    .execute(&mut *tx)
    .await;
    if let Err(e) = res {
        return Err(if unique_violation(&e) {
            DomainError::conflict("role.duplicate_name", "já existe um papel com esse nome").into()
        } else {
            e.into()
        });
    }
    write_values(&mut tx, id, &values).await?;
    recompute_effective_tx(&mut tx, org_id).await?;
    // Grupo Odoo: validado com o papel já criado (sem escalada sobre o papel final).
    let (set, _) = load_role_set(&mut tx, org_id).await?;
    if let Some(g) = check_group_mapping(&set, id, actor, req.odoo_group.as_deref())? {
        set_group(&mut tx, id, Some(&g)).await?;
    }
    tx.commit().await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "role.created",
        &audit_json(
            serde_json::json!({"role_id": id, "name": name, "inherits_from": parent,
            "scope": scope, "department_id": scope_dept, "capabilities": req.capabilities,
            "odoo_group": req.odoo_group}),
        ),
    )
    .await;
    let role = load_one_role(&state, org_id, id).await?;
    let (set, _) = load_role_set_db(&state, org_id).await?;
    let warnings = sod_warnings(&state, org_id, &set, id).await?;
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, format!("/api/orgs/{org_id}/roles/{id}"))],
        Json(RoleWithWarnings { role, warnings }),
    )
        .into_response())
}

async fn write_values(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    role_id: Uuid,
    values: &HashMap<Capability, CapabilityValue>,
) -> Result<(), ApiError> {
    sqlx::query("DELETE FROM org_role_capabilities WHERE role_id = $1")
        .bind(role_id)
        .execute(&mut **tx)
        .await?;
    let caps: Vec<String> = values.keys().map(|c| c.as_str().to_string()).collect();
    let vals: Vec<String> = values.values().map(|v| v.as_str().to_string()).collect();
    sqlx::query(
        "INSERT INTO org_role_capabilities (role_id, capability, value)
         SELECT $1, c, v FROM unnest($2::text[], $3::text[]) AS t(c, v) WHERE v <> 'inherit'",
    )
    .bind(role_id)
    .bind(&caps)
    .bind(&vals)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn set_group(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    role_id: Uuid,
    group: Option<&str>,
) -> Result<(), ApiError> {
    sqlx::query("UPDATE org_roles SET odoo_group = $2 WHERE id = $1")
        .bind(role_id)
        .bind(group)
        .execute(&mut **tx)
        .await
        .map_err(|e| {
            if unique_violation(&e) {
                DomainError::conflict(
                    "role.duplicate_odoo_group",
                    "esse grupo do Odoo já está ligado a outro papel",
                )
                .into()
            } else {
                ApiError::from(e)
            }
        })?;
    Ok(())
}

/// Um papel.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/roles/{role_id}", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("role_id" = Uuid, Path)),
    responses(
        (status = 200, body = Role),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_role(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, role_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Role>, ApiError> {
    require_roles(&state, org_id, auth.user_id).await?;
    Ok(Json(load_one_role(&state, org_id, role_id).await?))
}

async fn custom_role_or_refuse(set: &RoleSet, role_id: Uuid) -> Result<(), ApiError> {
    match set.get(role_id) {
        None => Err(DomainError::not_found("role.not_found").into()),
        Some(r) if r.system.is_some() => Err(DomainError::precondition(
            "role.system_immutable",
            "os papéis de sistema não se alteram nem se apagam",
        )
        .into()),
        Some(_) => Ok(()),
    }
}

/// Altera um papel personalizado.
#[utoipa::path(
    patch, path = "/api/orgs/{org_id}/roles/{role_id}", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("role_id" = Uuid, Path)),
    request_body = UpdateRoleReq,
    responses(
        (status = 200, body = RoleWithWarnings),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody),
        (status = 422, body = crate::openapi::ErrorBody, description = "`role.system_immutable`, `role.limit_not_enforced`, …"),
    )
)]
pub async fn update_role(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, role_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateRoleReq>,
) -> Result<Json<RoleWithWarnings>, ApiError> {
    require_roles(&state, org_id, auth.user_id).await?;
    let actor = actor_subject(&state, org_id, auth.user_id).await?;
    let before = load_one_role(&state, org_id, role_id).await?;
    let mut tx = state.db.begin().await?;
    let (set, _) = load_role_set(&mut tx, org_id).await?;
    custom_role_or_refuse(&set, role_id).await?;
    let current = set.get(role_id).cloned().ok_or(ApiError::NotFound)?;
    let name = req
        .name
        .as_deref()
        .map(authz::validate_role_name)
        .transpose()?;
    if let Some(l) = &req.limits {
        validate_limits(l)?;
    }
    if let Some(parent) = req.inherits_from {
        authz::validate_role_write(
            &set,
            &ProposedRole {
                id: role_id,
                inherits_from: Some(parent),
                values: current.values.clone(),
            },
            &authz::allowed_in_org(&set, actor),
        )?;
    }
    let scope = match &req.scope {
        Some(s) => Some(check_scope(&mut tx, org_id, s).await?),
        None => None,
    };
    let res = sqlx::query(
        "UPDATE org_roles SET
            name = COALESCE($3, name),
            description = COALESCE($4, description),
            inherits_from = COALESCE($5, inherits_from),
            scope = COALESCE($6, scope),
            scope_department_id = CASE WHEN $6::text IS NULL THEN scope_department_id ELSE $7 END,
            max_simultaneous_destinations = CASE WHEN $8 THEN $9 ELSE max_simultaneous_destinations END,
            max_external_guests_per_month = CASE WHEN $8 THEN $10 ELSE max_external_guests_per_month END,
            updated_at = now(), updated_by = $11
          WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id)
    .bind(role_id)
    .bind(name.as_deref())
    .bind(req.description.as_deref().map(str::trim))
    .bind(req.inherits_from)
    .bind(scope.as_ref().map(|s| s.0.as_str()))
    .bind(scope.as_ref().and_then(|s| s.1))
    .bind(req.limits.is_some())
    .bind(req.limits.as_ref().and_then(|l| l.max_simultaneous_destinations))
    .bind(req.limits.as_ref().and_then(|l| l.max_external_guests_per_month))
    .bind(auth.user_id)
    .execute(&mut *tx)
    .await;
    if let Err(e) = res {
        return Err(if unique_violation(&e) {
            DomainError::conflict("role.duplicate_name", "já existe um papel com esse nome").into()
        } else {
            e.into()
        });
    }
    recompute_effective_tx(&mut tx, org_id).await?;
    if let Some(g) = &req.odoo_group {
        let (set, _) = load_role_set(&mut tx, org_id).await?;
        let mapped = check_group_mapping(&set, role_id, actor, Some(g))?;
        set_group(&mut tx, role_id, mapped.as_deref()).await?;
    }
    tx.commit().await?;
    let role = load_one_role(&state, org_id, role_id).await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "role.updated",
        &audit_json(serde_json::json!({
            "role_id": role_id,
            "before": {"name": before.name, "description": before.description,
                       "inherits_from": before.inherits_from, "scope": before.scope.kind,
                       "department_id": before.scope.department_id, "odoo_group": before.odoo_group,
                       "max_simultaneous_destinations": before.limits.max_simultaneous_destinations.value,
                       "max_external_guests_per_month": before.limits.max_external_guests_per_month.value},
            "after": {"name": role.name, "description": role.description,
                      "inherits_from": role.inherits_from, "scope": role.scope.kind,
                      "department_id": role.scope.department_id, "odoo_group": role.odoo_group,
                      "max_simultaneous_destinations": role.limits.max_simultaneous_destinations.value,
                      "max_external_guests_per_month": role.limits.max_external_guests_per_month.value},
        })),
    )
    .await;
    let (set, _) = load_role_set_db(&state, org_id).await?;
    let warnings = sod_warnings(&state, org_id, &set, role_id).await?;
    Ok(Json(RoleWithWarnings { role, warnings }))
}

/// Apaga um papel personalizado. Com pessoas, exige `reassign_to` e passa-as
/// (e os convites pendentes) na mesma transacção.
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/roles/{role_id}", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("role_id" = Uuid, Path), DeleteRoleQuery),
    responses(
        (status = 204),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "sem `admin.manage_roles`, ou reatribuir seria escalada"),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "`role.has_children` (outro papel herda deste)"),
        (status = 422, body = crate::openapi::ErrorBody, description = "`role.system_immutable`, `role.reassignment_required`, `role.reassign_to_self`"),
    )
)]
pub async fn delete_role(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, role_id)): Path<(Uuid, Uuid)>,
    Query(q): Query<DeleteRoleQuery>,
) -> Result<StatusCode, ApiError> {
    require_roles(&state, org_id, auth.user_id).await?;
    let actor = actor_subject(&state, org_id, auth.user_id).await?;
    let mut tx = state.db.begin().await?;
    let (set, _) = load_role_set(&mut tx, org_id).await?;
    custom_role_or_refuse(&set, role_id).await?;
    if set.roles().any(|r| r.inherits_from == Some(role_id)) {
        return Err(DomainError::conflict(
            "role.has_children",
            "há papéis que herdam deste — mude-lhes a herança primeiro",
        )
        .into());
    }
    let counts = crate::org::member_counts_by_role(&state.db, org_id).await?;
    let people = counts.get(&role_id).copied().unwrap_or(0);
    let pending_invites: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM org_invitations WHERE org_id = $1 AND role_id = $2 AND status = 'pending'",
    )
    .bind(org_id)
    .bind(role_id)
    .fetch_one(&mut *tx)
    .await?;
    let mut moved = Vec::new();
    if people > 0 || pending_invites > 0 {
        let Some(to) = q.reassign_to else {
            return Err(DomainError::precondition(
                "role.reassignment_required",
                format!("o papel tem {people} pessoas e {pending_invites} convites — indique reassign_to"),
            )
            .with_field("reassign_to", "id de outro papel")
            .into());
        };
        if to == role_id {
            return Err(DomainError::precondition(
                "role.reassign_to_self",
                "reassign_to é o próprio papel",
            )
            .into());
        }
        authz::validate_assignment(&set, to, actor)?;
        moved = crate::org::reassign_role_tx(&mut tx, org_id, role_id, to).await?;
        sqlx::query(
            "UPDATE org_invitations SET role_id = $3 WHERE org_id = $1 AND role_id = $2 AND status = 'pending'",
        )
        .bind(org_id)
        .bind(role_id)
        .bind(to)
        .execute(&mut *tx)
        .await?;
    }
    // Convites já fechados e conflitos resolvidos que apontem para o papel caem com ele.
    sqlx::query("DELETE FROM org_invitations WHERE org_id = $1 AND role_id = $2")
        .bind(org_id)
        .bind(role_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM org_roles WHERE org_id = $1 AND id = $2")
        .bind(org_id)
        .bind(role_id)
        .execute(&mut *tx)
        .await?;
    recompute_effective_tx(&mut tx, org_id).await?;
    tx.commit()
        .await
        .map_err(crate::org::map_member_write_error)?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "role.deleted",
        &audit_json(serde_json::json!({"role_id": role_id, "name": set.get(role_id).map(|r| r.name.clone()),
            "reassign_to": q.reassign_to, "moved_user_ids": moved, "moved_invitations": pending_invites})),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// Duplica um papel (de sistema ou personalizado) num personalizado novo.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/roles/{role_id}/duplicate", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("role_id" = Uuid, Path)),
    request_body = DuplicateRoleReq,
    responses(
        (status = 201, body = RoleWithWarnings, headers(("Location" = String))),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`authz.escalation`"),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody),
        (status = 422, body = crate::openapi::ErrorBody, description = "duplicar o Proprietário (`role.duplicate_owner`)"),
    )
)]
pub async fn duplicate_role(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, role_id)): Path<(Uuid, Uuid)>,
    body: Option<Json<DuplicateRoleReq>>,
) -> Result<Response, ApiError> {
    require_roles(&state, org_id, auth.user_id).await?;
    let actor = actor_subject(&state, org_id, auth.user_id).await?;
    let mut tx = state.db.begin().await?;
    let (set, rows) = load_role_set(&mut tx, org_id).await?;
    let src = set
        .get(role_id)
        .cloned()
        .ok_or_else(|| ApiError::from(DomainError::not_found("role.not_found")))?;
    let src_row = rows
        .iter()
        .find(|r| r.id == role_id)
        .cloned()
        .ok_or(ApiError::NotFound)?;
    if src.system == Some(SystemRole::Owner) {
        return Err(DomainError::precondition(
            "role.duplicate_owner",
            "o Proprietário não se duplica",
        )
        .into());
    }
    // Um papel de sistema duplicado vira um personalizado com os valores explícitos,
    // menos o que o personalizado não pode ter (só sistema, não imposto).
    let values: HashMap<Capability, CapabilityValue> = Capability::ALL
        .into_iter()
        .filter_map(|c| {
            let v = if src.system.is_some() {
                src.value(c)
            } else {
                *src.values.get(&c)?
            };
            let info = c.info();
            if info.system_only || !info.enforced {
                None
            } else {
                Some((c, v))
            }
        })
        .collect();
    let id = Uuid::new_v4();
    let parent = src.inherits_from.or(Some(set.member_role()));
    authz::validate_role_write(
        &set,
        &ProposedRole {
            id,
            inherits_from: parent,
            values: values.clone(),
        },
        &authz::allowed_in_org(&set, actor),
    )?;
    let wanted = body.and_then(|b| b.0.name);
    let base = match &wanted {
        Some(n) => authz::validate_role_name(n)?,
        None => format!("{} (cópia)", src.name),
    };
    let mut name = base.clone();
    for i in 2..50 {
        let taken: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM org_roles WHERE org_id = $1 AND lower(name) = lower($2))",
        )
        .bind(org_id)
        .bind(&name)
        .fetch_one(&mut *tx)
        .await?;
        if !taken {
            break;
        }
        if wanted.is_some() {
            return Err(DomainError::conflict(
                "role.duplicate_name",
                "já existe um papel com esse nome",
            )
            .into());
        }
        name = format!("{base} {i}");
    }
    sqlx::query(
        "INSERT INTO org_roles (id, org_id, name, description, inherits_from, scope, scope_department_id,
                                max_simultaneous_destinations, max_external_guests_per_month, updated_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(id)
    .bind(org_id)
    .bind(&name)
    .bind(&src_row.description)
    .bind(parent)
    .bind(&src_row.scope)
    .bind(src_row.scope_department_id)
    .bind(src_row.max_simultaneous_destinations)
    .bind(src_row.max_external_guests_per_month)
    .bind(auth.user_id)
    .execute(&mut *tx)
    .await?;
    write_values(&mut tx, id, &values).await?;
    recompute_effective_tx(&mut tx, org_id).await?;
    tx.commit().await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "role.duplicated",
        &audit_json(serde_json::json!({"role_id": id, "from_role_id": role_id, "name": name})),
    )
    .await;
    let role = load_one_role(&state, org_id, id).await?;
    let (set, _) = load_role_set_db(&state, org_id).await?;
    let warnings = sod_warnings(&state, org_id, &set, id).await?;
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, format!("/api/orgs/{org_id}/roles/{id}"))],
        Json(RoleWithWarnings { role, warnings }),
    )
        .into_response())
}

fn capability_rows(set: &RoleSet, role_id: Uuid) -> Vec<RoleCapabilityRow> {
    let role = set.get(role_id);
    let eff = authz::effective_for_role(set, role_id);
    Capability::ALL
        .into_iter()
        .map(|c| {
            let info = c.info();
            let system = role.is_some_and(|r| r.system.is_some());
            let locked_reason = if system {
                Some("system_role")
            } else if info.system_only {
                Some("system_only")
            } else if !info.enforced {
                Some("not_enforced")
            } else {
                None
            };
            RoleCapabilityRow {
                capability: info.code,
                value: role.map(|r| value_str(r.value(c))).unwrap_or("inherit"),
                organization: decision_str(eff[&c].organization),
                own_department: decision_str(eff[&c].own_department),
                locked: locked_reason.is_some(),
                locked_reason,
            }
        })
        .collect()
}

/// A coluna da matriz de um papel: valor gravado e decisão efectiva nos dois âmbitos.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/roles/{role_id}/capabilities", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("role_id" = Uuid, Path)),
    responses(
        (status = 200, body = RoleCapabilities),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_capabilities(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, role_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<RoleCapabilities>, ApiError> {
    require_roles(&state, org_id, auth.user_id).await?;
    let (set, _) = load_role_set_db(&state, org_id).await?;
    if set.get(role_id).is_none() {
        return Err(DomainError::not_found("role.not_found").into());
    }
    Ok(Json(RoleCapabilities {
        role_id,
        catalog_version: authz::CATALOG_VERSION,
        items: capability_rows(&set, role_id),
        warnings: sod_warnings(&state, org_id, &set, role_id).await?,
    }))
}

/// Substitui a coluna da matriz de um papel personalizado.
#[utoipa::path(
    put, path = "/api/orgs/{org_id}/roles/{role_id}/capabilities", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("role_id" = Uuid, Path)),
    request_body = PutCapabilitiesReq,
    responses(
        (status = 200, body = RoleCapabilities),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`authz.escalation`"),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 422, body = crate::openapi::ErrorBody, description = "`role.system_immutable`, `authz.capability_not_enforced`, `authz.system_only_capability`, `authz.approval_not_supported`"),
    )
)]
pub async fn put_capabilities(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, role_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<PutCapabilitiesReq>,
) -> Result<Json<RoleCapabilities>, ApiError> {
    require_roles(&state, org_id, auth.user_id).await?;
    let actor = actor_subject(&state, org_id, auth.user_id).await?;
    let values = parse_values(&req.values)?;
    let mut tx = state.db.begin().await?;
    let (set, _) = load_role_set(&mut tx, org_id).await?;
    custom_role_or_refuse(&set, role_id).await?;
    let current = set.get(role_id).cloned().ok_or(ApiError::NotFound)?;
    authz::validate_role_write(
        &set,
        &ProposedRole {
            id: role_id,
            inherits_from: current.inherits_from,
            values: values.clone(),
        },
        &authz::allowed_in_org(&set, actor),
    )?;
    write_values(&mut tx, role_id, &values).await?;
    sqlx::query("UPDATE org_roles SET updated_at = now(), updated_by = $2 WHERE id = $1")
        .bind(role_id)
        .bind(auth.user_id)
        .execute(&mut *tx)
        .await?;
    recompute_effective_tx(&mut tx, org_id).await?;
    tx.commit().await?;
    let before: BTreeMap<&str, &str> = current
        .values
        .iter()
        .map(|(c, v)| (c.as_str(), v.as_str()))
        .collect();
    let after: BTreeMap<&str, &str> = values
        .iter()
        .filter(|(_, v)| **v != CapabilityValue::Inherit)
        .map(|(c, v)| (c.as_str(), v.as_str()))
        .collect();
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "role.capabilities_updated",
        &audit_json(serde_json::json!({"role_id": role_id, "before": before, "after": after})),
    )
    .await;
    let (set, _) = load_role_set_db(&state, org_id).await?;
    Ok(Json(RoleCapabilities {
        role_id,
        catalog_version: authz::CATALOG_VERSION,
        items: capability_rows(&set, role_id),
        warnings: sod_warnings(&state, org_id, &set, role_id).await?,
    }))
}

fn csv_field(s: &str) -> String {
    // Fórmulas não se interpretam ao abrir numa folha de cálculo.
    let s = if s.starts_with(['=', '+', '-', '@']) {
        format!("'{s}")
    } else {
        s.to_string()
    };
    if s.contains([',', '"', '\n', ';']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

/// A matriz inteira (capacidades × papéis), em JSON ou CSV.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/permission-matrix", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), MatrixQuery),
    responses(
        (status = 200, body = PermissionMatrix, description = "JSON; com `format=csv`, `text/csv` com uma linha por capacidade e uma coluna por papel"),
        (status = 400, body = crate::openapi::ErrorBody, description = "`matrix.invalid_format`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn matrix(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Query(q): Query<MatrixQuery>,
) -> Result<Response, ApiError> {
    require_roles(&state, org_id, auth.user_id).await?;
    let csv = match q.format.as_deref() {
        None | Some("json") => false,
        Some("csv") => true,
        Some(_) => {
            return Err(
                DomainError::invalid("matrix.invalid_format", "format é json ou csv")
                    .with_field("format", "json | csv")
                    .into(),
            )
        }
    };
    let (set, rows) = load_role_set_db(&state, org_id).await?;
    let counts = crate::org::member_counts_by_role(&state.db, org_id).await?;
    let effs: HashMap<Uuid, _> = rows
        .iter()
        .map(|r| (r.id, authz::effective_for_role(&set, r.id)))
        .collect();
    if csv {
        let mut out = String::from("\u{feff}grupo,capacidade,codigo,imposta");
        for r in &rows {
            out.push(',');
            out.push_str(&csv_field(&r.name));
        }
        out.push('\n');
        for c in Capability::ALL {
            let i = c.info();
            out.push_str(&format!(
                "{},{},{},{}",
                csv_field(i.group_label),
                csv_field(i.label),
                i.code,
                if i.enforced { "sim" } else { "não" }
            ));
            for r in &rows {
                let role = set.get(r.id);
                let v = role.map(|d| d.value(c)).unwrap_or(CapabilityValue::Inherit);
                let cell = if v == CapabilityValue::Inherit {
                    format!("inherit ({})", decision_str(effs[&r.id][&c].organization))
                } else {
                    v.as_str().to_string()
                };
                out.push(',');
                out.push_str(&cell);
            }
            out.push('\n');
        }
        crate::audit::log(
            &state.db,
            Some(org_id),
            auth.user_id,
            "role.matrix_exported",
            "csv",
        )
        .await;
        return Ok((
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"matriz-de-permissoes.csv\"".to_string(),
                ),
            ],
            out,
        )
            .into_response());
    }
    let matrix = PermissionMatrix {
        catalog_version: authz::CATALOG_VERSION,
        member_total: counts.values().sum(),
        roles: rows
            .iter()
            .map(|r| MatrixRole {
                id: r.id,
                key: r.system_key.clone(),
                name: r.name.clone(),
                member_count: counts.get(&r.id).copied().unwrap_or(0),
            })
            .collect(),
        rows: Capability::ALL
            .into_iter()
            .map(|c| {
                let i = c.info();
                MatrixRow {
                    capability: i.code,
                    group: i.group,
                    group_label: i.group_label,
                    label: i.label,
                    hint: i.hint,
                    enforced: i.enforced,
                    values: rows
                        .iter()
                        .map(|r| {
                            (
                                r.id,
                                set.get(r.id)
                                    .map(|d| value_str(d.value(c)))
                                    .unwrap_or("inherit"),
                            )
                        })
                        .collect(),
                    effective: rows
                        .iter()
                        .map(|r| (r.id, decision_str(effs[&r.id][&c].organization)))
                        .collect(),
                }
            })
            .collect(),
    };
    Ok(Json(matrix).into_response())
}

fn reason_str(r: authz::Reason) -> &'static str {
    match r {
        authz::Reason::RoleValue => "role_value",
        authz::Reason::Inherited => "inherited",
        authz::Reason::OutOfScopeMember => "out_of_scope_member",
        authz::Reason::NotGranted => "not_granted",
    }
}

/// Simular utilizador: avaliação SÓ DE LEITURA das capacidades de uma pessoa, com
/// o porquê. Nunca emite sessão nem token, nunca executa em nome de ninguém.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/authorization/evaluations", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = EvaluationReq,
    responses(
        (status = 200, body = EvaluationResult),
        (status = 400, body = crate::openapi::ErrorBody, description = "`authz.unknown_capability`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "sem `admin.manage_roles`"),
        (status = 404, body = crate::openapi::ErrorBody, description = "a pessoa não é membro activo desta org (`member.not_found`)"),
    )
)]
pub async fn evaluate(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<EvaluationReq>,
) -> Result<Json<EvaluationResult>, ApiError> {
    require_roles(&state, org_id, auth.user_id).await?;
    let caps: Vec<Capability> = match &req.capability {
        Some(c) => vec![Capability::parse(c)?],
        None => Capability::ALL.to_vec(),
    };
    let (role_id, dept) = crate::org::member_subject(&state.db, org_id, req.user_id)
        .await?
        .ok_or_else(|| ApiError::from(DomainError::not_found("member.not_found")))?;
    let (set, _) = load_role_set_db(&state, org_id).await?;
    let name_of = |id: Uuid| RoleRef {
        id,
        name: set.get(id).map(|r| r.name.clone()).unwrap_or_default(),
    };
    let scope = match req.department_id {
        Some(d) => ResourceScope::Department { department_id: d },
        None => ResourceScope::Organization,
    };
    let subject = Subject {
        role_id,
        department_id: dept,
    };
    let items = caps
        .into_iter()
        .map(|c| {
            let ev = authz::can(&set, subject, c, scope);
            EvaluationItem {
                capability: c.as_str(),
                decision: decision_str(ev.decision),
                reason: reason_str(ev.reason),
                via_role: ev.via_role_id.map(name_of),
                chain: ev.chain.iter().copied().map(name_of).collect(),
                enforced: c.info().enforced,
            }
        })
        .collect();
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "authorization.evaluated",
        &audit_json(
            serde_json::json!({"user_id": req.user_id, "capability": req.capability,
                                       "department_id": req.department_id}),
        ),
    )
    .await;
    Ok(Json(EvaluationResult {
        user_id: req.user_id,
        role: name_of(role_id),
        member_department_id: dept,
        resource_scope: if req.department_id.is_some() {
            "department"
        } else {
            "organization"
        },
        resource_department_id: req.department_id,
        items,
    }))
}

/// As minhas capacidades nesta org (para a UI gatear botões). Qualquer membro activo.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/members/me/capabilities", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses(
        (status = 200, body = MyCapabilities),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn my_capabilities(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<MyCapabilities>, ApiError> {
    let (grant, rows) = crate::org::my_capabilities(&state, org_id, auth.user_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(MyCapabilities {
        role_id: grant.role_id,
        role_key: grant.system.map(|s| s.key().to_string()),
        department_id: grant.department_id,
        catalog_version: authz::CATALOG_VERSION,
        items: rows
            .into_iter()
            .map(|r| MyCapability {
                capability: r.capability,
                organization: r.org_decision,
                own_department: r.dept_decision,
            })
            .collect(),
    }))
}

/// A decisão de `admin.manage_accounts` sobre uma pessoa: o âmbito é o
/// departamento DELA (um papel de departamento só gere o seu).
pub(crate) async fn require_accounts_over(
    state: &AppState,
    org_id: Uuid,
    actor: Uuid,
    target_department: Option<Uuid>,
) -> Result<crate::org::Grant, ApiError> {
    let scope = match target_department {
        Some(d) => ResourceScope::Department { department_id: d },
        None => ResourceScope::Organization,
    };
    crate::org::require_capability(state, org_id, actor, Capability::AdminManageAccounts, scope)
        .await
}

/// Atribui um papel a uma pessoa (sem escalada; último dono; `owner` só por `owner`).
#[utoipa::path(
    put, path = "/api/orgs/{org_id}/members/{user_id}/role", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("user_id" = Uuid, Path)),
    request_body = AssignRoleReq,
    responses(
        (status = 200, body = RoleAssignment),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "sem `admin.manage_accounts`, `authz.escalation`, `role.owner_assignment`"),
        (status = 404, body = crate::openapi::ErrorBody, description = "`member.not_found`, `role.not_found`"),
        (status = 409, body = crate::openapi::ErrorBody, description = "`role.last_owner`"),
        (status = 422, body = crate::openapi::ErrorBody, description = "`member.not_active`, `role.service_account`"),
    )
)]
pub async fn assign_role(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, user_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<AssignRoleReq>,
) -> Result<Json<RoleAssignment>, ApiError> {
    let (changed, name) =
        assign_role_as(&state, org_id, auth.user_id, user_id, req.role_id).await?;
    Ok(Json(RoleAssignment {
        user_id,
        role_id: req.role_id,
        role_name: name,
        changed,
    }))
}

/// Caminho único de atribuição manual (usado também pelas acções em massa).
pub(crate) async fn assign_role_as(
    state: &AppState,
    org_id: Uuid,
    actor: Uuid,
    user_id: Uuid,
    role_id: Uuid,
) -> Result<(bool, String), ApiError> {
    let target = crate::org::member_subject(&state.db, org_id, user_id).await?;
    // O âmbito é o da pessoa-alvo; um estranho à org responde como membro inexistente.
    require_accounts_over(state, org_id, actor, target.and_then(|t| t.1)).await?;
    let Some((before_role, _)) = target else {
        return Err(DomainError::not_found("member.not_found").into());
    };
    ensure_can_assign(state, org_id, actor, role_id).await?;
    let mut tx = state.db.begin().await?;
    let changed =
        crate::org::set_member_role_tx(&mut tx, org_id, user_id, role_id, "manual").await?;
    tx.commit()
        .await
        .map_err(crate::org::map_member_write_error)?;
    let (set, _) = load_role_set_db(state, org_id).await?;
    let name = set.get(role_id).map(|r| r.name.clone()).unwrap_or_default();
    if changed {
        crate::audit::log(
            &state.db,
            Some(org_id),
            actor,
            "member.role_changed",
            &audit_json(serde_json::json!({"user_id": user_id,
                "before": {"role_id": before_role, "role_name": set.get(before_role).map(|r| r.name.clone())},
                "after": {"role_id": role_id, "role_name": name}})),
        )
        .await;
    }
    Ok((changed, name))
}

// ---------------------------------------------------------------------------
//  Segregação de funções
// ---------------------------------------------------------------------------

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct SodRule {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub exempt_role_ids: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const SOD_COLUMNS: &str =
    "id, org_id, name, description, capabilities, exempt_role_ids, created_at, updated_at";

#[derive(Serialize, utoipa::ToSchema)]
pub struct SodRulePage {
    pub items: Vec<SodRule>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct SodRuleReq {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// ≥ 2 códigos do catálogo.
    pub capabilities: Vec<String>,
    /// Papéis isentos; ausente = o Proprietário.
    #[serde(default)]
    pub exempt_role_ids: Option<Vec<Uuid>>,
}

#[derive(Deserialize, Default, utoipa::ToSchema)]
pub struct UpdateSodRuleReq {
    pub name: Option<String>,
    pub description: Option<String>,
    pub capabilities: Option<Vec<String>>,
    pub exempt_role_ids: Option<Vec<Uuid>>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RiskAcceptance {
    pub rule_id: Uuid,
    pub user_id: Uuid,
    pub role_id: Uuid,
    pub justification: String,
    pub accepted_by: Option<Uuid>,
    pub accepted_by_name: Option<String>,
    pub accepted_at: DateTime<Utc>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SodViolation {
    pub rule_id: Uuid,
    pub rule_name: String,
    pub user_id: Uuid,
    pub username: String,
    pub email: String,
    pub role_id: Uuid,
    pub role_name: String,
    /// Aceitação válida (a pessoa tem o mesmo papel de quando foi aceite).
    pub accepted: Option<RiskAcceptance>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SodViolationPage {
    pub items: Vec<SodViolation>,
    pub total: usize,
    pub unaccepted: usize,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SodViolationQuery {
    pub rule_id: Option<Uuid>,
    /// Só pessoas com este papel («Ver as três»).
    pub role_id: Option<Uuid>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct RiskAcceptanceReq {
    pub user_id: Uuid,
    /// Obrigatória, ≥ 10 caracteres.
    pub justification: String,
}

async fn load_sod_rules(
    db: &sqlx::PgPool,
    org_id: Uuid,
) -> Result<Vec<(authz::SodRule, String)>, ApiError> {
    let rows: Vec<SodRule> = sqlx::query_as(&format!(
        "SELECT {SOD_COLUMNS} FROM sod_rules WHERE org_id = $1 ORDER BY lower(name), id"
    ))
    .bind(org_id)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            (
                authz::SodRule {
                    id: r.id,
                    capabilities: r
                        .capabilities
                        .iter()
                        .filter_map(|c| Capability::parse(c).ok())
                        .collect(),
                    exempt_roles: r.exempt_role_ids.into_iter().collect(),
                },
                r.name,
            )
        })
        .collect())
}

fn parse_caps(raw: &[String]) -> Result<BTreeSet<Capability>, ApiError> {
    let caps = raw
        .iter()
        .map(|c| Capability::parse(c))
        .collect::<Result<BTreeSet<_>, _>>()?;
    authz::validate_sod_rule(&caps)?;
    Ok(caps)
}

async fn check_exempt(state: &AppState, org_id: Uuid, ids: &[Uuid]) -> Result<(), ApiError> {
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM org_roles WHERE org_id = $1 AND id = ANY($2)")
            .bind(org_id)
            .bind(ids)
            .fetch_one(&state.db)
            .await?;
    if n as usize != ids.iter().collect::<BTreeSet<_>>().len() {
        return Err(DomainError::invalid(
            "sod.unknown_role",
            "um papel isento não é desta organização",
        )
        .with_field("exempt_role_ids", "ids de papéis desta org")
        .into());
    }
    Ok(())
}

async fn get_sod(state: &AppState, org_id: Uuid, id: Uuid) -> Result<SodRule, ApiError> {
    sqlx::query_as::<_, SodRule>(&format!(
        "SELECT {SOD_COLUMNS} FROM sod_rules WHERE org_id = $1 AND id = $2"
    ))
    .bind(org_id)
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| DomainError::not_found("sod.not_found").into())
}

/// Regras de segregação de funções.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/sod-rules", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), PageQuery),
    responses((status = 200, body = SodRulePage), (status = 401, body = crate::openapi::ErrorBody),
              (status = 403, body = crate::openapi::ErrorBody), (status = 404, body = crate::openapi::ErrorBody))
)]
pub async fn list_sod_rules(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Query(q): Query<PageQuery>,
) -> Result<Json<SodRulePage>, ApiError> {
    require_roles(&state, org_id, auth.user_id).await?;
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<NameCursor> = page.cursor()?;
    let rows: Vec<SodRule> = sqlx::query_as(&format!(
        "SELECT {SOD_COLUMNS} FROM sod_rules
          WHERE org_id = $1 AND ($2::text IS NULL OR (lower(name), id) > ($2, $3))
          ORDER BY lower(name), id LIMIT $4"
    ))
    .bind(org_id)
    .bind(cursor.as_ref().map(|c| c.name.clone()))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |r| NameCursor {
        name: r.name.to_lowercase(),
        id: r.id,
    });
    Ok(Json(SodRulePage {
        items: p.items,
        next_page_token: p.next_page_token,
    }))
}

/// Cria uma regra.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/sod-rules", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = SodRuleReq,
    responses((status = 201, body = SodRule, headers(("Location" = String))),
              (status = 400, body = crate::openapi::ErrorBody, description = "`sod.too_few_capabilities`, `authz.unknown_capability`, `sod.unknown_role`, `sod.invalid_name`"),
              (status = 401, body = crate::openapi::ErrorBody), (status = 403, body = crate::openapi::ErrorBody),
              (status = 404, body = crate::openapi::ErrorBody), (status = 409, body = crate::openapi::ErrorBody))
)]
pub async fn create_sod_rule(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<SodRuleReq>,
) -> Result<Response, ApiError> {
    require_roles(&state, org_id, auth.user_id).await?;
    let name = sod_name(&req.name)?;
    let caps = parse_caps(&req.capabilities)?;
    let exempt = match req.exempt_role_ids {
        Some(v) => v,
        None => vec![
            sqlx::query_scalar(
                "SELECT id FROM org_roles WHERE org_id = $1 AND system_key = 'owner'",
            )
            .bind(org_id)
            .fetch_one(&state.db)
            .await?,
        ],
    };
    check_exempt(&state, org_id, &exempt).await?;
    let caps: Vec<String> = caps.iter().map(|c| c.as_str().to_string()).collect();
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO sod_rules (org_id, name, description, capabilities, exempt_role_ids, created_by)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(org_id)
    .bind(&name)
    .bind(req.description.trim())
    .bind(&caps)
    .bind(&exempt)
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| {
        if unique_violation(&e) {
            DomainError::conflict("sod.duplicate_name", "já existe uma regra com esse nome").into()
        } else {
            ApiError::from(e)
        }
    })?;
    crate::audit::log(&state.db, Some(org_id), auth.user_id, "sod.rule_created",
        &audit_json(serde_json::json!({"rule_id": id, "name": name, "capabilities": caps, "exempt_role_ids": exempt}))).await;
    let rule = get_sod(&state, org_id, id).await?;
    Ok((
        StatusCode::CREATED,
        [(
            header::LOCATION,
            format!("/api/orgs/{org_id}/sod-rules/{id}"),
        )],
        Json(rule),
    )
        .into_response())
}

fn sod_name(raw: &str) -> Result<String, ApiError> {
    let n = raw.trim();
    if n.is_empty() || n.chars().count() > 80 {
        return Err(
            DomainError::invalid("sod.invalid_name", "nome com 1–80 caracteres")
                .with_field("name", "1–80")
                .into(),
        );
    }
    Ok(n.to_string())
}

/// Uma regra.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/sod-rules/{rule_id}", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("rule_id" = Uuid, Path)),
    responses((status = 200, body = SodRule), (status = 401, body = crate::openapi::ErrorBody),
              (status = 403, body = crate::openapi::ErrorBody), (status = 404, body = crate::openapi::ErrorBody))
)]
pub async fn get_sod_rule(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<SodRule>, ApiError> {
    require_roles(&state, org_id, auth.user_id).await?;
    Ok(Json(get_sod(&state, org_id, id).await?))
}

/// Altera uma regra.
#[utoipa::path(
    patch, path = "/api/orgs/{org_id}/sod-rules/{rule_id}", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("rule_id" = Uuid, Path)),
    request_body = UpdateSodRuleReq,
    responses((status = 200, body = SodRule), (status = 400, body = crate::openapi::ErrorBody),
              (status = 401, body = crate::openapi::ErrorBody), (status = 403, body = crate::openapi::ErrorBody),
              (status = 404, body = crate::openapi::ErrorBody), (status = 409, body = crate::openapi::ErrorBody))
)]
pub async fn update_sod_rule(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateSodRuleReq>,
) -> Result<Json<SodRule>, ApiError> {
    require_roles(&state, org_id, auth.user_id).await?;
    let before = get_sod(&state, org_id, id).await?;
    let name = req.name.as_deref().map(sod_name).transpose()?;
    let caps = req
        .capabilities
        .as_deref()
        .map(parse_caps)
        .transpose()?
        .map(|c| c.iter().map(|x| x.as_str().to_string()).collect::<Vec<_>>());
    if let Some(ex) = &req.exempt_role_ids {
        check_exempt(&state, org_id, ex).await?;
    }
    sqlx::query(
        "UPDATE sod_rules SET name = COALESCE($3, name), description = COALESCE($4, description),
                capabilities = COALESCE($5, capabilities), exempt_role_ids = COALESCE($6, exempt_role_ids),
                updated_at = now()
          WHERE org_id = $1 AND id = $2",
    )
    .bind(org_id)
    .bind(id)
    .bind(name)
    .bind(req.description.as_deref().map(str::trim))
    .bind(caps)
    .bind(req.exempt_role_ids)
    .execute(&state.db)
    .await
    .map_err(|e| {
        if unique_violation(&e) {
            DomainError::conflict("sod.duplicate_name", "já existe uma regra com esse nome").into()
        } else {
            ApiError::from(e)
        }
    })?;
    let after = get_sod(&state, org_id, id).await?;
    crate::audit::log(&state.db, Some(org_id), auth.user_id, "sod.rule_updated",
        &audit_json(serde_json::json!({"rule_id": id,
            "before": {"name": before.name, "capabilities": before.capabilities, "exempt_role_ids": before.exempt_role_ids},
            "after": {"name": after.name, "capabilities": after.capabilities, "exempt_role_ids": after.exempt_role_ids}}))).await;
    Ok(Json(after))
}

/// Apaga uma regra (e as aceitações de risco dela).
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/sod-rules/{rule_id}", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("rule_id" = Uuid, Path)),
    responses((status = 204), (status = 401, body = crate::openapi::ErrorBody),
              (status = 403, body = crate::openapi::ErrorBody), (status = 404, body = crate::openapi::ErrorBody))
)]
pub async fn delete_sod_rule(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    require_roles(&state, org_id, auth.user_id).await?;
    let before = get_sod(&state, org_id, id).await?;
    sqlx::query("DELETE FROM sod_rules WHERE org_id = $1 AND id = $2")
        .bind(org_id)
        .bind(id)
        .execute(&state.db)
        .await?;
    crate::audit::log(&state.db, Some(org_id), auth.user_id, "sod.rule_deleted",
        &audit_json(serde_json::json!({"rule_id": id, "name": before.name, "capabilities": before.capabilities}))).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(sqlx::FromRow)]
struct AcceptanceRow {
    rule_id: Uuid,
    user_id: Uuid,
    role_id: Uuid,
    justification: String,
    accepted_by: Option<Uuid>,
    accepted_by_name: Option<String>,
    accepted_at: DateTime<Utc>,
}

async fn compute_violations(
    state: &AppState,
    org_id: Uuid,
    q: &SodViolationQuery,
) -> Result<Vec<SodViolation>, ApiError> {
    let (set, _) = load_role_set_db(state, org_id).await?;
    let rules = load_sod_rules(&state.db, org_id).await?;
    let people = crate::org::active_member_subjects(&state.db, org_id, q.role_id).await?;
    let acceptances: Vec<AcceptanceRow> = sqlx::query_as(
        "SELECT a.rule_id, a.user_id, a.role_id, a.justification, a.accepted_by, u.username AS accepted_by_name, a.accepted_at
           FROM sod_risk_acceptances a JOIN sod_rules r ON r.id = a.rule_id
      LEFT JOIN users u ON u.id = a.accepted_by
          WHERE r.org_id = $1",
    )
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    let mut out = Vec::new();
    for (rule, rule_name) in &rules {
        if q.rule_id.is_some_and(|r| r != rule.id) {
            continue;
        }
        for (user_id, username, email, role_id, dept) in &people {
            let subject = Subject {
                role_id: *role_id,
                department_id: *dept,
            };
            if !authz::violates(&set, subject, rule) {
                continue;
            }
            let accepted = acceptances
                .iter()
                .find(|a| a.rule_id == rule.id && a.user_id == *user_id && a.role_id == *role_id)
                .map(|a| RiskAcceptance {
                    rule_id: a.rule_id,
                    user_id: a.user_id,
                    role_id: a.role_id,
                    justification: a.justification.clone(),
                    accepted_by: a.accepted_by,
                    accepted_by_name: a.accepted_by_name.clone(),
                    accepted_at: a.accepted_at,
                });
            out.push(SodViolation {
                rule_id: rule.id,
                rule_name: rule_name.clone(),
                user_id: *user_id,
                username: username.clone(),
                email: email.clone(),
                role_id: *role_id,
                role_name: set
                    .get(*role_id)
                    .map(|r| r.name.clone())
                    .unwrap_or_default(),
                accepted,
            });
        }
    }
    Ok(out)
}

/// Pessoas activas que violam as regras (com as aceitações de risco válidas).
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/sod-violations", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), SodViolationQuery),
    responses((status = 200, body = SodViolationPage), (status = 401, body = crate::openapi::ErrorBody),
              (status = 403, body = crate::openapi::ErrorBody), (status = 404, body = crate::openapi::ErrorBody))
)]
pub async fn sod_violations(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Query(q): Query<SodViolationQuery>,
) -> Result<Json<SodViolationPage>, ApiError> {
    require_roles(&state, org_id, auth.user_id).await?;
    let items = compute_violations(&state, org_id, &q).await?;
    let unaccepted = items.iter().filter(|v| v.accepted.is_none()).count();
    Ok(Json(SodViolationPage {
        total: items.len(),
        unaccepted,
        items,
    }))
}

/// «Aceitar risco» para uma pessoa que viola a regra, com justificação auditada.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/sod-rules/{rule_id}/risk-acceptances", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("rule_id" = Uuid, Path)),
    request_body = RiskAcceptanceReq,
    responses((status = 201, body = RiskAcceptance),
              (status = 400, body = crate::openapi::ErrorBody, description = "`sod.justification_required`"),
              (status = 401, body = crate::openapi::ErrorBody), (status = 403, body = crate::openapi::ErrorBody),
              (status = 404, body = crate::openapi::ErrorBody),
              (status = 422, body = crate::openapi::ErrorBody, description = "`sod.not_violating`"))
)]
pub async fn accept_risk(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, rule_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<RiskAcceptanceReq>,
) -> Result<Response, ApiError> {
    require_roles(&state, org_id, auth.user_id).await?;
    get_sod(&state, org_id, rule_id).await?;
    let justification = req.justification.trim().to_string();
    if justification.chars().count() < 10 {
        return Err(DomainError::invalid(
            "sod.justification_required",
            "a justificação tem pelo menos 10 caracteres",
        )
        .with_field("justification", "≥ 10 caracteres")
        .into());
    }
    let violations = compute_violations(
        &state,
        org_id,
        &SodViolationQuery {
            rule_id: Some(rule_id),
            role_id: None,
        },
    )
    .await?;
    let Some(v) = violations.into_iter().find(|v| v.user_id == req.user_id) else {
        return Err(DomainError::precondition(
            "sod.not_violating",
            "esta pessoa não viola a regra",
        )
        .into());
    };
    let accepted_at: DateTime<Utc> = sqlx::query_scalar(
        "INSERT INTO sod_risk_acceptances (rule_id, user_id, role_id, justification, accepted_by)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (rule_id, user_id) DO UPDATE
            SET role_id = EXCLUDED.role_id, justification = EXCLUDED.justification,
                accepted_by = EXCLUDED.accepted_by, accepted_at = now()
         RETURNING accepted_at",
    )
    .bind(rule_id)
    .bind(req.user_id)
    .bind(v.role_id)
    .bind(&justification)
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "sod.risk_accepted",
        &audit_json(
            serde_json::json!({"rule_id": rule_id, "user_id": req.user_id, "role_id": v.role_id,
                                       "justification": justification}),
        ),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(RiskAcceptance {
            rule_id,
            user_id: req.user_id,
            role_id: v.role_id,
            justification,
            accepted_by: Some(auth.user_id),
            accepted_by_name: None,
            accepted_at,
        }),
    )
        .into_response())
}

// ---------------------------------------------------------------------------
//  Conflitos de papel vindos do Odoo
// ---------------------------------------------------------------------------

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct RoleConflict {
    pub id: Uuid,
    pub org_id: Uuid,
    pub user_id: Uuid,
    pub username: String,
    pub email: String,
    pub current_role_id: Uuid,
    pub proposed_role_ids: Vec<Uuid>,
    pub odoo_groups: Vec<String>,
    /// `pending` | `resolved` | `superseded`.
    pub status: String,
    pub decision: Option<String>,
    pub applied_role_id: Option<Uuid>,
    pub resolved_by: Option<Uuid>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RoleConflictPage {
    pub items: Vec<RoleConflict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ConflictQuery {
    /// Omissão: `pending`.
    pub status: Option<String>,
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ResolveConflictReq {
    /// `keep_current` | `apply_proposed`.
    pub decision: String,
    /// Obrigatório com `apply_proposed` quando há mais de um papel proposto.
    #[serde(default)]
    pub role_id: Option<Uuid>,
}

const CONFLICT_COLUMNS: &str =
    "c.id, c.org_id, c.user_id, u.username, u.email, c.current_role_id, \
     c.proposed_role_ids, c.odoo_groups, c.status, c.decision, c.applied_role_id, c.resolved_by, \
     c.resolved_at, c.created_at";

#[derive(Serialize, Deserialize)]
struct AtCursor {
    at: DateTime<Utc>,
    id: Uuid,
}

/// Conflitos de papel que a sincronização do Odoo não decidiu sozinha.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/role-conflicts", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ConflictQuery),
    responses((status = 200, body = RoleConflictPage), (status = 400, body = crate::openapi::ErrorBody),
              (status = 401, body = crate::openapi::ErrorBody), (status = 403, body = crate::openapi::ErrorBody),
              (status = 404, body = crate::openapi::ErrorBody))
)]
pub async fn list_role_conflicts(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Query(q): Query<ConflictQuery>,
) -> Result<Json<RoleConflictPage>, ApiError> {
    require_accounts_over(&state, org_id, auth.user_id, None).await?;
    let status = q.status.unwrap_or_else(|| "pending".into());
    if !matches!(status.as_str(), "pending" | "resolved" | "superseded") {
        return Err(
            DomainError::invalid("role_conflict.invalid_status", "estado desconhecido")
                .with_field("status", "pending | resolved | superseded")
                .into(),
        );
    }
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<AtCursor> = page.cursor()?;
    let rows: Vec<RoleConflict> = sqlx::query_as(&format!(
        "SELECT {CONFLICT_COLUMNS} FROM role_conflicts c JOIN users u ON u.id = c.user_id
          WHERE c.org_id = $1 AND c.status = $2
            AND ($3::timestamptz IS NULL OR (c.created_at, c.id) > ($3, $4))
          ORDER BY c.created_at, c.id LIMIT $5"
    ))
    .bind(org_id)
    .bind(&status)
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |r| AtCursor {
        at: r.created_at,
        id: r.id,
    });
    Ok(Json(RoleConflictPage {
        items: p.items,
        next_page_token: p.next_page_token,
    }))
}

/// Decide um conflito: manter o papel actual ou aplicar o proposto pelo Odoo.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/role-conflicts/{conflict_id}/resolve", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("conflict_id" = Uuid, Path)),
    request_body = ResolveConflictReq,
    responses((status = 200, body = RoleConflict),
              (status = 400, body = crate::openapi::ErrorBody, description = "`role_conflict.invalid_decision`, `role_conflict.role_required`"),
              (status = 401, body = crate::openapi::ErrorBody),
              (status = 403, body = crate::openapi::ErrorBody, description = "`authz.escalation`"),
              (status = 404, body = crate::openapi::ErrorBody),
              (status = 409, body = crate::openapi::ErrorBody, description = "`role_conflict.not_pending`, `role.last_owner`"))
)]
pub async fn resolve_role_conflict(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, conflict_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<ResolveConflictReq>,
) -> Result<Json<RoleConflict>, ApiError> {
    require_accounts_over(&state, org_id, auth.user_id, None).await?;
    let load = || async {
        sqlx::query_as::<_, RoleConflict>(&format!(
            "SELECT {CONFLICT_COLUMNS} FROM role_conflicts c JOIN users u ON u.id = c.user_id
              WHERE c.org_id = $1 AND c.id = $2"
        ))
        .bind(org_id)
        .bind(conflict_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::from(DomainError::not_found("role_conflict.not_found")))
    };
    let c = load().await?;
    if c.status != "pending" {
        return Err(DomainError::conflict(
            "role_conflict.not_pending",
            "o conflito já foi decidido",
        )
        .into());
    }
    let applied = match req.decision.as_str() {
        "keep_current" => None,
        "apply_proposed" => {
            let role = match (req.role_id, c.proposed_role_ids.as_slice()) {
                (Some(r), props) if props.contains(&r) => r,
                (None, [only]) => *only,
                _ => {
                    return Err(DomainError::invalid(
                        "role_conflict.role_required",
                        "indique qual dos papéis propostos aplicar",
                    )
                    .with_field("role_id", "um de proposed_role_ids")
                    .into())
                }
            };
            ensure_can_assign(&state, org_id, auth.user_id, role).await?;
            let mut tx = state.db.begin().await?;
            crate::org::set_member_role_tx(&mut tx, org_id, c.user_id, role, "odoo_group").await?;
            tx.commit()
                .await
                .map_err(crate::org::map_member_write_error)?;
            Some(role)
        }
        _ => {
            return Err(DomainError::invalid(
                "role_conflict.invalid_decision",
                "decisão desconhecida",
            )
            .with_field("decision", "keep_current | apply_proposed")
            .into())
        }
    };
    let n = sqlx::query(
        "UPDATE role_conflicts SET status = 'resolved', decision = $3, applied_role_id = $4,
                resolved_by = $5, resolved_at = now()
          WHERE org_id = $1 AND id = $2 AND status = 'pending'",
    )
    .bind(org_id)
    .bind(conflict_id)
    .bind(&req.decision)
    .bind(applied)
    .bind(auth.user_id)
    .execute(&state.db)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(DomainError::conflict(
            "role_conflict.not_pending",
            "o conflito já foi decidido",
        )
        .into());
    }
    crate::audit::log(&state.db, Some(org_id), auth.user_id, "role_conflict.resolved",
        &audit_json(serde_json::json!({"conflict_id": conflict_id, "user_id": c.user_id,
            "decision": req.decision, "before_role_id": c.current_role_id, "applied_role_id": applied}))).await;
    Ok(Json(load().await?))
}

#[cfg(test)]
mod tests {
    use super::csv_field;

    #[test]
    fn csv_escapes_and_neutralises_formulas() {
        assert_eq!(csv_field("Gestor"), "Gestor");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("=HYPERLINK(1)"), "'=HYPERLINK(1)");
        assert_eq!(csv_field("diz \"olá\""), "\"diz \"\"olá\"\"\"");
        assert_eq!(csv_field("x;y"), "\"x;y\"");
    }
}
