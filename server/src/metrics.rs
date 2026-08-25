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
    /// Subscrições ativas (publicação → subscritor). É isto que dimensiona o
    /// downlink real do nó — `peers × publicações` não serve, porque cada peer
    /// recebe UMA camada por publicador.
    pub sfu_subscriptions: AtomicI64,
    /// Trocas de camada simulcast (cumulativo): sobe quando a sala cresce/encolhe
    /// ou quando a rede de um subscritor degrada.
    pub sfu_layer_switches_total: AtomicU64,
    /// Subscritores atualmente a receber uma camada ABAIXO do normal por perda
    /// de pacotes. >0 sustentado = rede dos clientes (ou do relay) em apuros.
    pub sfu_degraded_subscribers: AtomicI64,
    /// Keyframes (PLI) pedidos aos publicadores (cumulativo). Um valor alto e
    /// constante indica perda a forçar recuperação repetida.
    pub sfu_keyframes_requested_total: AtomicU64,
    /// Renegociações que falharam depois de todas as tentativas — cada uma é um
    /// peer que ficou sem receber media nova (era invisível antes).
    pub sfu_renegotiations_failed_total: AtomicU64,
    /// Ofertas do cliente adiadas por glare (diagnóstico da correção do glare).
    pub sfu_offers_deferred_total: AtomicU64,
    /// Gravações recuperadas de salas que esvaziaram por falha de ligação.
    pub sfu_recordings_orphaned_total: AtomicU64,
    /// Microfones fora do top-N de oradores (áudio não reencaminhado). É a
    /// medida directa da poupança de downlink de voz.
    pub sfu_audio_suppressed: AtomicI64,

    // ---- Filas de saída dos WebSockets (limitadas — ver signaling::PeerTx) ----
    /// Ocupação MÁXIMA já observada numa fila de socket (marca de água alta,
    /// nunca desce). É a medida de utilização: se ficar perto de `WS_QUEUE_CAP`
    /// a capacidade está no limite; se ficar em dezenas, sobra folga. Escolheu-se
    /// marca de água em vez de profundidade instantânea porque um gauge somado
    /// entre sockets vaza quando uma task de escrita é abortada a meio.
    pub ws_queue_high_water: AtomicI64,
    /// Mensagens DESCARTADAS por fila cheia (só as descartáveis: legenda
    /// parcial, traço de quadro, reacção). >0 sustentado = a sala está a
    /// perder conteúdo efémero por causa de um consumidor lento.
    pub ws_queue_dropped_total: AtomicU64,
    /// Sockets fechados por transbordo com uma mensagem que NÃO se pode
    /// descartar (sinalização/estado). Desligar é honesto; entregar meio
    /// protocolo não é. Cada um destes é um cliente que vai reentrar.
    pub ws_slow_consumer_kills_total: AtomicU64,
    /// Pedidos de renegociação do SFU descartados por fila cheia. A
    /// renegociação é coalescível (o estado mais recente vence), por isso
    /// descartar é correcto — mas se isto sobe, o `negotiation_loop` não está
    /// a acompanhar o ritmo de alterações de subscrição.
    pub nego_queue_dropped_total: AtomicU64,
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
             # HELP delonix_sfu_subscriptions Subscrições ativas (publicação→subscritor).\n\
             # TYPE delonix_sfu_subscriptions gauge\n\
             delonix_sfu_subscriptions {}\n\
             # HELP delonix_sfu_layer_switches_total Trocas de camada simulcast.\n\
             # TYPE delonix_sfu_layer_switches_total counter\n\
             delonix_sfu_layer_switches_total {}\n\
             # HELP delonix_sfu_degraded_subscribers Subscritores a receber camada reduzida por perda.\n\
             # TYPE delonix_sfu_degraded_subscribers gauge\n\
             delonix_sfu_degraded_subscribers {}\n\
             # HELP delonix_sfu_keyframes_requested_total PLIs enviados aos publicadores.\n\
             # TYPE delonix_sfu_keyframes_requested_total counter\n\
             delonix_sfu_keyframes_requested_total {}\n\
             # HELP delonix_sfu_renegotiations_failed_total Renegociações falhadas (peer sem media nova).\n\
             # TYPE delonix_sfu_renegotiations_failed_total counter\n\
             delonix_sfu_renegotiations_failed_total {}\n\
             # HELP delonix_sfu_offers_deferred_total Ofertas do cliente adiadas por glare.\n\
             # TYPE delonix_sfu_offers_deferred_total counter\n\
             delonix_sfu_offers_deferred_total {}\n\
             # HELP delonix_sfu_recordings_orphaned_total Gravações recuperadas de salas caídas.\n\
             # TYPE delonix_sfu_recordings_orphaned_total counter\n\
             delonix_sfu_recordings_orphaned_total {}\n\
             # HELP delonix_sfu_audio_suppressed Microfones fora do top-N de oradores.\n\
             # TYPE delonix_sfu_audio_suppressed gauge\n\
             delonix_sfu_audio_suppressed {}\n\
             # HELP delonix_ws_queue_high_water Ocupação máxima já vista numa fila de socket.\n\
             # TYPE delonix_ws_queue_high_water gauge\n\
             delonix_ws_queue_high_water {}\n\
             # HELP delonix_ws_queue_dropped_total Mensagens descartáveis perdidas por fila cheia.\n\
             # TYPE delonix_ws_queue_dropped_total counter\n\
             delonix_ws_queue_dropped_total {}\n\
             # HELP delonix_ws_slow_consumer_kills_total Sockets fechados por transbordo de fila.\n\
             # TYPE delonix_ws_slow_consumer_kills_total counter\n\
             delonix_ws_slow_consumer_kills_total {}\n\
             # HELP delonix_nego_queue_dropped_total Renegociações do SFU coalescidas por fila cheia.\n\
             # TYPE delonix_nego_queue_dropped_total counter\n\
             delonix_nego_queue_dropped_total {}\n\
             # HELP delonix_uptime_seconds Uptime do processo em segundos.\n\
             # TYPE delonix_uptime_seconds gauge\n\
             delonix_uptime_seconds {}\n",
            g(self.ws_signaling.load(Relaxed)),
            g(self.ws_presence.load(Relaxed)),
            g(self.sfu_pc_connected.load(Relaxed)),
            self.sfu_publications_total.load(Relaxed),
            self.sfu_peers_total.load(Relaxed),
            g(self.sfu_subscriptions.load(Relaxed)),
            self.sfu_layer_switches_total.load(Relaxed),
            g(self.sfu_degraded_subscribers.load(Relaxed)),
            self.sfu_keyframes_requested_total.load(Relaxed),
            self.sfu_renegotiations_failed_total.load(Relaxed),
            self.sfu_offers_deferred_total.load(Relaxed),
            self.sfu_recordings_orphaned_total.load(Relaxed),
            g(self.sfu_audio_suppressed.load(Relaxed)),
            g(self.ws_queue_high_water.load(Relaxed)),
            self.ws_queue_dropped_total.load(Relaxed),
            self.ws_slow_consumer_kills_total.load(Relaxed),
            self.nego_queue_dropped_total.load(Relaxed),
            uptime_secs,
        )
    }
}
