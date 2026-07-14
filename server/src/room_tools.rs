//! Contexto de colaboração in-room — sondagens, Q&A, temporizador, quadro branco
//! (avaliação de arquitetura #3, Martin Fowler). Extraído de signaling.rs: um
//! `impl SignalingHub` coeso, distinto do transporte SFU (offer/answer/ice) e da
//! moderação (admit/kick/lock). Persistência best-effort via redis_state.
use crate::pubsub::PubSubBus;
use crate::signaling::{ClientMsg, PollState, QaState, ServerMsg, SignalingHub};
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
            ClientMsg::WbStroke { stroke } => {
                if stroke.pts.len() >= 2 && stroke.pts.len() <= 2000 && stroke.c.len() <= 24 {
                    if let Some(mut room) = self.rooms.get_mut(&room_id) {
                        // Cap de memória: quadros gigantes descartam os mais antigos.
                        if room.wb_strokes.len() >= 3000 {
                            room.wb_strokes.drain(0..500);
                        }
                        room.wb_strokes.push(stroke.clone());
                    }
                    self.broadcast(room_id, peer_id, ServerMsg::WbStroke { stroke });
                }
            }
            ClientMsg::WbClear => {
                if let Some(mut room) = self.rooms.get_mut(&room_id) {
                    room.wb_strokes.clear();
                }
                self.broadcast_all(room_id, ServerMsg::WbClear);
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
