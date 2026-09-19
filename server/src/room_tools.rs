//! Contexto de colaboração in-room — sondagens, Q&A, temporizador, quadro branco
//! (avaliação de arquitetura #3, Martin Fowler). Extraído de signaling.rs: um
//! `impl SignalingHub` coeso, distinto do transporte SFU (offer/answer/ice) e da
//! moderação (admit/kick/lock). Persistência best-effort via redis_state.
use crate::pubsub::PubSubBus;
use crate::signaling::{
    ClientMsg, PollState, QaState, ServerMsg, SignalingHub, WbKind, WB_PAGES_CAP,
};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Epoch em milissegundos (prazos de quiz).
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Guarda no Redis perguntas alteradas (best-effort, fora do lock).
fn persist_qa(bus: Option<&Arc<PubSubBus>>, room_id: Uuid, qs: Vec<QaState>) {
    if qs.is_empty() {
        return;
    }
    if let Some(b) = bus {
        let b = b.clone();
        tokio::spawn(async move {
            for q in qs {
                crate::redis_state::qa_set(b.conn.clone(), room_id, &q).await;
            }
        });
    }
}

impl SignalingHub {
    /// Trata as mensagens de ferramentas de colaboração (chamado por `handle`).
    pub(crate) fn handle_tool_msg(
        &self,
        room_id: Uuid,
        peer_id: Uuid,
        msg: ClientMsg,
        bus: Option<&Arc<PubSubBus>>,
    ) {
        match msg {
            ClientMsg::PollCreate {
                question,
                options,
                correct_option,
                duration_secs,
            } => {
                let question = question.trim().to_string();
                let options: Vec<String> = options
                    .iter()
                    .map(|o| o.trim().to_string())
                    .filter(|o| !o.is_empty())
                    .collect();
                if self.is_host(room_id, peer_id)
                    && !question.is_empty()
                    && question.len() <= 200
                    && (2..=6).contains(&options.len())
                    && options.iter().all(|o| o.len() <= 80)
                    && correct_option.map_or(true, |c| c < options.len())
                {
                    let by = self
                        .username_of(room_id, peer_id)
                        .unwrap_or_else(|| "?".into());
                    // Quiz com tempo: o fim vai a todos (contagem no cliente);
                    // o servidor rejeita votos após o prazo e o anfitrião
                    // fecha/revela quando o tempo acaba.
                    let ends_at = duration_secs
                        .filter(|d| *d > 0)
                        .map(|d| now_ms() + (d.min(3600) as i64) * 1000);
                    let poll = PollState {
                        id: Uuid::new_v4(),
                        question,
                        options,
                        votes: HashMap::new(),
                        open: true,
                        by,
                        correct: correct_option,
                        ends_at,
                    };
                    if let Some(mut room) = self.rooms.get_mut(&room_id) {
                        if room.polls.len() < 20 {
                            room.polls.push(poll.clone());
                        }
                    }
                    self.broadcast_polls(room_id);
                    if let Some(b) = bus {
                        let b_cl = b.clone();
                        tokio::spawn(async move {
                            crate::redis_state::poll_set(b_cl.conn.clone(), room_id, &poll).await;
                        });
                    }
                }
            }
            ClientMsg::PollVote { poll, option } => {
                if let Some(mut room) = self.rooms.get_mut(&room_id) {
                    if let Some(p) = room.polls.iter_mut().find(|p| p.id == poll) {
                        // 1.5s de tolerância para latência no fim do quiz.
                        let within_time = p.ends_at.map_or(true, |e| now_ms() <= e + 1500);
                        if p.open && within_time && option < p.options.len() {
                            p.votes.insert(peer_id, option);
                        }
                    }
                }
                self.broadcast_polls(room_id);
                if let Some(b) = bus {
                    let b_cl = b.clone();
                    tokio::spawn(async move {
                        crate::redis_state::poll_vote(
                            b_cl.conn.clone(),
                            room_id,
                            poll,
                            peer_id,
                            option,
                        )
                        .await;
                    });
                }
            }
            ClientMsg::PollClose { poll } => {
                if self.is_host(room_id, peer_id) {
                    if let Some(mut room) = self.rooms.get_mut(&room_id) {
                        if let Some(p) = room.polls.iter_mut().find(|p| p.id == poll) {
                            p.open = false;
                        }
                    }
                    self.broadcast_polls(room_id);
                    if let Some(b) = bus {
                        let b_cl = b.clone();
                        tokio::spawn(async move {
                            crate::redis_state::poll_close(b_cl.conn.clone(), room_id, poll).await;
                        });
                    }
                }
            }
            ClientMsg::QaAsk { text } => {
                let text = text.trim().to_string();
                if !text.is_empty() && text.len() <= 300 {
                    let by = self
                        .username_of(room_id, peer_id)
                        .unwrap_or_else(|| "?".into());
                    let qa = QaState {
                        id: Uuid::new_v4(),
                        text,
                        by,
                        upvotes: std::collections::HashSet::new(),
                        answered: false,
                        hidden: false,
                        spotlight: false,
                    };
                    if let Some(mut room) = self.rooms.get_mut(&room_id) {
                        if room.questions.len() < 100 {
                            room.questions.push(qa.clone());
                        }
                    }
                    self.broadcast_qa(room_id);
                    if let Some(b) = bus {
                        let b_cl = b.clone();
                        tokio::spawn(async move {
                            crate::redis_state::qa_set(b_cl.conn.clone(), room_id, &qa).await;
                        });
                    }
                }
            }
            ClientMsg::QaUpvote { id } => {
                if let Some(mut room) = self.rooms.get_mut(&room_id) {
                    if let Some(q) = room.questions.iter_mut().find(|q| q.id == id) {
                        if !q.upvotes.insert(peer_id) {
                            q.upvotes.remove(&peer_id);
                        }
                    }
                }
                self.broadcast_qa(room_id);
                if let Some(b) = bus {
                    let b_cl = b.clone();
                    tokio::spawn(async move {
                        crate::redis_state::qa_upvote(b_cl.conn.clone(), room_id, id, peer_id)
                            .await;
                    });
                }
            }
            ClientMsg::QaAnswered { id } => {
                if self.is_host(room_id, peer_id) {
                    if let Some(mut room) = self.rooms.get_mut(&room_id) {
                        if let Some(q) = room.questions.iter_mut().find(|q| q.id == id) {
                            q.answered = !q.answered;
                        }
                    }
                    self.broadcast_qa(room_id);
                    if let Some(b) = bus {
                        let b_cl = b.clone();
                        tokio::spawn(async move {
                            crate::redis_state::qa_answered(b_cl.conn.clone(), room_id, id).await;
                        });
                    }
                }
            }
            ClientMsg::TimerSet { minutes } => {
                if self.is_host(room_id, peer_id) && (1..=240).contains(&minutes) {
                    let ends_at = chrono::Utc::now().timestamp() + (minutes as i64) * 60;
                    if let Some(mut room) = self.rooms.get_mut(&room_id) {
                        room.timer_ends_at = Some(ends_at);
                    }
                    self.broadcast_all(
                        room_id,
                        ServerMsg::Timer {
                            ends_at: Some(ends_at),
                        },
                    );
                    if let Some(b) = bus {
                        let b_cl = b.clone();
                        tokio::spawn(async move {
                            crate::redis_state::timer_set(b_cl.conn.clone(), room_id, ends_at)
                                .await;
                        });
                    }
                }
            }
            ClientMsg::WbStroke { mut stroke } => {
                let aceite = {
                    let Some(mut room) = self.rooms.get_mut(&room_id) else {
                        return;
                    };
                    let pages = room.wb_pages();
                    if !stroke.is_valid(pages) || !room.wb_can_write(peer_id) {
                        return;
                    }
                    let Some((user_id, username)) = room
                        .peers
                        .get(&peer_id)
                        .map(|p| (p.user_id, p.username.clone()))
                    else {
                        return;
                    };
                    // O id pode vir do cliente (para ele poder apagar/mover o
                    // que acabou de desenhar), mas nunca por cima de um que já
                    // existe: seria reescrever o objecto de outra pessoa.
                    let id = stroke.id.unwrap_or_else(Uuid::new_v4);
                    if room.wb_owner.contains_key(&id) {
                        return;
                    }
                    stroke.id = Some(id);
                    // O autor é o servidor que diz, não o cliente.
                    stroke.by = Some(username);
                    // Cap de memória: quadros gigantes descartam os mais antigos.
                    if room.wb_strokes.len() >= 3000 {
                        let velhos: Vec<Uuid> =
                            room.wb_strokes.drain(0..500).filter_map(|s| s.id).collect();
                        for v in velhos {
                            room.wb_owner.remove(&v);
                        }
                    }
                    room.wb_owner.insert(id, user_id);
                    room.wb_strokes.push(stroke.clone());
                    stroke
                };
                self.broadcast(room_id, peer_id, ServerMsg::WbStroke { stroke: aceite });
            }
            ClientMsg::WbClear => {
                let limpou = self
                    .rooms
                    .get_mut(&room_id)
                    .map(|mut room| {
                        if !room.wb_can_write(peer_id) {
                            return false;
                        }
                        room.wb_strokes.clear();
                        room.wb_owner.clear();
                        true
                    })
                    .unwrap_or(false);
                if limpou {
                    self.broadcast_all(room_id, ServerMsg::WbClear);
                }
            }
            ClientMsg::WbErase { id } => {
                let apagou = self
                    .rooms
                    .get_mut(&room_id)
                    .map(|mut room| {
                        if !room.wb_can_edit(peer_id, id) {
                            return false;
                        }
                        let antes = room.wb_strokes.len();
                        room.wb_strokes.retain(|s| s.id != Some(id));
                        room.wb_owner.remove(&id);
                        room.wb_strokes.len() != antes
                    })
                    .unwrap_or(false);
                if apagou {
                    self.broadcast_all(room_id, ServerMsg::WbErase { id });
                }
            }
            ClientMsg::WbTransform { id, dx, dy } => {
                if !dx.is_finite() || !dy.is_finite() || dx.abs() > 1.0 || dy.abs() > 1.0 {
                    return;
                }
                let moveu = self
                    .rooms
                    .get_mut(&room_id)
                    .map(|mut room| {
                        if !room.wb_can_edit(peer_id, id) {
                            return false;
                        }
                        match room.wb_strokes.iter_mut().find(|s| s.id == Some(id)) {
                            Some(s) => {
                                for pt in &mut s.pts {
                                    pt[0] += dx;
                                    pt[1] += dy;
                                }
                                true
                            }
                            None => false,
                        }
                    })
                    .unwrap_or(false);
                if moveu {
                    self.broadcast_all(room_id, ServerMsg::WbTransform { id, dx, dy });
                }
            }
            ClientMsg::WbUpdate { id, text } => {
                if text.trim().is_empty() || text.chars().count() > 2000 {
                    return;
                }
                let mudou = self
                    .rooms
                    .get_mut(&room_id)
                    .map(|mut room| {
                        if !room.wb_can_edit(peer_id, id) {
                            return false;
                        }
                        match room.wb_strokes.iter_mut().find(|s| {
                            s.id == Some(id) && matches!(s.kind, WbKind::Text | WbKind::Note)
                        }) {
                            Some(s) => {
                                s.text = Some(text.clone());
                                true
                            }
                            None => false,
                        }
                    })
                    .unwrap_or(false);
                if mudou {
                    self.broadcast_all(room_id, ServerMsg::WbUpdate { id, text });
                }
            }
            ClientMsg::WbCursor { x, y, laser, input } => {
                if !x.is_finite() || !y.is_finite() {
                    return;
                }
                if input
                    .as_deref()
                    .is_some_and(|i| !matches!(i, "mouse" | "pen" | "touch"))
                {
                    return;
                }
                // Travão por emissor: o cursor é o fluxo mais rápido que um
                // cliente pode gerar, e o fan-out é para a sala inteira.
                let permitido = self
                    .rooms
                    .get_mut(&room_id)
                    .and_then(|mut r| r.peers.get_mut(&peer_id).map(|p| p.cursor.allow()))
                    .unwrap_or(false);
                if !permitido {
                    return;
                }
                self.broadcast(
                    room_id,
                    peer_id,
                    ServerMsg::WbCursor {
                        from: peer_id,
                        x: x.clamp(0.0, 1.0),
                        y: y.clamp(0.0, 1.0),
                        laser,
                        input,
                    },
                );
            }
            ClientMsg::WbAddPage => {
                let paginas = self.rooms.get_mut(&room_id).and_then(|mut room| {
                    if !room.wb_can_write(peer_id) || room.wb_pages() >= WB_PAGES_CAP {
                        return None;
                    }
                    room.wb_extra_pages += 1;
                    Some((room.wb_pages(), room.wb_page))
                });
                if let Some((count, current)) = paginas {
                    self.broadcast_all(room_id, ServerMsg::WbPages { count, current });
                }
            }
            ClientMsg::WbPage { page } => {
                let paginas = self.rooms.get_mut(&room_id).and_then(|mut room| {
                    let manda = room.peers.get(&peer_id).is_some_and(|p| p.is_host)
                        || room.presenter == Some(peer_id);
                    if !manda || page >= room.wb_pages() {
                        return None;
                    }
                    room.wb_page = page;
                    Some((room.wb_pages(), page))
                });
                if let Some((count, current)) = paginas {
                    self.broadcast_all(room_id, ServerMsg::WbPages { count, current });
                }
            }
            ClientMsg::WbLock { on } => {
                if self.is_host(room_id, peer_id) {
                    let writers = self.rooms.get_mut(&room_id).map(|mut room| {
                        room.wb_restricted = on;
                        room.wb_writers.iter().copied().collect::<Vec<_>>()
                    });
                    if let Some(writers) = writers {
                        self.broadcast_all(
                            room_id,
                            ServerMsg::WbWriters {
                                restricted: on,
                                writers,
                            },
                        );
                    }
                }
            }
            ClientMsg::WbGrant { to, allowed } => {
                if self.is_host(room_id, peer_id) {
                    let estado = self.rooms.get_mut(&room_id).and_then(|mut room| {
                        if !room.peers.contains_key(&to) {
                            return None;
                        }
                        if allowed {
                            room.wb_writers.insert(to);
                        } else {
                            room.wb_writers.remove(&to);
                        }
                        Some((
                            room.wb_restricted,
                            room.wb_writers.iter().copied().collect::<Vec<_>>(),
                        ))
                    });
                    if let Some((restricted, writers)) = estado {
                        self.broadcast_all(
                            room_id,
                            ServerMsg::WbWriters {
                                restricted,
                                writers,
                            },
                        );
                    }
                }
            }
            ClientMsg::QaHide { id, hidden } => {
                if self.is_host(room_id, peer_id) {
                    let q = self.rooms.get_mut(&room_id).and_then(|mut room| {
                        let q = room.questions.iter_mut().find(|q| q.id == id)?;
                        q.hidden = hidden;
                        if hidden {
                            // Uma pergunta escondida não pode estar em destaque.
                            q.spotlight = false;
                        }
                        Some(q.clone())
                    });
                    if let Some(q) = q {
                        self.broadcast_qa(room_id);
                        persist_qa(bus, room_id, vec![q]);
                    }
                }
            }
            ClientMsg::QaSpotlight { id } => {
                if self.is_host(room_id, peer_id) {
                    let mudadas = self.rooms.get_mut(&room_id).and_then(|mut room| {
                        if id.is_some_and(|i| !room.questions.iter().any(|q| q.id == i)) {
                            return None;
                        }
                        let mut mudadas = Vec::new();
                        for q in room.questions.iter_mut() {
                            let alvo = Some(q.id) == id;
                            if q.spotlight != alvo || (alvo && q.hidden) {
                                q.spotlight = alvo;
                                if alvo {
                                    // Destacar é mostrar.
                                    q.hidden = false;
                                }
                                mudadas.push(q.clone());
                            }
                        }
                        Some(mudadas)
                    });
                    if let Some(mudadas) = mudadas {
                        self.broadcast_qa(room_id);
                        persist_qa(bus, room_id, mudadas);
                    }
                }
            }
            ClientMsg::WbClose => {
                // Fechar o quadro em todos (não só localmente). Não limpa os
                // traços — quem reabrir volta a vê-los.
                self.broadcast_all(room_id, ServerMsg::WbClose);
            }
            ClientMsg::TimerClear => {
                if self.is_host(room_id, peer_id) {
                    if let Some(mut room) = self.rooms.get_mut(&room_id) {
                        room.timer_ends_at = None;
                    }
                    self.broadcast_all(room_id, ServerMsg::Timer { ends_at: None });
                }
            }
            _ => {}
        }
    }
}
