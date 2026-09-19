//! Papéis e permissões: um papel por organização, com um único pai (herança
//! simples) e um conjunto fixo de capacidades definidas em código — ver
//! [`PERMISSIONS`]. Dois papéis de sistema (Administrador/Membro) nascem com
//! cada organização e não podem ser editados nem apagados; a autoridade para
//! "é admin" continua a ser `org_members.role` (org::require_admin_pub),
//! inalterada — este módulo só acrescenta uma segunda verificação, mais fina,
//! para quem NÃO é admin mas tem um papel com capacidades delegadas.
//!
//! Âmbito por departamento, sincronização com grupos do Odoo e "simular
//! utilizador" ficam de fora — cada um precisa do seu próprio desenho, e um
//! campo que não aplica nada é pior do que não existir.

use axum::{
    extract::{Path, State},
    http::header,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

/// Catálogo fixo: (chave, rótulo). Cada entrada tem uma verificação REAL por
/// trás — `require_permission` é chamada a partir de voice.rs (voice.manage),
/// sms.rs (sms.manage) e stream_destinations.rs (streaming.manage).
/// Acrescentar uma entrada aqui sem ligar a verificação correspondente é
/// exactamente o que a doutrina proíbe.
pub const PERMISSIONS: &[(&str, &str)] = &[
    (
        "voice.manage",
        "Gerir telefonia — números, CDR e facturação",
    ),
    (
        "sms.manage",
        "Gerir SMS — gateways, encaminhamento e política",
    ),
    ("streaming.manage", "Gerir destinos de emissão em directo"),
];

const APPROVAL_TTL_HOURS: i64 = 24;

fn known_permission(p: &str) -> bool {
    PERMISSIONS.iter().any(|(k, _)| *k == p)
}

// ---------------------------------------------------------------------------
//  Resolução de permissões efectivas (com herança) e a verificação delegável
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Grant {
    requires_approval: bool,
}

async fn effective_permissions(
    state: &AppState,
    role_id: Uuid,
) -> Result<HashMap<String, Grant>, ApiError> {
    let mut out: HashMap<String, Grant> = HashMap::new();
    let mut current = Some(role_id);
    let mut depth = 0;
    // A escrita impede ciclos (ver `assert_no_cycle`); a fasquia de profundidade
    // é só uma rede de segurança adicional.
    while let Some(rid) = current {
        depth += 1;
        if depth > 16 {
            break;
        }
        let rows: Vec<(String, bool)> = sqlx::query_as(
            "SELECT permission, requires_approval FROM org_role_permissions WHERE role_id = $1",
        )
        .bind(rid)
        .fetch_all(&state.db)
        .await?;
        for (perm, requires_approval) in rows {
            // O próprio papel tem precedência sobre o que herda do pai.
            out.entry(perm).or_insert(Grant { requires_approval });
        }
        let parent: Option<(Option<Uuid>,)> =
            sqlx::query_as("SELECT parent_role_id FROM org_roles WHERE id = $1")
                .bind(rid)
                .fetch_optional(&state.db)
                .await?;
        current = parent.and_then(|(p,)| p);
    }
    Ok(out)
}

/// A verificação nova, mais fina, para capacidades delegáveis. Admin passa
/// sempre — comportamento inalterado. Quem não é admin só passa se o papel
/// atribuído (ou um dos que ele herda) conceder a capacidade; se a concessão
/// exige aprovação, a primeira tentativa cria o pedido em vez de deixar
/// passar.
pub async fn require_permission(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
    permission: &str,
) -> Result<(), ApiError> {
    let row = crate::org::active_member_roles(&state, org_id, user_id).await?;
    let Some((role, role_id)) = row else {
        return Err(ApiError::NotFound);
    };
    if role == "admin" {
        return Ok(());
    }
    let Some(role_id) = role_id else {
        return Err(ApiError::Forbidden);
    };
    let grants = effective_permissions(state, role_id).await?;
    let Some(grant) = grants.get(permission) else {
        return Err(ApiError::Forbidden);
    };
    if !grant.requires_approval {
        return Ok(());
    }

    let existing: Option<(String, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT status, expires_at FROM org_permission_requests
         WHERE org_id = $1 AND requester_id = $2 AND permission = $3
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(permission)
    .fetch_optional(&state.db)
    .await?;

    if let Some((status, expires_at)) = &existing {
        if status == "approved" && expires_at.is_some_and(|e| e > Utc::now()) {
            return Ok(());
        }
        if status == "pending" {
            return Err(ApiError::Unprocessable(
                "pedido de aprovação já enviado — aguarda decisão de um administrador".into(),
            ));
        }
    }

    sqlx::query(
        "INSERT INTO org_permission_requests (org_id, requester_id, permission) VALUES ($1, $2, $3)",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(permission)
    .execute(&state.db)
    .await?;
    Err(ApiError::Unprocessable(
        "esta acção exige aprovação — pedido enviado a um administrador".into(),
    ))
}

/// As próprias permissões efectivas (para a UI decidir o que mostrar a quem
/// não é admin mas tem um papel delegado).
pub async fn my_permissions(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let row = crate::org::active_member_roles(&state, org_id, auth.user_id).await?;
    let Some((role, role_id)) = row else {
        return Err(ApiError::NotFound);
    };
    if role == "admin" {
        return Ok(Json(serde_json::json!({
            "admin": true,
            "permissions": PERMISSIONS.iter().map(|(k, _)| k).collect::<Vec<_>>(),
            "pending": Vec::<String>::new(),
        })));
    }
    let granted = match role_id {
        Some(rid) => effective_permissions(&state, rid).await?,
        None => HashMap::new(),
    };
    let permissions: Vec<&str> = granted
        .iter()
        .filter(|(_, g)| !g.requires_approval)
        .map(|(k, _)| k.as_str())
        .collect();
    let pending_or_approval: Vec<&str> = granted
        .iter()
        .filter(|(_, g)| g.requires_approval)
        .map(|(k, _)| k.as_str())
        .collect();
    Ok(Json(serde_json::json!({
        "admin": false,
        "permissions": permissions,
        "pending": pending_or_approval,
    })))
}

// ---------------------------------------------------------------------------
//  CRUD de papéis (admin-only — atribuir/editar papéis não é, em si, uma
//  capacidade delegável: seria um caminho de escalada de privilégio).
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct PermissionCatalogEntry {
    key: &'static str,
    label: &'static str,
}

#[derive(Serialize, Clone)]
pub struct PermissionGrant {
    permission: String,
    requires_approval: bool,
}

#[derive(Serialize)]
pub struct RoleOut {
    id: Uuid,
    name: String,
    is_system: bool,
    parent_role_id: Option<Uuid>,
    parent_role_name: Option<String>,
    member_count: i64,
    /// Concessões directas deste papel.
    permissions: Vec<PermissionGrant>,
    /// Concessões que chegam por herança do pai (e não estão já em `permissions`).
    inherited: Vec<PermissionGrant>,
}

#[derive(Serialize)]
pub struct RolesResp {
    catalog: Vec<PermissionCatalogEntry>,
    roles: Vec<RoleOut>,
}

async fn load_role(state: &AppState, org_id: Uuid, role_id: Uuid) -> Result<RoleOut, ApiError> {
    let row: Option<(Uuid, String, bool, Option<Uuid>)> = sqlx::query_as(
        "SELECT id, name, is_system, parent_role_id FROM org_roles WHERE id = $1 AND org_id = $2",
    )
    .bind(role_id)
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?;
    let Some((id, name, is_system, parent_role_id)) = row else {
        return Err(ApiError::NotFound);
    };
    let parent_role_name: Option<String> = match parent_role_id {
        Some(pid) => {
            sqlx::query_scalar("SELECT name FROM org_roles WHERE id = $1")
                .bind(pid)
                .fetch_optional(&state.db)
                .await?
        }
        None => None,
    };
    let member_count: (i64,) = (crate::org::count_members_with_role_id(&state, id).await?,);

    let direct: Vec<(String, bool)> = sqlx::query_as(
        "SELECT permission, requires_approval FROM org_role_permissions WHERE role_id = $1",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    let direct_keys: std::collections::HashSet<String> =
        direct.iter().map(|(p, _)| p.clone()).collect();
    let permissions: Vec<PermissionGrant> = direct
        .into_iter()
        .map(|(permission, requires_approval)| PermissionGrant {
            permission,
            requires_approval,
        })
        .collect();

    let all = effective_permissions(state, id).await?;
    let inherited: Vec<PermissionGrant> = all
        .into_iter()
        .filter(|(p, _)| !direct_keys.contains(p))
        .map(|(permission, g)| PermissionGrant {
            permission,
            requires_approval: g.requires_approval,
        })
        .collect();

    Ok(RoleOut {
        id,
        name,
        is_system,
        parent_role_id,
        parent_role_name,
        member_count: member_count.0,
        permissions,
        inherited,
    })
}

pub async fn list_roles(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<RolesResp>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let ids: Vec<(Uuid,)> =
        sqlx::query_as("SELECT id FROM org_roles WHERE org_id = $1 ORDER BY is_system DESC, name")
            .bind(org_id)
            .fetch_all(&state.db)
            .await?;
    let mut roles = Vec::with_capacity(ids.len());
    for (id,) in ids {
        roles.push(load_role(&state, org_id, id).await?);
    }
    Ok(Json(RolesResp {
        catalog: PERMISSIONS
            .iter()
            .map(|(key, label)| PermissionCatalogEntry { key, label })
            .collect(),
        roles,
    }))
}

#[derive(Deserialize)]
pub struct PermissionInput {
    permission: String,
    #[serde(default)]
    requires_approval: bool,
}

#[derive(Deserialize)]
pub struct CreateRoleReq {
    name: String,
    #[serde(default)]
    parent_role_id: Option<Uuid>,
    #[serde(default)]
    permissions: Vec<PermissionInput>,
}

fn validate_permissions(perms: &[PermissionInput]) -> Result<(), ApiError> {
    for p in perms {
        if !known_permission(&p.permission) {
            return Err(ApiError::BadRequest(format!(
                "permissão desconhecida: {}",
                p.permission
            )));
        }
    }
    Ok(())
}

async fn assert_parent_valid(
    state: &AppState,
    org_id: Uuid,
    role_id: Option<Uuid>,
    parent_id: Uuid,
) -> Result<(), ApiError> {
    let exists: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM org_roles WHERE id = $1 AND org_id = $2")
            .bind(parent_id)
            .bind(org_id)
            .fetch_optional(&state.db)
            .await?;
    if exists.is_none() {
        return Err(ApiError::BadRequest("papel-pai inexistente".into()));
    }
    if Some(parent_id) == role_id {
        return Err(ApiError::BadRequest(
            "um papel não pode ser o seu próprio pai".into(),
        ));
    }
    // Anti-ciclo: se `role_id` está a ser editado, o pai proposto não pode
    // descender dele (percorre a cadeia de pais do pai proposto).
    if let Some(rid) = role_id {
        let mut current = Some(parent_id);
        let mut depth = 0;
        while let Some(c) = current {
            depth += 1;
            if depth > 16 {
                break;
            }
            if c == rid {
                return Err(ApiError::BadRequest(
                    "isso criaria um ciclo de herança".into(),
                ));
            }
            let parent: Option<(Option<Uuid>,)> =
                sqlx::query_as("SELECT parent_role_id FROM org_roles WHERE id = $1")
                    .bind(c)
                    .fetch_optional(&state.db)
                    .await?;
            current = parent.and_then(|(p,)| p);
        }
    }
    Ok(())
}

pub async fn create_role(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateRoleReq>,
) -> Result<Json<RoleOut>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let name = req.name.trim();
    if name.is_empty() || name.len() > 60 {
        return Err(ApiError::BadRequest(
            "nome do papel deve ter 1-60 caracteres".into(),
        ));
    }
    validate_permissions(&req.permissions)?;
    if let Some(parent_id) = req.parent_role_id {
        assert_parent_valid(&state, org_id, None, parent_id).await?;
    }

    let mut tx = state.db.begin().await?;
    let (role_id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO org_roles (org_id, name, is_system, parent_role_id)
         VALUES ($1, $2, FALSE, $3) RETURNING id",
    )
    .bind(org_id)
    .bind(name)
    .bind(req.parent_role_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| ApiError::from_unique(e, "já existe um papel com este nome"))?;
    for p in &req.permissions {
        sqlx::query(
            "INSERT INTO org_role_permissions (role_id, permission, requires_approval) VALUES ($1, $2, $3)",
        )
        .bind(role_id)
        .bind(&p.permission)
        .bind(p.requires_approval)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "rbac.role_created",
        name,
    )
    .await;
    Ok(Json(load_role(&state, org_id, role_id).await?))
}

#[derive(Deserialize)]
pub struct UpdateRoleReq {
    name: Option<String>,
    /// `Some(None)` limpa o pai; `Some(Some(id))` muda-o; `None` deixa como está.
    #[serde(default, deserialize_with = "de_double_option")]
    parent_role_id: Option<Option<Uuid>>,
    permissions: Option<Vec<PermissionInput>>,
}

// `serde(default)` sozinho não distingue "campo ausente" de "campo null" num
// `Option<Option<T>>` — este helper é o padrão serde para isso.
fn de_double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Some(Option::deserialize(d)?))
}

pub async fn update_role(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, role_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateRoleReq>,
) -> Result<Json<RoleOut>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let existing: Option<(bool,)> =
        sqlx::query_as("SELECT is_system FROM org_roles WHERE id = $1 AND org_id = $2")
            .bind(role_id)
            .bind(org_id)
            .fetch_optional(&state.db)
            .await?;
    let Some((is_system,)) = existing else {
        return Err(ApiError::NotFound);
    };
    if is_system {
        return Err(ApiError::BadRequest(
            "papéis do sistema não podem ser editados".into(),
        ));
    }
    if let Some(perms) = &req.permissions {
        validate_permissions(perms)?;
    }
    if let Some(Some(parent_id)) = req.parent_role_id {
        assert_parent_valid(&state, org_id, Some(role_id), parent_id).await?;
    }

    let mut tx = state.db.begin().await?;
    if let Some(name) = &req.name {
        let name = name.trim();
        if name.is_empty() || name.len() > 60 {
            return Err(ApiError::BadRequest(
                "nome do papel deve ter 1-60 caracteres".into(),
            ));
        }
        sqlx::query("UPDATE org_roles SET name = $1 WHERE id = $2")
            .bind(name)
            .bind(role_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| ApiError::from_unique(e, "já existe um papel com este nome"))?;
    }
    if let Some(parent) = req.parent_role_id {
        sqlx::query("UPDATE org_roles SET parent_role_id = $1 WHERE id = $2")
            .bind(parent)
            .bind(role_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(perms) = &req.permissions {
        sqlx::query("DELETE FROM org_role_permissions WHERE role_id = $1")
            .bind(role_id)
            .execute(&mut *tx)
            .await?;
        for p in perms {
            sqlx::query(
                "INSERT INTO org_role_permissions (role_id, permission, requires_approval) VALUES ($1, $2, $3)",
            )
            .bind(role_id)
            .bind(&p.permission)
            .bind(p.requires_approval)
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;

    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "rbac.role_updated",
        &role_id.to_string(),
    )
    .await;
    Ok(Json(load_role(&state, org_id, role_id).await?))
}

pub async fn delete_role(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, role_id)): Path<(Uuid, Uuid)>,
) -> Result<axum::http::StatusCode, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let existing: Option<(bool,)> =
        sqlx::query_as("SELECT is_system FROM org_roles WHERE id = $1 AND org_id = $2")
            .bind(role_id)
            .bind(org_id)
            .fetch_optional(&state.db)
            .await?;
    let Some((is_system,)) = existing else {
        return Err(ApiError::NotFound);
    };
    if is_system {
        return Err(ApiError::BadRequest(
            "papéis do sistema não podem ser apagados".into(),
        ));
    }
    let membro_id: (Uuid,) = sqlx::query_as(
        "SELECT id FROM org_roles WHERE org_id = $1 AND is_system = TRUE AND name = 'Membro'",
    )
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;

    let mut tx = state.db.begin().await?;
    crate::org::reassign_role_id_tx(&mut tx, role_id, membro_id.0).await?;
    // Qualquer papel filho perde a herança deste (fica sem pai, não em cadeia partida).
    sqlx::query("UPDATE org_roles SET parent_role_id = NULL WHERE parent_role_id = $1")
        .bind(role_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM org_roles WHERE id = $1")
        .bind(role_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "rbac.role_deleted",
        &role_id.to_string(),
    )
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn duplicate_role(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, role_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<RoleOut>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let source: Option<(String, Option<Uuid>)> =
        sqlx::query_as("SELECT name, parent_role_id FROM org_roles WHERE id = $1 AND org_id = $2")
            .bind(role_id)
            .bind(org_id)
            .fetch_optional(&state.db)
            .await?;
    let Some((source_name, parent_role_id)) = source else {
        return Err(ApiError::NotFound);
    };
    let direct: Vec<(String, bool)> = sqlx::query_as(
        "SELECT permission, requires_approval FROM org_role_permissions WHERE role_id = $1",
    )
    .bind(role_id)
    .fetch_all(&state.db)
    .await?;

    let mut new_name = format!("{source_name} (cópia)");
    let mut suffix = 2;
    loop {
        let taken: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM org_roles WHERE org_id = $1 AND name = $2")
                .bind(org_id)
                .bind(&new_name)
                .fetch_optional(&state.db)
                .await?;
        if taken.is_none() {
            break;
        }
        new_name = format!("{source_name} (cópia {suffix})");
        suffix += 1;
    }

    let mut tx = state.db.begin().await?;
    let (new_id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO org_roles (org_id, name, is_system, parent_role_id)
         VALUES ($1, $2, FALSE, $3) RETURNING id",
    )
    .bind(org_id)
    .bind(&new_name)
    .bind(parent_role_id)
    .fetch_one(&mut *tx)
    .await?;
    for (permission, requires_approval) in &direct {
        sqlx::query(
            "INSERT INTO org_role_permissions (role_id, permission, requires_approval) VALUES ($1, $2, $3)",
        )
        .bind(new_id)
        .bind(permission)
        .bind(requires_approval)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "rbac.role_duplicated",
        &new_name,
    )
    .await;
    Ok(Json(load_role(&state, org_id, new_id).await?))
}

#[derive(Deserialize)]
pub struct AssignRoleReq {
    role_id: Uuid,
}

/// Atribuir um papel a um membro fica admin-only: é a fronteira de escalada
/// de privilégio — nenhuma capacidade delegada inclui "atribuir papéis".
pub async fn assign_role(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, user_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<AssignRoleReq>,
) -> Result<axum::http::StatusCode, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let role_exists: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM org_roles WHERE id = $1 AND org_id = $2")
            .bind(req.role_id)
            .bind(org_id)
            .fetch_optional(&state.db)
            .await?;
    if role_exists.is_none() {
        return Err(ApiError::BadRequest("papel inexistente".into()));
    }
    let updated = crate::org::set_member_role_id(&state, org_id, user_id, req.role_id).await?;
    if !updated {
        return Err(ApiError::NotFound);
    }
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "rbac.member_role_assigned",
        &format!("{user_id} -> {}", req.role_id),
    )
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
//  Pedidos de aprovação
// ---------------------------------------------------------------------------

#[derive(Serialize, sqlx::FromRow)]
pub struct PermissionRequestOut {
    id: Uuid,
    requester_id: Uuid,
    requester_username: String,
    requester_email: String,
    permission: String,
    status: String,
    created_at: DateTime<Utc>,
    decided_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
}

pub async fn list_permission_requests(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<PermissionRequestOut>>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let rows: Vec<PermissionRequestOut> = sqlx::query_as(
        "SELECT r.id, r.requester_id, u.username AS requester_username, u.email AS requester_email,
                r.permission, r.status, r.created_at, r.decided_at, r.expires_at
         FROM org_permission_requests r JOIN users u ON u.id = r.requester_id
         WHERE r.org_id = $1
         ORDER BY (r.status = 'pending') DESC, r.created_at DESC
         LIMIT 200",
    )
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct DecideRequestReq {
    approve: bool,
}

pub async fn decide_permission_request(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, request_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<DecideRequestReq>,
) -> Result<axum::http::StatusCode, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let status = if req.approve { "approved" } else { "denied" };
    let expires_at = req
        .approve
        .then(|| Utc::now() + chrono::Duration::hours(APPROVAL_TTL_HOURS));
    let updated = sqlx::query(
        "UPDATE org_permission_requests
         SET status = $1, decided_by = $2, decided_at = now(), expires_at = $3
         WHERE id = $4 AND org_id = $5 AND status = 'pending'",
    )
    .bind(status)
    .bind(auth.user_id)
    .bind(expires_at)
    .bind(request_id)
    .bind(org_id)
    .execute(&state.db)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        if req.approve {
            "rbac.request_approved"
        } else {
            "rbac.request_denied"
        },
        &request_id.to_string(),
    )
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
//  Exportação CSV da matriz
// ---------------------------------------------------------------------------

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub async fn export_csv(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Response, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let ids: Vec<(Uuid,)> =
        sqlx::query_as("SELECT id FROM org_roles WHERE org_id = $1 ORDER BY is_system DESC, name")
            .bind(org_id)
            .fetch_all(&state.db)
            .await?;
    let mut roles = Vec::with_capacity(ids.len());
    for (id,) in ids {
        roles.push(load_role(&state, org_id, id).await?);
    }

    let mut out = String::new();
    out.push_str("Capacidade");
    for r in &roles {
        out.push(',');
        out.push_str(&csv_escape(&r.name));
    }
    out.push('\n');
    for (key, label) in PERMISSIONS {
        out.push_str(&csv_escape(label));
        for r in &roles {
            out.push(',');
            let cell = if let Some(g) = r.permissions.iter().find(|g| g.permission == *key) {
                if g.requires_approval {
                    "permitido (requer aprovação)"
                } else {
                    "permitido"
                }
            } else if let Some(g) = r.inherited.iter().find(|g| g.permission == *key) {
                if g.requires_approval {
                    "herdado (requer aprovação)"
                } else {
                    "herdado"
                }
            } else {
                "negado"
            };
            out.push_str(&csv_escape(cell));
        }
        out.push('\n');
    }

    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "rbac.matrix_exported",
        "",
    )
    .await;
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"papeis-e-permissoes.csv\"".to_string(),
            ),
        ],
        out,
    )
        .into_response())
}
