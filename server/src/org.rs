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
        sqlx::query_as("SELECT role FROM org_members WHERE org_id = $1 AND user_id = $2")
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

async fn require_member(state: &AppState, org_id: Uuid, user_id: Uuid) -> Result<(), ApiError> {
    match role_in_org(state, org_id, user_id).await? {
        Some(_) => Ok(()),
        None => Err(ApiError::NotFound),
    }
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
    if trimmed.is_empty() { "org".into() } else { trimmed }
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
    // slug único com sufixo em colisão.
    let base = slugify(name);
    let mut org: Option<Organization> = None;
    for i in 0..6 {
        let slug = if i == 0 { base.clone() } else { format!("{base}-{i}") };
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
               (SELECT COUNT(*) FROM org_members mm WHERE mm.org_id = o.id) AS member_count
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
            if password.len() < 8 {
                return Err(ApiError::BadRequest("password mínima 8 caracteres".into()));
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
           WHERE m.org_id = $1 ORDER BY u.username"#,
    )
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(emps))
}

pub async fn remove_employee(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;
    if user_id == auth.user_id {
        return Err(ApiError::BadRequest("não podes remover-te a ti próprio".into()));
    }
    sqlx::query("DELETE FROM org_members WHERE org_id = $1 AND user_id = $2")
        .bind(org_id)
        .bind(user_id)
        .execute(&state.db)
        .await?;
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
    let (group_id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO employee_groups (org_id, name, created_by) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(org_id)
    .bind(name)
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;

    // O criador entra sempre; membros validados como pertencentes à org.
    let mut ids = req.member_ids.clone();
    ids.push(auth.user_id);
    for uid in ids {
        if role_in_org(&state, org_id, uid).await?.is_some() {
            sqlx::query("INSERT INTO group_members (group_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
                .bind(group_id)
                .bind(uid)
                .execute(&state.db)
                .await?;
        }
    }

    let group: Group = sqlx::query_as(
        "SELECT g.id, g.org_id, g.name, (SELECT COUNT(*) FROM group_members gm WHERE gm.group_id = g.id) AS member_count
         FROM employee_groups g WHERE g.id = $1",
    )
    .bind(group_id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(group))
}

pub async fn list_groups(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<Group>>, ApiError> {
    require_member(&state, org_id, auth.user_id).await?;
    let groups: Vec<Group> = sqlx::query_as(
        "SELECT g.id, g.org_id, g.name, (SELECT COUNT(*) FROM group_members gm WHERE gm.group_id = g.id) AS member_count
         FROM employee_groups g WHERE g.org_id = $1 ORDER BY g.name",
    )
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
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
    require_member(&state, org_id, auth.user_id).await?;

    const ORG_MEETING: &str = "(EXISTS (SELECT 1 FROM org_members om WHERE om.org_id = $1 AND om.user_id = m.owner_id)
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
    let avg_duration_min = if meetings_30d > 0 { meeting_minutes_30d / meetings_30d } else { 0 };

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
    }))
}

/// user_ids dos membros de um grupo (para iniciar chamada de grupo).
pub async fn group_member_ids(state: &AppState, group_id: Uuid) -> Result<Vec<Uuid>, ApiError> {
    let rows: Vec<(Uuid,)> = sqlx::query_as("SELECT user_id FROM group_members WHERE group_id = $1")
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
