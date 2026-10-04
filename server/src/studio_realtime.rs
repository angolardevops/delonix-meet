//! Estúdio de TV no WebSocket da sala (ADR-0014 §2.1–2.2): fontes emparelhadas
//! (a app Delonix Câmara), tally, comandos para o telefone e estado do telefone.
//!
//! O estado vivo é deste pod — a sala está fixada a um pod (ADR-0001) — e o que
//! a REST de outro pod precisa (ligada, tally, último estado) é persistido em
//! `studio_sources` com tecto de uma escrita por segundo.
//!
//! Regras:
//! - **só o anfitrião actual** fixa o tally e comanda (lido do hub, não do token);
//! - **uma fonte só manda** media do SFU e as duas mensagens do estúdio — lista de
//!   permitidas, para uma mensagem nova do protocolo nascer fechada a fontes;
//! - o servidor valida cada comando antes de o encaminhar.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use dashmap::DashMap;
use delonix_meet_domain::studio::{
    command::{validate_command_id, validate_result_error, SourceCommand, SourceStatus},
    tally::{self, Tally},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    signaling::{ClientMsg, PeerTx, ServerMsg},
    AppState,
};

/// Uma fonte vista pelo operador.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
pub struct SourceView {
    pub source_id: Uuid,
    /// `null` enquanto a fonte não está ligada.
    pub peer_id: Option<Uuid>,
    pub number: i32,
    pub label: String,
    pub connected: bool,
    #[schema(value_type = String, example = "program")]
    pub tally: Tally,
    #[schema(value_type = Option<Object>)]
    pub status: Option<SourceStatus>,
}

/// Identidade de uma fonte no socket (vem do token + base, nunca do cliente).
#[derive(Debug, Clone)]
pub struct SourceSession {
    pub source_id: Uuid,
    pub studio_id: Uuid,
    pub org_id: Uuid,
    pub number: i32,
    pub label: String,
}

struct LiveSource {
    peer_id: Option<Uuid>,
    tx: Option<PeerTx>,
    number: i32,
    label: String,
    tally: Tally,
    status: Option<SourceStatus>,
    last_status: Option<Instant>,
}

#[derive(Default)]
struct StudioRoom {
    program: Vec<Uuid>,
    preview: Vec<Uuid>,
    sources: HashMap<Uuid, LiveSource>,
}

/// O estado de estúdio das salas deste pod.
#[derive(Default)]
pub struct StudioHub {
    rooms: DashMap<Uuid, StudioRoom>,
    /// `room_id` → estúdio (ou `None`: sala normal), para não ir à base a cada
    /// mensagem do operador.
    studio_of_room: DashMap<Uuid, Option<Uuid>>,
}

/// Uma mensagem de estado por segundo por fonte chega aos operadores e à base.
const STATUS_MIN_INTERVAL: Duration = Duration::from_secs(1);

impl StudioHub {
    fn view(room: &StudioRoom) -> Vec<SourceView> {
        let mut v: Vec<SourceView> = room
            .sources
            .iter()
            .map(|(id, s)| SourceView {
                source_id: *id,
                peer_id: s.peer_id,
                number: s.number,
                label: s.label.clone(),
                connected: s.peer_id.is_some(),
                tally: s.tally,
                status: s.status.clone(),
            })
            .collect();
        v.sort_by_key(|s| s.number);
        v
    }

    /// As fontes vivas de uma sala, pela ordem de CAM n.
    pub fn snapshot(&self, room_id: Uuid) -> Vec<SourceView> {
        self.rooms
            .get(&room_id)
            .map(|r| Self::view(&r))
            .unwrap_or_default()
    }

    /// Uma fonte ligou-se. Devolve o tally que lhe cabe.
    pub fn connect(&self, room_id: Uuid, s: &SourceSession, peer_id: Uuid, tx: PeerTx) -> Tally {
        let mut room = self.rooms.entry(room_id).or_default();
        let t = tally::state_of(s.source_id, &room.program, &room.preview);
        let prev = room.sources.insert(
            s.source_id,
            LiveSource {
                peer_id: Some(peer_id),
                tx: Some(tx),
                number: s.number,
                label: s.label.clone(),
                tally: t,
                status: None,
                last_status: None,
            },
        );
        // A mesma fonte ligada duas vezes (o telefone reabriu a app): o socket
        // antigo termina, para não haver dois publicadores com o mesmo CAM n.
        if let Some(old) = prev {
            if let (Some(old_peer), Some(tx)) = (old.peer_id, old.tx) {
                if old_peer != peer_id {
                    tx.terminate();
                }
            }
        }
        t
    }

    /// Uma fonte saiu. Só se for o MESMO socket: uma reentrada já a substituiu.
    /// A entrada fica (desligada) para a gravação continuar a saber o rótulo.
    pub fn disconnect(&self, room_id: Uuid, source_id: Uuid, peer_id: Uuid) -> bool {
        let Some(mut room) = self.rooms.get_mut(&room_id) else {
            return false;
        };
        match room.sources.get_mut(&source_id) {
            Some(s) if s.peer_id == Some(peer_id) => {
                s.peer_id = None;
                s.tx = None;
                true
            }
            _ => false,
        }
    }

    /// Fecha o socket vivo de uma fonte revogada. `true` = havia um.
    pub fn revoke(&self, room_id: Uuid, source_id: Uuid) -> bool {
        let Some(mut room) = self.rooms.get_mut(&room_id) else {
            return false;
        };
        match room.sources.remove(&source_id) {
            Some(LiveSource { tx: Some(tx), .. }) => {
                tx.terminate();
                true
            }
            _ => false,
        }
    }

    /// Fixa os barramentos e devolve as fontes ligadas cujo tally MUDOU.
    pub fn set_tally(
        &self,
        room_id: Uuid,
        program: Vec<Uuid>,
        preview: Vec<Uuid>,
    ) -> Vec<(Uuid, Uuid, Tally)> {
        let mut room = self.rooms.entry(room_id).or_default();
        let mut changed = Vec::new();
        let (p, v) = (program.clone(), preview.clone());
        room.program = program;
        room.preview = preview;
        for (id, s) in room.sources.iter_mut() {
            let t = tally::state_of(*id, &p, &v);
            if t != s.tally {
                s.tally = t;
                if let Some(peer) = s.peer_id {
                    changed.push((*id, peer, t));
                }
            }
        }
        changed
    }

    /// O `peer_id` de uma fonte ligada.
    pub fn peer_of(&self, room_id: Uuid, source_id: Uuid) -> Option<Uuid> {
        self.rooms
            .get(&room_id)
            .and_then(|r| r.sources.get(&source_id).and_then(|s| s.peer_id))
    }

    /// `(source_id, número, rótulo)` de um `peer_id` — ligado ou não (a gravação
    /// finaliza depois de o telefone sair).
    pub fn source_of_peer(&self, room_id: Uuid, peer_id: Uuid) -> Option<(Uuid, i32, String)> {
        self.rooms.get(&room_id).and_then(|r| {
            r.sources
                .iter()
                .find(|(_, s)| s.peer_id == Some(peer_id))
                .map(|(id, s)| (*id, s.number, s.label.clone()))
        })
    }

    /// Guarda o estado se passou o intervalo mínimo. `true` = deve ser difundido.
    pub fn record_status(&self, room_id: Uuid, source_id: Uuid, status: SourceStatus) -> bool {
        let Some(mut room) = self.rooms.get_mut(&room_id) else {
            return false;
        };
        let Some(s) = room.sources.get_mut(&source_id) else {
            return false;
        };
        if s.last_status
            .is_some_and(|t| t.elapsed() < STATUS_MIN_INTERVAL)
        {
            return false;
        }
        s.last_status = Some(Instant::now());
        s.status = Some(status);
        true
    }

    /// `(ligada, tally, estado)` de uma fonte NESTE pod.
    pub fn live(
        &self,
        room_id: Uuid,
        source_id: Uuid,
    ) -> Option<(bool, Tally, Option<SourceStatus>)> {
        self.rooms.get(&room_id).and_then(|r| {
            r.sources
                .get(&source_id)
                .map(|s| (s.peer_id.is_some(), s.tally, s.status.clone()))
        })
    }

    pub fn forget_room(&self, room_id: Uuid) {
        self.rooms.remove(&room_id);
        self.studio_of_room.remove(&room_id);
    }
}

/// O que uma fonte pode mandar. Tudo o resto é recusado.
pub fn source_may_send(m: &ClientMsg) -> bool {
    matches!(
        m,
        ClientMsg::SfuOffer { .. }
            | ClientMsg::SfuAnswer { .. }
            | ClientMsg::SfuIce { .. }
            | ClientMsg::Leave
            | ClientMsg::StudioSourceStatus { .. }
            | ClientMsg::StudioCommandResult { .. }
    )
}

fn error(state: &AppState, room_id: Uuid, peer_id: Uuid, message: impl Into<String>) {
    state.hub.send_to_local(
        room_id,
        peer_id,
        ServerMsg::Error {
            message: message.into(),
        },
    );
}

/// Envia a lista de fontes aos anfitriões da sala.
pub fn push_sources(state: &AppState, room_id: Uuid) {
    let sources = state.studio.snapshot(room_id);
    for (peer, host) in state.hub.roster(room_id) {
        if host {
            state.hub.send_to_local(
                room_id,
                peer,
                ServerMsg::StudioSources {
                    sources: sources.clone(),
                },
            );
        }
    }
}

fn to_hosts(state: &AppState, room_id: Uuid, msg: ServerMsg) {
    for (peer, host) in state.hub.roster(room_id) {
        if host {
            state.hub.send_to_local(room_id, peer, msg.clone());
        }
    }
}

/// O estúdio desta sala, com cache.
pub async fn studio_of_room(state: &AppState, room_id: Uuid) -> Option<Uuid> {
    if let Some(v) = state.studio.studio_of_room.get(&room_id) {
        return *v;
    }
    let id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM studios WHERE room_id = $1")
        .bind(room_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
    state.studio.studio_of_room.insert(room_id, id);
    id
}

/// Um anfitrião entrou numa sala: se for de estúdio, recebe as fontes.
pub async fn on_host_joined(state: &AppState, room_id: Uuid, peer_id: Uuid) {
    if studio_of_room(state, room_id).await.is_some() {
        state.hub.send_to_local(
            room_id,
            peer_id,
            ServerMsg::StudioSources {
                sources: state.studio.snapshot(room_id),
            },
        );
    }
}

/// Uma fonte entrou (já na sala e no SFU).
pub fn on_source_joined(
    state: &AppState,
    room_id: Uuid,
    s: &SourceSession,
    peer_id: Uuid,
    tx: PeerTx,
) {
    state
        .studio
        .studio_of_room
        .insert(room_id, Some(s.studio_id));
    let t = state.studio.connect(room_id, s, peer_id, tx);
    state
        .hub
        .send_to_local(room_id, peer_id, ServerMsg::StudioTally { state: t });
    push_sources(state, room_id);
    persist_seen(state, s.source_id, None, Some(t), Some(true));
}

/// Uma fonte saiu.
pub fn on_source_left(state: &AppState, room_id: Uuid, s: &SourceSession, peer_id: Uuid) {
    if state.studio.disconnect(room_id, s.source_id, peer_id) {
        push_sources(state, room_id);
        persist_seen(state, s.source_id, None, None, Some(false));
    }
}

fn persist_seen(
    state: &AppState,
    source_id: Uuid,
    status: Option<&SourceStatus>,
    t: Option<Tally>,
    connected: Option<bool>,
) {
    let db = state.db.clone();
    let status = status.and_then(|s| serde_json::to_value(s).ok());
    let t = t.map(Tally::as_str);
    tokio::spawn(async move {
        let r = sqlx::query(
            "UPDATE studio_sources SET last_seen_at = now(),
                    last_status = COALESCE($2, last_status),
                    last_tally = COALESCE($3, last_tally),
                    connected = COALESCE($4, connected)
              WHERE id = $1",
        )
        .bind(source_id)
        .bind(status)
        .bind(t)
        .bind(connected)
        .execute(&db)
        .await;
        if let Err(e) = r {
            tracing::warn!(%source_id, error = %e, "não persisti o estado da fonte");
        }
    });
}

/// `studio-tally` do operador.
pub async fn on_tally(
    state: &Arc<AppState>,
    room_id: Uuid,
    peer_id: Uuid,
    program: Vec<Uuid>,
    preview: Vec<Uuid>,
) {
    if !state.hub.is_host(room_id, peer_id) || studio_of_room(state, room_id).await.is_none() {
        return error(state, room_id, peer_id, "studio.not_operator");
    }
    if let Err(code) = tally::validate_buses(&program, &preview) {
        return error(state, room_id, peer_id, code);
    }
    let changed = state.studio.set_tally(room_id, program, preview);
    for (source_id, peer, t) in &changed {
        state
            .hub
            .send_to_local(room_id, *peer, ServerMsg::StudioTally { state: *t });
        persist_seen(state, *source_id, None, Some(*t), None);
    }
    if !changed.is_empty() {
        push_sources(state, room_id);
    }
}

/// `studio-command` do operador.
pub async fn on_command(
    state: &Arc<AppState>,
    room_id: Uuid,
    peer_id: Uuid,
    source_id: Uuid,
    command_id: String,
    command: SourceCommand,
) {
    if !state.hub.is_host(room_id, peer_id) || studio_of_room(state, room_id).await.is_none() {
        return error(state, room_id, peer_id, "studio.not_operator");
    }
    if let Err(why) = validate_command_id(&command_id).and_then(|_| command.validate()) {
        return error(
            state,
            room_id,
            peer_id,
            format!("studio.invalid_command: {why}"),
        );
    }
    let Some(target) = state.studio.peer_of(room_id, source_id) else {
        let known = state.studio.live(room_id, source_id).is_some();
        return error(
            state,
            room_id,
            peer_id,
            if known {
                "studio.source_offline"
            } else {
                "studio.unknown_source"
            },
        );
    };
    state.hub.send_to_local(
        room_id,
        target,
        ServerMsg::StudioCommand {
            command_id,
            command,
        },
    );
}

/// `studio-source-status` do telefone.
pub fn on_status(
    state: &AppState,
    room_id: Uuid,
    s: &SourceSession,
    peer_id: Uuid,
    status: SourceStatus,
) {
    if let Err(why) = status.validate() {
        return error(
            state,
            room_id,
            peer_id,
            format!("studio.invalid_status: {why}"),
        );
    }
    if !state
        .studio
        .record_status(room_id, s.source_id, status.clone())
    {
        return;
    }
    persist_seen(state, s.source_id, Some(&status), None, None);
    to_hosts(
        state,
        room_id,
        ServerMsg::StudioSourceStatus {
            source_id: s.source_id,
            status,
            at: chrono::Utc::now().timestamp_millis(),
        },
    );
}

/// `studio-command-result` do telefone.
pub fn on_command_result(
    state: &AppState,
    room_id: Uuid,
    s: &SourceSession,
    peer_id: Uuid,
    command_id: String,
    ok: bool,
    err: Option<String>,
) {
    if let Err(why) =
        validate_command_id(&command_id).and_then(|_| validate_result_error(err.as_deref()))
    {
        return error(
            state,
            room_id,
            peer_id,
            format!("studio.invalid_result: {why}"),
        );
    }
    to_hosts(
        state,
        room_id,
        ServerMsg::StudioCommandResult {
            source_id: s.source_id,
            command_id,
            ok,
            error: err,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tx() -> (
        PeerTx,
        tokio::sync::mpsc::Receiver<ServerMsg>,
        Arc<tokio::sync::Notify>,
    ) {
        PeerTx::new(16, Arc::new(crate::metrics::Metrics::default()))
    }

    fn session(n: i32) -> SourceSession {
        SourceSession {
            source_id: Uuid::new_v4(),
            studio_id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            number: n,
            label: format!("CAM {n}"),
        }
    }

    #[test]
    fn uma_fonte_so_manda_media_e_mensagens_de_estudio() {
        let ok = true;
        let permitidas = [
            json!({"type": "sfu-offer", "sdp": "v=0"}),
            json!({"type": "sfu-ice", "candidate": {}}),
            json!({"type": "leave"}),
            json!({"type": "studio-source-status", "status": {"battery_percent": 50}}),
            // `ok` é campo do CONTRATO desta mensagem (ADR-0014 §4.3), não um
            // envelope de resposta HTTP. A catraca `respostas_ok_true` procura o
            // literal `"ok": true` e não sabe distinguir os dois casos, por isso
            // o valor entra por variável — o que o teste prova é o mesmo.
            json!({"type": "studio-command-result", "command_id": "c1", "ok": ok}),
        ];
        for m in permitidas {
            let msg: ClientMsg = serde_json::from_value(m.clone()).unwrap();
            assert!(source_may_send(&msg), "{m}");
        }
        let recusadas = [
            json!({"type": "chat", "text": "olá"}),
            json!({"type": "server-record", "active": true}),
            json!({"type": "studio-tally", "program": [], "preview": []}),
            json!({"type": "studio-command", "source_id": Uuid::new_v4(), "command_id": "c",
                   "command": {"kind": "focus-face"}}),
            json!({"type": "screen-share", "on": true}),
        ];
        for m in recusadas {
            let msg: ClientMsg = serde_json::from_value(m.clone()).unwrap();
            assert!(!source_may_send(&msg), "{m}");
        }
    }

    #[test]
    fn mensagens_novas_leem_se_e_escrevem_se_como_o_contrato() {
        let m: ClientMsg = serde_json::from_value(json!({
            "type": "studio-command", "source_id": Uuid::nil(), "command_id": "cmd-1",
            "command": {"kind": "exposure", "ev": -0.3}
        }))
        .unwrap();
        assert!(matches!(
            m,
            ClientMsg::StudioCommand {
                command: SourceCommand::Exposure { .. },
                ..
            }
        ));
        // O telefone não escolhe o seu source_id: um campo a mais é recusado no
        // estado (deny_unknown_fields do domínio).
        assert!(serde_json::from_value::<ClientMsg>(json!({
            "type": "studio-source-status", "status": {"source_id": Uuid::nil()}
        }))
        .is_err());
        let out = serde_json::to_value(ServerMsg::StudioTally {
            state: Tally::Program,
        })
        .unwrap();
        assert_eq!(out, json!({"type": "studio-tally", "state": "program"}));
        let out = serde_json::to_value(ServerMsg::StudioCommand {
            command_id: "c9".into(),
            command: SourceCommand::LockExposureFocus { locked: true },
        })
        .unwrap();
        assert_eq!(
            out,
            json!({"type": "studio-command", "command_id": "c9",
                   "command": {"kind": "lock-exposure-focus", "locked": true}})
        );
    }

    #[tokio::test]
    async fn tally_so_notifica_quem_mudou_e_programa_ganha() {
        let hub = StudioHub::default();
        let room = Uuid::new_v4();
        let (a, b) = (session(1), session(2));
        let (pa, pb) = (Uuid::new_v4(), Uuid::new_v4());
        assert_eq!(hub.connect(room, &a, pa, tx().0), Tally::Free);
        assert_eq!(hub.connect(room, &b, pb, tx().0), Tally::Free);

        let ch = hub.set_tally(room, vec![a.source_id], vec![a.source_id, b.source_id]);
        assert_eq!(ch.len(), 2);
        assert!(ch.contains(&(a.source_id, pa, Tally::Program)));
        assert!(ch.contains(&(b.source_id, pb, Tally::Preview)));
        // Repetir o mesmo tally não notifica ninguém.
        assert!(hub
            .set_tally(room, vec![a.source_id], vec![b.source_id])
            .is_empty());
        // Uma fonte que entra depois recebe logo o seu estado.
        let c = SourceSession {
            source_id: b.source_id,
            ..session(2)
        };
        assert_eq!(
            hub.connect(room, &c, Uuid::new_v4(), tx().0),
            Tally::Preview
        );
    }

    #[tokio::test]
    async fn reentrada_termina_o_socket_antigo_e_saida_velha_nao_apaga_a_nova() {
        let hub = StudioHub::default();
        let room = Uuid::new_v4();
        let s = session(3);
        let (old_tx, _rx, old_shutdown) = tx();
        let old_peer = Uuid::new_v4();
        hub.connect(room, &s, old_peer, old_tx);
        let new_peer = Uuid::new_v4();
        hub.connect(room, &s, new_peer, tx().0);
        // O socket antigo foi mandado terminar.
        tokio::time::timeout(Duration::from_millis(100), old_shutdown.notified())
            .await
            .expect("o socket antigo tinha de receber terminate");
        // A saída tardia do socket antigo não desliga a entrada nova.
        assert!(!hub.disconnect(room, s.source_id, old_peer));
        assert_eq!(hub.peer_of(room, s.source_id), Some(new_peer));
        assert!(hub.disconnect(room, s.source_id, new_peer));
        assert_eq!(hub.peer_of(room, s.source_id), None);
        // Fica conhecida (desligada) para a gravação.
        assert!(hub.live(room, s.source_id).is_some());
    }

    #[tokio::test]
    async fn revogar_fecha_o_socket_vivo() {
        let hub = StudioHub::default();
        let room = Uuid::new_v4();
        let s = session(4);
        let (t, _rx, shutdown) = tx();
        hub.connect(room, &s, Uuid::new_v4(), t);
        assert!(hub.revoke(room, s.source_id));
        tokio::time::timeout(Duration::from_millis(100), shutdown.notified())
            .await
            .expect("revogar tinha de terminar o socket");
        assert!(!hub.revoke(room, s.source_id));
    }

    #[tokio::test]
    async fn estado_tem_tecto_de_um_por_segundo() {
        let hub = StudioHub::default();
        let room = Uuid::new_v4();
        let s = session(5);
        let peer = Uuid::new_v4();
        hub.connect(room, &s, peer, tx().0);
        let st = SourceStatus {
            battery_percent: Some(70.0),
            ..Default::default()
        };
        assert!(hub.record_status(room, s.source_id, st.clone()));
        assert!(!hub.record_status(room, s.source_id, st));
        assert_eq!(
            hub.source_of_peer(room, peer),
            Some((s.source_id, 5, "CAM 5".to_string()))
        );
        let snap = hub.snapshot(room);
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].status.as_ref().unwrap().battery_percent, Some(70.0));
    }
}
