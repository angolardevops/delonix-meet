use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::Response,
};
use dashmap::DashMap;
use futures_util::{stream::SplitStream, SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::{auth::verify_jwt, error::ApiError, AppState};

// ---------- Protocol ----------

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ClientMsg {
    Offer { to: Uuid, sdp: String },
    Answer { to: Uuid, sdp: String },
    Ice { to: Uuid, candidate: serde_json::Value },
    Chat { text: String },
    Reaction { emoji: String },
    Hand { raised: bool },
    /// Estado local de câmara/microfone — os outros mostram avatar/ícone
    /// em vez de vídeo preto ou indicador de som errado.
    Media { cam: bool, mic: bool },
    Recording { active: bool },
    Transcript { text: String },
    // Só o anfitrião:
    Admit { to: Uuid },
    Deny { to: Uuid },
    ForceMute { to: Uuid },
    Kick { to: Uuid },
    BreakoutsCreate { count: u32, #[serde(default)] minutes: Option<u32> },
    BreakoutRename { code: String, label: String },
    BreakoutAdd,
    /// Move um participante (identificado pelo username) para o grupo `code`
    /// — ou de volta à principal se `code` for o da sala-mãe.
    BreakoutMoveUser { name: String, code: String },
    BreakoutsClose,
    Leave,
    // SFU mode: SDP/ICE exchanged with the server itself, not another peer.
    SfuOffer { sdp: String },
    SfuAnswer { sdp: String },
    SfuIce { candidate: serde_json::Value },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ServerMsg {
    Joined { peer_id: Uuid, peers: Vec<PeerInfo> },
    PeerJoined { peer: PeerInfo },
    PeerLeft { peer_id: Uuid },
    Offer { from: Uuid, sdp: String },
    Answer { from: Uuid, sdp: String },
    Ice { from: Uuid, candidate: serde_json::Value },
    Chat { from: Uuid, username: String, text: String },
    Reaction { from: Uuid, username: String, emoji: String },
    Hand { from: Uuid, raised: bool },
    Media { from: Uuid, cam: bool, mic: bool },
    Recording { from: Uuid, username: String, active: bool },
    Transcript { from: Uuid, username: String, text: String },
    // Sala de espera:
    Waiting,                          // para o convidado: estás em espera
    WaitingJoin { peer: PeerInfo },   // para o anfitrião: alguém espera
    WaitingLeft { peer_id: Uuid },    // para o anfitrião: desistiu
    Denied,                           // para o convidado: entrada recusada
    // Controlo do anfitrião:
    ForceMuted,                       // para o alvo: foste silenciado
    Kicked,                           // para o alvo: foste removido
    // Breakout rooms:
    BreakoutMove { code: String, label: String, back: bool, ends_at: Option<i64> },
    BreakoutsCreated { rooms: Vec<BreakoutInfo>, ends_at: Option<i64> },
    Error { message: String },
    SfuOffer { sdp: String },
    SfuAnswer { sdp: String },
    SfuIce { candidate: serde_json::Value },
}

#[derive(Debug, Clone, Serialize)]
pub struct BreakoutInfo {
    pub code: String,
    pub label: String,
    /// Quem está agora neste grupo (usernames) — para o anfitrião mover pessoas.
    pub people: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BreakoutChild {
    pub id: Uuid,
    pub code: String,
    pub label: String,
}

/// Conjunto de salas de grupo ativas de uma sala principal.
pub struct BreakoutSet {
    pub parent_code: String,
    pub children: Vec<BreakoutChild>,
    /// Deadline (epoch segundos) do temporizador, se definido.
    pub ends_at: Option<i64>,
    /// Distingue gerações de grupos: um timer antigo não fecha grupos novos.
    pub nonce: Uuid,
}

#[derive(Debug, Clone, Serialize)]
pub struct PeerInfo {
    pub peer_id: Uuid,
    pub username: String,
    pub host: bool,
    #[serde(default)]
    pub hand: bool,
    /// Estado de media conhecido (true até o peer dizer o contrário).
    pub cam: bool,
    pub mic: bool,
}

// ---------- Hub (room registry, WS-agnostic and unit-testable) ----------

struct Peer {
    username: String,
    is_host: bool,
    hand: bool,
    cam_on: bool,
    mic_on: bool,
    tx: mpsc::UnboundedSender<ServerMsg>,
}

struct WaitingPeer {
    username: String,
    admit_tx: oneshot::Sender<bool>,
}

#[derive(Default)]
struct Room {
    peers: HashMap<Uuid, Peer>,
    waiting: HashMap<Uuid, WaitingPeer>,
}

#[derive(Default)]
pub struct SignalingHub {
    rooms: DashMap<Uuid, Room>,
}

impl SignalingHub {
    /// Adds a peer to a room. Announces it to the others and returns the
    /// current roster (excluding the new peer itself). Hosts also receive
    /// the queue of guests already waiting.
    pub fn join(
        &self,
        room_id: Uuid,
        peer_id: Uuid,
        username: String,
        is_host: bool,
        tx: mpsc::UnboundedSender<ServerMsg>,
    ) -> Vec<PeerInfo> {
        let mut room = self.rooms.entry(room_id).or_default();
        let existing: Vec<PeerInfo> = room
            .peers
            .iter()
            .map(|(id, p)| PeerInfo {
                peer_id: *id,
                username: p.username.clone(),
                host: p.is_host,
                hand: p.hand,
                cam: p.cam_on,
                mic: p.mic_on,
            })
            .collect();
        let announce = ServerMsg::PeerJoined {
            peer: PeerInfo { peer_id, username: username.clone(), host: is_host, hand: false, cam: true, mic: true },
        };
        for peer in room.peers.values() {
            let _ = peer.tx.send(announce.clone());
        }
        if is_host {
            for (id, w) in room.waiting.iter() {
                let _ = tx.send(ServerMsg::WaitingJoin {
                    peer: PeerInfo {
                        peer_id: *id,
                        username: w.username.clone(),
                        host: false,
                        hand: false,
                        cam: true,
                        mic: true,
                    },
                });
            }
        }
        room.peers.insert(peer_id, Peer { username, is_host, hand: false, cam_on: true, mic_on: true, tx });
        existing
    }

    /// Regista um convidado na fila de espera e avisa os anfitriões presentes.
    pub fn add_waiting(
        &self,
        room_id: Uuid,
        peer_id: Uuid,
        username: String,
        admit_tx: oneshot::Sender<bool>,
    ) {
        let mut room = self.rooms.entry(room_id).or_default();
        let info = PeerInfo { peer_id, username: username.clone(), host: false, hand: false, cam: true, mic: true };
        for peer in room.peers.values().filter(|p| p.is_host) {
            let _ = peer.tx.send(ServerMsg::WaitingJoin { peer: info.clone() });
        }
        room.waiting.insert(peer_id, WaitingPeer { username, admit_tx });
    }

    pub fn remove_waiting(&self, room_id: Uuid, peer_id: Uuid) {
        if let Some(mut room) = self.rooms.get_mut(&room_id) {
            if room.waiting.remove(&peer_id).is_some() {
                let msg = ServerMsg::WaitingLeft { peer_id };
                for peer in room.peers.values().filter(|p| p.is_host) {
                    let _ = peer.tx.send(msg.clone());
                }
            }
        }
    }

    /// Decisão do anfitrião sobre um convidado em espera.
    fn decide_waiting(&self, room_id: Uuid, host: Uuid, target: Uuid, admit: bool) {
        let Some(mut room) = self.rooms.get_mut(&room_id) else { return };
        if !room.peers.get(&host).map(|p| p.is_host).unwrap_or(false) {
            return; // só o anfitrião decide
        }
        if let Some(w) = room.waiting.remove(&target) {
            let _ = w.admit_tx.send(admit);
            let msg = ServerMsg::WaitingLeft { peer_id: target };
            for peer in room.peers.values().filter(|p| p.is_host) {
                let _ = peer.tx.send(msg.clone());
            }
        }
    }

    pub fn leave(&self, room_id: Uuid, peer_id: Uuid) {
        let mut empty = false;
        if let Some(mut room) = self.rooms.get_mut(&room_id) {
            if room.peers.remove(&peer_id).is_some() {
                let msg = ServerMsg::PeerLeft { peer_id };
                for peer in room.peers.values() {
                    let _ = peer.tx.send(msg.clone());
                }
            }
            empty = room.peers.is_empty() && room.waiting.is_empty();
        }
        if empty {
            self.rooms
                .remove_if(&room_id, |_, room| room.peers.is_empty() && room.waiting.is_empty());
        }
    }

    pub fn send_to(&self, room_id: Uuid, target: Uuid, msg: ServerMsg) -> bool {
        self.rooms
            .get(&room_id)
            .and_then(|room| room.peers.get(&target).map(|p| p.tx.send(msg).is_ok()))
            .unwrap_or(false)
    }

    pub fn broadcast(&self, room_id: Uuid, from: Uuid, msg: ServerMsg) {
        if let Some(room) = self.rooms.get(&room_id) {
            for (id, peer) in room.peers.iter() {
                if *id != from {
                    let _ = peer.tx.send(msg.clone());
                }
            }
        }
    }

    /// Envia a todos os presentes (incluindo anfitriões) — usado pelos breakouts.
    pub fn broadcast_all(&self, room_id: Uuid, msg: ServerMsg) {
        if let Some(room) = self.rooms.get(&room_id) {
            for peer in room.peers.values() {
                let _ = peer.tx.send(msg.clone());
            }
        }
    }

    /// Lista (peer_id, é_anfitrião) dos presentes na sala.
    /// Roster com usernames — identidade estável entre salas (o peer_id muda
    /// a cada ligação, o username não).
    pub fn roster_named(&self, room_id: Uuid) -> Vec<(Uuid, String, bool)> {
        self.rooms
            .get(&room_id)
            .map(|r| {
                r.peers
                    .iter()
                    .map(|(id, p)| (*id, p.username.clone(), p.is_host))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn roster(&self, room_id: Uuid) -> Vec<(Uuid, bool)> {
        self.rooms
            .get(&room_id)
            .map(|room| room.peers.iter().map(|(id, p)| (*id, p.is_host)).collect())
            .unwrap_or_default()
    }

    pub fn username_of(&self, room_id: Uuid, peer_id: Uuid) -> Option<String> {
        self.rooms
            .get(&room_id)
            .and_then(|room| room.peers.get(&peer_id).map(|p| p.username.clone()))
    }

    fn is_host(&self, room_id: Uuid, peer_id: Uuid) -> bool {
        self.rooms
            .get(&room_id)
            .and_then(|room| room.peers.get(&peer_id).map(|p| p.is_host))
            .unwrap_or(false)
    }

    pub fn room_size(&self, room_id: Uuid) -> usize {
        self.rooms.get(&room_id).map(|r| r.peers.len()).unwrap_or(0)
    }

    /// Routes one client message. Returns `false` when the peer asked to leave.
    pub fn handle(&self, room_id: Uuid, peer_id: Uuid, msg: ClientMsg) -> bool {
        // Mensagens de quem já não está na sala (ex.: expulso) são ignoradas.
        let in_room = self.username_of(room_id, peer_id).is_some();
        match msg {
            ClientMsg::Leave => return false,
            _ if !in_room => return true,
            ClientMsg::Offer { to, sdp } => {
                self.send_to(room_id, to, ServerMsg::Offer { from: peer_id, sdp });
            }
            ClientMsg::Answer { to, sdp } => {
                self.send_to(room_id, to, ServerMsg::Answer { from: peer_id, sdp });
            }
            ClientMsg::Ice { to, candidate } => {
                self.send_to(room_id, to, ServerMsg::Ice { from: peer_id, candidate });
            }
            ClientMsg::Chat { text } => {
                if text.is_empty() || text.len() > 4000 {
                    return true;
                }
                let username = self.username_of(room_id, peer_id).unwrap_or_else(|| "?".into());
                self.broadcast(room_id, peer_id, ServerMsg::Chat { from: peer_id, username, text });
            }
            ClientMsg::Reaction { emoji } => {
                // Só emojis curtos — nada de spam de texto por aqui.
                if emoji.is_empty() || emoji.chars().count() > 4 {
                    return true;
                }
                let username = self.username_of(room_id, peer_id).unwrap_or_else(|| "?".into());
                self.broadcast(room_id, peer_id, ServerMsg::Reaction { from: peer_id, username, emoji });
            }
            ClientMsg::Hand { raised } => {
                if let Some(mut room) = self.rooms.get_mut(&room_id) {
                    if let Some(p) = room.peers.get_mut(&peer_id) {
                        p.hand = raised;
                    }
                }
                self.broadcast(room_id, peer_id, ServerMsg::Hand { from: peer_id, raised });
            }
            ClientMsg::Media { cam, mic } => {
                if let Some(mut room) = self.rooms.get_mut(&room_id) {
                    if let Some(p) = room.peers.get_mut(&peer_id) {
                        p.cam_on = cam;
                        p.mic_on = mic;
                    }
                }
                self.broadcast(room_id, peer_id, ServerMsg::Media { from: peer_id, cam, mic });
            }
            ClientMsg::Recording { active } => {
                let username = self.username_of(room_id, peer_id).unwrap_or_else(|| "?".into());
                self.broadcast(room_id, peer_id, ServerMsg::Recording { from: peer_id, username, active });
            }
            ClientMsg::Transcript { text } => {
                // Frase final da fala do próprio microfone — difunde aos outros
                // para montarem a transcrição partilhada, legendada por orador.
                if text.is_empty() || text.len() > 4000 {
                    return true;
                }
                let username = self.username_of(room_id, peer_id).unwrap_or_else(|| "?".into());
                self.broadcast(room_id, peer_id, ServerMsg::Transcript { from: peer_id, username, text });
            }
            ClientMsg::Admit { to } => self.decide_waiting(room_id, peer_id, to, true),
            ClientMsg::Deny { to } => self.decide_waiting(room_id, peer_id, to, false),
            ClientMsg::ForceMute { to } => {
                if self.is_host(room_id, peer_id) {
                    self.send_to(room_id, to, ServerMsg::ForceMuted);
                }
            }
            ClientMsg::Kick { to } => {
                if self.is_host(room_id, peer_id) && to != peer_id {
                    self.send_to(room_id, to, ServerMsg::Kicked);
                    self.leave(room_id, to);
                }
            }
            // SFU e breakouts são tratados na camada de transporte (async/DB).
            ClientMsg::SfuOffer { .. }
            | ClientMsg::SfuAnswer { .. }
            | ClientMsg::SfuIce { .. }
            | ClientMsg::BreakoutsCreate { .. }
            | ClientMsg::BreakoutRename { .. }
            | ClientMsg::BreakoutAdd
            | ClientMsg::BreakoutMoveUser { .. }
            | ClientMsg::BreakoutsClose => {}
        }
        true
    }
}

// ---------- WebSocket transport ----------

#[derive(Deserialize)]
pub struct WsQuery {
    token: String,
}

pub async fn ws_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    // Only short-lived, room-scoped tokens open a signaling socket.
    let claims = verify_jwt(&state.config.jwt_secret, &query.token, "room")?;
    let room_id = claims.room.ok_or(ApiError::Unauthorized)?;
    let username = claims.name.unwrap_or_else(|| "anonymous".into());
    let sfu_mode = claims.topo.as_deref() == Some("sfu");
    let is_host = claims.owner;
    let must_wait = claims.wait && !claims.owner;
    let user_id = claims.sub;
    Ok(ws.on_upgrade(move |socket| {
        handle_socket(state, socket, room_id, user_id, username, sfu_mode, is_host, must_wait)
    }))
}

/// Cria uma sala-filha de grupo (herda topologia/E2EE da principal).
async fn breakout_new_child(
    state: &Arc<AppState>,
    owner: Uuid,
    parent_name: &str,
    topology: &str,
    e2ee: bool,
    label: &str,
) -> Option<BreakoutChild> {
    match crate::rooms::insert_room(&state.db, owner, &format!("{label} — {parent_name}"), topology, false, e2ee)
        .await
    {
        Ok(r) => Some(BreakoutChild { id: r.id, code: r.code, label: label.to_string() }),
        Err(e) => {
            tracing::warn!(error = %e, "breakout insert_room failed");
            None
        }
    }
}

/// Estado atual dos grupos (com quem está em cada um) para os anfitriões
/// na sala principal — chamado após qualquer mudança.
fn broadcast_breakout_state(state: &Arc<AppState>, parent_id: Uuid) {
    let Some(set) = state.breakouts.get(&parent_id) else {
        state.hub.broadcast_all(parent_id, ServerMsg::BreakoutsCreated { rooms: vec![], ends_at: None });
        return;
    };
    let infos: Vec<BreakoutInfo> = set
        .children
        .iter()
        .map(|c| BreakoutInfo {
            code: c.code.clone(),
            label: c.label.clone(),
            people: state.hub.roster_named(c.id).into_iter().map(|(_, n, _)| n).collect(),
        })
        .collect();
    let ends_at = set.ends_at;
    for (id, host) in state.hub.roster(parent_id) {
        if host {
            state
                .hub
                .send_to(parent_id, id, ServerMsg::BreakoutsCreated { rooms: infos.clone(), ends_at });
        }
    }
}

/// Se `room_id` for filha de um conjunto de grupos, devolve o id da sala-mãe.
fn breakout_parent_of(state: &Arc<AppState>, room_id: Uuid) -> Option<Uuid> {
    state
        .breakouts
        .iter()
        .find(|e| e.value().children.iter().any(|c| c.id == room_id))
        .map(|e| *e.key())
}

/// Anfitrião pediu salas de grupo: cria N salas, distribui os participantes
/// round-robin, avisa os anfitriões e (opcional) agenda o retorno automático.
async fn breakouts_create(state: &Arc<AppState>, room_id: Uuid, owner: Uuid, count: u32, minutes: Option<u32>) {
    let count = count.clamp(2, 8) as usize;
    let parent: Option<(String, String, bool, String)> = sqlx::query_as(
        "SELECT code, topology, e2ee, name FROM rooms WHERE id = $1",
    )
    .bind(room_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    let Some((parent_code, topology, e2ee, name)) = parent else { return };

    let mut children: Vec<BreakoutChild> = Vec::new();
    for i in 1..=count {
        match breakout_new_child(state, owner, &name, &topology, e2ee, &format!("Grupo {i}")).await {
            Some(c) => children.push(c),
            None => return,
        }
    }

    let ends_at = minutes
        .filter(|m| *m > 0)
        .map(|m| chrono::Utc::now().timestamp() + (m.min(180) as i64) * 60);
    let nonce = Uuid::new_v4();

    // Distribuir os não-anfitriões pelas salas, à vez.
    let guests: Vec<Uuid> = state
        .hub
        .roster(room_id)
        .into_iter()
        .filter(|(_, host)| !host)
        .map(|(id, _)| id)
        .collect();
    for (idx, peer) in guests.iter().enumerate() {
        let c = &children[idx % children.len()];
        state.hub.send_to(
            room_id,
            *peer,
            ServerMsg::BreakoutMove { code: c.code.clone(), label: c.label.clone(), back: false, ends_at },
        );
    }

    state
        .breakouts
        .insert(room_id, BreakoutSet { parent_code, children, ends_at, nonce });
    broadcast_breakout_state(state, room_id);

    // Temporizador: no fim, devolve todos à principal (se esta geração ainda existir).
    if let Some(deadline) = ends_at {
        let state = state.clone();
        tokio::spawn(async move {
            let wait = (deadline - chrono::Utc::now().timestamp()).max(0) as u64;
            tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
            let still_current = state
                .breakouts
                .get(&room_id)
                .map(|s| s.nonce == nonce)
                .unwrap_or(false);
            if still_current {
                tracing::info!(%room_id, "breakout timer expired — returning everyone");
                breakouts_close(&state, room_id);
            }
        });
    }
    tracing::info!(%room_id, count, ?minutes, "breakout rooms created");
}

/// Renomeia um grupo existente.
fn breakout_rename(state: &Arc<AppState>, room_id: Uuid, code: &str, label: &str) {
    let label = label.trim();
    if label.is_empty() || label.len() > 60 {
        return;
    }
    if let Some(mut set) = state.breakouts.get_mut(&room_id) {
        if let Some(c) = set.children.iter_mut().find(|c| c.code == code) {
            c.label = label.to_string();
        }
    }
    broadcast_breakout_state(state, room_id);
}

/// Acrescenta mais um grupo ao conjunto ativo.
async fn breakout_add(state: &Arc<AppState>, room_id: Uuid, owner: Uuid) {
    let parent: Option<(String, String, bool, String)> = sqlx::query_as(
        "SELECT code, topology, e2ee, name FROM rooms WHERE id = $1",
    )
    .bind(room_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    let Some((_, topology, e2ee, name)) = parent else { return };
    let n = state.breakouts.get(&room_id).map(|s| s.children.len()).unwrap_or(0);
    if n == 0 || n >= 12 {
        return;
    }
    if let Some(child) = breakout_new_child(state, owner, &name, &topology, e2ee, &format!("Grupo {}", n + 1)).await {
        if let Some(mut set) = state.breakouts.get_mut(&room_id) {
            set.children.push(child);
        }
        broadcast_breakout_state(state, room_id);
    }
}

/// Move um participante (por username) para outro grupo ou de volta à
/// principal. Procura-o na principal e em todas as filhas.
fn breakout_move_user(state: &Arc<AppState>, room_id: Uuid, name: &str, target_code: &str) {
    let Some(set) = state.breakouts.get(&room_id) else { return };
    let back = target_code == set.parent_code;
    let label = if back {
        "Sala principal".to_string()
    } else {
        match set.children.iter().find(|c| c.code == target_code) {
            Some(c) => c.label.clone(),
            None => return,
        }
    };
    let ends_at = set.ends_at;
    // Salas onde ele pode estar: principal + todas as filhas.
    let mut locations: Vec<Uuid> = vec![room_id];
    locations.extend(set.children.iter().map(|c| c.id));
    drop(set);
    for loc in locations {
        if let Some((peer, _, _)) = state
            .hub
            .roster_named(loc)
            .into_iter()
            .find(|(_, n, host)| n == name && !host)
        {
            state.hub.send_to(
                loc,
                peer,
                ServerMsg::BreakoutMove { code: target_code.to_string(), label, back, ends_at },
            );
            return;
        }
    }
}

/// Fecha os grupos: devolve toda a gente à sala principal.
fn breakouts_close(state: &Arc<AppState>, room_id: Uuid) {
    if let Some((_, set)) = state.breakouts.remove(&room_id) {
        for c in &set.children {
            state.hub.broadcast_all(
                c.id,
                ServerMsg::BreakoutMove {
                    code: set.parent_code.clone(),
                    label: "Sala principal".into(),
                    back: true,
                    ends_at: None,
                },
            );
        }
        // Anfitriões na sala principal limpam a lista.
        state
            .hub
            .broadcast_all(room_id, ServerMsg::BreakoutsCreated { rooms: vec![], ends_at: None });
        tracing::info!(%room_id, "breakout rooms closed");
    }
}

/// Consome o socket até fechar (ou o cliente desistir) enquanto espera admissão.
async fn drain_while_waiting(stream: &mut SplitStream<WebSocket>) {
    while let Some(Ok(msg)) = stream.next().await {
        match msg {
            Message::Close(_) => break,
            Message::Text(text) => {
                if matches!(serde_json::from_str::<ClientMsg>(&text), Ok(ClientMsg::Leave)) {
                    break;
                }
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_socket(
    state: Arc<AppState>,
    socket: WebSocket,
    room_id: Uuid,
    user_id: Uuid,
    username: String,
    sfu_mode: bool,
    is_host: bool,
    must_wait: bool,
) {
    let peer_id = Uuid::new_v4();
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMsg>();

    // Outbound: hub -> websocket.
    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let text = match serde_json::to_string(&msg) {
                Ok(t) => t,
                Err(_) => continue,
            };
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    // Sala de espera: convidados aguardam a decisão do anfitrião.
    if must_wait {
        let (admit_tx, admit_rx) = oneshot::channel::<bool>();
        state.hub.add_waiting(room_id, peer_id, username.clone(), admit_tx);
        let _ = tx.send(ServerMsg::Waiting);
        tracing::info!(%room_id, %peer_id, %username, "guest waiting for admission");

        let admitted = tokio::select! {
            decision = admit_rx => decision.unwrap_or(false),
            _ = drain_while_waiting(&mut stream) => {
                // Desistiu/desligou enquanto esperava.
                state.hub.remove_waiting(room_id, peer_id);
                writer.abort();
                return;
            }
        };
        if !admitted {
            let _ = tx.send(ServerMsg::Denied);
            tracing::info!(%room_id, %peer_id, "guest denied");
            // Dá tempo ao writer de entregar a mensagem antes de fechar.
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            writer.abort();
            return;
        }
    }

    let peers = state.hub.join(room_id, peer_id, username.clone(), is_host, tx.clone());
    let _ = tx.send(ServerMsg::Joined { peer_id, peers });
    if sfu_mode {
        if let Err(e) = state.sfu.add_peer(room_id, peer_id, tx.clone()).await {
            tracing::error!(%room_id, %peer_id, error = %e, "sfu add_peer failed");
            let _ = tx.send(ServerMsg::Error { message: "sfu unavailable".into() });
        }
    }
    tracing::info!(%room_id, %peer_id, %username, sfu = sfu_mode, host = is_host, "peer joined");
    // Entrou numa sala de grupo? Atualiza o mapa de grupos dos anfitriões.
    if let Some(parent) = breakout_parent_of(&state, room_id) {
        broadcast_breakout_state(&state, parent);
    }

    // Inbound: websocket -> hub.
    while let Some(Ok(msg)) = stream.next().await {
        match msg {
            Message::Text(text) => match serde_json::from_str::<ClientMsg>(&text) {
                Ok(msg @ (ClientMsg::SfuOffer { .. } | ClientMsg::SfuAnswer { .. } | ClientMsg::SfuIce { .. }))
                    if sfu_mode =>
                {
                    if let Err(e) = state.sfu.on_client_msg(room_id, peer_id, msg).await {
                        tracing::warn!(%room_id, %peer_id, error = %e, "sfu message failed");
                    }
                }
                Ok(ClientMsg::BreakoutsCreate { count, minutes }) if is_host => {
                    breakouts_create(&state, room_id, user_id, count, minutes).await;
                }
                Ok(ClientMsg::BreakoutRename { code, label }) if is_host => {
                    breakout_rename(&state, room_id, &code, &label);
                }
                Ok(ClientMsg::BreakoutAdd) if is_host => {
                    breakout_add(&state, room_id, user_id).await;
                }
                Ok(ClientMsg::BreakoutMoveUser { name, code }) if is_host => {
                    breakout_move_user(&state, room_id, &name, &code);
                }
                Ok(ClientMsg::BreakoutsClose) if is_host => {
                    breakouts_close(&state, room_id);
                }
                Ok(client_msg) => {
                    if !state.hub.handle(room_id, peer_id, client_msg) {
                        break;
                    }
                }
                Err(_) => {
                    let _ = tx.send(ServerMsg::Error { message: "invalid message".into() });
                }
            },
            Message::Close(_) => break,
            _ => {}
        }
    }

    if sfu_mode {
        state.sfu.remove_peer(room_id, peer_id).await;
    }
    state.hub.leave(room_id, peer_id);
    if let Some(parent) = breakout_parent_of(&state, room_id) {
        broadcast_breakout_state(&state, parent);
    }
    writer.abort();
    tracing::info!(%room_id, %peer_id, "peer left");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer() -> (Uuid, mpsc::UnboundedSender<ServerMsg>, mpsc::UnboundedReceiver<ServerMsg>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Uuid::new_v4(), tx, rx)
    }

    fn drain(rx: &mut mpsc::UnboundedReceiver<ServerMsg>) {
        while rx.try_recv().is_ok() {}
    }

    #[tokio::test]
    async fn join_announces_and_returns_roster() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let (a, tx_a, mut rx_a) = peer();
        let (b, tx_b, _rx_b) = peer();

        let roster_a = hub.join(room, a, "alice".into(), true, tx_a);
        assert!(roster_a.is_empty());

        let roster_b = hub.join(room, b, "bob".into(), false, tx_b);
        assert_eq!(roster_b.len(), 1);
        assert_eq!(roster_b[0].peer_id, a);
        assert_eq!(roster_b[0].username, "alice");
        assert!(roster_b[0].host, "alice é anfitriã");

        match rx_a.recv().await.unwrap() {
            ServerMsg::PeerJoined { peer } => {
                assert_eq!(peer.peer_id, b);
                assert!(!peer.host);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn offer_is_routed_to_target_only() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let (a, tx_a, _rx_a) = peer();
        let (b, tx_b, mut rx_b) = peer();
        let (c, tx_c, mut rx_c) = peer();
        hub.join(room, a, "a".into(), false, tx_a);
        hub.join(room, b, "b".into(), false, tx_b);
        hub.join(room, c, "c".into(), false, tx_c);
        drain(&mut rx_b);
        drain(&mut rx_c);

        hub.handle(room, a, ClientMsg::Offer { to: b, sdp: "sdp-offer".into() });

        match rx_b.recv().await.unwrap() {
            ServerMsg::Offer { from, sdp } => {
                assert_eq!(from, a);
                assert_eq!(sdp, "sdp-offer");
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert!(rx_c.try_recv().is_err(), "c must not receive a targeted offer");
    }

    #[tokio::test]
    async fn chat_broadcasts_to_everyone_else() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let (a, tx_a, mut rx_a) = peer();
        let (b, tx_b, mut rx_b) = peer();
        hub.join(room, a, "alice".into(), false, tx_a);
        hub.join(room, b, "bob".into(), false, tx_b);
        drain(&mut rx_a);

        hub.handle(room, b, ClientMsg::Chat { text: "olá!".into() });

        match rx_a.recv().await.unwrap() {
            ServerMsg::Chat { from, username, text } => {
                assert_eq!(from, b);
                assert_eq!(username, "bob");
                assert_eq!(text, "olá!");
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert!(rx_b.try_recv().is_err(), "sender must not echo its own chat");
    }

    #[tokio::test]
    async fn leave_notifies_and_cleans_empty_rooms() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let (a, tx_a, mut rx_a) = peer();
        let (b, tx_b, _rx_b) = peer();
        hub.join(room, a, "a".into(), false, tx_a);
        hub.join(room, b, "b".into(), false, tx_b);
        drain(&mut rx_a);

        hub.leave(room, b);
        match rx_a.recv().await.unwrap() {
            ServerMsg::PeerLeft { peer_id } => assert_eq!(peer_id, b),
            other => panic!("unexpected: {other:?}"),
        }
        assert_eq!(hub.room_size(room), 1);

        hub.leave(room, a);
        assert_eq!(hub.room_size(room), 0);
        assert!(hub.rooms.is_empty(), "empty rooms must be garbage-collected");
    }

    #[tokio::test]
    async fn rooms_are_isolated() {
        let hub = SignalingHub::default();
        let room1 = Uuid::new_v4();
        let room2 = Uuid::new_v4();
        let (a, tx_a, _rx_a) = peer();
        let (b, tx_b, mut rx_b) = peer();
        hub.join(room1, a, "a".into(), false, tx_a);
        hub.join(room2, b, "b".into(), false, tx_b);

        hub.handle(room1, a, ClientMsg::Chat { text: "room1 only".into() });
        assert!(rx_b.try_recv().is_err(), "messages must never cross rooms");
        assert!(!hub.send_to(room1, b, ServerMsg::PeerLeft { peer_id: a }));
    }

    #[tokio::test]
    async fn waiting_guest_is_admitted_by_host_only() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let (host, tx_h, mut rx_h) = peer();
        let (other, tx_o, _rx_o) = peer();
        hub.join(room, host, "host".into(), true, tx_h);
        hub.join(room, other, "other".into(), false, tx_o);
        drain(&mut rx_h);

        let guest = Uuid::new_v4();
        let (admit_tx, mut admit_rx) = oneshot::channel();
        hub.add_waiting(room, guest, "guest".into(), admit_tx);

        // Anfitrião é notificado.
        match rx_h.recv().await.unwrap() {
            ServerMsg::WaitingJoin { peer } => assert_eq!(peer.peer_id, guest),
            other => panic!("unexpected: {other:?}"),
        }

        // Um não-anfitrião a tentar admitir é ignorado.
        hub.handle(room, other, ClientMsg::Admit { to: guest });
        assert!(admit_rx.try_recv().is_err(), "non-host must not admit");

        // O anfitrião admite.
        hub.handle(room, host, ClientMsg::Admit { to: guest });
        assert_eq!(admit_rx.await, Ok(true));
    }

    #[tokio::test]
    async fn waiting_guest_can_be_denied() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let (host, tx_h, mut rx_h) = peer();
        hub.join(room, host, "host".into(), true, tx_h);
        drain(&mut rx_h);

        let guest = Uuid::new_v4();
        let (admit_tx, admit_rx) = oneshot::channel();
        hub.add_waiting(room, guest, "guest".into(), admit_tx);

        hub.handle(room, host, ClientMsg::Deny { to: guest });
        assert_eq!(admit_rx.await, Ok(false));
    }

    #[tokio::test]
    async fn host_joining_later_sees_waiting_queue() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let guest = Uuid::new_v4();
        let (admit_tx, _admit_rx) = oneshot::channel();
        hub.add_waiting(room, guest, "guest".into(), admit_tx);

        let (host, tx_h, mut rx_h) = peer();
        hub.join(room, host, "host".into(), true, tx_h);
        match rx_h.recv().await.unwrap() {
            ServerMsg::WaitingJoin { peer } => assert_eq!(peer.peer_id, guest),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn reaction_and_hand_are_broadcast() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let (a, tx_a, mut rx_a) = peer();
        let (b, tx_b, _rx_b) = peer();
        hub.join(room, a, "alice".into(), false, tx_a);
        hub.join(room, b, "bob".into(), false, tx_b);
        drain(&mut rx_a);

        hub.handle(room, b, ClientMsg::Reaction { emoji: "🎉".into() });
        match rx_a.recv().await.unwrap() {
            ServerMsg::Reaction { username, emoji, .. } => {
                assert_eq!(username, "bob");
                assert_eq!(emoji, "🎉");
            }
            other => panic!("unexpected: {other:?}"),
        }

        hub.handle(room, b, ClientMsg::Hand { raised: true });
        match rx_a.recv().await.unwrap() {
            ServerMsg::Hand { from, raised } => {
                assert_eq!(from, b);
                assert!(raised);
            }
            other => panic!("unexpected: {other:?}"),
        }
        // Estado da mão fica no roster para quem entrar depois.
        let (c, tx_c, _rx_c) = peer();
        let roster = hub.join(room, c, "carol".into(), false, tx_c);
        let bob = roster.iter().find(|p| p.peer_id == b).unwrap();
        assert!(bob.hand);
    }

    #[tokio::test]
    async fn host_can_mute_and_kick_but_others_cannot() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let (host, tx_h, _rx_h) = peer();
        let (a, tx_a, mut rx_a) = peer();
        let (b, tx_b, mut rx_b) = peer();
        hub.join(room, host, "host".into(), true, tx_h);
        hub.join(room, a, "a".into(), false, tx_a);
        hub.join(room, b, "b".into(), false, tx_b);
        drain(&mut rx_a);
        drain(&mut rx_b);

        // Não-anfitrião não silencia ninguém.
        hub.handle(room, a, ClientMsg::ForceMute { to: b });
        assert!(rx_b.try_recv().is_err());

        hub.handle(room, host, ClientMsg::ForceMute { to: b });
        assert!(matches!(rx_b.recv().await.unwrap(), ServerMsg::ForceMuted));

        hub.handle(room, host, ClientMsg::Kick { to: b });
        assert!(matches!(rx_b.recv().await.unwrap(), ServerMsg::Kicked));
        assert_eq!(hub.room_size(room), 2, "expulso sai da sala");
        // O kick difunde peer-left aos restantes.
        assert!(matches!(rx_a.recv().await.unwrap(), ServerMsg::PeerLeft { .. }));
        // Mensagens do expulso passam a ser ignoradas.
        hub.handle(room, b, ClientMsg::Chat { text: "ainda cá estou?".into() });
        assert!(rx_a.try_recv().is_err());
    }
}
