//! Centro de notificações (G8) — adaptador HTTP + Postgres + push pelo `/rtc`.
//! As regras (tipos, textos, coalescência, retenção) estão em
//! `delonix_meet_domain::notification`.
//!
//! Contrato (rotas NOVAS seguem as regras do ADR-0004 §4):
//! - `GET    /api/users/me/notifications`                  lista paginada, mais recentes primeiro, com `unread_count`
//! - `GET    /api/users/me/notifications/{id}`             uma notificação
//! - `PATCH  /api/users/me/notifications/{id}`             `{"read": true|false}`
//! - `POST   /api/users/me/notifications/mark-all-read`    método personalizado → `{"updated": n}`
//! - `DELETE /api/users/me/notifications/{id}`             `204`
//!
//! A caixa é PESSOAL: todas as consultas filtram pelo `user_id` da sessão, e um
//! id de outra pessoa dá a mesma resposta que um id inexistente (`404`). Não é
//! uma rota de organização, por isso não entra em `web/e2e/isolamento.mjs`.
//!
//! **Produtores** — [`notify`] e os atalhos por evento. São best-effort: uma
//! falha aqui é um `warn` no log e NUNCA faz falhar a operação de origem (o
//! convite, a gravação, a transcrição já aconteceram).
//!
//! **Tempo real** — depois de inserir, a notificação é empurrada para o
//! utilizador como `{"type":"notification","notification":{…}}` pelo
//! `PresenceHub::notify` (entrega local, ou Redis para o nó onde ele está).
//! O cliente web actual ignora tipos que não conhece (`default: break` no
//! `PresenceProvider.tsx`), por isso a mensagem nova é compatível para trás.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::page::{Page, PageRequest};
use delonix_meet_domain::notification::{self as rules, Draft};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Notification {
    pub id: Uuid,
    /// `meeting.invited` | `meeting.starting` | `meeting.cancelled` |
    /// `call.missed` | `recording.ready` | `transcription.ready`.
    pub kind: String,
    pub title: String,
    pub body: String,
    /// Caminho relativo da app (p.ex. `/#/recordings`). Nunca um URL externo.
    pub link: String,
    /// Identificadores do evento (`meeting_id`, `recording_id`, `room_code`, …).
    #[schema(value_type = Object)]
    pub data: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
}

const COLUMNS: &str = "id, kind, title, body, link, data, created_at, read_at";

#[derive(Serialize, utoipa::ToSchema)]
pub struct NotificationPage {
    pub items: Vec<Notification>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
    /// Não lidas no total (independente da página e do filtro).
    pub unread_count: i64,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateNotificationReq {
    /// `true` marca como lida; `false` volta a não lida.
    pub read: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct MarkAllReadResp {
    /// Quantas passaram de não lidas a lidas.
    pub updated: u64,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListQuery {
    /// 1-100, omissão 50.
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
    /// Só as não lidas.
    #[serde(default)]
    pub unread_only: bool,
}

#[derive(Serialize, Deserialize)]
struct Cursor {
    at: DateTime<Utc>,
    id: Uuid,
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(list, get_one, update, mark_all_read, delete),
    components(schemas(Notification, NotificationPage, UpdateNotificationReq, MarkAllReadResp))
)]
pub struct ApiDoc;

/// A minha caixa de notificações, mais recentes primeiro.
#[utoipa::path(
    get, path = "/api/users/me/notifications", tag = "notifications",
    security(("session" = [])),
    params(ListQuery),
    responses(
        (status = 200, body = NotificationPage),
        (status = 400, body = crate::openapi::ErrorBody, description = "page_token inválido"),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<ListQuery>,
) -> Result<Json<NotificationPage>, ApiError> {
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<Cursor> = page.cursor()?;
    let rows: Vec<Notification> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM notifications
          WHERE user_id = $1
            AND ($2::timestamptz IS NULL OR (created_at, id) < ($2, $3))
            AND (NOT $4 OR read_at IS NULL)
          ORDER BY created_at DESC, id DESC
          LIMIT $5"
    ))
    .bind(auth.user_id)
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(q.unread_only)
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let unread_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE user_id = $1 AND read_at IS NULL",
    )
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |n| Cursor {
        at: n.created_at,
        id: n.id,
    });
    Ok(Json(NotificationPage {
        items: p.items,
        next_page_token: p.next_page_token,
        unread_count,
    }))
}

/// Uma notificação minha.
#[utoipa::path(
    get, path = "/api/users/me/notifications/{notification_id}", tag = "notifications",
    security(("session" = [])),
    params(("notification_id" = Uuid, Path)),
    responses(
        (status = 200, body = Notification),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "não existe ou é de outra pessoa"),
    )
)]
pub async fn get_one(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Notification>, ApiError> {
    sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM notifications WHERE id = $1 AND user_id = $2"
    ))
    .bind(id)
    .bind(auth.user_id)
    .fetch_optional(&state.db)
    .await?
    .map(Json)
    .ok_or(ApiError::NotFound)
}

/// Marca como lida ou não lida. Marcar como lida o que já está lido mantém a
/// data da primeira leitura.
#[utoipa::path(
    patch, path = "/api/users/me/notifications/{notification_id}", tag = "notifications",
    security(("session" = [])),
    params(("notification_id" = Uuid, Path)),
    request_body = UpdateNotificationReq,
    responses(
        (status = 200, body = Notification),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "não existe ou é de outra pessoa"),
        (status = 422, body = crate::openapi::ErrorBody, description = "corpo sem `read` booleano"),
    )
)]
pub async fn update(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateNotificationReq>,
) -> Result<Json<Notification>, ApiError> {
    sqlx::query_as(&format!(
        "UPDATE notifications
            SET read_at = CASE WHEN $3 THEN COALESCE(read_at, now()) ELSE NULL END
          WHERE id = $1 AND user_id = $2
         RETURNING {COLUMNS}"
    ))
    .bind(id)
    .bind(auth.user_id)
    .bind(req.read)
    .fetch_optional(&state.db)
    .await?
    .map(Json)
    .ok_or(ApiError::NotFound)
}

/// Método personalizado: marca todas as minhas não lidas como lidas.
#[utoipa::path(
    post, path = "/api/users/me/notifications/mark-all-read", tag = "notifications",
    security(("session" = [])),
    responses(
        (status = 200, body = MarkAllReadResp),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn mark_all_read(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<MarkAllReadResp>, ApiError> {
    let r = sqlx::query(
        "UPDATE notifications SET read_at = now() WHERE user_id = $1 AND read_at IS NULL",
    )
    .bind(auth.user_id)
    .execute(&state.db)
    .await?;
    Ok(Json(MarkAllReadResp {
        updated: r.rows_affected(),
    }))
}

/// Apaga uma notificação minha.
#[utoipa::path(
    delete, path = "/api/users/me/notifications/{notification_id}", tag = "notifications",
    security(("session" = [])),
    params(("notification_id" = Uuid, Path)),
    responses(
        (status = 204, description = "Apagada."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "não existe ou é de outra pessoa"),
    )
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let r = sqlx::query("DELETE FROM notifications WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(auth.user_id)
        .execute(&state.db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Produtores ----------

/// Guarda a notificação e empurra-a pelo `/rtc`. Best-effort: devolve `None`
/// (e deixa um `warn`) se falhar, e também `None` se já existia uma com a
/// mesma chave de coalescência para este destinatário — nesse caso não há
/// segundo push.
pub(crate) async fn notify(
    state: &AppState,
    user_id: Uuid,
    draft: Draft,
    data: serde_json::Value,
) -> Option<Notification> {
    // A pertença decide-se em `org.rs` (ADR-0004 §5 regra 1).
    let org_id = crate::org::orgs_of_user(state, user_id)
        .await
        .first()
        .copied();
    let res: Result<Option<Notification>, sqlx::Error> = sqlx::query_as(&format!(
        "INSERT INTO notifications (user_id, org_id, kind, title, body, link, data, dedupe_key)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (user_id, dedupe_key) WHERE dedupe_key IS NOT NULL DO NOTHING
         RETURNING {COLUMNS}"
    ))
    .bind(user_id)
    .bind(org_id)
    .bind(draft.kind.as_str())
    .bind(&draft.title)
    .bind(&draft.body)
    .bind(&draft.link)
    .bind(&data)
    .bind(&draft.dedupe_key)
    .fetch_optional(&state.db)
    .await;
    match res {
        Ok(Some(n)) => {
            state.presence.notify(
                user_id,
                crate::presence::CallServerMsg::Notification {
                    notification: n.clone(),
                },
            );
            Some(n)
        }
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(%user_id, kind = draft.kind.as_str(), error = %e,
                "notificação não gravada (a operação de origem segue)");
            None
        }
    }
}

async fn username(state: &AppState, user_id: Uuid) -> String {
    crate::users::fetch_public(&state.db, user_id)
        .await
        .map(|u| u.username)
        .unwrap_or_else(|_| "Alguém".to_string())
}

/// Convite para uma reunião: a cada convidado, nunca ao anfitrião, e só a
/// colegas de organização do anfitrião — uma notificação empurrada a uma conta
/// de outra org seria um canal de spam/phishing entre inquilinos (a mesma
/// regra que o `/rtc` aplica às chamadas).
pub(crate) async fn meeting_invited(
    state: &AppState,
    meeting: &crate::meetings::Meeting,
    host_id: Uuid,
    invitee_ids: &[Uuid],
) {
    let co: std::collections::HashSet<Uuid> = crate::org::org_co_members(state, host_id)
        .await
        .into_iter()
        .collect();
    let recipients: std::collections::BTreeSet<Uuid> = invitee_ids
        .iter()
        .copied()
        .filter(|u| *u != host_id && co.contains(u))
        .collect();
    if recipients.is_empty() {
        return;
    }
    let host = username(state, host_id).await;
    let data = serde_json::json!({ "meeting_id": meeting.id, "starts_at": meeting.starts_at });
    for uid in recipients {
        let draft = rules::meeting_invited(meeting.id, &meeting.title, &host, meeting.starts_at);
        notify(state, uid, draft, data.clone()).await;
    }
}

/// Quem tem de saber que a reunião foi cancelada — lido ANTES de a apagar
/// (os convidados vão em cascata com ela).
pub(crate) struct MeetingAudience {
    meeting_id: Uuid,
    title: String,
    starts_at: DateTime<Utc>,
    host_id: Uuid,
    invitees: Vec<Uuid>,
}

pub(crate) async fn meeting_audience(
    state: &AppState,
    meeting_id: Uuid,
    host_id: Uuid,
) -> Option<MeetingAudience> {
    let row: Option<(String, DateTime<Utc>)> =
        sqlx::query_as("SELECT title, starts_at FROM meetings WHERE id = $1 AND owner_id = $2")
            .bind(meeting_id)
            .bind(host_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    let (title, starts_at) = row?;
    let invitees: Vec<Uuid> = sqlx::query_scalar(
        "SELECT user_id FROM meeting_invitees
          WHERE meeting_id = $1 AND user_id <> $2 AND status <> 'declined'",
    )
    .bind(meeting_id)
    .bind(host_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    Some(MeetingAudience {
        meeting_id,
        title,
        starts_at,
        host_id,
        invitees,
    })
}

pub(crate) async fn meeting_cancelled(state: &AppState, audience: Option<MeetingAudience>) {
    let Some(a) = audience else { return };
    if a.invitees.is_empty() {
        return;
    }
    let host = username(state, a.host_id).await;
    let data = serde_json::json!({ "meeting_id": a.meeting_id, "starts_at": a.starts_at });
    for uid in a.invitees {
        let draft = rules::meeting_cancelled(a.meeting_id, &a.title, &host, a.starts_at);
        notify(state, uid, draft, data.clone()).await;
    }
}

/// A reunião agendada está a começar (auto-ring). Coalescida por reunião.
pub(crate) async fn meeting_starting(
    state: &AppState,
    meeting_id: Uuid,
    title: &str,
    room_code: &str,
    targets: &std::collections::HashSet<Uuid>,
) {
    let data = serde_json::json!({ "meeting_id": meeting_id, "room_code": room_code });
    for uid in targets {
        let draft = rules::meeting_starting(meeting_id, title, room_code);
        notify(state, *uid, draft, data.clone()).await;
    }
}

/// Chamada que tocou num alvo offline. Coalescida por sala.
pub(crate) async fn call_missed(
    state: &AppState,
    user_id: Uuid,
    caller_id: Uuid,
    caller_name: &str,
    room_code: &str,
    kind: &str,
) {
    let draft = rules::call_missed(room_code, caller_name, kind == "voice");
    let data =
        serde_json::json!({ "room_code": room_code, "caller_id": caller_id, "call_kind": kind });
    notify(state, user_id, draft, data).await;
}

/// Gravação do servidor pronta: a quem a gravou.
pub(crate) async fn recording_ready(
    state: &AppState,
    user_id: Uuid,
    recording_id: Uuid,
    filename: &str,
    room_code: &str,
) {
    let draft = rules::recording_ready(recording_id, filename);
    let data = serde_json::json!({ "recording_id": recording_id, "room_code": room_code });
    notify(state, user_id, draft, data).await;
}

/// Transcrição entregue: a quem carregou/gravou a gravação.
pub(crate) async fn transcription_ready(state: &AppState, recording_id: Uuid) {
    let row: Option<(Uuid, String)> = match sqlx::query_as(
        "SELECT uploader_id, filename FROM recordings WHERE id = $1",
    )
    .bind(recording_id)
    .fetch_optional(&state.db)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(%recording_id, error = %e, "notificação de transcrição: gravação ilegível");
            return;
        }
    };
    let Some((uploader, filename)) = row else {
        return;
    };
    let draft = rules::transcription_ready(recording_id, &filename);
    let data = serde_json::json!({ "recording_id": recording_id });
    notify(state, uploader, draft, data).await;
}

/// Retenção: lidas com mais de 90 dias, todas com mais de 180.
pub(crate) async fn retention_sweep(db: &sqlx::PgPool) -> Result<u64, sqlx::Error> {
    let (read_cut, all_cut) = rules::retention_cutoffs(Utc::now());
    let r = sqlx::query(
        "DELETE FROM notifications
          WHERE created_at < $2
             OR (read_at IS NOT NULL AND created_at < $1)",
    )
    .bind(read_cut)
    .bind(all_cut)
    .execute(db)
    .await?;
    Ok(r.rows_affected())
}
