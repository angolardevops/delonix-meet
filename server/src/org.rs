//! Multi-tenant enterprise: organizações, filiais, employees e grupos.
//!
//! Quem cria uma organização torna-se `admin`. Admins gerem filiais,
//! employees e grupos. Membros veem o diretório e podem iniciar chamadas.

use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Organization {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct OrgSummary {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub role: String,
    pub member_count: i64,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub retention_days: i32,
    pub max_groups: Option<i32>,
    pub max_rooms: Option<i32>,
    pub max_meetings: Option<i32>,
}

#[derive(Deserialize)]
pub struct OrgSettingsReq {
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub retention_days: i32,
    #[serde(default)]
    pub max_groups: Option<i32>,
    #[serde(default)]
    pub max_rooms: Option<i32>,
    #[serde(default)]
    pub max_meetings: Option<i32>,
    /// Dial-in PSTN: backend de media ('freeswitch' | 'provider').
    #[serde(default)]
    pub voice_media_backend: Option<String>,
    /// Dial-in PSTN: modelo de DID ('shared' | 'dedicated').
    #[serde(default)]
    pub voice_did_model: Option<String>,
}

/// Definições da organização (só admin): domínio de produção + retenção.
/// O domínio (ex.: `meet.acme.com`) é usado nos links partilháveis; a
/// retenção (>0) apaga gravações mais antigas que N dias.
pub async fn update_settings(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<OrgSettingsReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;
    // Normaliza o domínio: sem esquema, sem barra final, minúsculas.
    let domain = req
        .domain
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_lowercase();
    if domain.len() > 253 || domain.contains(' ') {
        return Err(ApiError::BadRequest("domínio inválido".into()));
    }
    let retention = req.retention_days.clamp(0, 3650);
    // Quotas: None/negativo => ilimitado (NULL).
    let norm = |v: Option<i32>| v.filter(|n| *n >= 0);
    // Dial-in PSTN: valida os enums (None => mantém o atual via COALESCE).
    let backend = req
        .voice_media_backend
        .as_deref()
        .and_then(|b| matches!(b, "freeswitch" | "provider").then(|| b.to_string()));
    let did_model = req
        .voice_did_model
        .as_deref()
        .and_then(|m| matches!(m, "shared" | "dedicated").then(|| m.to_string()));
    sqlx::query(
        "UPDATE organizations SET domain = $1, retention_days = $2,
             max_groups = $3, max_rooms = $4, max_meetings = $5,
             voice_media_backend = COALESCE($7, voice_media_backend),
             voice_did_model = COALESCE($8, voice_did_model)
         WHERE id = $6",
    )
    .bind(&domain)
    .bind(retention)
    .bind(norm(req.max_groups))
    .bind(norm(req.max_rooms))
    .bind(norm(req.max_meetings))
    .bind(org_id)
    .bind(backend)
    .bind(did_model)
    .execute(&state.db)
    .await?;
    crate::audit::log(&state.db, Some(org_id), auth.user_id, "org.settings_updated", &domain).await;
    Ok(Json(
        serde_json::json!({ "ok": true, "domain": domain, "retention_days": retention }),
    ))
}

/// Faz cumprir uma quota da organização: se o limite (`limit_col`) estiver
/// definido e a contagem atual em `count_sql` já o atingir, devolve 409.
/// `count_sql` deve contar por `org_id = $1` (grupos/salas) — ver chamadas.
async fn enforce_quota(
    state: &AppState,
    org_id: Uuid,
    limit_col: &str,
    count_sql: &str,
) -> Result<(), ApiError> {
    let limit: Option<i32> = sqlx::query_scalar(&format!(
        "SELECT {limit_col} FROM organizations WHERE id = $1"
    ))
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;
    if let Some(max) = limit {
        let count: i64 = sqlx::query_scalar(count_sql)
            .bind(org_id)
            .fetch_one(&state.db)
            .await?;
        if count >= max as i64 {
            return Err(ApiError::Conflict(format!(
                "limite da organização atingido ({max})"
            )));
        }
    }
    Ok(())
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Branch {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub location: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Employee {
    pub user_id: Uuid,
    pub username: String,
    pub email: String,
    pub role: String,
    pub title: String,
    pub branch_id: Option<Uuid>,
    pub branch_name: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Group {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub member_count: i64,
}

// ---------- helpers ----------

pub async fn role_in_org(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<Option<String>, ApiError> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT role FROM org_members WHERE org_id = $1 AND user_id = $2 AND archived_at IS NULL")
            .bind(org_id)
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;
    Ok(row.map(|r| r.0))
}

async fn require_admin(state: &AppState, org_id: Uuid, user_id: Uuid) -> Result<(), ApiError> {
    match role_in_org(state, org_id, user_id).await? {
        Some(r) if r == "admin" => Ok(()),
        Some(_) => Err(ApiError::Unauthorized),
        None => Err(ApiError::NotFound),
    }
}

/// Igual a `require_admin`, exposto para outros módulos (webhooks, retenção).
pub async fn require_admin_pub(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<(), ApiError> {
    require_admin(state, org_id, user_id).await
}

async fn require_member(state: &AppState, org_id: Uuid, user_id: Uuid) -> Result<(), ApiError> {
    match role_in_org(state, org_id, user_id).await? {
        Some(_) => Ok(()),
        None => Err(ApiError::NotFound),
    }
}

pub async fn require_member_pub(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<(), ApiError> {
    require_member(state, org_id, user_id).await
}

/// Slugify exposto para o registo de organizações (auth.rs).
pub fn slugify_pub(name: &str) -> String {
    slugify(name)
}

fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-'); // colapsa runs de separadores num só hífen
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "org".into()
    } else {
        trimmed
    }
}

// ---------- organizations ----------

#[derive(Deserialize)]
pub struct CreateOrgReq {
    pub name: String,
}

pub async fn create_org(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<CreateOrgReq>,
) -> Result<Json<Organization>, ApiError> {
    let name = req.name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(ApiError::BadRequest("nome da organização inválido".into()));
    }
    // Backstop anti-abuso: um utilizador não pode criar organizações sem limite.
    let owned: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM organizations WHERE created_by = $1")
        .bind(auth.user_id)
        .fetch_one(&state.db)
        .await?;
    if owned >= 20 {
        return Err(ApiError::Conflict(
            "limite de organizações por utilizador atingido".into(),
        ));
    }
    // slug único com sufixo em colisão.
    let base = slugify(name);
    let mut org: Option<Organization> = None;
    for i in 0..6 {
        let slug = if i == 0 {
            base.clone()
        } else {
            format!("{base}-{i}")
        };
        let res: Result<Organization, sqlx::Error> = sqlx::query_as(
            "INSERT INTO organizations (name, slug, created_by) VALUES ($1, $2, $3)
             RETURNING id, name, slug, created_by, created_at",
        )
        .bind(name)
        .bind(&slug)
        .bind(auth.user_id)
        .fetch_one(&state.db)
        .await;
        match res {
            Ok(o) => {
                org = Some(o);
                break;
            }
            Err(sqlx::Error::Database(db)) if db.is_unique_violation() => continue,
            Err(e) => return Err(e.into()),
        }
    }
    let org = org.ok_or_else(|| ApiError::internal("could not allocate org slug"))?;

    // O criador entra como admin.
    sqlx::query("INSERT INTO org_members (org_id, user_id, role, title) VALUES ($1, $2, 'admin', 'Administrador')")
        .bind(org.id)
        .bind(auth.user_id)
        .execute(&state.db)
        .await?;

    Ok(Json(org))
}

pub async fn my_orgs(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Vec<OrgSummary>>, ApiError> {
    let orgs: Vec<OrgSummary> = sqlx::query_as(
        r#"
        SELECT o.id, o.name, o.slug, m.role,
               (SELECT COUNT(*) FROM org_members mm WHERE mm.org_id = o.id) AS member_count,
               o.domain, o.retention_days, o.max_groups, o.max_rooms, o.max_meetings
        FROM organizations o
        JOIN org_members m ON m.org_id = o.id AND m.user_id = $1
        ORDER BY o.name
        "#,
    )
    .bind(auth.user_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(orgs))
}

// ---------- branches ----------

#[derive(Deserialize)]
pub struct CreateBranchReq {
    pub name: String,
    #[serde(default)]
    pub location: String,
}

pub async fn create_branch(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateBranchReq>,
) -> Result<Json<Branch>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;
    let name = req.name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(ApiError::BadRequest("nome da filial inválido".into()));
    }
    let branch: Branch = sqlx::query_as(
        "INSERT INTO branches (org_id, name, location) VALUES ($1, $2, $3)
         RETURNING id, org_id, name, location, created_at",
    )
    .bind(org_id)
    .bind(name)
    .bind(req.location.trim())
    .fetch_one(&state.db)
    .await?;
    Ok(Json(branch))
}

pub async fn list_branches(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<Branch>>, ApiError> {
    require_member(&state, org_id, auth.user_id).await?;
    let branches: Vec<Branch> = sqlx::query_as(
        "SELECT id, org_id, name, location, created_at FROM branches WHERE org_id = $1 ORDER BY name",
    )
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(branches))
}

// ---------- employees ----------

#[derive(Deserialize)]
pub struct AddEmployeeReq {
    /// Adicionar por email: se o utilizador existir liga-o; senão cria a conta.
    pub email: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub branch_id: Option<Uuid>,
}

pub async fn add_employee(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<AddEmployeeReq>,
) -> Result<Json<Employee>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;
    let email = req.email.trim().to_lowercase();
    if !email.contains('@') {
        return Err(ApiError::BadRequest("email inválido".into()));
    }
    // Org-first: o email do colaborador tem de ser do domínio da organização.
    let org_domain: Option<(String,)> =
        sqlx::query_as("SELECT email_domain FROM organizations WHERE id = $1")
            .bind(org_id)
            .fetch_optional(&state.db)
            .await?;
    if let Some((dom,)) = org_domain {
        if !dom.is_empty() && email.split('@').nth(1) != Some(dom.as_str()) {
            return Err(ApiError::BadRequest(format!(
                "o email tem de ser do domínio da organização (@{dom})"
            )));
        }
    }
    let role = req.role.as_deref().unwrap_or("member");
    if !matches!(role, "admin" | "member") {
        return Err(ApiError::BadRequest("role inválido".into()));
    }

    // Utilizador existente ou criar novo.
    let existing: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM users WHERE email = $1")
        .bind(&email)
        .fetch_optional(&state.db)
        .await?;
    let user_id = match existing {
        Some((id,)) => id,
        None => {
            let username = req
                .username
                .as_deref()
                .map(|s| s.trim().to_string())
                .filter(|s| s.len() >= 2)
                .unwrap_or_else(|| email.split('@').next().unwrap_or("employee").to_string());
            let password = req.password.as_deref().unwrap_or("changeme123");
            if !(8..=128).contains(&password.len()) {
                return Err(ApiError::BadRequest(
                    "password deve ter 8-128 caracteres".into(),
                ));
            }
            let hash = crate::auth::hash_password(password)?;
            let row: Result<(Uuid,), sqlx::Error> = sqlx::query_as(
                "INSERT INTO users (email, username, password_hash) VALUES ($1, $2, $3) RETURNING id",
            )
            .bind(&email)
            .bind(&username)
            .bind(&hash)
            .fetch_one(&state.db)
            .await;
            match row {
                Ok((id,)) => id,
                Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
                    return Err(ApiError::Conflict("email ou username já existe".into()))
                }
                Err(e) => return Err(e.into()),
            }
        }
    };

    sqlx::query(
        "INSERT INTO org_members (org_id, user_id, branch_id, role, title) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (org_id, user_id) DO UPDATE SET branch_id = $3, role = $4, title = $5",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(req.branch_id)
    .bind(role)
    .bind(req.title.trim())
    .execute(&state.db)
    .await?;

    let emp: Employee = sqlx::query_as(
        r#"SELECT m.user_id, u.username, u.email, m.role, m.title, m.branch_id, b.name AS branch_name
           FROM org_members m JOIN users u ON u.id = m.user_id
           LEFT JOIN branches b ON b.id = m.branch_id
           WHERE m.org_id = $1 AND m.user_id = $2"#,
    )
    .bind(org_id)
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;
    crate::audit::log(&state.db, Some(org_id), auth.user_id, "member.added", &emp.email).await;
    Ok(Json(emp))
}

pub async fn list_employees(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<Employee>>, ApiError> {
    require_member(&state, org_id, auth.user_id).await?;
    let emps: Vec<Employee> = sqlx::query_as(
        r#"SELECT m.user_id, u.username, u.email, m.role, m.title, m.branch_id, b.name AS branch_name
           FROM org_members m JOIN users u ON u.id = m.user_id
           LEFT JOIN branches b ON b.id = m.branch_id
           WHERE m.org_id = $1 AND m.archived_at IS NULL ORDER BY u.username"#,
    )
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(emps))
}

#[derive(Deserialize)]
pub struct UpdateEmployeeReq {
    pub role: Option<String>,
    pub title: Option<String>,
    pub branch_id: Option<Uuid>,
}

pub async fn update_employee(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, user_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateEmployeeReq>,
) -> Result<Json<Employee>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;
    if let Some(role) = &req.role {
        if !matches!(role.as_str(), "admin" | "member") {
            return Err(ApiError::BadRequest("role inválido".into()));
        }
    }
    if let Some(role) = &req.role {
        sqlx::query("UPDATE org_members SET role = $1 WHERE org_id = $2 AND user_id = $3")
            .bind(role)
            .bind(org_id)
            .bind(user_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(title) = &req.title {
        let title = title.trim();
        sqlx::query("UPDATE org_members SET title = $1 WHERE org_id = $2 AND user_id = $3")
            .bind(title)
            .bind(org_id)
            .bind(user_id)
            .execute(&state.db)
            .await?;
    }
    if req.branch_id.is_some() || req.role.is_some() {
        if let Some(branch_id) = req.branch_id {
            sqlx::query("UPDATE org_members SET branch_id = $1 WHERE org_id = $2 AND user_id = $3")
                .bind(branch_id)
                .bind(org_id)
                .bind(user_id)
                .execute(&state.db)
                .await?;
        }
    }
    let emp: Employee = sqlx::query_as(
        r#"SELECT m.user_id, u.username, u.email, m.role, m.title, m.branch_id, b.name AS branch_name
           FROM org_members m JOIN users u ON u.id = m.user_id
           LEFT JOIN branches b ON b.id = m.branch_id
           WHERE m.org_id = $1 AND m.user_id = $2"#,
    )
    .bind(org_id)
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(emp))
}

pub async fn remove_employee(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;
    if user_id == auth.user_id {
        return Err(ApiError::BadRequest(
            "não podes arquivar o teu próprio acesso".into(),
        ));
    }
    // Soft delete: archived_at + archived_by para auditoria futura
    sqlx::query(
        "UPDATE org_members SET archived_at = NOW(), archived_by = $3
         WHERE org_id = $1 AND user_id = $2 AND archived_at IS NULL",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(auth.user_id)
    .execute(&state.db)
    .await?;
    crate::audit::log(&state.db, Some(org_id), auth.user_id, "member.removed", &user_id.to_string()).await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------- groups ----------

#[derive(Deserialize)]
pub struct CreateGroupReq {
    pub name: String,
    #[serde(default)]
    pub member_ids: Vec<Uuid>,
}

pub async fn create_group(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateGroupReq>,
) -> Result<Json<Group>, ApiError> {
    require_member(&state, org_id, auth.user_id).await?;
    let name = req.name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(ApiError::BadRequest("nome do grupo inválido".into()));
    }
    // RLS (migração 0024): employee_groups corre no contexto de tenant. O limite
    // de quota vive em `organizations` (sem RLS); o COUNT dos grupos existentes
    // corre na MESMA tx (sob RLS) para ser correto. Ver AppState::tenant_tx.
    let mut tx = state.tenant_tx(auth.user_id).await?;
    let limit: Option<i32> = sqlx::query_scalar("SELECT max_groups FROM organizations WHERE id = $1")
        .bind(org_id)
        .fetch_one(&mut *tx)
        .await?;
    if let Some(max) = limit {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM employee_groups WHERE org_id = $1")
            .bind(org_id)
            .fetch_one(&mut *tx)
            .await?;
        if count >= max as i64 {
            return Err(ApiError::Conflict(format!(
                "limite da organização atingido ({max})"
            )));
        }
    }
    let (group_id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO employee_groups (org_id, name, created_by) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(org_id)
    .bind(name)
    .bind(auth.user_id)
    .fetch_one(&mut *tx)
    .await?;

    // O criador entra sempre; membros validados como pertencentes à org.
    let mut ids = req.member_ids.clone();
    ids.push(auth.user_id);
    for uid in ids {
        if role_in_org(&state, org_id, uid).await?.is_some() {
            sqlx::query("INSERT INTO group_members (group_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
                .bind(group_id)
                .bind(uid)
                .execute(&mut *tx)
                .await?;
        }
    }

    let group: Group = sqlx::query_as(
        "SELECT g.id, g.org_id, g.name, (SELECT COUNT(*) FROM group_members gm WHERE gm.group_id = g.id) AS member_count
         FROM employee_groups g WHERE g.id = $1",
    )
    .bind(group_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(group))
}

pub async fn list_groups(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<Group>>, ApiError> {
    require_member(&state, org_id, auth.user_id).await?;
    // RLS: employee_groups tem Row-Level Security (migração 0024). A query corre
    // no contexto de tenant do utilizador — ver AppState::tenant_tx / ADR-0002.
    let mut tx = state.tenant_tx(auth.user_id).await?;
    let groups: Vec<Group> = sqlx::query_as(
        "SELECT g.id, g.org_id, g.name, (SELECT COUNT(*) FROM group_members gm WHERE gm.group_id = g.id) AS member_count
         FROM employee_groups g WHERE g.org_id = $1 ORDER BY g.name",
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(groups))
}

// ---------- salas presenciais (físicas) ----------

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct MeetingRoom {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub location: String,
    pub capacity: i32,
}

#[derive(Deserialize)]
pub struct CreateMeetingRoomReq {
    pub name: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub capacity: i32,
}

pub async fn create_meeting_room(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateMeetingRoomReq>,
) -> Result<Json<MeetingRoom>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;
    let name = req.name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(ApiError::BadRequest("nome da sala inválido".into()));
    }
    enforce_quota(
        &state,
        org_id,
        "max_rooms",
        "SELECT COUNT(*) FROM meeting_rooms WHERE org_id = $1",
    )
    .await?;
    let room: MeetingRoom = sqlx::query_as(
        "INSERT INTO meeting_rooms (org_id, name, location, capacity) VALUES ($1, $2, $3, $4)
         RETURNING id, org_id, name, location, capacity",
    )
    .bind(org_id)
    .bind(name)
    .bind(req.location.trim())
    .bind(req.capacity.max(0))
    .fetch_one(&state.db)
    .await?;
    Ok(Json(room))
}

pub async fn list_meeting_rooms(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<MeetingRoom>>, ApiError> {
    require_member(&state, org_id, auth.user_id).await?;
    let rooms: Vec<MeetingRoom> = sqlx::query_as(
        "SELECT id, org_id, name, location, capacity FROM meeting_rooms WHERE org_id = $1 ORDER BY name",
    )
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rooms))
}

// ---------- Estatísticas da organização (consola admin) ----------

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct WeekBucket {
    pub week_start: DateTime<Utc>,
    pub count: i64,
    pub minutes: i64,
}

#[derive(Debug, Serialize)]
pub struct OrgStats {
    pub meetings_30d: i64,
    pub meeting_minutes_30d: i64,
    pub active_users_30d: i64,
    pub members_total: i64,
    pub recordings_total: i64,
    pub recordings_bytes: i64,
    pub video_30d: i64,
    pub voice_30d: i64,
    pub avg_duration_min: i64,
    pub top_organizers: Vec<Organizer>,
    pub meetings_per_week: Vec<WeekBucket>,
    /// Qualidade das chamadas (amostras QoS dos clientes, últimos 30 dias).
    pub quality_samples_30d: i64,
    pub avg_rtt_ms: Option<i64>,
    pub avg_loss_pct: f64,
    /// % de amostras boas (perda < 2%) e fracas (perda > 5%).
    pub pct_good: i64,
    pub pct_poor: i64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Organizer {
    pub username: String,
    pub count: i64,
}

/// KPIs agregados da organização: reuniões e minutos dos últimos 30 dias,
/// utilizadores envolvidos, gravações e série semanal (8 semanas).
/// Uma reunião "pertence" à org se o dono ou um convidado for membro.
pub async fn org_stats(
    State(state): State<Arc<AppState>>,
    Path(org_id): Path<Uuid>,
    auth: AuthUser,
) -> Result<Json<OrgStats>, ApiError> {
    // Least-privilege: KPIs da org (volume, leaderboard nominal) só para admins.
    require_admin(&state, org_id, auth.user_id).await?;

    const ORG_MEETING: &str =
        "(EXISTS (SELECT 1 FROM org_members om WHERE om.org_id = $1 AND om.user_id = m.owner_id)
         OR EXISTS (SELECT 1 FROM meeting_invitees mi
                    JOIN org_members om2 ON om2.user_id = mi.user_id AND om2.org_id = $1
                    WHERE mi.meeting_id = m.id))";

    let (meetings_30d, meeting_minutes_30d, video_30d, voice_30d): (i64, i64, i64, i64) =
        sqlx::query_as(&format!(
            "SELECT COUNT(*), COALESCE(SUM(m.duration_min), 0)::bigint,
                    COUNT(*) FILTER (WHERE m.kind = 'video'),
                    COUNT(*) FILTER (WHERE m.kind = 'voice')
             FROM meetings m
             WHERE m.starts_at >= now() - interval '30 days' AND m.starts_at <= now() AND {ORG_MEETING}"
        ))
        .bind(org_id)
        .fetch_one(&state.db)
        .await?;
    let avg_duration_min = if meetings_30d > 0 {
        meeting_minutes_30d / meetings_30d
    } else {
        0
    };

    let top_organizers: Vec<Organizer> = sqlx::query_as(&format!(
        "SELECT u.username, COUNT(*) AS count FROM meetings m
         JOIN users u ON u.id = m.owner_id
         WHERE m.starts_at >= now() - interval '30 days' AND m.starts_at <= now() AND {ORG_MEETING}
         GROUP BY u.username ORDER BY count DESC, u.username LIMIT 5"
    ))
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;

    let (active_users_30d,): (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT u) FROM (
             SELECT m.owner_id AS u FROM meetings m
             WHERE m.starts_at >= now() - interval '30 days'
           UNION
             SELECT mi.user_id FROM meeting_invitees mi
             JOIN meetings m ON m.id = mi.meeting_id
             WHERE m.starts_at >= now() - interval '30 days'
         ) act
         JOIN org_members om ON om.user_id = act.u AND om.org_id = $1",
    )
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;

    let (members_total,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM org_members WHERE org_id = $1")
            .bind(org_id)
            .fetch_one(&state.db)
            .await?;

    let (recordings_total, recordings_bytes): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(r.size_bytes), 0)::bigint FROM recordings r
         WHERE EXISTS (SELECT 1 FROM org_members om WHERE om.org_id = $1 AND om.user_id = r.uploader_id)",
    )
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;

    let meetings_per_week: Vec<WeekBucket> = sqlx::query_as(&format!(
        "SELECT date_trunc('week', m.starts_at) AS week_start, COUNT(*) AS count,
                COALESCE(SUM(m.duration_min), 0)::bigint AS minutes
         FROM meetings m
         WHERE m.starts_at >= date_trunc('week', now()) - interval '7 weeks'
           AND m.starts_at < date_trunc('week', now()) + interval '1 week'
           AND {ORG_MEETING}
         GROUP BY 1 ORDER BY 1",
    ))
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;

    // Qualidade das chamadas: agregado das amostras QoS dos membros da org
    // (rooms não têm org_id — o scoping é pelo utilizador que reporta).
    let (quality_samples_30d, avg_rtt_ms, avg_loss_pct, pct_good, pct_poor): (
        i64,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
    ) = sqlx::query_as(
        "SELECT COUNT(*),
                AVG(q.rtt_ms)::float8,
                AVG(q.loss_pct)::float8,
                (100.0 * COUNT(*) FILTER (WHERE q.loss_pct < 2.0) / NULLIF(COUNT(*), 0))::float8,
                (100.0 * COUNT(*) FILTER (WHERE q.loss_pct > 5.0) / NULLIF(COUNT(*), 0))::float8
         FROM call_quality_samples q
         JOIN org_members om ON om.user_id = q.user_id AND om.org_id = $1
         WHERE q.created_at > now() - interval '30 days'",
    )
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(OrgStats {
        meetings_30d,
        meeting_minutes_30d,
        active_users_30d,
        members_total,
        recordings_total,
        recordings_bytes,
        video_30d,
        voice_30d,
        avg_duration_min,
        top_organizers,
        meetings_per_week,
        quality_samples_30d,
        avg_rtt_ms: avg_rtt_ms.map(|v| v.round() as i64),
        avg_loss_pct: (avg_loss_pct.unwrap_or(0.0) * 10.0).round() / 10.0,
        pct_good: pct_good.unwrap_or(0.0).round() as i64,
        pct_poor: pct_poor.unwrap_or(0.0).round() as i64,
    }))
}

/// user_ids dos membros de um grupo (para iniciar chamada de grupo).
/// Organizações a que um utilizador pertence (para disparar webhooks dos
/// eventos das suas reuniões/gravações).
pub async fn orgs_of_user(state: &AppState, user_id: Uuid) -> Vec<Uuid> {
    sqlx::query_as::<_, (Uuid,)>("SELECT org_id FROM org_members WHERE user_id = $1 AND archived_at IS NULL")
        .bind(user_id)
        .fetch_all(&state.db)
        .await
        .map(|rows| rows.into_iter().map(|r| r.0).collect())
        .unwrap_or_default()
}

/// Organizações onde `user_id` é admin (para analytics/ações administrativas).
pub async fn admin_orgs_of_user(state: &AppState, user_id: Uuid) -> Vec<Uuid> {
    sqlx::query_as::<_, (Uuid,)>(
        "SELECT org_id FROM org_members WHERE user_id = $1 AND role = 'admin' AND archived_at IS NULL",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map(|rows| rows.into_iter().map(|r| r.0).collect())
    .unwrap_or_default()
}

/// Utilizadores que partilham pelo menos uma organização com `user_id` (exclui
/// o próprio). Base do isolamento multi-tenant em presença/pesquisa/chamadas.
pub async fn org_co_members(state: &AppState, user_id: Uuid) -> Vec<Uuid> {
    sqlx::query_as::<_, (Uuid,)>(
        "SELECT DISTINCT b.user_id FROM org_members a
         JOIN org_members b ON a.org_id = b.org_id
         WHERE a.user_id = $1 AND b.user_id <> $1
           AND a.archived_at IS NULL AND b.archived_at IS NULL",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map(|rows| rows.into_iter().map(|r| r.0).collect())
    .unwrap_or_default()
}

/// Domínio de produção da primeira organização do utilizador (para links
/// partilháveis); vazio se nenhuma org tiver domínio configurado.
pub async fn primary_domain(state: &AppState, user_id: Uuid) -> String {
    sqlx::query_as::<_, (String,)>(
        "SELECT o.domain FROM organizations o
         JOIN org_members m ON m.org_id = o.id
         WHERE m.user_id = $1 AND o.domain <> '' ORDER BY o.name LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .map(|r| r.0)
    .unwrap_or_default()
}

// ---------- SSO Config (admin) ----------

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SsoConfigPublic {
    pub org_id: Uuid,
    pub issuer_url: String,
    pub client_id: String,
    pub enforce_sso: bool,
}

#[derive(Deserialize)]
pub struct SsoConfigReq {
    pub issuer_url: String,
    pub client_id: String,
    /// Vazio → mantém o segredo existente (no-op em update sem nova rotação).
    #[serde(default)]
    pub client_secret: String,
    pub enforce_sso: bool,
}

/// `GET /api/orgs/:org_id/sso` — lê a config OIDC (sem devolver o segredo).
pub async fn get_sso_config(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Option<SsoConfigPublic>>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;
    let row: Option<SsoConfigPublic> = sqlx::query_as(
        "SELECT org_id, issuer_url, client_id, enforce_sso
         FROM org_sso_configs WHERE org_id = $1",
    )
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?;
    Ok(Json(row))
}

/// `PUT /api/orgs/:org_id/sso` — cria ou atualiza a config OIDC.
/// O `client_secret` só é atualizado se for enviado não-vazio.
pub async fn upsert_sso_config(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<SsoConfigReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;

    let issuer = req.issuer_url.trim().to_string();
    let client_id = req.client_id.trim().to_string();
    if issuer.is_empty() || client_id.is_empty() {
        return Err(ApiError::BadRequest(
            "issuer_url e client_id são obrigatórios".into(),
        ));
    }
    // Validação mínima do issuer URL.
    if !issuer.starts_with("https://") {
        return Err(ApiError::BadRequest(
            "issuer_url deve começar por https://".into(),
        ));
    }

    if req.client_secret.trim().is_empty() {
        // Atualizar sem tocar no segredo (rotação lazy).
        sqlx::query(
            "INSERT INTO org_sso_configs (org_id, issuer_url, client_id, client_secret, enforce_sso)
             VALUES ($1, $2, $3, '', $4)
             ON CONFLICT (org_id) DO UPDATE
             SET issuer_url = EXCLUDED.issuer_url,
                 client_id  = EXCLUDED.client_id,
                 enforce_sso = EXCLUDED.enforce_sso,
                 updated_at = now()",
        )
        .bind(org_id)
        .bind(&issuer)
        .bind(&client_id)
        .bind(req.enforce_sso)
        .execute(&state.db)
        .await?;
    } else {
        sqlx::query(
            "INSERT INTO org_sso_configs (org_id, issuer_url, client_id, client_secret, enforce_sso)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (org_id) DO UPDATE
             SET issuer_url     = EXCLUDED.issuer_url,
                 client_id      = EXCLUDED.client_id,
                 client_secret  = EXCLUDED.client_secret,
                 enforce_sso    = EXCLUDED.enforce_sso,
                 updated_at     = now()",
        )
        .bind(org_id)
        .bind(&issuer)
        .bind(&client_id)
        .bind(req.client_secret.trim())
        .bind(req.enforce_sso)
        .execute(&state.db)
        .await?;
    }

    tracing::info!(%org_id, %issuer, "SSO config upserted");
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `DELETE /api/orgs/:org_id/sso` — remove a config OIDC (desativa SSO).
pub async fn delete_sso_config(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;
    sqlx::query("DELETE FROM org_sso_configs WHERE org_id = $1")
        .bind(org_id)
        .execute(&state.db)
        .await?;
    tracing::info!(%org_id, "SSO config deleted");
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn group_member_ids(state: &AppState, group_id: Uuid) -> Result<Vec<Uuid>, ApiError> {
    let rows: Vec<(Uuid,)> =
        sqlx::query_as("SELECT user_id FROM group_members WHERE group_id = $1")
            .bind(group_id)
            .fetch_all(&state.db)
            .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

#[cfg(test)]
mod tests {
    use super::slugify;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Kaeso Lda"), "kaeso-lda");
        assert_eq!(slugify("  Açores & Cia!  "), "a-ores-cia");
        assert_eq!(slugify("***"), "org");
    }
}
