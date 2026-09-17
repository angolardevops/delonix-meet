//! Calendário: reuniões agendadas com convidados. A partir de uma reunião
//! é possível arrancar a chamada (cria a sala e devolve o código), seja
//! vídeo ou só voz.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Datelike, Duration as ChronoDuration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
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
    /// `daily` | `weekly` | `monthly` | `yearly`, ou `null` se não recorrente.
    pub recurrence_freq: Option<String>,
    pub recurrence_interval: i16,
    pub recurrence_until: Option<NaiveDate>,
    pub recurrence_count: Option<i16>,
    pub recurrence_byday: Option<String>,
    pub recurrence_parent_id: Option<Uuid>,
}

/// Documentação OpenAPI das rotas deste módulo (`openapi.rs` junta-as).
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        list,
        get_one,
        create,
        check_conflicts,
        delete,
        start,
        ics,
        save_minutes,
        invitees,
        respond,
        quarantine_analytics,
        save_minutes_by_room,
        notes_by_room
    ),
    components(schemas(
        Meeting,
        MeetingItem,
        CreateMeetingReq,
        CreateMeetingResp,
        ParticipantConflict,
        RoomConflict,
        Conflicts,
        MinutesReq,
        RoomNotes,
        StartResp,
        ConflictCheckReq,
        InviteeResponse,
        RespondReq,
        QuarantineRow
    ))
)]
pub struct ApiDoc;

/// Lista de colunas que cobre **todos** os campos de `Meeting` — usar sempre
/// que se hidrata `Meeting` (`SELECT`, `INSERT ... RETURNING`,
/// `UPDATE ... RETURNING`). O `FromRow` derivado faz `try_get` por campo: uma
/// coluna em falta é um erro de RUNTIME, não de compilação — foi assim que a
/// migração 0022 (recorrência) partiu `start` e `ics` em silêncio, com esta
/// mesma lista repetida à mão em três sítios diferentes dentro deste ficheiro
/// e mais um em `meetings_v1.rs`. Um só lugar, uma só vez (ADR-0004, Fase 3).
pub const MEETING_COLUMNS: &str =
    "id, owner_id, title, description, kind, starts_at, duration_min, \
     room_code, created_at, room_ref, minutes, transcript, recurrence_freq, \
     recurrence_interval, recurrence_until, recurrence_count, recurrence_byday, \
     recurrence_parent_id";

/// Reunião enriquecida para a UI do calendário.
#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
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

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateMeetingReq {
    /// 1-140 caracteres (depois de `trim`).
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// `video` (omissão) | `voice`.
    #[serde(default = "default_kind")]
    #[schema(default = "video")]
    pub kind: String,
    pub starts_at: DateTime<Utc>,
    /// 5-1440 minutos.
    #[serde(default = "default_duration")]
    #[schema(default = 30)]
    pub duration_min: i32,
    #[serde(default)]
    pub invitee_ids: Vec<Uuid>,
    #[serde(default)]
    pub room_ref: Option<Uuid>,
    // Recorrência
    pub recurrence_freq: Option<String>,
    #[serde(default = "default_rrule_interval")]
    #[schema(default = 1)]
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

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct ParticipantConflict {
    pub user_id: Uuid,
    pub username: String,
    pub meeting_id: Uuid,
    pub meeting_title: String,
    pub starts_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct RoomConflict {
    pub meeting_id: Uuid,
    pub meeting_title: String,
    pub starts_at: DateTime<Utc>,
}

/// Sobreposições encontradas. As de participantes só avisam; as de sala
/// física bloqueiam a criação.
#[derive(Debug, Serialize, utoipa::ToSchema)]
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
/// já começaram, em toda a base. Idempotente. Corre só na tarefa de fundo
/// (`run_quarantine_sweeper`): NUNCA num handler, porque o custo cresce com
/// todos os convidados que nunca responderam (medido a 2026-09-17: 1,2-1,8 s
/// com 287 000 convidados, e corria em cada `GET /api/meetings`).
///
/// O `NOT EXISTS` filtra antes de inserir o que já está em quarentena: o
/// `ON CONFLICT` sozinho pagava uma inserção especulativa por linha repetida
/// (48 000 por passagem). Fica na mesma para a corrida entre réplicas.
///
/// Não é incremental por `starts_at` de propósito: uma reunião criada com
/// início no passado, ou remarcada para trás pela v1, nunca entraria numa
/// janela «desde a última passagem».
pub async fn quarantine_sweep(db: &sqlx::PgPool) -> Result<u64, ApiError> {
    let res = sqlx::query(
        "INSERT INTO meet_quarantine (user_id, meeting_id)
         SELECT i.user_id, i.meeting_id FROM meeting_invitees i
         JOIN meetings m ON m.id = i.meeting_id
         WHERE i.status = 'pending' AND m.starts_at < now()
           AND NOT EXISTS (
                 SELECT 1 FROM meet_quarantine q
                 WHERE q.user_id = i.user_id AND q.meeting_id = i.meeting_id)
         ON CONFLICT DO NOTHING",
    )
    .execute(db)
    .await?;
    Ok(res.rows_affected())
}

/// A mesma marcação, limitada ao que `quarantine_analytics` lê: membros da
/// organização e reuniões começadas nos últimos `days` dias. Deixa a
/// analítica exacta sem esperar pela tarefa de fundo, a um custo que depende
/// da organização e não da base inteira (5-26 ms nas orgs maiores da base
/// semeada).
async fn quarantine_sweep_org(db: &sqlx::PgPool, org_id: Uuid, days: i32) -> Result<u64, ApiError> {
    let in_org = crate::org::quarantine_subject_in_org_sql("$1", "i.user_id");
    let res = sqlx::query(&format!(
        "INSERT INTO meet_quarantine (user_id, meeting_id)
         SELECT i.user_id, i.meeting_id FROM meeting_invitees i
         JOIN meetings m ON m.id = i.meeting_id
         WHERE i.status = 'pending'
           AND m.starts_at >= now() - make_interval(days => $2)
           AND m.starts_at < now()
           AND {in_org}
           AND NOT EXISTS (
                 SELECT 1 FROM meet_quarantine q
                 WHERE q.user_id = i.user_id AND q.meeting_id = i.meeting_id)
         ON CONFLICT DO NOTHING"
    ))
    .bind(org_id)
    .bind(days)
    .execute(db)
    .await?;
    Ok(res.rows_affected())
}

/// Tarefa de fundo da quarentena: uma passagem de `quarantine_sweep` a cada
/// `every`, a primeira logo ao arrancar. Pára quando `stop` é cancelado —
/// entre passagens; uma passagem em curso acaba antes de sair.
pub async fn run_quarantine_sweeper(
    db: sqlx::PgPool,
    every: std::time::Duration,
    stop: tokio_util::sync::CancellationToken,
) {
    let mut ticker = tokio::time::interval(every);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = stop.cancelled() => break,
            _ = ticker.tick() => {}
        }
        match quarantine_sweep(&db).await {
            Ok(n) if n > 0 => tracing::info!(added = n, "quarantine sweep"),
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "quarantine sweep failed"),
        }
    }
}

/// A reunião criada (campos de `Meeting` ao nível de topo) mais os avisos de
/// colisão de agenda.
#[derive(Serialize, utoipa::ToSchema)]
pub struct CreateMeetingResp {
    #[serde(flatten)]
    pub meeting: Meeting,
    pub conflicts: Conflicts,
}

#[utoipa::path(
    post, path = "/api/meetings", tag = "meetings",
    security(("session" = [])),
    request_body = CreateMeetingReq,
    responses(
        (status = 200, body = CreateMeetingResp),
        (status = 400, body = crate::openapi::ErrorBody, description = "título, `kind` ou duração inválidos"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "quota de reuniões da organização atingida, ou sala física já reservada nesse horário"),
    )
)]
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

    let meeting: Meeting = sqlx::query_as(&format!(
        "INSERT INTO meetings (owner_id, title, description, kind, starts_at, duration_min, room_ref,
                               recurrence_freq, recurrence_interval, recurrence_until, recurrence_count, recurrence_byday)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
         RETURNING {MEETING_COLUMNS}"
    ))
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
    crate::notifications::meeting_invited(&state, &meeting, auth.user_id, &req.invitee_ids).await;

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

/// Só o dono ou um convidado pode arrancar/exportar a reunião. Antes desta
/// função, `start` e `ics` repetiam a mesma verificação lado a lado
/// (ADR-0004, Fase 3).
async fn is_owner_or_invitee(
    state: &AppState,
    meeting_id: Uuid,
    owner_id: Uuid,
    user_id: Uuid,
) -> Result<bool, ApiError> {
    if owner_id == user_id {
        return Ok(true);
    }
    let row: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM meeting_invitees WHERE meeting_id = $1 AND user_id = $2")
            .bind(meeting_id)
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;
    Ok(row.is_some())
}

/// Convidados que ainda não estão na sala (participantes ativos menos quem
/// já está dentro e o próprio anfitrião). O alvo do "toca ao vivo" que
/// `start`, `ring_upcoming_meetings` (cron) e `meetings_v1::ring` (API
/// pública) reimplementavam cada um à sua maneira (ADR-0004, Fase 3).
pub(crate) async fn invitees_to_ring(
    state: &AppState,
    meeting_id: Uuid,
    owner_id: Uuid,
    room_code: &str,
) -> std::collections::HashSet<Uuid> {
    let already_in: std::collections::HashSet<Uuid> =
        match sqlx::query_as::<_, (Uuid,)>("SELECT id FROM rooms WHERE code = $1")
            .bind(room_code)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten()
        {
            Some((rid,)) => state.hub.users_in_room(rid),
            None => Default::default(),
        };

    let invitees: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT user_id FROM meeting_invitees WHERE meeting_id = $1 AND status <> 'declined'",
    )
    .bind(meeting_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    invitees
        .into_iter()
        .map(|(uid,)| uid)
        .filter(|uid| !already_in.contains(uid) && *uid != owner_id)
        .collect()
}

/// Regista a chamada e toca aos alvos já filtrados por `invitees_to_ring`.
/// O `register_call` é o que falta para quem atende entrar directo na sala
/// em vez de cair na sala de espera — mesma mecânica nos três chamadores.
pub(crate) async fn register_and_ring(
    state: &Arc<AppState>,
    room_code: &str,
    owner_id: Uuid,
    owner_name: &str,
    targets: std::collections::HashSet<Uuid>,
    kind: &str,
    title: &str,
) -> (Vec<Uuid>, Vec<Uuid>) {
    state
        .presence
        .register_call(room_code.to_string(), owner_id, targets.clone());
    crate::presence::ring_users(state, owner_id, owner_name, targets, room_code, kind, title).await
}

/// Reuniões do utilizador: as que criou + aquelas para que foi convidado.
#[utoipa::path(
    get, path = "/api/meetings", tag = "meetings",
    security(("session" = [])),
    responses(
        (status = 200, body = Vec<MeetingItem>, description = "Reuniões criadas pelo utilizador e aquelas para que foi convidado, por data"),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Vec<MeetingItem>>, ApiError> {
    // Sem varredura da quarentena aqui: esta lista não lê `meet_quarantine`.
    // Parte dos dois índices (`meetings_owner_idx`, `meeting_invitees_user_idx`)
    // e só depois junta `meetings`: um `WHERE m.owner_id = $1 OR i.user_id IS NOT
    // NULL` obriga a varrer a tabela inteira. O `UNION` tira o duplicado do dono
    // que também é convidado.
    let items: Vec<MeetingItem> = sqlx::query_as(
        r#"
        WITH mine AS (
            SELECT id FROM meetings WHERE owner_id = $1
            UNION
            SELECT meeting_id FROM meeting_invitees WHERE user_id = $1
        )
        SELECT m.id, m.owner_id, u.username AS owner_name, m.title, m.description,
               m.kind, m.starts_at, m.duration_min, m.room_code,
               (m.owner_id = $1) AS is_owner, m.minutes,
               m.room_ref, mr.name AS room_name,
               CASE WHEN m.owner_id = $1 THEN 'owner' ELSE COALESCE(i.status, 'pending') END AS my_status,
               m.recurrence_freq, m.recurrence_interval, m.recurrence_parent_id
        FROM mine
        JOIN meetings m ON m.id = mine.id
        JOIN users u ON u.id = m.owner_id
        LEFT JOIN meeting_invitees i ON i.meeting_id = m.id AND i.user_id = $1
        LEFT JOIN meeting_rooms mr ON mr.id = m.room_ref
        ORDER BY m.starts_at ASC
        "#,
    )
    .bind(auth.user_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(items))
}

#[utoipa::path(
    delete, path = "/api/meetings/{meeting_id}", tag = "meetings",
    security(("session" = [])),
    params(("meeting_id" = Uuid, Path, description = "Id da reunião")),
    responses(
        (status = 204, description = "Apagada."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "não existe ou não é o dono"),
    )
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<axum::http::StatusCode, ApiError> {
    let audience = crate::notifications::meeting_audience(&state, id, auth.user_id).await;
    let res = sqlx::query("DELETE FROM meetings WHERE id = $1 AND owner_id = $2")
        .bind(id)
        .bind(auth.user_id)
        .execute(&state.db)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    crate::notifications::meeting_cancelled(&state, audience).await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Deserialize, utoipa::ToSchema)]
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
#[utoipa::path(
    put, path = "/api/meetings/{meeting_id}/minutes", tag = "meetings",
    security(("session" = [])),
    params(("meeting_id" = Uuid, Path, description = "Id da reunião")),
    request_body = MinutesReq,
    responses(
        (status = 204, description = "Guardada. O resumo AI é gerado em segundo plano e substitui `minutes`."),
        (status = 401, body = crate::openapi::ErrorBody, description = "sessão inválida"),
        (status = 404, body = crate::openapi::ErrorBody, description = "a reunião não existe, ou não é dono nem convidado"),
    )
)]
pub async fn save_minutes(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<MinutesReq>,
) -> Result<StatusCode, ApiError> {
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
        return Err(ApiError::NotFound);
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
    Ok(StatusCode::NO_CONTENT)
}

/// Guarda MoM associando pela sala: encontra a reunião cuja `room_code` bate
/// certo (reuniões iniciadas a partir do calendário). Usado quando se grava
/// a partir de dentro da chamada.
#[utoipa::path(
    put, path = "/api/rooms/{room_code}/minutes", tag = "meetings",
    security(("session" = [])),
    params(("room_code" = String, Path, description = "Código da sala")),
    request_body = MinutesReq,
    responses(
        (status = 204, description = "Guardada. O resumo AI é gerado em segundo plano."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "sem reunião nesta sala de que seja dono ou convidado (os dois casos não se distinguem)"),
    )
)]
pub async fn save_minutes_by_room(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
    Json(req): Json<MinutesReq>,
) -> Result<StatusCode, ApiError> {
    // A AUTORIZAÇÃO VEM PRIMEIRO (R96). Antes, a consulta corria para toda a
    // gente e a resposta dizia se a sala tinha reunião agendada — a quem
    // apenas soubesse o código, e de outra organização. O código da sala é uma
    // capability para VER metadados e PEDIR entrada; não é um passe para saber
    // o que está agendado lá dentro.
    //
    // Encontrado pelo teste de isolamento: `POST` de outro inquilino devolvia
    // `200 {"ok":false,"reason":"no meeting for room"}`. Nada era escrito — o
    // delegado `save_minutes` autoriza —, mas o 200 e a razão já eram resposta
    // a mais.
    let meeting: Option<(Uuid,)> = sqlx::query_as(
        "SELECT m.id FROM meetings m
         LEFT JOIN meeting_invitees i ON i.meeting_id = m.id AND i.user_id = $2
         WHERE m.room_code = $1 AND (m.owner_id = $2 OR i.user_id IS NOT NULL)",
    )
    .bind(&code)
    .bind(auth.user_id)
    .fetch_optional(&state.db)
    .await?;
    let Some((mid,)) = meeting else {
        // Uma resposta só para os dois casos: «não há reunião» e «não é tua».
        // Distingui-los é o que fazia a fuga — e um `404` é o que as outras
        // rotas de recurso já devolvem (ver R95).
        return Err(ApiError::NotFound);
    };
    save_minutes(State(state), auth, Path(mid), Json(req)).await
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct RoomNotes {
    pub title: String,
    pub minutes: String,
    pub transcript: String,
}

/// Ata e transcrição da reunião associada a uma sala — para o leitor da
/// biblioteca de gravações. Só participantes da sala têm acesso.
#[utoipa::path(
    get, path = "/api/rooms/{room_code}/minutes", tag = "meetings",
    security(("session" = [])),
    params(("room_code" = String, Path, description = "Código da sala")),
    responses(
        (status = 200, body = RoomNotes, description = "Ata da reunião mais recente da sala; campos vazios se a sala não tiver reunião"),
        (status = 401, body = crate::openapi::ErrorBody, description = "sessão inválida"),
        (status = 404, body = crate::openapi::ErrorBody, description = "a sala não existe, ou não participou nela"),
    )
)]
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
        return Err(ApiError::NotFound);
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

/// Resposta de `POST /api/meetings/{id}/start`.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct StartResp {
    /// Código da sala a que o cliente se liga.
    pub code: String,
    /// `video` | `voice`.
    pub kind: String,
}

/// Arranca a reunião: cria a sala (se ainda não existe) e devolve o código.
/// Reuniões de voz criam na mesma uma sala — o cliente entra sem vídeo.
#[utoipa::path(
    post, path = "/api/meetings/{meeting_id}/start", tag = "meetings",
    security(("session" = [])),
    params(("meeting_id" = Uuid, Path, description = "Id da reunião")),
    responses(
        (status = 200, body = StartResp),
        (status = 400, body = crate::openapi::ErrorBody, description = "um convidado tentou arrancar antes do anfitrião"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "não existe, ou não és dono nem convidado"),
    )
)]
pub async fn start(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<StartResp>, ApiError> {
    let meeting: Meeting = sqlx::query_as(&format!(
        "SELECT {MEETING_COLUMNS} FROM meetings WHERE id = $1"
    ))
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    // Só dono ou convidado pode arrancar/entrar; para os outros não existe.
    if !is_owner_or_invitee(&state, id, meeting.owner_id, auth.user_id).await? {
        return Err(ApiError::NotFound);
    }

    // Se já foi arrancada, reutiliza a sala.
    if let Some(code) = meeting.room_code.clone() {
        if sqlx::query_as::<_, (Uuid,)>("SELECT id FROM rooms WHERE code = $1")
            .bind(&code)
            .fetch_optional(&state.db)
            .await?
            .is_some()
        {
            return Ok(Json(StartResp {
                code,
                kind: meeting.kind,
            }));
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
    let targets = invitees_to_ring(&state, id, auth.user_id, &room.code).await;
    if !targets.is_empty() {
        let caller_name: (String,) = sqlx::query_as("SELECT username FROM users WHERE id = $1")
            .bind(auth.user_id)
            .fetch_one(&state.db)
            .await?;
        let (ringing, offline) = register_and_ring(
            &state,
            &room.code,
            auth.user_id,
            &caller_name.0,
            targets,
            &meeting.kind,
            &meeting.title,
        )
        .await;
        tracing::info!(meeting = %id, ringing = ringing.len(), offline = offline.len(), "meeting start ring");
    }

    Ok(Json(StartResp {
        code: room.code,
        kind: meeting.kind,
    }))
}

/// Uma reunião (dono ou convidado). Recurso completo: existe `DELETE`, existe `GET`.
#[utoipa::path(
    get, path = "/api/meetings/{meeting_id}", tag = "meetings",
    security(("session" = [])),
    params(("meeting_id" = Uuid, Path, description = "Id da reunião")),
    responses(
        (status = 200, body = Meeting),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "não existe, ou não és dono nem convidado"),
    )
)]
pub async fn get_one(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Meeting>, ApiError> {
    let meeting: Option<Meeting> = sqlx::query_as(&format!(
        "SELECT {MEETING_COLUMNS} FROM meetings WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    let meeting = meeting.ok_or(ApiError::NotFound)?;
    if !is_owner_or_invitee(&state, id, meeting.owner_id, auth.user_id).await? {
        return Err(ApiError::NotFound);
    }
    Ok(Json(meeting))
}

/// Exportação iCalendar (roadmap "Google e Outlook Calendar"): um .ics por
/// reunião — importa/abre no Google Calendar, Outlook, Apple Calendar, etc.
#[utoipa::path(
    get, path = "/api/meetings/{meeting_id}/calendar.ics", tag = "meetings",
    security(("session" = [])),
    params(("meeting_id" = Uuid, Path, description = "Id da reunião")),
    responses(
        (status = 200, body = String, content_type = "text/calendar", description = "Um VEVENT iCalendar, como anexo `reuniao.ics`"),
        (status = 401, body = crate::openapi::ErrorBody, description = "não é dono nem convidado"),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn ics(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<axum::response::Response, ApiError> {
    let meeting: Meeting = sqlx::query_as(&format!(
        "SELECT {MEETING_COLUMNS} FROM meetings WHERE id = $1"
    ))
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    if !is_owner_or_invitee(&state, id, meeting.owner_id, auth.user_id).await? {
        // Quem não é dono nem convidado não fica a saber que a reunião existe.
        return Err(ApiError::NotFound);
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

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ConflictCheckReq {
    pub starts_at: DateTime<Utc>,
    #[serde(default = "default_duration")]
    #[schema(default = 30)]
    pub duration_min: i32,
    #[serde(default)]
    pub invitee_ids: Vec<Uuid>,
    #[serde(default)]
    pub room_ref: Option<Uuid>,
}

#[utoipa::path(
    post, path = "/api/meetings/check-conflicts", tag = "meetings",
    security(("session" = [])),
    request_body = ConflictCheckReq,
    responses(
        (status = 200, body = Conflicts),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
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

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct InviteeResponse {
    pub user_id: Uuid,
    pub username: String,
    /// `pending` | `accepted` | `declined`.
    pub status: String,
    pub decline_reason: String,
    pub responded_at: Option<DateTime<Utc>>,
}

/// Lista as respostas dos convidados (só o anfitrião vê tudo).
#[utoipa::path(
    get, path = "/api/meetings/{meeting_id}/invitees", tag = "meetings",
    security(("session" = [])),
    params(("meeting_id" = Uuid, Path, description = "Id da reunião")),
    responses(
        (status = 200, body = Vec<InviteeResponse>),
        (status = 401, body = crate::openapi::ErrorBody, description = "sessão inválida"),
        (status = 403, body = crate::openapi::ErrorBody, description = "`meeting.not_host`: é convidado, não anfitrião"),
        (status = 404, body = crate::openapi::ErrorBody, description = "não existe, ou não é dono nem convidado"),
    )
)]
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
        Some((o,)) if is_owner_or_invitee(&state, id, o, auth.user_id).await? => {
            return Err(
                delonix_meet_core::DomainError::forbidden("meeting.not_host")
                    .with_message("só o anfitrião vê as respostas dos convidados")
                    .into(),
            );
        }
        _ => return Err(ApiError::NotFound),
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

#[derive(Deserialize, utoipa::ToSchema)]
pub struct RespondReq {
    /// `accepted` | `declined`.
    pub status: String,
    /// Obrigatório quando `status` é `declined`.
    #[serde(default)]
    pub reason: String,
}

/// O convidado aceita ou recusa (recusar exige motivo). Ao recusar, o
/// anfitrião é notificado em tempo real (se online) com o motivo.
#[utoipa::path(
    put, path = "/api/meetings/{meeting_id}/invitees/me", tag = "meetings",
    security(("session" = [])),
    params(("meeting_id" = Uuid, Path, description = "Id da reunião")),
    request_body = RespondReq,
    responses(
        (status = 200, body = InviteeResponse, description = "A resposta de quem pede, como ficou gravada."),
        (status = 400, body = crate::openapi::ErrorBody, description = "`status` inválido, ou recusa sem motivo"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "não é convidado desta reunião"),
    )
)]
pub async fn respond(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<RespondReq>,
) -> Result<Json<InviteeResponse>, ApiError> {
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

    let me: InviteeResponse = sqlx::query_as(
        "SELECT i.user_id, u.username, i.status, i.decline_reason, i.responded_at
         FROM meeting_invitees i JOIN users u ON u.id = i.user_id
         WHERE i.meeting_id = $1 AND i.user_id = $2",
    )
    .bind(id)
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(me))
}

// ---------- analytics de quarentena ----------

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct QuarantineRow {
    pub user_id: Uuid,
    pub username: String,
    pub count: i64,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AnalyticsQuery {
    /// `week` | `month` (omissão) | `quarter` | `year`. Valor desconhecido conta como `month`.
    #[serde(default = "default_period")]
    #[param(default = "month")]
    pub period: String,
}
fn default_period() -> String {
    "month".into()
}

/// Ranking de quem mais fica em quarentena na organização, no período pedido.
/// Só administradores da org.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/analytics/quarantine", tag = "meetings",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), AnalyticsQuery),
    responses(
        (status = 200, body = Vec<QuarantineRow>, description = "Até 100 linhas"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "membro sem papel de admin"),
        (status = 404, body = crate::openapi::ErrorBody, description = "não é membro da organização"),
    )
)]
pub async fn quarantine_analytics(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    axum::extract::Query(q): axum::extract::Query<AnalyticsQuery>,
) -> Result<Json<Vec<QuarantineRow>>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let days: i32 = match q.period.as_str() {
        "week" => 7,
        "month" => 30,
        "quarter" => 90,
        "year" => 365,
        _ => 30,
    };
    // Depois da autorização: quem não é admin da org não põe a base a escrever.
    quarantine_sweep_org(&state.db, org_id, days).await?;
    let in_org = crate::org::quarantine_subject_in_org_sql("$2", "u.id");
    let rows: Vec<QuarantineRow> = sqlx::query_as(&format!(
        "SELECT u.id AS user_id, u.username, COUNT(*) AS count
         FROM meet_quarantine mq
         JOIN users u ON u.id = mq.user_id
         JOIN meetings m ON m.id = mq.meeting_id
         WHERE m.starts_at >= now() - make_interval(days => $1::int)
           AND {in_org}
         GROUP BY u.id, u.username
         ORDER BY count DESC, u.username
         LIMIT 100"
    ))
    .bind(days)
    .bind(org_id)
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
    let parents: Vec<Meeting> = sqlx::query_as(&format!(
        "SELECT {MEETING_COLUMNS}
         FROM meetings
         WHERE recurrence_freq IS NOT NULL
           AND recurrence_parent_id IS NULL
           AND (recurrence_until IS NULL OR recurrence_until > now()::date)"
    ))
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

        let targets = invitees_to_ring(state, meeting_id, owner_id, &room_code).await;
        if targets.is_empty() {
            continue;
        }
        crate::notifications::meeting_starting(state, meeting_id, &title, &room_code, &targets)
            .await;
        let (ringing, offline) = register_and_ring(
            state,
            &room_code,
            owner_id,
            &owner_name,
            targets,
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
