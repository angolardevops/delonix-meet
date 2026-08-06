//! Calendário: reuniões agendadas com convidados. A partir de uma reunião
//! é possível arrancar a chamada (cria a sala e devolve o código), seja
//! vídeo ou só voz.

use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, Datelike, Duration as ChronoDuration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
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
    pub recurrence_freq: Option<String>,
    pub recurrence_interval: i16,
    pub recurrence_until: Option<NaiveDate>,
    pub recurrence_count: Option<i16>,
    pub recurrence_byday: Option<String>,
    pub recurrence_parent_id: Option<Uuid>,
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
    pub recurrence_freq: Option<String>,
    pub recurrence_interval: i16,
    pub recurrence_parent_id: Option<Uuid>,
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
    // Recorrência
    pub recurrence_freq: Option<String>,
    #[serde(default = "default_rrule_interval")]
    pub recurrence_interval: i16,
    pub recurrence_until: Option<NaiveDate>,
    pub recurrence_count: Option<i16>,
    /// Dias da semana para freq=weekly: "MON,WED,FRI"
    pub recurrence_byday: Option<String>,
}

fn default_kind() -> String {
    "video".into()
}
fn default_duration() -> i32 {
    30
}
fn default_rrule_interval() -> i16 {
    1
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
        Some(r) => {
            sqlx::query_as(
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
            .await?
        }
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
        return Err(ApiError::BadRequest(
            "kind must be 'video' or 'voice'".into(),
        ));
    }
    if !(5..=1440).contains(&req.duration_min) {
        return Err(ApiError::BadRequest("duration must be 5-1440 min".into()));
    }

    // Quota de reuniões da organização (agenda): conta as reuniões cujo dono é
    // membro da org do criador. NULL => ilimitado.
    if let Some(org_id) = crate::org::orgs_of_user(&state, auth.user_id)
        .await
        .first()
        .copied()
    {
        let limit: Option<i32> =
            sqlx::query_scalar("SELECT max_meetings FROM organizations WHERE id = $1")
                .bind(org_id)
                .fetch_one(&state.db)
                .await?;
        if let Some(max) = limit {
            let count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM meetings WHERE owner_id IN
                     (SELECT user_id FROM org_members WHERE org_id = $1)",
            )
            .bind(org_id)
            .fetch_one(&state.db)
            .await?;
            if count >= max as i64 {
                return Err(ApiError::Conflict(format!(
                    "limite de reuniões da organização atingido ({max})"
                )));
            }
        }
    }

    // Colisão de agenda (anfitrião + convidados) e de sala física, antes de criar.
    let mut all_users: Vec<Uuid> = req
        .invitee_ids
        .iter()
        .copied()
        .filter(|u| *u != auth.user_id)
        .collect();
    all_users.push(auth.user_id);
    let conflicts = detect_conflicts(
        &state,
        req.starts_at,
        req.duration_min,
        &all_users,
        req.room_ref,
        None,
    )
    .await?;

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
        "INSERT INTO meetings (owner_id, title, description, kind, starts_at, duration_min, room_ref,
                               recurrence_freq, recurrence_interval, recurrence_until, recurrence_count, recurrence_byday)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
         RETURNING id, owner_id, title, description, kind, starts_at, duration_min, room_code, created_at, room_ref,
                   minutes, transcript, recurrence_freq, recurrence_interval, recurrence_until, recurrence_count,
                   recurrence_byday, recurrence_parent_id",
    )
    .bind(auth.user_id)
    .bind(title)
    .bind(req.description.trim().chars().take(4000).collect::<String>())
    .bind(&req.kind)
    .bind(req.starts_at)
    .bind(req.duration_min)
    .bind(req.room_ref)
    .bind(&req.recurrence_freq)
    .bind(req.recurrence_interval)
    .bind(req.recurrence_until)
    .bind(req.recurrence_count)
    .bind(&req.recurrence_byday)
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

    // Gera instâncias filhas para reuniões recorrentes (até 6 meses).
    if meeting.recurrence_freq.is_some() {
        let invitees: Vec<Uuid> = req
            .invitee_ids
            .iter()
            .filter(|u| **u != auth.user_id)
            .copied()
            .collect();
        generate_instances(&state.db, &meeting, &invitees).await;
    }

    fire_meeting_webhook(&state, &meeting, auth.user_id, "meeting.created").await;

    Ok(Json(CreateMeetingResp { meeting, conflicts }))
}

/// Dispara um evento de reunião para os webhooks das organizações do dono.
/// O link usa o domínio de produção da org, se configurado (settings).
pub(crate) async fn fire_meeting_webhook(
    state: &Arc<AppState>,
    meeting: &Meeting,
    owner: Uuid,
    event: &'static str,
) {
    let orgs = crate::org::orgs_of_user(state, owner).await;
    if orgs.is_empty() {
        return;
    }
    let domain = crate::org::primary_domain(state, owner).await;
    let base = if domain.is_empty() {
        "".to_string()
    } else {
        format!("https://{domain}")
    };
    let link = meeting
        .room_code
        .as_ref()
        .map(|c| format!("{base}/#/r/{c}"))
        .unwrap_or_default();
    let when = meeting.starts_at.format("%Y-%m-%d %H:%M UTC");
    let verb = if event == "meeting.started" {
        "começou"
    } else {
        "agendada"
    };
    let text = format!(
        "Reunião «{}» {} para {}{}",
        meeting.title,
        verb,
        when,
        if link.is_empty() {
            String::new()
        } else {
            format!(" · {link}")
        }
    );
    let payload = serde_json::json!({
        "meeting_id": meeting.id,
        "title": meeting.title,
        "kind": meeting.kind,
        "starts_at": meeting.starts_at,
        "duration_min": meeting.duration_min,
        "room_code": meeting.room_code,
        "link": link,
    });
    for org_id in orgs {
        crate::webhooks::fire(
            state.clone(),
            org_id,
            crate::webhooks::Event {
                name: event,
                title: "Delonix Meet".into(),
                text: text.clone(),
                payload: payload.clone(),
            },
        );
    }
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
               CASE WHEN m.owner_id = $1 THEN 'owner' ELSE COALESCE(i.status, 'pending') END AS my_status,
               m.recurrence_freq, m.recurrence_interval, m.recurrence_parent_id
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
        .bind(req.minutes.trim().chars().take(200_000).collect::<String>())
        .bind(
            req.transcript
                .trim()
                .chars()
                .take(200_000)
                .collect::<String>(),
        )
        .bind(id)
        .execute(&state.db)
        .await?;
    // A transcrição (ata bruta) ficou persistida acima; em background o LLM
    // local gera o resumo elegante e substitui `minutes` (ai.rs — no-op sem
    // OLLAMA_URL; se falhar, fica a ata por regras enviada pelo cliente).
    crate::ai::spawn_mom_summary(state.clone(), id);
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
    let meeting: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM meetings WHERE room_code = $1")
        .bind(&code)
        .fetch_optional(&state.db)
        .await?;
    let Some((mid,)) = meeting else {
        // Sala sem reunião agendada associada — nada a guardar, mas não é erro.
        return Ok(Json(serde_json::json!({ "ok": false, "reason": "no meeting for room" })));
    };
    save_minutes(State(state), auth, Path(mid), Json(req)).await
}

#[derive(Debug, Serialize)]
pub struct RoomNotes {
    pub title: String,
    pub minutes: String,
    pub transcript: String,
}

/// Ata e transcrição da reunião associada a uma sala — para o leitor da
/// biblioteca de gravações. Só participantes da sala têm acesso.
pub async fn notes_by_room(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
) -> Result<Json<RoomNotes>, ApiError> {
    let participated: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM room_participants rp JOIN rooms r ON r.id = rp.room_id
         WHERE r.code = $1 AND rp.user_id = $2",
    )
    .bind(&code)
    .bind(auth.user_id)
    .fetch_optional(&state.db)
    .await?;
    if participated.is_none() {
        return Err(ApiError::Unauthorized);
    }
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT title, minutes, transcript FROM meetings WHERE room_code = $1
         ORDER BY starts_at DESC LIMIT 1",
    )
    .bind(&code)
    .fetch_optional(&state.db)
    .await?;
    let (title, minutes, transcript) = row.unwrap_or_default();
    Ok(Json(RoomNotes {
        title,
        minutes,
        transcript,
    }))
}

/// Arranca a reunião: cria a sala (se ainda não existe) e devolve o código.
/// Reuniões de voz criam na mesma uma sala — o cliente entra sem vídeo.
pub async fn start(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let meeting: Meeting = sqlx::query_as(
        // A lista TEM de cobrir todos os campos de `Meeting` — o `FromRow`
        // derivado faz `try_get` de cada um e uma coluna em falta é um erro em
        // runtime (não em compilação). Foi assim que a recorrência (0022)
        // partiu `start` e `ics` em silêncio.
        "SELECT id, owner_id, title, description, kind, starts_at, duration_min, room_code,
                created_at, room_ref, minutes, transcript,
                recurrence_freq, recurrence_interval, recurrence_until, recurrence_count,
                recurrence_byday, recurrence_parent_id
         FROM meetings WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    // Só dono ou convidado pode arrancar/entrar.
    let allowed = meeting.owner_id == auth.user_id || {
        let row: Option<(i32,)> =
            sqlx::query_as("SELECT 1 FROM meeting_invitees WHERE meeting_id = $1 AND user_id = $2")
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
            return Ok(Json(
                serde_json::json!({ "code": code, "kind": meeting.kind }),
            ));
        }
    }

    // O dono cria a sala; um convidado que chegue antes recebe erro amigável.
    if meeting.owner_id != auth.user_id {
        return Err(ApiError::BadRequest(
            "a reunião ainda não foi iniciada pelo anfitrião".into(),
        ));
    }
    let room = crate::rooms::insert_room(
        &state.db,
        auth.user_id,
        &meeting.title,
        "sfu",
        false,
        false,
        "normal",
    )
    .await?;
    sqlx::query("UPDATE meetings SET room_code = $1 WHERE id = $2")
        .bind(&room.code)
        .bind(id)
        .execute(&state.db)
        .await?;

    // Webhook meeting.started (com o room_code já preenchido).
    let mut started_meeting = meeting.clone();
    started_meeting.room_code = Some(room.code.clone());
    fire_meeting_webhook(&state, &started_meeting, auth.user_id, "meeting.started").await;

    // Estilo Teams: a reunião começou → "desperta" os convidados. Quem está
    // online recebe a chamada a tocar (aceitar entra na sala); quem não está
    // fica com chamada perdida. Quem recusou o convite não é incomodado.
    let invitees: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT user_id FROM meeting_invitees
         WHERE meeting_id = $1 AND status <> 'declined'",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    if !invitees.is_empty() {
        let caller_name: (String,) = sqlx::query_as("SELECT username FROM users WHERE id = $1")
            .bind(auth.user_id)
            .fetch_one(&state.db)
            .await?;
        let targets: std::collections::HashSet<Uuid> = invitees.into_iter().map(|r| r.0).collect();
        // Registar a chamada para que os convidados entrem diretamente (sem sala de espera).
        state
            .presence
            .register_call(room.code.clone(), auth.user_id, targets.clone());
        let (ringing, offline) = crate::presence::ring_users(
            &state,
            auth.user_id,
            &caller_name.0,
            targets,
            &room.code,
            &meeting.kind,
            &meeting.title,
        )
        .await;
        tracing::info!(meeting = %id, ringing = ringing.len(), offline = offline.len(), "meeting start ring");
    }

    Ok(Json(
        serde_json::json!({ "code": room.code, "kind": meeting.kind }),
    ))
}

/// Exportação iCalendar (roadmap "Google e Outlook Calendar"): um .ics por
/// reunião — importa/abre no Google Calendar, Outlook, Apple Calendar, etc.
pub async fn ics(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<axum::response::Response, ApiError> {
    let meeting: Meeting = sqlx::query_as(
        // A lista TEM de cobrir todos os campos de `Meeting` — o `FromRow`
        // derivado faz `try_get` de cada um e uma coluna em falta é um erro em
        // runtime (não em compilação). Foi assim que a recorrência (0022)
        // partiu `start` e `ics` em silêncio.
        "SELECT id, owner_id, title, description, kind, starts_at, duration_min, room_code,
                created_at, room_ref, minutes, transcript,
                recurrence_freq, recurrence_interval, recurrence_until, recurrence_count,
                recurrence_byday, recurrence_parent_id
         FROM meetings WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    let allowed = meeting.owner_id == auth.user_id
        || sqlx::query_as::<_, (i32,)>(
            "SELECT 1 FROM meeting_invitees WHERE meeting_id = $1 AND user_id = $2",
        )
        .bind(id)
        .bind(auth.user_id)
        .fetch_optional(&state.db)
        .await?
        .is_some();
    if !allowed {
        return Err(ApiError::Unauthorized);
    }

    let esc = |s: &str| {
        s.replace('\\', "\\\\")
            .replace(';', "\\;")
            .replace(',', "\\,")
            .replace('\n', "\\n")
    };
    let dt = |t: &chrono::DateTime<Utc>| t.format("%Y%m%dT%H%M%SZ").to_string();
    let mut desc = meeting.description.clone();
    if let Some(code) = &meeting.room_code {
        if !desc.is_empty() {
            desc.push('\n');
        }
        desc.push_str(&format!("Delonix Meet — código da sala: {code}"));
    }
    let body = format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Delonix Meet//PT\r\nMETHOD:PUBLISH\r\nBEGIN:VEVENT\r\nUID:{id}@delonix-meet\r\nDTSTAMP:{now}\r\nDTSTART:{start}\r\nDURATION:PT{dur}M\r\nSUMMARY:{title}\r\nDESCRIPTION:{desc}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        id = meeting.id,
        now = dt(&Utc::now()),
        start = dt(&meeting.starts_at),
        dur = meeting.duration_min,
        title = esc(&meeting.title),
        desc = esc(&desc),
    );
    Ok(axum::response::Response::builder()
        .header("Content-Type", "text/calendar; charset=utf-8")
        .header(
            "Content-Disposition",
            "attachment; filename=\"reuniao.ics\"",
        )
        .body(axum::body::Body::from(body))
        .unwrap())
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
    let mut users: Vec<Uuid> = req
        .invitee_ids
        .iter()
        .copied()
        .filter(|u| *u != auth.user_id)
        .collect();
    users.push(auth.user_id);
    let c = detect_conflicts(
        &state,
        req.starts_at,
        req.duration_min,
        &users,
        req.room_ref,
        None,
    )
    .await?;
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
        return Err(ApiError::BadRequest(
            "é obrigatório indicar o motivo da recusa".into(),
        ));
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
        if let Some((owner, title)) = sqlx::query_as::<_, (Uuid, String)>(
            "SELECT owner_id, title FROM meetings WHERE id = $1",
        )
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
    // Admin-only e escopado às orgs que o pedinte administra (nunca global
    // cross-tenant): "todas" = todas as MINHAS orgs de admin.
    let admin_orgs: Vec<Uuid> = match q.org_id {
        Some(org_id) => {
            crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
            vec![org_id]
        }
        None => crate::org::admin_orgs_of_user(&state, auth.user_id).await,
    };
    if admin_orgs.is_empty() {
        return Ok(Json(vec![]));
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
           AND EXISTS (
                 SELECT 1 FROM org_members om
                 WHERE om.org_id = ANY($2) AND om.user_id = u.id)
         GROUP BY u.id, u.username
         ORDER BY count DESC, u.username
         LIMIT 100",
    )
    .bind(days as i32)
    .bind(&admin_orgs)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

// ─── Recorrência ─────────────────────────────────────────────────────────────

/// Gera instâncias filhas para uma reunião recorrente.
/// Produz ocorrências de agora até 6 meses no futuro.
pub async fn generate_instances(db: &sqlx::PgPool, parent: &Meeting, invitee_ids: &[Uuid]) {
    let freq = match parent.recurrence_freq.as_deref() {
        Some(f) => f,
        None => return,
    };
    let horizon = Utc::now() + ChronoDuration::days(183);
    let byday: Vec<&str> = parent
        .recurrence_byday
        .as_deref()
        .unwrap_or("MON,TUE,WED,THU,FRI")
        .split(',')
        .collect();
    let interval = parent.recurrence_interval.max(1) as i64;

    let mut occurs: Vec<DateTime<Utc>> = Vec::new();
    let mut cursor = parent.starts_at;

    // Avança cursor para a próxima ocorrência após o pai.
    loop {
        cursor = next_occurrence(cursor, freq, interval, &byday);
        if parent
            .recurrence_until
            .map_or(false, |u| cursor.date_naive() > u)
        {
            break;
        }
        if cursor > horizon {
            break;
        }
        if let Some(max) = parent.recurrence_count {
            if occurs.len() as i16 >= max - 1 {
                // -1 porque o pai conta como a 1ª ocorrência
                occurs.push(cursor);
                break;
            }
        }
        occurs.push(cursor);
    }

    for ts in occurs {
        let child_id: Option<(Uuid,)> = sqlx::query_as(
            "INSERT INTO meetings (owner_id, title, description, kind, starts_at, duration_min, room_ref,
                                   recurrence_freq, recurrence_interval, recurrence_until, recurrence_count,
                                   recurrence_byday, recurrence_parent_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
             ON CONFLICT DO NOTHING
             RETURNING id",
        )
        .bind(parent.owner_id)
        .bind(&parent.title)
        .bind(&parent.description)
        .bind(&parent.kind)
        .bind(ts)
        .bind(parent.duration_min)
        .bind(parent.room_ref)
        .bind(&parent.recurrence_freq)
        .bind(parent.recurrence_interval)
        .bind(parent.recurrence_until)
        .bind(parent.recurrence_count)
        .bind(&parent.recurrence_byday)
        .bind(parent.id)
        .fetch_optional(db)
        .await
        .ok()
        .flatten();

        if let Some((child_id,)) = child_id {
            for uid in invitee_ids {
                let _ = sqlx::query(
                    "INSERT INTO meeting_invitees (meeting_id, user_id) VALUES ($1, $2)
                     ON CONFLICT DO NOTHING",
                )
                .bind(child_id)
                .bind(uid)
                .execute(db)
                .await;
            }
        }
    }
}

fn next_occurrence(
    from: DateTime<Utc>,
    freq: &str,
    interval: i64,
    byday: &[&str],
) -> DateTime<Utc> {
    match freq {
        "daily" => from + ChronoDuration::days(interval),
        "monthly" => {
            let y = from.year();
            let m = from.month() as i64 + interval;
            let (ny, nm) = ((y as i64 + (m - 1) / 12) as i32, ((m - 1) % 12 + 1) as u32);
            let day = from.day().min(days_in_month(ny, nm));
            from.with_year(ny)
                .and_then(|d| d.with_month(nm))
                .and_then(|d| d.with_day(day))
                .unwrap_or(from)
        }
        "yearly" => from
            .with_year(from.year() + interval as i32)
            .unwrap_or(from),
        "weekly" => {
            // Avança dia a dia até encontrar o próximo dia permitido
            let day_names = ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"];
            let mut next = from + ChronoDuration::days(1);
            // Limite de 7*interval dias para não entrar em loop infinito
            let max = 7 * interval + 1;
            let mut tried = 0i64;
            while tried < max {
                let wd = next.weekday().number_from_monday() as usize - 1;
                if byday.contains(&day_names[wd]) {
                    return next;
                }
                next = next + ChronoDuration::days(1);
                tried += 1;
                // Quando completamos N semanas, só voltamos se passámos N semanas completas
            }
            next
        }
        _ => from + ChronoDuration::days(1),
    }
}

/// Cron diário: para cada reunião recorrente (pai) ainda ativa, garante que
/// existem instâncias filhas para os próximos 6 meses.
pub async fn extend_recurrence_horizon(db: &sqlx::PgPool) {
    // Pais com recorrência ativa (sem data de fim ou com data futura)
    let parents: Vec<Meeting> = sqlx::query_as(
        "SELECT id, owner_id, title, description, kind, starts_at, duration_min, room_code,
                created_at, room_ref, minutes, transcript,
                recurrence_freq, recurrence_interval, recurrence_until, recurrence_count,
                recurrence_byday, recurrence_parent_id
         FROM meetings
         WHERE recurrence_freq IS NOT NULL
           AND recurrence_parent_id IS NULL
           AND (recurrence_until IS NULL OR recurrence_until > now()::date)",
    )
    .fetch_all(db)
    .await
    .unwrap_or_default();

    for parent in parents {
        // Conta instâncias filhas já existentes
        let existing: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM meetings WHERE recurrence_parent_id = $1")
                .bind(parent.id)
                .fetch_one(db)
                .await
                .unwrap_or((0,));

        // Invitees do pai
        let invitees: Vec<Uuid> =
            sqlx::query_scalar("SELECT user_id FROM meeting_invitees WHERE meeting_id = $1")
                .bind(parent.id)
                .fetch_all(db)
                .await
                .unwrap_or_default();

        // Se count limitado e já gerámos tudo, skip
        if let Some(max) = parent.recurrence_count {
            if existing.0 >= (max - 1) as i64 {
                continue;
            }
        }

        generate_instances(db, &parent, &invitees).await;
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let next_month = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    };
    next_month
        .and_then(|d| d.pred_opt())
        .map(|d| d.day())
        .unwrap_or(30)
}

// ─── Auto-ring de reuniões agendadas ─────────────────────────────────────────

/// Cron: corre a cada minuto. Encontra reuniões que começam no próximo minuto
/// e envia chamada via Presence aos convidados que ainda não estão na sala.
pub async fn ring_upcoming_meetings(state: &Arc<AppState>) {
    let rows: Vec<(Uuid, String, String, String, Option<String>, Uuid)> = sqlx::query_as(
        "SELECT m.id, m.title, m.kind, u.username AS owner_name, m.room_code, m.owner_id
         FROM meetings m
         JOIN users u ON u.id = m.owner_id
         WHERE m.starts_at >= now()
           AND m.starts_at < now() + interval '1 minute'
           AND m.room_code IS NOT NULL",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    for (meeting_id, title, kind, owner_name, room_code, owner_id) in rows {
        let room_code = match room_code {
            Some(c) => c,
            None => continue,
        };

        // Resolve o room_id para verificar quem já está na sala.
        let room_id_row: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM rooms WHERE code = $1")
            .bind(&room_code)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();

        let already_in: std::collections::HashSet<Uuid> = room_id_row
            .map(|(rid,)| state.hub.users_in_room(rid))
            .unwrap_or_default();

        let invitees: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT user_id FROM meeting_invitees
             WHERE meeting_id = $1 AND status <> 'declined'",
        )
        .bind(meeting_id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

        let targets: std::collections::HashSet<Uuid> = invitees
            .into_iter()
            .map(|(uid,)| uid)
            .filter(|uid| !already_in.contains(uid) && *uid != owner_id)
            .collect();

        if targets.is_empty() {
            continue;
        }

        // Registar a chamada para que os convidados entrem diretamente (sem sala de espera).
        state
            .presence
            .register_call(room_code.clone(), owner_id, targets.clone());
        let (ringing, offline) = crate::presence::ring_users(
            state,
            owner_id,
            &owner_name,
            targets,
            &room_code,
            &kind,
            &title,
        )
        .await;
        tracing::info!(
            meeting = %meeting_id,
            ringing = ringing.len(),
            offline = offline.len(),
            "auto-ring de reunião agendada"
        );
    }
}
