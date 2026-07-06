//! Presença e chamadas estilo WhatsApp.
//!
//! Cada utilizador abre um WebSocket pessoal (`/rtc?token=<access_token>`)
//! que fica ligado enquanto a app está aberta. Ao iniciar uma chamada, o
//! servidor cria a sala e faz "tocar" em todos os alvos online; quem aceita
//! entra na mesma sala (via o fluxo normal de `join_room`).

use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, Query, State},
    response::Response,
};
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{auth::verify_jwt, error::ApiError, AppState};

// ---------- Protocolo ----------

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum CallClientMsg {
    /// Inicia chamada para utilizadores e/ou um grupo.
    CallStart {
        #[serde(default)]
        targets: Vec<Uuid>,
        #[serde(default)]
        group_id: Option<Uuid>,
        kind: String, // "video" | "voice"
        #[serde(default)]
        title: Option<String>,
    },
    CallAccept { room_code: String },
    CallDecline { room_code: String },
    CallCancel { room_code: String },
    Ping,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum CallServerMsg {
    /// Para o alvo: estás a receber uma chamada.
    IncomingCall {
        room_code: String,
        kind: String,
        caller_id: Uuid,
        caller_name: String,
        title: String,
    },
    /// Para o chamador: a sala foi criada e está a tocar nestes utilizadores.
    Ringing { room_code: String, kind: String, ringing: Vec<Uuid>, offline: Vec<Uuid> },
    /// Para o chamador: alguém aceitou.
    Accepted { room_code: String, by_id: Uuid, by_name: String },
    /// Para o chamador: alguém recusou.
    Declined { room_code: String, by_id: Uuid, by_name: String },
    /// Para os alvos: o chamador cancelou / a chamada terminou de tocar.
    Cancelled { room_code: String },
    /// Lista de contactos atualmente online (user_ids).
    Presence { online: Vec<Uuid> },
    /// Ao ligar: chamadas perdidas (recebidas enquanto estava offline).
    MissedCalls { calls: Vec<MissedCall> },
    /// Para o anfitrião: um convidado recusou uma reunião (com motivo).
    MeetingDeclined {
        meeting_id: Uuid,
        meeting_title: String,
        by_id: Uuid,
        by_name: String,
        reason: String,
    },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct MissedCall {
    pub id: Uuid,
    pub room_code: String,
    pub caller_id: Uuid,
    pub caller_name: String,
    pub kind: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// ---------- Hub ----------

struct Conn {
    tx: mpsc::UnboundedSender<CallServerMsg>,
    username: String,
}

#[derive(Default)]
pub struct PresenceHub {
    /// user_id -> ligações ativas (pode ter vários dispositivos).
    conns: DashMap<Uuid, Vec<Conn>>,
    /// room_code -> quem iniciou (para encaminhar accept/decline).
    calls: DashMap<String, Uuid>,
}

impl PresenceHub {
    fn add(&self, user_id: Uuid, tx: mpsc::UnboundedSender<CallServerMsg>, username: String) {
        self.conns.entry(user_id).or_default().push(Conn { tx, username });
    }

    fn remove(&self, user_id: Uuid, tx: &mpsc::UnboundedSender<CallServerMsg>) {
        if let Some(mut v) = self.conns.get_mut(&user_id) {
            v.retain(|c| !c.tx.same_channel(tx));
        }
        self.conns.remove_if(&user_id, |_, v| v.is_empty());
    }

    fn is_online(&self, user_id: Uuid) -> bool {
        self.conns.get(&user_id).map(|v| !v.is_empty()).unwrap_or(false)
    }

    fn online_ids(&self) -> Vec<Uuid> {
        self.conns.iter().map(|e| *e.key()).collect()
    }

    /// Envia a todas as ligações de um utilizador. Devolve true se entregou a alguém.
    fn send_to_user(&self, user_id: Uuid, msg: CallServerMsg) -> bool {
        let mut delivered = false;
        if let Some(v) = self.conns.get(&user_id) {
            for c in v.iter() {
                if c.tx.send(msg.clone()).is_ok() {
                    delivered = true;
                }
            }
        }
        delivered
    }

    pub fn register_call(&self, room_code: String, caller: Uuid) {
        self.calls.insert(room_code, caller);
    }

    /// Envia uma mensagem de servidor a um utilizador (usado por outros módulos).
    pub fn notify(&self, user_id: Uuid, msg: CallServerMsg) {
        self.send_to_user(user_id, msg);
    }

    fn caller_of(&self, room_code: &str) -> Option<Uuid> {
        self.calls.get(room_code).map(|c| *c)
    }
}

// ---------- WebSocket ----------

#[derive(Deserialize)]
pub struct WsQuery {
    token: String,
}

pub async fn rtc_handler(
    State(state): State<Arc<AppState>>,
    Query(q): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    // WebSocket pessoal: autenticado pelo access token.
    let claims = verify_jwt(&state.config.jwt_secret, &q.token, "access")?;
    let user_id = claims.sub;
    let user = crate::users::fetch_public(&state.db, user_id).await?;
    Ok(ws.on_upgrade(move |socket| handle(state, socket, user_id, user.username)))
}

async fn handle(state: Arc<AppState>, socket: WebSocket, user_id: Uuid, username: String) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<CallServerMsg>();

    state.presence.add(user_id, tx.clone(), username.clone());
    tracing::info!(%user_id, %username, "presence connected");

    // Estado inicial: quem está online.
    let _ = tx.send(CallServerMsg::Presence { online: state.presence.online_ids() });

    // Chamadas perdidas enquanto esteve offline (não vistas).
    if let Ok(calls) = sqlx::query_as::<_, MissedCall>(
        "SELECT id, room_code, caller_id, caller_name, kind, created_at
         FROM missed_calls WHERE user_id = $1 AND NOT seen ORDER BY created_at DESC LIMIT 50",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    {
        if !calls.is_empty() {
            let _ = tx.send(CallServerMsg::MissedCalls { calls });
        }
    }

    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let Ok(text) = serde_json::to_string(&msg) else { continue };
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = stream.next().await {
        let Message::Text(text) = msg else {
            if matches!(msg, Message::Close(_)) { break }
            continue;
        };
        let Ok(cmd) = serde_json::from_str::<CallClientMsg>(&text) else { continue };
        match cmd {
            CallClientMsg::Ping => {}
            CallClientMsg::CallStart { targets, group_id, kind, title } => {
                handle_call_start(&state, user_id, &username, targets, group_id, kind, title, &tx).await;
            }
            CallClientMsg::CallAccept { room_code } => {
                if let Some(caller) = state.presence.caller_of(&room_code) {
                    state.presence.send_to_user(
                        caller,
                        CallServerMsg::Accepted { room_code, by_id: user_id, by_name: username.clone() },
                    );
                }
            }
            CallClientMsg::CallDecline { room_code } => {
                if let Some(caller) = state.presence.caller_of(&room_code) {
                    state.presence.send_to_user(
                        caller,
                        CallServerMsg::Declined { room_code, by_id: user_id, by_name: username.clone() },
                    );
                }
            }
            CallClientMsg::CallCancel { room_code } => {
                // Só o chamador cancela; toca "cancelado" em toda a gente.
                if state.presence.caller_of(&room_code) == Some(user_id) {
                    state.presence.calls.remove(&room_code);
                    // Não sabemos exatamente quem estava a tocar; o cliente
                    // ignora Cancelled de salas que não está a receber.
                    for id in state.presence.online_ids() {
                        state.presence.send_to_user(id, CallServerMsg::Cancelled { room_code: room_code.clone() });
                    }
                }
            }
        }
    }

    state.presence.remove(user_id, &tx);
    writer.abort();
    tracing::info!(%user_id, "presence disconnected");
}

#[allow(clippy::too_many_arguments)]
async fn handle_call_start(
    state: &Arc<AppState>,
    caller: Uuid,
    caller_name: &str,
    targets: Vec<Uuid>,
    group_id: Option<Uuid>,
    kind: String,
    title: Option<String>,
    tx: &mpsc::UnboundedSender<CallServerMsg>,
) {
    if !matches!(kind.as_str(), "video" | "voice") {
        let _ = tx.send(CallServerMsg::Error { message: "tipo de chamada inválido".into() });
        return;
    }
    // Resolver alvos (união de targets + membros do grupo), sem o próprio.
    let mut set: std::collections::HashSet<Uuid> = targets.into_iter().collect();
    if let Some(gid) = group_id {
        if let Ok(ids) = crate::org::group_member_ids(state, gid).await {
            set.extend(ids);
        }
    }
    set.remove(&caller);
    if set.is_empty() {
        let _ = tx.send(CallServerMsg::Error { message: "sem destinatários".into() });
        return;
    }

    let title = title.unwrap_or_else(|| format!("Chamada de {caller_name}"));
    let room = match crate::rooms::insert_room(&state.db, caller, &title, "sfu", false, false).await {
        Ok(r) => r,
        Err(_) => {
            let _ = tx.send(CallServerMsg::Error { message: "não foi possível criar a sala".into() });
            return;
        }
    };
    state.presence.register_call(room.code.clone(), caller);

    let (mut ringing, mut offline) = (Vec::new(), Vec::new());
    for uid in set {
        let msg = CallServerMsg::IncomingCall {
            room_code: room.code.clone(),
            kind: kind.clone(),
            caller_id: caller,
            caller_name: caller_name.to_string(),
            title: title.clone(),
        };
        if state.presence.is_online(uid) && state.presence.send_to_user(uid, msg) {
            ringing.push(uid);
        } else {
            offline.push(uid);
            // Alvo offline: regista chamada perdida (vê ao voltar online).
            let _ = sqlx::query(
                "INSERT INTO missed_calls (user_id, room_code, caller_id, caller_name, kind)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(uid)
            .bind(&room.code)
            .bind(caller)
            .bind(caller_name)
            .bind(&kind)
            .execute(&state.db)
            .await;
        }
    }

    let _ = tx.send(CallServerMsg::Ringing { room_code: room.code, kind, ringing, offline });
}

// ---------- endpoints REST (ack de chamadas perdidas) ----------

/// Marca todas as chamadas perdidas do utilizador como vistas.
pub async fn ack_missed_calls(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    auth: crate::auth::AuthUser,
) -> Result<axum::Json<serde_json::Value>, crate::error::ApiError> {
    sqlx::query("UPDATE missed_calls SET seen = TRUE WHERE user_id = $1 AND NOT seen")
        .bind(auth.user_id)
        .execute(&state.db)
        .await?;
    Ok(axum::Json(serde_json::json!({ "ok": true })))
}
