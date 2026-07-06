//! Calendário: reuniões agendadas com convidados. A partir de uma reunião
//! é possível arrancar a chamada (cria a sala e devolve o código), seja
//! vídeo ou só voz.

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
pub struct Meeting {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub title: String,
    pub description: String,
    pub kind: String,
    pub starts_at: DateTime<Utc>,
    pub duration_min: i32,
    pub room_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub room_ref: Option<Uuid>,
    #[serde(default)]
    pub minutes: String,
    #[serde(default)]
    pub transcript: String,
}

/// Reunião enriquecida para a UI do calendário.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct MeetingItem {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub owner_name: String,
    pub title: String,
    pub description: String,
    pub kind: String,
    pub starts_at: DateTime<Utc>,
    pub duration_min: i32,
    pub room_code: Option<String>,
    pub is_owner: bool,
    pub minutes: String,
    pub room_ref: Option<Uuid>,
    pub room_name: Option<String>,
    /// A minha resposta enquanto convidado: 'owner' | 'pending' | 'accepted' | 'declined'.
    pub my_status: String,
}

#[derive(Deserialize)]
pub struct CreateMeetingReq {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    pub starts_at: DateTime<Utc>,
    #[serde(default = "default_duration")]
    pub duration_min: i32,
    #[serde(default)]
    pub invitee_ids: Vec<Uuid>,
    #[serde(default)]
    pub room_ref: Option<Uuid>,
}

fn default_kind() -> String {
    "video".into()
}
fn default_duration() -> i32 {
    30
}

// ---------- deteção de colisão ----------

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ParticipantConflict {
    pub user_id: Uuid,
    pub username: String,
    pub meeting_id: Uuid,
    pub meeting_title: String,
    pub starts_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct RoomConflict {
    pub meeting_id: Uuid,
    pub meeting_title: String,
    pub starts_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct Conflicts {
    pub participants: Vec<ParticipantConflict>,
    pub room: Vec<RoomConflict>,
}

/// Deteta sobreposições de agenda (para os utilizadores dados) e de sala
/// física, na janela [starts_at, starts_at+duration]. Ignora `exclude`.
/// Sobreposição: a.start < b.end AND b.start < a.end.
async fn detect_conflicts(
    state: &AppState,
    starts_at: DateTime<Utc>,
    duration_min: i32,
    user_ids: &[Uuid],
    room_ref: Option<Uuid>,
    exclude: Option<Uuid>,
) -> Result<Conflicts, ApiError> {
    let ends_at = starts_at + chrono::Duration::minutes(duration_min as i64);

    let participants: Vec<ParticipantConflict> = if user_ids.is_empty() {
        vec![]
    } else {
        sqlx::query_as(
            r#"
            SELECT DISTINCT part.user_id, u.username,
                   m.id AS meeting_id, m.title AS meeting_title, m.starts_at
            FROM meetings m
            JOIN LATERAL (
                SELECT m.owner_id AS user_id
                UNION
                SELECT i.user_id FROM meeting_invitees i
                WHERE i.meeting_id = m.id AND i.status <> 'declined'
            ) part ON part.user_id = ANY($3)
            JOIN users u ON u.id = part.user_id
            WHERE m.starts_at < $2
              AND (m.starts_at + make_interval(mins => m.duration_min)) > $1
              AND ($4::uuid IS NULL OR m.id <> $4)
            ORDER BY m.starts_at
            "#,
        )
        .bind(starts_at)
        .bind(ends_at)
        .bind(user_ids)
        .bind(exclude)
        .fetch_all(&state.db)
        .await?
    };

    let room: Vec<RoomConflict> = match room_ref {
        None => vec![],
        Some(r) => sqlx::query_as(
            r#"
            SELECT m.id AS meeting_id, m.title AS meeting_title, m.starts_at
            FROM meetings m
            WHERE m.room_ref = $1
              AND m.starts_at < $3
              AND (m.starts_at + make_interval(mins => m.duration_min)) > $2
              AND ($4::uuid IS NULL OR m.id <> $4)
            ORDER BY m.starts_at
            "#,
        )
        .bind(r)
        .bind(starts_at)
        .bind(ends_at)
        .bind(exclude)
        .fetch_all(&state.db)
        .await?,
    };

    Ok(Conflicts { participants, room })
}

/// Marca em quarentena quem não respondeu (ainda 'pending') a reuniões que
/// já começaram. Idempotente. Corre periodicamente (cron) e também antes de
/// leituras relevantes como backstop.
pub async fn quarantine_sweep(db: &sqlx::PgPool) -> Result<u64, ApiError> {
    let res = sqlx::query(
        "INSERT INTO meet_quarantine (user_id, meeting_id)
         SELECT i.user_id, i.meeting_id FROM meeting_invitees i
         JOIN meetings m ON m.id = i.meeting_id
         WHERE i.status = 'pending' AND m.starts_at < now()
         ON CONFLICT DO NOTHING",
    )
    .execute(db)
    .await?;
    Ok(res.rows_affected())
}

#[derive(Serialize)]
pub struct CreateMeetingResp {
    #[serde(flatten)]
    pub meeting: Meeting,
    pub conflicts: Conflicts,
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<CreateMeetingReq>,
) -> Result<Json<CreateMeetingResp>, ApiError> {
    let title = req.title.trim();
    if title.is_empty() || title.len() > 140 {
        return Err(ApiError::BadRequest("title must be 1-140 chars".into()));
    }
    if !matches!(req.kind.as_str(), "video" | "voice") {
        return Err(ApiError::BadRequest("kind must be 'video' or 'voice'".into()));
    }
    if !(5..=1440).contains(&req.duration_min) {
        return Err(ApiError::BadRequest("duration must be 5-1440 min".into()));
    }

    // Colisão de agenda (anfitrião + convidados) e de sala física, antes de criar.
    let mut all_users: Vec<Uuid> =
        req.invitee_ids.iter().copied().filter(|u| *u != auth.user_id).collect();
    all_users.push(auth.user_id);
    let conflicts =
        detect_conflicts(&state, req.starts_at, req.duration_min, &all_users, req.room_ref, None).await?;

    // Uma sala física não pode acolher duas reuniões em simultâneo — bloqueio.
    // (A colisão de agenda de participantes só avisa; eles aceitam/recusam.)
    if !conflicts.room.is_empty() {
        let other = &conflicts.room[0];
        return Err(ApiError::Conflict(format!(
            "a sala presencial já está reservada por «{}» nesse horário",
            other.meeting_title
        )));
    }

    let meeting: Meeting = sqlx::query_as(
        "INSERT INTO meetings (owner_id, title, description, kind, starts_at, duration_min, room_ref)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, owner_id, title, description, kind, starts_at, duration_min, room_code, created_at, room_ref, minutes, transcript",
    )
    .bind(auth.user_id)
    .bind(title)
    .bind(req.description.trim())
    .bind(&req.kind)
    .bind(req.starts_at)
    .bind(req.duration_min)
    .bind(req.room_ref)
    .fetch_one(&state.db)
    .await?;

    for uid in req.invitee_ids.iter().filter(|u| **u != auth.user_id) {
        sqlx::query(
            "INSERT INTO meeting_invitees (meeting_id, user_id) VALUES ($1, $2)
             ON CONFLICT DO NOTHING",
        )
        .bind(meeting.id)
        .bind(uid)
        .execute(&state.db)
        .await?;
    }

    Ok(Json(CreateMeetingResp { meeting, conflicts }))
}

/// Reuniões do utilizador: as que criou + aquelas para que foi convidado.
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Vec<MeetingItem>>, ApiError> {
    quarantine_sweep(&state.db).await?;
    let items: Vec<MeetingItem> = sqlx::query_as(
        r#"
        SELECT m.id, m.owner_id, u.username AS owner_name, m.title, m.description,
               m.kind, m.starts_at, m.duration_min, m.room_code,
               (m.owner_id = $1) AS is_owner, m.minutes,
               m.room_ref, mr.name AS room_name,
               CASE WHEN m.owner_id = $1 THEN 'owner' ELSE COALESCE(i.status, 'pending') END AS my_status
        FROM meetings m
        JOIN users u ON u.id = m.owner_id
        LEFT JOIN meeting_invitees i ON i.meeting_id = m.id AND i.user_id = $1
        LEFT JOIN meeting_rooms mr ON mr.id = m.room_ref
        WHERE m.owner_id = $1 OR i.user_id IS NOT NULL
        ORDER BY m.starts_at ASC
        "#,
    )
    .bind(auth.user_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(items))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let res = sqlx::query("DELETE FROM meetings WHERE id = $1 AND owner_id = $2")
        .bind(id)
        .bind(auth.user_id)
        .execute(&state.db)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct MinutesReq {
    #[serde(default)]
    pub minutes: String,
    #[serde(default)]
    pub transcript: String,
    /// Se dado, associa/cria pela sala em vez do id da reunião.
    #[serde(default)]
    pub room_code: Option<String>,
}

/// Guarda as MoM (notas AI) numa reunião. Dono ou convidado podem guardar.
pub async fn save_minutes(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<MinutesReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let allowed = {
        let row: Option<(i32,)> = sqlx::query_as(
            "SELECT 1 FROM meetings m
             LEFT JOIN meeting_invitees i ON i.meeting_id = m.id AND i.user_id = $2
             WHERE m.id = $1 AND (m.owner_id = $2 OR i.user_id IS NOT NULL)",
        )
        .bind(id)
        .bind(auth.user_id)
        .fetch_optional(&state.db)
        .await?;
        row.is_some()
    };
    if !allowed {
        return Err(ApiError::Unauthorized);
    }
    sqlx::query("UPDATE meetings SET minutes = $1, transcript = $2 WHERE id = $3")
        .bind(req.minutes.trim())
        .bind(req.transcript.trim())
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Guarda MoM associando pela sala: encontra a reunião cuja `room_code` bate
/// certo (reuniões iniciadas a partir do calendário). Usado quando se grava
/// a partir de dentro da chamada.
pub async fn save_minutes_by_room(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
    Json(req): Json<MinutesReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let meeting: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM meetings WHERE room_code = $1")
            .bind(&code)
            .fetch_optional(&state.db)
            .await?;
    let Some((mid,)) = meeting else {
        // Sala sem reunião agendada associada — nada a guardar, mas não é erro.
        return Ok(Json(serde_json::json!({ "ok": false, "reason": "no meeting for room" })));
    };
    save_minutes(State(state), auth, Path(mid), Json(req)).await
}

/// Arranca a reunião: cria a sala (se ainda não existe) e devolve o código.
/// Reuniões de voz criam na mesma uma sala — o cliente entra sem vídeo.
pub async fn start(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let meeting: Meeting = sqlx::query_as(
        "SELECT id, owner_id, title, description, kind, starts_at, duration_min, room_code, created_at, room_ref, minutes, transcript
         FROM meetings WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    // Só dono ou convidado pode arrancar/entrar.
    let allowed = meeting.owner_id == auth.user_id || {
        let row: Option<(i32,)> = sqlx::query_as(
            "SELECT 1 FROM meeting_invitees WHERE meeting_id = $1 AND user_id = $2",
        )
        .bind(id)
        .bind(auth.user_id)
        .fetch_optional(&state.db)
        .await?;
        row.is_some()
    };
    if !allowed {
        return Err(ApiError::Unauthorized);
    }

    // Se já foi arrancada, reutiliza a sala.
    if let Some(code) = meeting.room_code.clone() {
        if sqlx::query_as::<_, (Uuid,)>("SELECT id FROM rooms WHERE code = $1")
            .bind(&code)
            .fetch_optional(&state.db)
            .await?
            .is_some()
        {
            return Ok(Json(serde_json::json!({ "code": code, "kind": meeting.kind })));
        }
    }

    // O dono cria a sala; um convidado que chegue antes recebe erro amigável.
    if meeting.owner_id != auth.user_id {
        return Err(ApiError::BadRequest(
            "a reunião ainda não foi iniciada pelo anfitrião".into(),
        ));
    }
    let room = crate::rooms::insert_room(&state.db, auth.user_id, &meeting.title, "sfu", false, false).await?;
    sqlx::query("UPDATE meetings SET room_code = $1 WHERE id = $2")
        .bind(&room.code)
        .bind(id)
        .execute(&state.db)
        .await?;

    Ok(Json(serde_json::json!({ "code": room.code, "kind": meeting.kind })))
}

// ---------- pré-verificação de conflitos (antes de agendar) ----------

#[derive(Deserialize)]
pub struct ConflictCheckReq {
    pub starts_at: DateTime<Utc>,
    #[serde(default = "default_duration")]
    pub duration_min: i32,
    #[serde(default)]
    pub invitee_ids: Vec<Uuid>,
    #[serde(default)]
    pub room_ref: Option<Uuid>,
}

pub async fn check_conflicts(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<ConflictCheckReq>,
) -> Result<Json<Conflicts>, ApiError> {
    let mut users: Vec<Uuid> = req.invitee_ids.iter().copied().filter(|u| *u != auth.user_id).collect();
    users.push(auth.user_id);
    let c = detect_conflicts(&state, req.starts_at, req.duration_min, &users, req.room_ref, None).await?;
    Ok(Json(c))
}

// ---------- respostas dos convidados ----------

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct InviteeResponse {
    pub user_id: Uuid,
    pub username: String,
    pub status: String,
    pub decline_reason: String,
    pub responded_at: Option<DateTime<Utc>>,
}

/// Lista as respostas dos convidados (só o anfitrião vê tudo).
pub async fn invitees(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<InviteeResponse>>, ApiError> {
    let owner: Option<(Uuid,)> = sqlx::query_as("SELECT owner_id FROM meetings WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    match owner {
        Some((o,)) if o == auth.user_id => {}
        Some(_) => return Err(ApiError::Unauthorized),
        None => return Err(ApiError::NotFound),
    }
    let rows: Vec<InviteeResponse> = sqlx::query_as(
        "SELECT i.user_id, u.username, i.status, i.decline_reason, i.responded_at
         FROM meeting_invitees i JOIN users u ON u.id = i.user_id
         WHERE i.meeting_id = $1 ORDER BY u.username",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct RespondReq {
    pub status: String, // 'accepted' | 'declined'
    #[serde(default)]
    pub reason: String,
}

/// O convidado aceita ou recusa (recusar exige motivo). Ao recusar, o
/// anfitrião é notificado em tempo real (se online) com o motivo.
pub async fn respond(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<RespondReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !matches!(req.status.as_str(), "accepted" | "declined") {
        return Err(ApiError::BadRequest("status inválido".into()));
    }
    let reason = req.reason.trim();
    if req.status == "declined" && reason.is_empty() {
        return Err(ApiError::BadRequest("é obrigatório indicar o motivo da recusa".into()));
    }

    let res = sqlx::query(
        "UPDATE meeting_invitees SET status = $1, decline_reason = $2, responded_at = now()
         WHERE meeting_id = $3 AND user_id = $4",
    )
    .bind(&req.status)
    .bind(reason)
    .bind(id)
    .bind(auth.user_id)
    .execute(&state.db)
    .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound); // não é convidado desta reunião
    }

    // Sair da quarentena desta reunião — respondeu.
    sqlx::query("DELETE FROM meet_quarantine WHERE meeting_id = $1 AND user_id = $2")
        .bind(id)
        .bind(auth.user_id)
        .execute(&state.db)
        .await?;

    // Notificar o anfitrião da recusa (tempo real, se online).
    if req.status == "declined" {
        if let Some((owner, title)) =
            sqlx::query_as::<_, (Uuid, String)>("SELECT owner_id, title FROM meetings WHERE id = $1")
                .bind(id)
                .fetch_optional(&state.db)
                .await?
        {
            let me = crate::users::fetch_public(&state.db, auth.user_id).await?;
            state.presence.notify(
                owner,
                crate::presence::CallServerMsg::MeetingDeclined {
                    meeting_id: id,
                    meeting_title: title,
                    by_id: auth.user_id,
                    by_name: me.username,
                    reason: reason.to_string(),
                },
            );
        }
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------- analytics de quarentena ----------

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct QuarantineRow {
    pub user_id: Uuid,
    pub username: String,
    pub count: i64,
}

#[derive(Deserialize)]
pub struct AnalyticsQuery {
    #[serde(default = "default_period")]
    pub period: String, // week|month|quarter|year
    /// Se dado, restringe a membros dessa organização (o pedinte tem de ser membro).
    #[serde(default)]
    pub org_id: Option<Uuid>,
}
fn default_period() -> String {
    "month".into()
}

/// Ranking de quem mais fica em quarentena, no período pedido. Opcionalmente
/// filtrado a uma organização (só membros dessa org).
pub async fn quarantine_analytics(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    axum::extract::Query(q): axum::extract::Query<AnalyticsQuery>,
) -> Result<Json<Vec<QuarantineRow>>, ApiError> {
    quarantine_sweep(&state.db).await?;
    // Se filtrar por org, o pedinte tem de pertencer a ela.
    if let Some(org_id) = q.org_id {
        if crate::org::role_in_org(&state, org_id, auth.user_id).await?.is_none() {
            return Err(ApiError::Unauthorized);
        }
    }
    let days: i64 = match q.period.as_str() {
        "week" => 7,
        "month" => 30,
        "quarter" => 90,
        "year" => 365,
        _ => 30,
    };
    let rows: Vec<QuarantineRow> = sqlx::query_as(
        "SELECT u.id AS user_id, u.username, COUNT(*) AS count
         FROM meet_quarantine mq
         JOIN users u ON u.id = mq.user_id
         JOIN meetings m ON m.id = mq.meeting_id
         WHERE m.starts_at >= now() - make_interval(days => $1::int)
           AND ($2::uuid IS NULL OR EXISTS (
                 SELECT 1 FROM org_members om WHERE om.org_id = $2 AND om.user_id = u.id))
         GROUP BY u.id, u.username
         ORDER BY count DESC, u.username
         LIMIT 100",
    )
    .bind(days as i32)
    .bind(q.org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}
