//! «Ligar a…» a partir da sala — fatia F1: fazer tocar um RAMAL da própria
//! organização e pôr a perna na sala (`docs/ligar-a-partir-da-sala.md`).
//!
//! - `POST /api/rooms/{room_code}/dial-outs` `{ "extension_id" }` → `202` com o
//!   pedido em `queued`; a chamada corre numa tarefa.
//! - `GET  /api/rooms/{room_code}/dial-outs` → os pedidos da sala, do mais recente.
//! - `POST /api/rooms/{room_code}/dial-outs/{id}/hangup` → cancela enquanto toca,
//!   ou desliga em chamada. Idempotente.
//!
//! Quem pode: o anfitrião ou co-anfitrião da sala (a mesma regra da sala de espera)
//! E quem tem `sessions.dial_out` na organização do ramal — um ramal de outra
//! organização (aquela onde o anfitrião não é membro) responde como se não existisse. Salas E2EE recusam (a ponte não
//! decifra). O estado escreve-o este serviço a partir dos eventos do `originate`
//! (`DialOutStatus::can_go` ignora o que chega fora de ordem); nunca o navegador.
//!
//! Por fazer (e dito na PR): o aviso de gravação ao chamado (D3), o bilhete de
//! identidade para a perna aparecer com o nome do ramal, o varredor que reconcilia
//! `in_call` depois de um reinício, o evento `DialOutUpdated` aos anfitriões (hoje
//! lê-se por `GET`), e a F2 (número externo por tronco).

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::{DomainError, ErrorKind};
use delonix_meet_domain::conferencing::channels::{status_from_hangup_cause, DialOutStatus};
use delonix_meet_domain::identity::authorization::{Capability, ResourceScope};
use delonix_meet_domain::telephony::ports::{
    AfterAnswer, CallEvent, CallEventSink, CallOriginator, InternalLeg, OriginateRequest,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, rooms::Room, AppState};

/// Pedidos vivos ao mesmo tempo numa sala.
const MAX_LIVE_PER_ROOM: i64 = 5;
/// Tempo máximo a tocar.
const ANSWER_TIMEOUT_SECS: u32 = 30;
/// Um pedido que não saiu de `queued`/`dialing`/`ringing` neste tempo falhou:
/// o processo pode ter parado a meio e a linha ficaria viva para sempre,
/// bloqueando um novo pedido para o mesmo ramal.
const STALE_SECS: i64 = 90;

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(create, list, hangup),
    components(schemas(CreateDialOutReq, DialOutView, DialOutPage))
)]
pub struct ApiDoc;

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateDialOutReq {
    /// O ramal a fazer tocar (um ramal activo de uma organização do anfitrião, com `sessions.dial_out`).
    pub extension_id: Uuid,
}

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct DialOutView {
    pub id: Uuid,
    pub room_code: String,
    pub extension_id: Option<Uuid>,
    /// O número curto do ramal, quando ainda existe.
    pub extension: Option<String>,
    pub display_name: Option<String>,
    /// `queued` | `dialing` | `ringing` | `in_call` | `ended` | `declined` |
    /// `no_answer` | `failed` | `cancelled`.
    pub status: String,
    /// Causa estável quando falhou (`USER_NOT_REGISTERED`, `stale`…).
    pub failure_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub answered_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub billsec: Option<i32>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DialOutPage {
    pub items: Vec<DialOutView>,
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListQuery {
    /// 1-100, omissão 50.
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Cursor {
    at: DateTime<Utc>,
    id: Uuid,
}

const VIEW_SELECT: &str = "SELECT d.id, d.room_code, d.extension_id, e.extension, d.display_name,
        d.status, d.failure_code, d.created_at, d.answered_at, d.ended_at, d.billsec
   FROM room_dial_outs d LEFT JOIN voice_extensions e ON e.id = d.extension_id";

fn refuse(code: &'static str, msg: impl Into<String>) -> ApiError {
    DomainError::precondition(code, msg).into()
}

/// A sala pelo código, só para quem a gere (anfitrião ou co-anfitrião). Quem
/// não tem sequer acesso à sala recebe o mesmo `404` que um código inexistente.
async fn room_for_host(state: &AppState, user_id: Uuid, code: &str) -> Result<Room, ApiError> {
    let room: Room = sqlx::query_as(&format!(
        "SELECT {} FROM rooms WHERE code = $1",
        crate::rooms::ROOM_COLUMNS
    ))
    .bind(code.to_lowercase())
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    let access = crate::rooms::room_access(state, user_id, &room).await?;
    if !access.admitter && !state.hub.user_admits(room.id, user_id) {
        return Err(if access.authorized {
            ApiError::Forbidden
        } else {
            ApiError::NotFound
        });
    }
    Ok(room)
}

/// Fecha o que ficou a tocar sem fim (ver `STALE_SECS`).
async fn finish_stale(state: &AppState, room_id: Uuid) -> Result<(), ApiError> {
    sqlx::query(
        "UPDATE room_dial_outs
            SET status = 'failed', failure_code = 'stale', ended_at = now()
          WHERE room_id = $1 AND status IN ('queued','dialing','ringing')
            AND created_at < now() - make_interval(secs => $2)",
    )
    .bind(room_id)
    .bind(STALE_SECS as f64)
    .execute(&state.db)
    .await?;
    Ok(())
}

/// Escreve a transição, se for permitida a partir do estado actual (um evento
/// fora de ordem ou repetido não faz nada).
async fn transition(
    db: &sqlx::PgPool,
    id: Uuid,
    to: DialOutStatus,
    failure_code: Option<&str>,
    billsec: Option<i64>,
) -> Result<bool, sqlx::Error> {
    let from: Vec<&str> = [
        DialOutStatus::Queued,
        DialOutStatus::Dialing,
        DialOutStatus::Ringing,
        DialOutStatus::InCall,
    ]
    .into_iter()
    .filter(|s| s.can_go(to))
    .map(DialOutStatus::as_str)
    .collect();
    let r = sqlx::query(
        "UPDATE room_dial_outs
            SET status = $2,
                failure_code = COALESCE($3, failure_code),
                answered_at = CASE WHEN $2 = 'in_call' THEN now() ELSE answered_at END,
                ended_at = CASE WHEN $4 THEN now() ELSE ended_at END,
                billsec = COALESCE($5, billsec)
          WHERE id = $1 AND status = ANY($6)",
    )
    .bind(id)
    .bind(to.as_str())
    .bind(failure_code)
    .bind(to.is_final())
    .bind(billsec.map(|b| b as i32))
    .bind(&from)
    .execute(db)
    .await?;
    Ok(r.rows_affected() == 1)
}

/// Os eventos do `originate` chegam por aqui, pela ordem, numa só tarefa.
struct Sink(tokio::sync::mpsc::UnboundedSender<CallEvent>);

impl CallEventSink for Sink {
    fn on_event(&self, _: Uuid, event: CallEvent) {
        let _ = self.0.send(event);
    }
}

async fn apply_event(
    db: &sqlx::PgPool,
    originator: &Arc<dyn CallOriginator>,
    id: Uuid,
    call_id: Uuid,
    ev: &CallEvent,
) -> Result<(), sqlx::Error> {
    let (changed, vivo) = match ev {
        CallEvent::Dialing { .. } => (
            transition(db, id, DialOutStatus::Dialing, None, None).await?,
            true,
        ),
        CallEvent::Ringing { .. } => (
            transition(db, id, DialOutStatus::Ringing, None, None).await?,
            true,
        ),
        CallEvent::Answered { .. } => (
            transition(db, id, DialOutStatus::InCall, None, None).await?,
            true,
        ),
        CallEvent::AttemptFailed { .. } => (false, false),
        CallEvent::Ended {
            answered,
            cause,
            billsec,
        } => {
            let to = status_from_hangup_cause(cause, *answered);
            // A causa só interessa a quem não chegou a atender e falhou.
            let code = (to == DialOutStatus::Failed).then_some(cause.as_str());
            (transition(db, id, to, code, *billsec).await?, false)
        }
    };
    // O anfitrião cancelou antes de o canal existir: o `hupall` do cancelamento não
    // apanhou nada e o ramal tocou na mesma. Um sinal de vida de uma linha já
    // `cancelled` desliga-o agora (e, se atendeu, a perna sai da sala).
    if vivo && !changed {
        let cancelado: bool =
            sqlx::query_scalar("SELECT status = 'cancelled' FROM room_dial_outs WHERE id = $1")
                .bind(id)
                .fetch_optional(db)
                .await?
                .unwrap_or(false);
        if cancelado {
            if let Err(e) = originator.hangup(call_id).await {
                tracing::warn!(dial_out = %id, "não consegui desligar uma perna cancelada: {e}");
            }
        }
    }
    Ok(())
}

/// `host:porta` da ponte como endereço de socket (o `AfterAnswer::RoomBridge`
/// leva um IP). `None` se a ponte está desligada ou anunciada por um nome.
fn bridge_target(state: &AppState) -> Option<std::net::SocketAddr> {
    let bind = state.config.phone_bridge_sip_bind?;
    if state.config.phone_bridge_freeswitch_ips.is_empty()
        && state.config.phone_bridge_freeswitch_names.is_empty()
    {
        return None; // a ponte recusaria o INVITE (fail-closed)
    }
    crate::voice::bridge_advertise(state, bind).parse().ok()
}

/// Faz tocar um ramal e põe a perna na sala.
#[utoipa::path(
    post, path = "/api/rooms/{room_code}/dial-outs", tag = "dial-outs",
    security(("session" = [])),
    params(("room_code" = String, Path, description = "Código da sala."), ("room" = Option<String>, Query, description = "O mesmo código: chave de afinidade do balanceador.")),
    request_body = CreateDialOutReq,
    responses(
        (status = 202, body = DialOutView, description = "A tocar; acompanhar por `GET`."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "Não gere a sala, ou falta `sessions.dial_out`."),
        (status = 404, body = crate::openapi::ErrorBody, description = "Sala ou ramal inexistente (ou de outra organização)."),
        (status = 409, body = crate::openapi::ErrorBody, description = "`dial_out.already_active`: este ramal já está a ser chamado nesta sala."),
        (status = 422, body = crate::openapi::ErrorBody, description = "`dial_out.room_e2ee`, `dial_out.room_recording`, `dial_out.extension_inactive`, `dial_out.not_configured`, `dial_out.bridge_not_configured`."),
        (status = 429, body = crate::openapi::ErrorBody, description = "`dial_out.too_many`, `dial_out.rate_limited`."),
    )
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
    Json(req): Json<CreateDialOutReq>,
) -> Result<(StatusCode, Json<DialOutView>), ApiError> {
    let room = room_for_host(&state, auth.user_id, &code).await?;
    if room.e2ee {
        return Err(refuse(
            "dial_out.room_e2ee",
            "uma sala cifrada de ponta a ponta não aceita um telefone: a ponte não decifra",
        ));
    }
    // Quem atende entra na sala com o microfone aberto, sem aviso: numa sala a gravar
    // seria uma gravação sem consentimento. Até haver o aviso ao chamado (D3), recusa-se.
    if state.sfu.recording_by(room.id).await.is_some() {
        return Err(refuse(
            "dial_out.room_recording",
            "a sala está a gravar e o chamado não seria avisado: pára a gravação para ligar a um ramal",
        ));
    }
    let originator = state.telephony.originator.clone().ok_or_else(|| {
        refuse(
            "dial_out.not_configured",
            "sem servidor de media (TELEPHONY_ESL_ADDR) — nenhuma chamada sai desta instalação",
        )
    })?;
    let bridge = bridge_target(&state).ok_or_else(|| {
        refuse(
            "dial_out.bridge_not_configured",
            "a ponte telefone↔sala está desligada ou anunciada por um nome (PHONE_BRIDGE_*)",
        )
    })?;

    // O ramal: um id de outra organização, ou onde o anfitrião não está, é um
    // `404` — nunca se confirma que existe.
    let ext: Option<(Uuid, String, String, bool)> = sqlx::query_as(
        "SELECT org_id, sip_username, extension, active FROM voice_extensions WHERE id = $1",
    )
    .bind(req.extension_id)
    .fetch_optional(&state.db)
    .await?;
    let (org_id, sip_username, extension, active) = ext.ok_or(ApiError::NotFound)?;
    crate::org::require_capability(
        &state,
        org_id,
        auth.user_id,
        Capability::SessionsDialOut,
        ResourceScope::Organization,
    )
    .await?;
    if !active {
        return Err(refuse(
            "dial_out.extension_inactive",
            "este ramal está desactivado",
        ));
    }

    finish_stale(&state, room.id).await?;
    let live: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM room_dial_outs
          WHERE room_id = $1 AND status IN ('queued','dialing','ringing','in_call')",
    )
    .bind(room.id)
    .fetch_one(&state.db)
    .await?;
    if live >= MAX_LIVE_PER_ROOM {
        return Err(DomainError::new(
            ErrorKind::ResourceExhausted,
            "dial_out.too_many",
            format!("já há {MAX_LIVE_PER_ROOM} chamadas a sair desta sala"),
        )
        .into());
    }
    if let Err(wait) = state.telephony_call_limiter.acquire(&org_id.to_string()) {
        return Err(DomainError::new(
            ErrorKind::ResourceExhausted,
            "dial_out.rate_limited",
            format!(
                "demasiadas chamadas desta organização; tenta daqui a {} s",
                wait.as_secs().max(1)
            ),
        )
        .into());
    }

    let sip_domain = crate::ramais::sip_domain_for_org(&state, org_id).await?;
    let call_id = Uuid::new_v4();
    let id = Uuid::new_v4();
    let inserted = sqlx::query(
        "INSERT INTO room_dial_outs
            (id, org_id, room_id, room_code, kind, channel, extension_id, display_name,
             status, telephony_call_id, requested_by)
         VALUES ($1, $2, $3, $4, 'voice', 'phone', $5,
                 COALESCE(NULLIF((SELECT label FROM voice_extensions WHERE id = $5), ''), $6),
                 'queued', $7, $8)",
    )
    .bind(id)
    .bind(org_id)
    .bind(room.id)
    .bind(&room.code)
    .bind(req.extension_id)
    .bind(&extension)
    .bind(call_id)
    .bind(auth.user_id)
    .execute(&state.db)
    .await;
    match inserted {
        Ok(_) => {}
        Err(sqlx::Error::Database(d))
            if d.constraint() == Some("room_dial_outs_ramal_vivo_uidx") =>
        {
            return Err(DomainError::conflict(
                "dial_out.already_active",
                "este ramal já está a ser chamado a partir desta sala",
            )
            .into());
        }
        Err(e) => return Err(e.into()),
    }
    // O alvo é o id: nem o número nem o nome entram numa auditoria imutável.
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "room.dial_out.requested",
        &id.to_string(),
    )
    .await;

    let origin = OriginateRequest {
        call_id,
        org_id,
        legs: vec![],
        internal: Some(InternalLeg {
            sip_username,
            domain: sip_domain,
        }),
        caller_id: None,
        record: false,
        emergency: false,
        rule_position: None,
        answer_timeout_secs: ANSWER_TIMEOUT_SECS,
        after_answer: AfterAnswer::RoomBridge {
            room_code: room.code.clone(),
            bridge_host: bridge.ip(),
            bridge_port: bridge.port(),
            codec: Some("PCMA".into()),
        },
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<CallEvent>();
    let db = state.db.clone();
    let listener = originator.clone();
    tokio::spawn(async move {
        let mut terminou = false;
        while let Some(ev) = rx.recv().await {
            if let Err(e) = apply_event(&db, &listener, id, call_id, &ev).await {
                tracing::error!(dial_out = %id, "não consegui gravar o evento da chamada: {e}");
            }
            if matches!(ev, CallEvent::Ended { .. }) {
                terminou = true;
                break;
            }
        }
        // O ESL fechou sem um fim (a ligação caiu, passaram 6 h): a linha não
        // fica `in_call` para sempre, a bloquear o ramal e a contar para o limite.
        if !terminou {
            let _ = transition(&db, id, DialOutStatus::Failed, Some("esl_lost"), None).await;
        }
    });
    let db = state.db.clone();
    tokio::spawn(async move {
        // Cancelado antes de arrancar (`queued` → `cancelled`): nem se origina.
        if !transition(&db, id, DialOutStatus::Dialing, None, None)
            .await
            .unwrap_or(false)
        {
            return;
        }
        if let Err(e) = originator.originate(&origin, Arc::new(Sink(tx))).await {
            // O ESL em baixo, a ponte inacessível: a pessoa nunca fica «a tocar».
            tracing::warn!(dial_out = %id, "originate falhou: {e}");
            let _ = transition(
                &db,
                id,
                DialOutStatus::Failed,
                Some("media_server_unavailable"),
                None,
            )
            .await;
        }
    });

    let view = one(&state, room.id, id).await?;
    Ok((StatusCode::ACCEPTED, Json(view)))
}

async fn one(state: &AppState, room_id: Uuid, id: Uuid) -> Result<DialOutView, ApiError> {
    sqlx::query_as(&format!("{VIEW_SELECT} WHERE d.id = $1 AND d.room_id = $2"))
        .bind(id)
        .bind(room_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::NotFound)
}

/// Os pedidos da sala, do mais recente para o mais antigo.
#[utoipa::path(
    get, path = "/api/rooms/{room_code}/dial-outs", tag = "dial-outs",
    security(("session" = [])),
    params(("room_code" = String, Path, description = "Código da sala."), ("room" = Option<String>, Query, description = "O mesmo código: chave de afinidade do balanceador."), ListQuery),
    responses(
        (status = 200, body = DialOutPage),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
    Query(q): Query<ListQuery>,
) -> Result<Json<DialOutPage>, ApiError> {
    use delonix_meet_core::page::{Page, PageRequest};
    let room = room_for_host(&state, auth.user_id, &code).await?;
    finish_stale(&state, room.id).await?;
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<Cursor> = page.cursor()?;
    let rows: Vec<DialOutView> = sqlx::query_as(&format!(
        "{VIEW_SELECT} WHERE d.room_id = $1
            AND ($2::timestamptz IS NULL OR (d.created_at, d.id) < ($2, $3))
          ORDER BY d.created_at DESC, d.id DESC LIMIT $4"
    ))
    .bind(room.id)
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |d| Cursor {
        at: d.created_at,
        id: d.id,
    });
    Ok(Json(DialOutPage {
        items: p.items,
        next_page_token: p.next_page_token,
    }))
}

/// Cancela enquanto toca, ou desliga em chamada. Idempotente: um pedido já
/// terminado devolve-se como está.
#[utoipa::path(
    post, path = "/api/rooms/{room_code}/dial-outs/{dial_out_id}/hangup", tag = "dial-outs",
    security(("session" = [])),
    params(("room_code" = String, Path), ("dial_out_id" = Uuid, Path), ("room" = Option<String>, Query, description = "O mesmo código: chave de afinidade do balanceador.")),
    responses(
        (status = 200, body = DialOutView),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 422, body = crate::openapi::ErrorBody, description = "`dial_out.not_configured`"),
        (status = 503, body = crate::openapi::ErrorBody, description = "O servidor de media não respondeu ao desligar."),
    )
)]
pub async fn hangup(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((code, id)): Path<(String, Uuid)>,
) -> Result<Json<DialOutView>, ApiError> {
    let room = room_for_host(&state, auth.user_id, &code).await?;
    let row: Option<(String, Option<Uuid>, Uuid)> = sqlx::query_as(
        "SELECT status, telephony_call_id, org_id FROM room_dial_outs WHERE id = $1 AND room_id = $2",
    )
    .bind(id)
    .bind(room.id)
    .fetch_optional(&state.db)
    .await?;
    let (status, call_id, org_id) = row.ok_or(ApiError::NotFound)?;
    let status = DialOutStatus::parse(&status).unwrap_or(DialOutStatus::Failed);
    if status.is_final() {
        return Ok(Json(one(&state, room.id, id).await?));
    }
    let originator = state.telephony.originator.clone().ok_or_else(|| {
        refuse(
            "dial_out.not_configured",
            "sem servidor de media (TELEPHONY_ESL_ADDR)",
        )
    })?;
    // Ainda a tocar: fica `cancelled` já (um `ORIGINATOR_CANCEL` tardio é
    // ignorado por já ser final). Em chamada, quem fecha é o evento de fim.
    if status.cancellable() {
        transition(&state.db, id, DialOutStatus::Cancelled, None, None).await?;
    }
    if let Some(call_id) = call_id {
        originator
            .hangup(call_id)
            .await
            .map_err(crate::telephony_service::port_error)?;
    }
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "room.dial_out.hangup",
        &id.to_string(),
    )
    .await;
    Ok(Json(one(&state, room.id, id).await?))
}
