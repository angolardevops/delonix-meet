//! Agenda de reunião (tópicos com controlo de execução) e Plano de Ação 5W2H.
//!
//! Segurança:
//! - Agenda: qualquer membro da reunião pode ver e marcar tópicos como feitos.
//!   Só o anfitrião pode criar/editar/apagar tópicos.
//! - Plano de Ação: qualquer membro pode ver. Só o anfitrião pode criar/editar itens.

use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

// ─── helpers de autorização ────────────────────────────────────────────────

/// Devolve (owner_id, org_id) se a reunião existir. 404 se não.
async fn meeting_owner(db: &sqlx::PgPool, meeting_id: Uuid) -> Result<Uuid, ApiError> {
    let row: Option<(Uuid,)> = sqlx::query_as("SELECT owner_id FROM meetings WHERE id = $1")
        .bind(meeting_id)
        .fetch_optional(db)
        .await?;
    row.map(|r| r.0).ok_or(ApiError::NotFound)
}

/// Garante que o utilizador é o anfitrião. 403 caso contrário.
async fn require_owner(db: &sqlx::PgPool, meeting_id: Uuid, user_id: Uuid) -> Result<(), ApiError> {
    let owner = meeting_owner(db, meeting_id).await?;
    if owner != user_id {
        return Err(ApiError::Unauthorized);
    }
    Ok(())
}

/// Garante que o utilizador é membro (convidado aceite ou anfitrião) ou o dono.
/// Reuniões fechadas: qualquer utilizador autenticado pode ver a agenda/plano
/// (os convidados já têm acesso à reunião pelo calendário).
async fn require_member_or_owner(
    db: &sqlx::PgPool,
    meeting_id: Uuid,
    user_id: Uuid,
) -> Result<(), ApiError> {
    let row: Option<(bool,)> = sqlx::query_as(
        "SELECT TRUE FROM meetings m
         LEFT JOIN meeting_invitees mi ON mi.meeting_id = m.id AND mi.user_id = $2
         WHERE m.id = $1 AND (m.owner_id = $2 OR mi.user_id IS NOT NULL)",
    )
    .bind(meeting_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    row.ok_or(ApiError::Unauthorized)?;
    Ok(())
}

// ─── Agenda ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AgendaItem {
    pub id: Uuid,
    pub meeting_id: Uuid,
    pub position: i16,
    pub topic: String,
    pub description: String,
    pub duration_min: i16,
    pub done: bool,
    pub done_at: Option<DateTime<Utc>>,
    pub done_by_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct AgendaItemReq {
    pub topic: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_dur")]
    pub duration_min: i16,
    #[serde(default)]
    pub position: Option<i16>,
}
fn default_dur() -> i16 {
    5
}

#[derive(Deserialize)]
pub struct AgendaPatchReq {
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub duration_min: Option<i16>,
    #[serde(default)]
    pub done: Option<bool>,
    #[serde(default)]
    pub position: Option<i16>,
}

/// `GET /api/meetings/:id/agenda`
pub async fn list_agenda(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(meeting_id): Path<Uuid>,
) -> Result<Json<Vec<AgendaItem>>, ApiError> {
    require_member_or_owner(&state.db, meeting_id, auth.user_id).await?;
    let items: Vec<AgendaItem> = sqlx::query_as(
        "SELECT id, meeting_id, position, topic, description, duration_min,
                done, done_at, done_by_id, created_at
         FROM meeting_agenda_items WHERE meeting_id = $1 ORDER BY position, created_at",
    )
    .bind(meeting_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(items))
}

/// `POST /api/meetings/:id/agenda` — adiciona tópico (só anfitrião).
pub async fn add_agenda_item(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(meeting_id): Path<Uuid>,
    Json(req): Json<AgendaItemReq>,
) -> Result<Json<AgendaItem>, ApiError> {
    require_owner(&state.db, meeting_id, auth.user_id).await?;
    let topic = req.topic.trim().to_string();
    if topic.is_empty() || topic.len() > 200 {
        return Err(ApiError::BadRequest(
            "tópico deve ter 1-200 caracteres".into(),
        ));
    }
    // Position: próximo disponível se não especificado.
    let pos = if let Some(p) = req.position {
        p
    } else {
        let max: Option<i16> = sqlx::query_scalar(
            "SELECT MAX(position) FROM meeting_agenda_items WHERE meeting_id = $1",
        )
        .bind(meeting_id)
        .fetch_one(&state.db)
        .await?;
        max.unwrap_or(0) + 1
    };
    let item: AgendaItem = sqlx::query_as(
        "INSERT INTO meeting_agenda_items
            (meeting_id, position, topic, description, duration_min)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, meeting_id, position, topic, description, duration_min,
                   done, done_at, done_by_id, created_at",
    )
    .bind(meeting_id)
    .bind(pos)
    .bind(&topic)
    .bind(req.description.trim())
    .bind(req.duration_min.clamp(1, 480))
    .fetch_one(&state.db)
    .await?;
    Ok(Json(item))
}

/// `PATCH /api/meetings/:id/agenda/:item_id` — editar ou marcar como feito.
/// Qualquer membro pode marcar como feito; só o anfitrião pode editar os campos.
pub async fn patch_agenda_item(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((meeting_id, item_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<AgendaPatchReq>,
) -> Result<Json<AgendaItem>, ApiError> {
    // Verificar que o item pertence a esta reunião.
    let owner = meeting_owner(&state.db, meeting_id).await?;
    let exists: Option<(bool,)> =
        sqlx::query_as("SELECT TRUE FROM meeting_agenda_items WHERE id = $1 AND meeting_id = $2")
            .bind(item_id)
            .bind(meeting_id)
            .fetch_optional(&state.db)
            .await?;
    exists.ok_or(ApiError::NotFound)?;

    // Campos de edição requerem ser anfitrião.
    let is_owner = owner == auth.user_id;
    if (req.topic.is_some()
        || req.description.is_some()
        || req.duration_min.is_some()
        || req.position.is_some())
        && !is_owner
    {
        return Err(ApiError::Unauthorized);
    }
    // `done` pode ser alterado por qualquer membro.
    require_member_or_owner(&state.db, meeting_id, auth.user_id).await?;

    if let Some(done) = req.done {
        let (done_at, done_by): (Option<DateTime<Utc>>, Option<Uuid>) = if done {
            (Some(Utc::now()), Some(auth.user_id))
        } else {
            (None, None)
        };
        sqlx::query(
            "UPDATE meeting_agenda_items SET done=$1, done_at=$2, done_by_id=$3 WHERE id=$4",
        )
        .bind(done)
        .bind(done_at)
        .bind(done_by)
        .bind(item_id)
        .execute(&state.db)
        .await?;
    }
    if let Some(topic) = &req.topic {
        let t = topic.trim();
        if t.is_empty() || t.len() > 200 {
            return Err(ApiError::BadRequest("tópico inválido".into()));
        }
        sqlx::query("UPDATE meeting_agenda_items SET topic=$1 WHERE id=$2")
            .bind(t)
            .bind(item_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(desc) = &req.description {
        sqlx::query("UPDATE meeting_agenda_items SET description=$1 WHERE id=$2")
            .bind(desc.trim())
            .bind(item_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(dur) = req.duration_min {
        sqlx::query("UPDATE meeting_agenda_items SET duration_min=$1 WHERE id=$2")
            .bind(dur.clamp(1, 480))
            .bind(item_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(pos) = req.position {
        sqlx::query("UPDATE meeting_agenda_items SET position=$1 WHERE id=$2")
            .bind(pos)
            .bind(item_id)
            .execute(&state.db)
            .await?;
    }

    let item: AgendaItem = sqlx::query_as(
        "SELECT id, meeting_id, position, topic, description, duration_min,
                done, done_at, done_by_id, created_at
         FROM meeting_agenda_items WHERE id = $1",
    )
    .bind(item_id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(item))
}

/// `DELETE /api/meetings/:id/agenda/:item_id` — só anfitrião.
pub async fn delete_agenda_item(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((meeting_id, item_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_owner(&state.db, meeting_id, auth.user_id).await?;
    sqlx::query("DELETE FROM meeting_agenda_items WHERE id=$1 AND meeting_id=$2")
        .bind(item_id)
        .bind(meeting_id)
        .execute(&state.db)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ─── Plano de Ação 5W2H ────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ActionItem {
    pub id: Uuid,
    pub plan_id: Uuid,
    pub position: i16,
    pub what: String,
    pub when_date: Option<NaiveDate>,
    pub where_text: String,
    pub who_id: Option<Uuid>,
    pub who_name: String,
    pub why: String,
    pub how: String,
    pub resources: String,
    /// 'todo' | 'doing' | 'done'
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ActionPlan {
    pub id: Uuid,
    pub meeting_id: Uuid,
    pub goal: String,
    pub items: Vec<ActionItem>,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct ActionPlanGoalReq {
    pub goal: String,
}

#[derive(Deserialize)]
pub struct ActionItemReq {
    #[serde(default)]
    pub what: String,
    #[serde(default)]
    pub when_date: Option<NaiveDate>,
    #[serde(default)]
    pub where_text: String,
    #[serde(default)]
    pub who_id: Option<Uuid>,
    #[serde(default)]
    pub who_name: String,
    #[serde(default)]
    pub why: String,
    #[serde(default)]
    pub how: String,
    #[serde(default)]
    pub resources: String,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default)]
    pub position: Option<i16>,
}
fn default_status() -> String {
    "todo".into()
}

#[derive(Deserialize)]
pub struct ActionItemPatch {
    #[serde(default)]
    pub what: Option<String>,
    #[serde(default)]
    pub when_date: Option<NaiveDate>,
    #[serde(default)]
    pub where_text: Option<String>,
    #[serde(default)]
    pub who_id: Option<Uuid>,
    #[serde(default)]
    pub who_name: Option<String>,
    #[serde(default)]
    pub why: Option<String>,
    #[serde(default)]
    pub how: Option<String>,
    #[serde(default)]
    pub resources: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub position: Option<i16>,
}

async fn load_plan_with_items(
    db: &sqlx::PgPool,
    meeting_id: Uuid,
) -> Result<Option<ActionPlan>, ApiError> {
    let plan_row: Option<(Uuid, String, DateTime<Utc>)> =
        sqlx::query_as("SELECT id, goal, created_at FROM action_plans WHERE meeting_id = $1")
            .bind(meeting_id)
            .fetch_optional(db)
            .await?;

    let Some((plan_id, goal, created_at)) = plan_row else {
        return Ok(None);
    };

    let items: Vec<ActionItem> = sqlx::query_as(
        "SELECT id, plan_id, position, what, when_date, where_text,
                who_id, who_name, why, how, resources, status, created_at, updated_at
         FROM action_items WHERE plan_id = $1 ORDER BY position, created_at",
    )
    .bind(plan_id)
    .fetch_all(db)
    .await?;

    Ok(Some(ActionPlan {
        id: plan_id,
        meeting_id,
        goal,
        items,
        created_at,
    }))
}

/// `GET /api/meetings/:id/action-plan`
pub async fn get_action_plan(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(meeting_id): Path<Uuid>,
) -> Result<Json<Option<ActionPlan>>, ApiError> {
    require_member_or_owner(&state.db, meeting_id, auth.user_id).await?;
    Ok(Json(load_plan_with_items(&state.db, meeting_id).await?))
}

/// `PUT /api/meetings/:id/action-plan` — cria ou atualiza a META do plano.
pub async fn upsert_action_plan(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(meeting_id): Path<Uuid>,
    Json(req): Json<ActionPlanGoalReq>,
) -> Result<Json<ActionPlan>, ApiError> {
    require_owner(&state.db, meeting_id, auth.user_id).await?;
    sqlx::query(
        "INSERT INTO action_plans (meeting_id, goal)
         VALUES ($1, $2)
         ON CONFLICT (meeting_id) DO UPDATE SET goal = EXCLUDED.goal",
    )
    .bind(meeting_id)
    .bind(req.goal.trim())
    .execute(&state.db)
    .await?;
    let plan = load_plan_with_items(&state.db, meeting_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(plan))
}

/// `POST /api/meetings/:id/action-plan/items` — adiciona linha 5W2H.
pub async fn add_action_item(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(meeting_id): Path<Uuid>,
    Json(req): Json<ActionItemReq>,
) -> Result<Json<ActionItem>, ApiError> {
    require_owner(&state.db, meeting_id, auth.user_id).await?;
    if !matches!(req.status.as_str(), "todo" | "doing" | "done") {
        return Err(ApiError::BadRequest("status inválido".into()));
    }
    // Garantir que o plano existe (cria sem META se ainda não houver).
    sqlx::query(
        "INSERT INTO action_plans (meeting_id, goal) VALUES ($1, '')
         ON CONFLICT (meeting_id) DO NOTHING",
    )
    .bind(meeting_id)
    .execute(&state.db)
    .await?;

    let (plan_id,): (Uuid,) = sqlx::query_as("SELECT id FROM action_plans WHERE meeting_id = $1")
        .bind(meeting_id)
        .fetch_one(&state.db)
        .await?;

    let pos = if let Some(p) = req.position {
        p
    } else {
        let max: Option<i16> =
            sqlx::query_scalar("SELECT MAX(position) FROM action_items WHERE plan_id = $1")
                .bind(plan_id)
                .fetch_one(&state.db)
                .await?;
        max.unwrap_or(0) + 1
    };

    // Resolver nome do responsável (who_name) se who_id fornecido e who_name vazio.
    let who_name = if req.who_name.trim().is_empty() {
        if let Some(who) = req.who_id {
            sqlx::query_scalar::<_, String>("SELECT username FROM users WHERE id = $1")
                .bind(who)
                .fetch_optional(&state.db)
                .await?
                .unwrap_or_default()
        } else {
            String::new()
        }
    } else {
        req.who_name.trim().to_string()
    };

    let item: ActionItem = sqlx::query_as(
        "INSERT INTO action_items
            (plan_id, position, what, when_date, where_text, who_id, who_name,
             why, how, resources, status)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
         RETURNING id, plan_id, position, what, when_date, where_text,
                   who_id, who_name, why, how, resources, status, created_at, updated_at",
    )
    .bind(plan_id)
    .bind(pos)
    .bind(req.what.trim())
    .bind(req.when_date)
    .bind(req.where_text.trim())
    .bind(req.who_id)
    .bind(&who_name)
    .bind(req.why.trim())
    .bind(req.how.trim())
    .bind(req.resources.trim())
    .bind(&req.status)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(item))
}

/// `PATCH /api/action-items/:item_id` — atualiza campos ou status.
/// Qualquer membro pode mudar o status; só o anfitrião pode editar campos.
pub async fn patch_action_item(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(item_id): Path<Uuid>,
    Json(req): Json<ActionItemPatch>,
) -> Result<Json<ActionItem>, ApiError> {
    // Obter meeting_id pelo plano.
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT ap.meeting_id FROM action_items ai
         JOIN action_plans ap ON ap.id = ai.plan_id
         WHERE ai.id = $1",
    )
    .bind(item_id)
    .fetch_optional(&state.db)
    .await?;
    let (meeting_id,) = row.ok_or(ApiError::NotFound)?;

    let owner = meeting_owner(&state.db, meeting_id).await?;
    let is_owner = owner == auth.user_id;

    // Campos de edição requerem anfitrião.
    let editing = req.what.is_some()
        || req.where_text.is_some()
        || req.who_id.is_some()
        || req.who_name.is_some()
        || req.why.is_some()
        || req.how.is_some()
        || req.resources.is_some()
        || req.position.is_some()
        || req.when_date.is_some();
    if editing && !is_owner {
        return Err(ApiError::Unauthorized);
    }
    // Status pode ser alterado por qualquer membro.
    if req.status.is_some() {
        require_member_or_owner(&state.db, meeting_id, auth.user_id).await?;
    }

    if let Some(s) = &req.status {
        if !matches!(s.as_str(), "todo" | "doing" | "done") {
            return Err(ApiError::BadRequest("status inválido".into()));
        }
        sqlx::query("UPDATE action_items SET status=$1, updated_at=now() WHERE id=$2")
            .bind(s)
            .bind(item_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(v) = &req.what {
        sqlx::query("UPDATE action_items SET what=$1, updated_at=now() WHERE id=$2")
            .bind(v.trim())
            .bind(item_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(d) = req.when_date {
        sqlx::query("UPDATE action_items SET when_date=$1, updated_at=now() WHERE id=$2")
            .bind(d)
            .bind(item_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(v) = &req.where_text {
        sqlx::query("UPDATE action_items SET where_text=$1, updated_at=now() WHERE id=$2")
            .bind(v.trim())
            .bind(item_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(id) = req.who_id {
        let name = req.who_name.as_deref().unwrap_or("").trim().to_string();
        let resolved = if name.is_empty() {
            sqlx::query_scalar::<_, String>("SELECT username FROM users WHERE id = $1")
                .bind(id)
                .fetch_optional(&state.db)
                .await?
                .unwrap_or_default()
        } else {
            name
        };
        sqlx::query("UPDATE action_items SET who_id=$1, who_name=$2, updated_at=now() WHERE id=$3")
            .bind(id)
            .bind(&resolved)
            .bind(item_id)
            .execute(&state.db)
            .await?;
    } else if let Some(name) = &req.who_name {
        sqlx::query("UPDATE action_items SET who_name=$1, updated_at=now() WHERE id=$2")
            .bind(name.trim())
            .bind(item_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(v) = &req.why {
        sqlx::query("UPDATE action_items SET why=$1, updated_at=now() WHERE id=$2")
            .bind(v.trim())
            .bind(item_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(v) = &req.how {
        sqlx::query("UPDATE action_items SET how=$1, updated_at=now() WHERE id=$2")
            .bind(v.trim())
            .bind(item_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(v) = &req.resources {
        sqlx::query("UPDATE action_items SET resources=$1, updated_at=now() WHERE id=$2")
            .bind(v.trim())
            .bind(item_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(p) = req.position {
        sqlx::query("UPDATE action_items SET position=$1, updated_at=now() WHERE id=$2")
            .bind(p)
            .bind(item_id)
            .execute(&state.db)
            .await?;
    }

    let item: ActionItem = sqlx::query_as(
        "SELECT id, plan_id, position, what, when_date, where_text,
                who_id, who_name, why, how, resources, status, created_at, updated_at
         FROM action_items WHERE id = $1",
    )
    .bind(item_id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(item))
}

/// `DELETE /api/action-items/:item_id` — só anfitrião.
pub async fn delete_action_item(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(item_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT ap.meeting_id FROM action_items ai
         JOIN action_plans ap ON ap.id = ai.plan_id
         WHERE ai.id = $1",
    )
    .bind(item_id)
    .fetch_optional(&state.db)
    .await?;
    let (meeting_id,) = row.ok_or(ApiError::NotFound)?;
    require_owner(&state.db, meeting_id, auth.user_id).await?;
    sqlx::query("DELETE FROM action_items WHERE id = $1")
        .bind(item_id)
        .execute(&state.db)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
