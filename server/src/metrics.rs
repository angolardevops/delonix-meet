//! Observabilidade de media (avaliação de arquitetura, ponto #6).
//! Contadores atómicos partilhados (`Arc<Metrics>`), expostos em `/metrics` no
//! formato de exposição Prometheus. Sem dependência externa — o custo é uma
//! operação atómica `Relaxed` nos pontos de evento (nunca no hot path por-pacote).

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering::Relaxed};
use std::sync::Arc;

/// Guard RAII para o gauge de ligações WS: incrementa ao criar, decrementa no
/// Drop (cobre TODOS os caminhos de saída do handler, incluindo erros/panics).
pub struct WsGuard {
    m: Arc<Metrics>,
    presence: bool,
}
impl WsGuard {
    pub fn signaling(m: Arc<Metrics>) -> Self {
        Metrics::inc(&m.ws_signaling);
        Self { m, presence: false }
    }
    pub fn presence(m: Arc<Metrics>) -> Self {
        Metrics::inc(&m.ws_presence);
        Self { m, presence: true }
    }
}
impl Drop for WsGuard {
    fn drop(&mut self) {
        if self.presence {
            Metrics::dec(&self.m.ws_presence);
        } else {
            Metrics::dec(&self.m.ws_signaling);
        }
    }
}

#[derive(Default)]
pub struct Metrics {
    /// Ligações WebSocket `/ws` (sinalização/SFU) ativas.
    pub ws_signaling: AtomicI64,
    /// Ligações WebSocket `/rtc` (presença) ativas.
    pub ws_presence: AtomicI64,
    /// `RTCPeerConnection`s do SFU em estado `connected` (gauge).
    pub sfu_pc_connected: AtomicI64,
    /// Tracks publicadas no SFU (cumulativo).
    pub sfu_publications_total: AtomicU64,
    /// Peers admitidos ao SFU (cumulativo).
    pub sfu_peers_total: AtomicU64,
}

impl Metrics {
    pub fn inc(a: &AtomicI64) {
        a.fetch_add(1, Relaxed);
    }
    pub fn dec(a: &AtomicI64) {
        a.fetch_sub(1, Relaxed);
    }
    pub fn bump(a: &AtomicU64) {
        a.fetch_add(1, Relaxed);
    }

    /// Renderiza no formato de exposição Prometheus (text/plain; version=0.0.4).
    pub fn render(&self, uptime_secs: u64) -> String {
        let g = |v: i64| v.max(0);
        format!(
            "# HELP delonix_ws_connections Ligações WebSocket ativas por canal.\n\
             # TYPE delonix_ws_connections gauge\n\
             delonix_ws_connections{{channel=\"signaling\"}} {}\n\
             delonix_ws_connections{{channel=\"presence\"}} {}\n\
             # HELP delonix_sfu_peer_connections RTCPeerConnections em estado connected.\n\
             # TYPE delonix_sfu_peer_connections gauge\n\
             delonix_sfu_peer_connections {}\n\
             # HELP delonix_sfu_publications_total Tracks publicadas no SFU (cumulativo).\n\
             # TYPE delonix_sfu_publications_total counter\n\
             delonix_sfu_publications_total {}\n\
             # HELP delonix_sfu_peers_total Peers admitidos ao SFU (cumulativo).\n\
             # TYPE delonix_sfu_peers_total counter\n\
             delonix_sfu_peers_total {}\n\
             # HELP delonix_uptime_seconds Uptime do processo em segundos.\n\
             # TYPE delonix_uptime_seconds gauge\n\
             delonix_uptime_seconds {}\n",
            g(self.ws_signaling.load(Relaxed)),
            g(self.ws_presence.load(Relaxed)),
            g(self.sfu_pc_connected.load(Relaxed)),
            self.sfu_publications_total.load(Relaxed),
            self.sfu_peers_total.load(Relaxed),
            uptime_secs,
        )
    }
}
