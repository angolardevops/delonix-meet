//! Presença e chamadas estilo WhatsApp.
//!
//! Cada utilizador abre um WebSocket pessoal (`/rtc?token=<access_token>`)
//! que fica ligado enquanto a app está aberta. Ao iniciar uma chamada, o
//! servidor cria a sala e faz "tocar" em todos os alvos online; quem aceita
//! entra na mesma sala (via o fluxo normal de `join_room`).

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::Response,
};
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{auth::verify_jwt, error::ApiError, pubsub::PubSubBus, AppState};

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
    CallAccept {
        room_code: String,
    },
    CallDecline {
        room_code: String,
    },
    CallCancel {
        room_code: String,
    },
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    Ringing {
        room_code: String,
        kind: String,
        ringing: Vec<Uuid>,
        offline: Vec<Uuid>,
    },
    /// Para o chamador: alguém aceitou.
    Accepted {
        room_code: String,
        by_id: Uuid,
        by_name: String,
    },
    /// Para o chamador: alguém recusou.
    Declined {
        room_code: String,
        by_id: Uuid,
        by_name: String,
    },
    /// Para os alvos: o chamador cancelou / a chamada terminou de tocar.
    Cancelled {
        room_code: String,
    },
    /// Lista de contactos atualmente online (user_ids).
    Presence {
        online: Vec<Uuid>,
    },
    /// Um colega acabou de entrar online (atualização incremental).
    UserOnline {
        user_id: Uuid,
    },
    /// Um colega saiu (atualização incremental).
    UserOffline {
        user_id: Uuid,
    },
    /// Ao ligar: chamadas perdidas (recebidas enquanto estava offline).
    MissedCalls {
        calls: Vec<MissedCall>,
    },
    /// Para o anfitrião: um convidado recusou uma reunião (com motivo).
    MeetingDeclined {
        meeting_id: Uuid,
        meeting_title: String,
        by_id: Uuid,
        by_name: String,
        reason: String,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
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

pub struct ActiveCall {
    pub caller: Uuid,
    pub targets: std::collections::HashSet<Uuid>,
}

pub struct PresenceHub {
    /// user_id -> ligações ativas (pode ter vários dispositivos).
    conns: DashMap<Uuid, Vec<Conn>>,
    /// room_code -> dados da chamada ativa (para encaminhar accept/decline e permitir entrada direta).
    pub calls: DashMap<String, ActiveCall>,
    /// Bus Redis para entrega cross-nó. None em modo single-node.
    pub bus: Option<Arc<PubSubBus>>,
}

impl Default for PresenceHub {
    fn default() -> Self {
        Self {
            conns: DashMap::new(),
            calls: DashMap::new(),
            bus: None,
        }
    }
}

impl PresenceHub {
    fn add(&self, user_id: Uuid, tx: mpsc::UnboundedSender<CallServerMsg>, username: String) {
        let is_first = !self.is_online_local(user_id);
        self.conns
            .entry(user_id)
            .or_default()
            .push(Conn { tx, username });
        // Registar no Redis apenas na primeira ligação deste utilizador neste nó.
        if is_first {
            if let Some(bus) = &self.bus {
                let bus = bus.clone();
                tokio::spawn(async move { bus.set_online(user_id).await });
            }
        }
    }

    fn remove(&self, user_id: Uuid, tx: &mpsc::UnboundedSender<CallServerMsg>) {
        if let Some(mut v) = self.conns.get_mut(&user_id) {
            v.retain(|c| !c.tx.same_channel(tx));
        }
        self.conns.remove_if(&user_id, |_, v| v.is_empty());
        // Se saiu do último canal local, remover do conjunto Redis.
        if !self.is_online_local(user_id) {
            if let Some(bus) = &self.bus {
                let bus = bus.clone();
                tokio::spawn(async move { bus.set_offline(user_id).await });
            }
        }
    }

    /// Online neste nó (verificação local, sem I/O).
    pub fn is_online_local(&self, user_id: Uuid) -> bool {
        self.conns
            .get(&user_id)
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    }

    /// Online neste nó — mantido por compatibilidade com chamadas locais.
    pub fn is_online(&self, user_id: Uuid) -> bool {
        self.is_online_local(user_id)
    }

    /// Online em QUALQUER nó: verifica local primeiro (rápido), depois Redis.
    pub async fn is_online_any(&self, user_id: Uuid) -> bool {
        if self.is_online_local(user_id) {
            return true;
        }
        if let Some(bus) = &self.bus {
            return bus.is_online(user_id).await;
        }
        false
    }

    fn online_ids(&self) -> Vec<Uuid> {
        self.conns.iter().map(|e| *e.key()).collect()
    }

    /// Entrega localmente a todas as ligações do utilizador neste nó.
    /// Devolve true se entregou a pelo menos uma ligação.
    pub fn send_to_user(&self, user_id: Uuid, msg: CallServerMsg) -> bool {
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

    /// Entrega localmente; se não entregue, publica no Redis para outros nós.
    pub fn send_or_publish(&self, user_id: Uuid, msg: CallServerMsg) -> bool {
        let delivered = self.send_to_user(user_id, msg.clone());
        if !delivered {
            if let Some(bus) = &self.bus {
                let bus = bus.clone();
                tokio::spawn(async move { bus.publish(user_id, &msg).await });
            }
        }
        delivered
    }

    pub fn register_call(
        &self,
        room_code: String,
        caller: Uuid,
        targets: std::collections::HashSet<Uuid>,
    ) {
        // Acumula targets em vez de substituir: convites sequenciais para a
        // mesma sala não anulam os anteriores (evita falso "sala de espera").
        match self.calls.get_mut(&room_code) {
            Some(mut entry) => entry.targets.extend(targets),
            None => { self.calls.insert(room_code, ActiveCall { caller, targets }); }
        }
    }

    /// Envia uma mensagem de servidor a um utilizador (usado por outros módulos).
    /// Tenta entrega local; se falhar, publica no Redis.
    pub fn notify(&self, user_id: Uuid, msg: CallServerMsg) {
        self.send_or_publish(user_id, msg);
    }

    pub fn caller_of(&self, room_code: &str) -> Option<Uuid> {
        self.calls.get(room_code).map(|c| c.caller)
    }

    pub fn is_invited(&self, room_code: &str, user_id: Uuid) -> bool {
        self.calls
            .get(room_code)
            .map(|c| c.targets.contains(&user_id))
            .unwrap_or(false)
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

/// RAII guard: garante que presence.remove() é chamado mesmo se o future
/// for dropped (ex: servidor a desligar, timeout de ligação).
struct PresenceGuard {
    state: Arc<AppState>,
    user_id: Uuid,
    tx: tokio::sync::mpsc::UnboundedSender<CallServerMsg>,
    writer: tokio::task::JoinHandle<()>,
}

impl Drop for PresenceGuard {
    fn drop(&mut self) {
        self.state.presence.remove(self.user_id, &self.tx);
        self.writer.abort();
        // Se saiu do último canal local, notificar colegas online.
        if !self.state.presence.is_online_local(self.user_id) {
            let state = self.state.clone();
            let user_id = self.user_id;
            tokio::spawn(async move {
                let co_members = crate::org::org_co_members(&state, user_id).await;
                for uid in co_members {
                    state.presence.send_or_publish(uid, CallServerMsg::UserOffline { user_id });
                }
            });
        }
    }
}

async fn handle(state: Arc<AppState>, socket: WebSocket, user_id: Uuid, username: String) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<CallServerMsg>();

    state.presence.add(user_id, tx.clone(), username.clone());
    tracing::info!(%user_id, %username, "presence connected");

    // Estado inicial: quem está online em QUALQUER nó — SÓ colegas da mesma org.
    let co_members = crate::org::org_co_members(&state, user_id).await;
    let mut online = Vec::with_capacity(co_members.len());
    for uid in &co_members {
        if state.presence.is_online_any(*uid).await {
            online.push(*uid);
        }
    }
    let _ = tx.send(CallServerMsg::Presence { online });

    // Notificar os colegas online que este utilizador acabou de entrar
    // (atualização incremental — cada nó entrega ao WS local ou publica no Redis).
    for uid in &co_members {
        state.presence.send_or_publish(*uid, CallServerMsg::UserOnline { user_id });
    }

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

    // RAII: garante cleanup mesmo que o future seja dropped a meio.
    let _guard = PresenceGuard {
        state: state.clone(),
        user_id,
        tx: tx.clone(),
        writer,
    };

    // Rate-limit por socket (anti-flood): pico anormal desliga o cliente.
    let mut rl_window = std::time::Instant::now();
    let mut rl_count: u32 = 0;
    while let Some(Ok(msg)) = stream.next().await {
        if rl_window.elapsed() >= std::time::Duration::from_secs(1) {
            rl_window = std::time::Instant::now();
            rl_count = 0;
        }
        rl_count += 1;
        if rl_count > 40 {
            tracing::warn!(%user_id, "flood de mensagens de presença — a desligar");
            break;
        }
        let Message::Text(text) = msg else {
            if matches!(msg, Message::Close(_)) { break }
            continue;
        };
        let Ok(cmd) = serde_json::from_str::<CallClientMsg>(&text) else { continue };
        match cmd {
            CallClientMsg::Ping => {}
            CallClientMsg::CallStart {
                targets,
                group_id,
                kind,
                title,
            } => {
                handle_call_start(
                    &state, user_id, &username, targets, group_id, kind, title, &tx,
                )
                .await;
            }
            CallClientMsg::CallAccept { room_code } => {
                if let Some(caller) = state.presence.caller_of(&room_code) {
                    state.presence.send_or_publish(
                        caller,
                        CallServerMsg::Accepted {
                            room_code,
                            by_id: user_id,
                            by_name: username.clone(),
                        },
                    );
                }
            }
            CallClientMsg::CallDecline { room_code } => {
                if let Some(caller) = state.presence.caller_of(&room_code) {
                    state.presence.send_or_publish(
                        caller,
                        CallServerMsg::Declined {
                            room_code,
                            by_id: user_id,
                            by_name: username.clone(),
                        },
                    );
                }
            }
            CallClientMsg::CallCancel { room_code } => {
                // Só o chamador cancela; difunde "cancelado" a todos os locais
                // e publica no Redis para nós remotos.
                if state.presence.caller_of(&room_code) == Some(user_id) {
                    state.presence.calls.remove(&room_code);
                    for id in state.presence.online_ids() {
                        state.presence.send_or_publish(
                            id,
                            CallServerMsg::Cancelled {
                                room_code: room_code.clone(),
                            },
                        );
                    }
                }
            }
        }
    }

    // _guard dropped aqui: chama presence.remove() + writer.abort() automaticamente.
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
        let _ = tx.send(CallServerMsg::Error {
            message: "tipo de chamada inválido".into(),
        });
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
    // Isolamento multi-tenant: só se pode tocar em colegas da(s) mesma(s) org(s)
    // — impede spam/phishing cross-org e criação de sala com alvos arbitrários.
    let co: std::collections::HashSet<Uuid> = crate::org::org_co_members(state, caller)
        .await
        .into_iter()
        .collect();
    set.retain(|u| co.contains(u));
    if set.is_empty() {
        let _ = tx.send(CallServerMsg::Error {
            message: "sem destinatários válidos".into(),
        });
        return;
    }

    let title = title.unwrap_or_else(|| format!("Chamada de {caller_name}"));
    let room =
        match crate::rooms::insert_room(&state.db, caller, &title, "sfu", false, false, "normal")
            .await
        {
            Ok(r) => r,
            Err(_) => {
                let _ = tx.send(CallServerMsg::Error {
                    message: "não foi possível criar a sala".into(),
                });
                return;
            }
        };
    state
        .presence
        .register_call(room.code.clone(), caller, set.clone());

    let (ringing, offline) =
        ring_users(state, caller, caller_name, set, &room.code, &kind, &title).await;
    let _ = tx.send(CallServerMsg::Ringing {
        room_code: room.code,
        kind,
        ringing,
        offline,
    });
}

/// Faz tocar uma chamada nos alvos: online recebem o ring (UI de chamada a
/// tocar); offline ficam com chamada perdida. Usado nas chamadas diretas/de
/// grupo E ao iniciar reuniões agendadas (estilo Teams: a reunião começou
/// no canal → os convidados são "despertados").
pub async fn ring_users(
    state: &Arc<AppState>,
    caller: Uuid,
    caller_name: &str,
    targets: std::collections::HashSet<Uuid>,
    room_code: &str,
    kind: &str,
    title: &str,
) -> (Vec<Uuid>, Vec<Uuid>) {
    // Pré-alocar strings uma vez fora do loop — dentro seria N alocações por campo.
    let room_code_s = room_code.to_string();
    let kind_s = kind.to_string();
    let caller_name_s = caller_name.to_string();
    let title_s = title.to_string();

    let (mut ringing, mut offline) = (Vec::new(), Vec::new());
    for uid in targets {
        if uid == caller {
            continue;
        }
        let msg = CallServerMsg::IncomingCall {
            room_code: room_code_s.clone(),
            kind: kind_s.clone(),
            caller_id: caller,
            caller_name: caller_name_s.clone(),
            title: title_s.clone(),
        };
        if state.presence.is_online_any(uid).await && state.presence.send_or_publish(uid, msg) {
            ringing.push(uid);
        } else {
            offline.push(uid);
            // Alvo offline: regista chamada perdida (vê ao voltar online).
            let _ = sqlx::query(
                "INSERT INTO missed_calls (user_id, room_code, caller_id, caller_name, kind)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(uid)
            .bind(room_code)
            .bind(caller)
            .bind(caller_name)
            .bind(kind)
            .execute(&state.db)
            .await;
        }
    }
    (ringing, offline)
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
