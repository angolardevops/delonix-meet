//! SFU (Selective Forwarding Unit) em Rust puro com `webrtc-rs`.
//!
//! Cada participante liga um único RTCPeerConnection ao servidor e publica
//! as suas tracks; o SFU encaminha os pacotes RTP para os restantes
//! participantes sem descodificar media (DTLS/SRTP termina aqui, o conteúdo
//! não é inspecionado). Escala O(n) uplinks por sala em vez do O(n²) do mesh.
//!
//! Negociação: o cliente oferta para publicar (entrada, ecrã, câmara ligada) e
//! o servidor oferta para subscrever. AMBOS os sentidos passam pelo mesmo canal
//! serializado por peer (`NegoMsg` → `negotiation_loop`): o webrtc-rs não tem
//! rollback, por isso uma oferta do cliente que chegue com a nossa por
//! responder é ADIADA, nunca aplicada fora de estado (era aí que a partilha de
//! ecrã se perdia em silêncio).
//!
//! Escolha de camada (simulcast): cada subscritor recebe UMA camada por
//! publicador, escolhida a partir do tamanho da sala E da perda de pacotes que
//! ele reporta por RTCP (`Quality`). Reavaliada a cada entrada/saída e a cada
//! degradação/recuperação de rede — ver `reevaluate_peer`.

use dashmap::DashMap;
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering::Relaxed},
        Arc, Weak,
    },
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

use webrtc::{
    api::{
        interceptor_registry::register_default_interceptors, media_engine::MediaEngine,
        setting_engine::SettingEngine, APIBuilder,
    },
    ice_transport::{
        ice_candidate::RTCIceCandidateInit, ice_candidate_type::RTCIceCandidateType,
        ice_server::RTCIceServer,
    },
    interceptor::registry::Registry,
    peer_connection::{
        configuration::RTCConfiguration, peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription, RTCPeerConnection,
    },
    rtcp::{
        payload_feedbacks::{
            full_intra_request::FullIntraRequest, picture_loss_indication::PictureLossIndication,
        },
        receiver_report::ReceiverReport,
    },
    rtp::extension::audio_level_extension::AudioLevelExtension,
    rtp_transceiver::{
        rtp_codec::{RTCRtpHeaderExtensionCapability, RTPCodecType},
        rtp_receiver::RTCRtpReceiver,
        rtp_sender::RTCRtpSender,
    },
    track::{
        track_local::{track_local_static_rtp::TrackLocalStaticRTP, TrackLocal, TrackLocalWriter},
        track_remote::TrackRemote,
    },
    util::Unmarshal,
};

use crate::signaling::{ClientMsg, ServerMsg};

type Result<T> = std::result::Result<T, webrtc::Error>;

/// Uma track publicada por um participante, com fan-out para subscritores.
/// Com simulcast, cada camada (rid `q`/`h`/`f`) é uma Publication distinta.
struct Publication {
    publisher: Uuid,
    kind: String,
    /// Camada simulcast ("f" = full; publicações sem rid contam como "f").
    rid: String,
    /// Subscritores: peer -> (track local que alimenta esse peer, sender para remover).
    subscribers: Mutex<HashMap<Uuid, (Arc<TrackLocalStaticRTP>, Arc<RTCRtpSender>)>>,
    /// Versão do conjunto de subscritores. A bomba de RTP mantém um snapshot e
    /// só volta a pegar no `Mutex` quando isto muda — sem isto, o lock ficava
    /// retido através do `write_rtp().await` e UM subscritor lento bloqueava a
    /// entrega a todos os outros (head-of-line blocking da sala inteira).
    subs_version: AtomicU64,
    remote: Arc<TrackRemote>,
    publisher_pc: Arc<RTCPeerConnection>,
    /// Writer da gravação server-side (ativo só durante uma gravação).
    rec: Mutex<Option<crate::recorder::RecWriter>>,
    /// Último PLI reencaminhado ao publicador (rate-limit de keyframes: em
    /// salas grandes, N subscritores a pedir keyframe seriam N PLIs).
    last_pli: std::sync::Mutex<Option<Instant>>,
    /// Id da extensão RTP de nível de áudio (RFC 6464) negociada com este
    /// publicador; 0 = não negociada (sem seleção de oradores possível).
    audio_level_id: u8,
    /// Energia de voz e numeração de saída (só relevante em áudio).
    audio: AudioMeter,
    /// Esta publicação está a ser reencaminhada? O seletor de oradores põe a
    /// `false` os microfones fora do top-N. Vídeo e ecrã nunca são suprimidos.
    forwarding: AtomicBool,
}

/// Medição de voz e renumeração de uma publicação de áudio. Vive à parte da
/// `Publication` por ser a única parte testável sem uma `RTCPeerConnection`.
#[derive(Default)]
struct AudioMeter {
    activity: AtomicU32,
    out_seq: AtomicU32,
}

impl AudioMeter {
    /// Regista o nível de áudio de um pacote (RFC 6464: 0 dBov = MAIS ALTO,
    /// 127 = silêncio — a escala é invertida). Ataque imediato, para quem
    /// começa a falar entrar no top-N já na tick seguinte; decaimento lento,
    /// para uma pausa entre palavras não o expulsar a meio da frase.
    fn observe_level(&self, level: u8) {
        let energy = 127u32.saturating_sub(level.min(127) as u32);
        self.activity.fetch_max(energy, Relaxed);
    }

    /// Decaimento por TEMPO (uma vez por tick do seletor), não por pacote.
    ///
    /// Tem mesmo de ser assim por causa do DTX: quem se cala deixa de enviar
    /// pacotes, portanto um decaimento por-pacote nunca chegaria a correr e
    /// essa pessoa ficaria eternamente no top-N — a bloquear a entrada de quem
    /// começa a falar. A 5 ticks/s, ×0,8 dá ~2 s de "hold" depois da última
    /// palavra: sobrevive às pausas da frase, liberta o lugar num silêncio real.
    fn decay(&self) {
        let cur = self.activity.load(Relaxed);
        if cur > 0 {
            self.activity.store(cur * 4 / 5, Relaxed);
        }
    }

    fn energy(&self) -> u32 {
        self.activity.load(Relaxed)
    }

    /// Numeração de saída CONTÍGUA.
    ///
    /// O `TrackLocalStaticRTP` reescreve SSRC e payload type mas **preserva a
    /// sequência de origem**. Suprimir um orador sem renumerar deixaria buracos
    /// que o recetor reporta como PERDA — e essa perda falsa faria o `Quality`
    /// baixar a camada de vídeo dele sem razão nenhuma.
    fn next_seq(&self) -> u16 {
        (self.out_seq.fetch_add(1, Relaxed) & 0xffff) as u16
    }
}

impl Publication {
    /// Marca o conjunto de subscritores como alterado (invalida o snapshot da
    /// bomba de RTP). Chamar SEMPRE a seguir a inserir/remover subscritores.
    fn touch_subs(&self) {
        self.subs_version.fetch_add(1, Relaxed);
    }

    /// `true` no máximo uma vez por `PLI_MIN_INTERVAL` — evita a tempestade de
    /// keyframes quando vários subscritores pedem ao mesmo tempo.
    fn pli_allowed(&self) -> bool {
        let mut last = match self.last_pli.lock() {
            Ok(l) => l,
            Err(p) => p.into_inner(),
        };
        let now = Instant::now();
        match *last {
            Some(t) if now.duration_since(t) < PLI_MIN_INTERVAL => false,
            _ => {
                *last = Some(now);
                true
            }
        }
    }
}

/// Intervalo mínimo entre keyframes pedidos ao mesmo publicador.
const PLI_MIN_INTERVAL: Duration = Duration::from_millis(1000);

/// Quantos microfones o SFU reencaminha em simultâneo.
///
/// Sem isto o downlink de áudio é O(n): numa sala de 20, cada participante
/// descarrega 19 fluxos de Opus. Reencaminhar só os oradores ativos torna-o
/// O(1) — é o que Zoom/Meet fazem. 3 chega para conversa natural com
/// interrupções; o 4.º a falar entra assim que passa um dos outros.
const MAX_ACTIVE_SPEAKERS: usize = 3;
/// Abaixo deste tamanho de sala não há ganho e não se mexe em nada.
const SPEAKER_SELECTION_MIN_ROOM: usize = 5;
/// Cadência da reavaliação de oradores.
const SPEAKER_TICK: Duration = Duration::from_millis(200);

/// Saúde do downlink de um subscritor, derivada dos Receiver Reports RTCP que
/// ele envia. É isto que permite ao SFU **descer de camada** quando a rede do
/// subscritor não aguenta — antes, a camada dependia só do tamanho da sala e
/// um participante em rede fraca perdia pacotes até o vídeo colapsar.
#[derive(Default)]
struct Quality {
    /// 0 = camada normal para o tamanho da sala; 1/2 = uma/duas camadas abaixo.
    shift: AtomicU8,
    /// Relatórios consecutivos com perda alta / com perda desprezável.
    bad: AtomicU8,
    good: AtomicU8,
}

/// `fraction_lost` é 0..255 (fração de 256). 26 ≈ 10 %, 5 ≈ 2 %.
const LOSS_BAD: u8 = 26;
const LOSS_GOOD: u8 = 5;
/// Relatórios (≈1/s) necessários para descer / voltar a subir de camada.
/// Descer é rápido (a perda dói já); subir é lento (evita oscilação).
const BAD_TO_DOWNGRADE: u8 = 3;
const GOOD_TO_UPGRADE: u8 = 15;
const MAX_SHIFT: u8 = 2;

impl Quality {
    /// Regista um Receiver Report. Devolve `true` se o nível desejado mudou
    /// (o chamador reavalia as subscrições deste peer).
    fn observe(&self, fraction_lost: u8) -> bool {
        if fraction_lost >= LOSS_BAD {
            self.good.store(0, Relaxed);
            let bad = self.bad.fetch_add(1, Relaxed).saturating_add(1);
            if bad >= BAD_TO_DOWNGRADE && self.shift.load(Relaxed) < MAX_SHIFT {
                self.bad.store(0, Relaxed);
                self.shift.fetch_add(1, Relaxed);
                return true;
            }
        } else if fraction_lost <= LOSS_GOOD {
            self.bad.store(0, Relaxed);
            let good = self.good.fetch_add(1, Relaxed).saturating_add(1);
            if good >= GOOD_TO_UPGRADE && self.shift.load(Relaxed) > 0 {
                self.good.store(0, Relaxed);
                self.shift.fetch_sub(1, Relaxed);
                return true;
            }
        }
        false
    }
}

struct SfuPeer {
    pc: Arc<RTCPeerConnection>,
    /// Este peer anunciou partilha de ecrã: a próxima track de vídeo SEM rid
    /// é o ecrã (a câmara publica sempre com rids de simulcast).
    sharing: AtomicBool,
    out: crate::signaling::PeerTx,
    /// Canal ÚNICO de negociação: ofertas do cliente, respostas do cliente e
    /// pedidos de renegociação do servidor passam todos por aqui e são
    /// processados em série pela `negotiation_loop`. Ver `NegoMsg`.
    nego_tx: mpsc::Sender<NegoMsg>,
    /// Já há um pedido de renegociação em fila? (colapsa rajadas de subscrição)
    renegotiate_queued: AtomicBool,
    /// Contadores partilhados — as funções livres que falam com este peer
    /// (`trigger_renegotiate`) precisam de contar descartes sem ter o `SfuState`.
    metrics: Arc<crate::metrics::Metrics>,
    pending_ice: Mutex<Vec<RTCIceCandidateInit>>,
    /// Subscrições ativas deste peer: (publisher, kind) -> (rid, sender).
    /// Garante 1 camada por publicador/tipo e permite trocar de camada.
    subscribed: Mutex<HashMap<(Uuid, String), (String, Arc<RTCRtpSender>)>>,
    /// Saúde do downlink deste peer (Receiver Reports) → escolha de camada.
    quality: Quality,
    /// De que publicadores este peer quer VÍDEO (a página da grelha que está a
    /// ver). `None` = todos — é o default e o comportamento de clientes antigos.
    /// Áudio e ecrã partilhado nunca dependem disto.
    video_interest: Mutex<Option<std::collections::HashSet<Uuid>>>,
}

/// Mensagens da máquina de negociação de um peer.
///
/// **Porquê um canal único:** o webrtc-rs NÃO faz rollback implícito e nem
/// sequer aceita `set_local_description(rollback)` a partir de
/// `have-local-offer` (verificado em `signaling_state.rs` da 0.17.1). Se uma
/// oferta do cliente (partilha de ecrã, ligar câmara) chegasse enquanto o
/// servidor tinha uma oferta por responder, o `set_remote_description` falhava
/// com `ErrSignalingStateProposedTransitionInvalid`, o erro era só um `warn` e
/// a track do cliente ficava adicionada mas NUNCA negociada — partilha de ecrã
/// que não aparece, sem qualquer erro visível. Serializar tudo aqui elimina o
/// glare do lado do servidor por construção (o cliente já o resolve com
/// perfect negotiation em `webrtc.ts`).
enum NegoMsg {
    /// O servidor precisa de (re)ofertar — nova/removida subscrição.
    Renegotiate,
    /// Oferta do cliente (publicação inicial, ecrã, câmara ligada).
    ClientOffer(String),
    /// Resposta do cliente a uma oferta nossa.
    ClientAnswer(String),
}

/// Camada desejada para um subscritor: parte do tamanho da sala (salas grandes
/// recebem camadas mais leves) e desce `shift` degraus se a rede DELE estiver a
/// perder pacotes. É a combinação dos dois que faz o downlink escalar.
fn wanted_rid(kind: &str, room_size: usize, shift: u8) -> &'static str {
    if kind != "video" {
        return "f";
    }
    // 2 = f, 1 = h, 0 = q
    let base: u8 = match room_size {
        n if n > 8 => 0,
        n if n > 4 => 1,
        _ => 2,
    };
    match base.saturating_sub(shift) {
        2 => "f",
        1 => "h",
        _ => "q",
    }
}

fn rid_rank(rid: &str) -> u8 {
    match rid {
        "f" => 3,
        "h" => 2,
        _ => 1,
    }
}

/// Escolhe a camada a servir a um subscritor.
///
/// - camada exata desejada disponível → usa-a;
/// - senão, se a camada atual ainda existe → **mantém** (devolve `None`): sem
///   isto, cada camada simulcast a chegar provocava uma troca + renegociação
///   (`q`→`h`→`f`) logo no arranque de cada publicador;
/// - senão → a melhor disponível (fallback quando a camada em uso desapareceu).
fn pick_layer<'a>(
    layers: &'a [Arc<Publication>],
    wanted: &str,
    current: Option<&str>,
) -> Option<&'a Arc<Publication>> {
    let rids: Vec<&str> = layers.iter().map(|p| p.rid.as_str()).collect();
    pick_layer_idx(&rids, wanted, current).map(|i| &layers[i])
}

/// A decisão em si, sobre rids — sem `Publication` para ser testável.
fn pick_layer_idx(rids: &[&str], wanted: &str, current: Option<&str>) -> Option<usize> {
    if let Some(i) = rids.iter().position(|r| *r == wanted) {
        return Some(i);
    }
    if let Some(cur) = current {
        if rids.contains(&cur) {
            return None;
        }
    }
    rids.iter()
        .enumerate()
        .max_by_key(|(_, r)| rid_rank(r))
        .map(|(i, _)| i)
}

#[derive(Default)]
struct SfuRoom {
    peers: Mutex<HashMap<Uuid, Arc<SfuPeer>>>,
    /// O seletor de oradores já arrancou para esta sala?
    selector_started: AtomicBool,
    publications: Mutex<Vec<Arc<Publication>>>,
    /// Sessão de gravação server-side em curso (metadados; writers nas publicações).
    recording: Mutex<Option<crate::recorder::RecordingSession>>,
}

#[derive(Default)]
pub struct SfuState {
    rooms: DashMap<Uuid, Arc<SfuRoom>>,
    /// Mapeia o UUID da sala para a porta UDP local que recebe a mixagem PSTN do FreeSWITCH
    pub phantom_listeners: DashMap<Uuid, u16>,
    /// Mapeia o UUID da sala para o IP:Porta do FreeSWITCH (PSTN Outbound)
    pub pstn_outbounds: DashMap<Uuid, std::net::SocketAddr>,
    /// Gravações cuja sala esvaziou por FALHA de ligação (ICE failed) em vez de
    /// saída limpa. O handler de estado da PC não tem `AppState` para chamar o
    /// `recorder::finalize`, por isso deixa-a aqui e o `remove_peer` do
    /// `signaling.rs` recolhe-a. Sem isto a gravação perdia-se em silêncio e o
    /// diretório `tmp-<uuid>` ficava órfão no volume de gravações.
    orphan_recordings: DashMap<Uuid, crate::recorder::RecordingSession>,
    /// Config de conectividade ICE do SFU (IP externo, STUN/TURN).
    ice: IceConfig,
    /// Contadores de observabilidade partilhados (ver metrics.rs).
    metrics: Arc<crate::metrics::Metrics>,
    /// Capacidade da fila de renegociação por peer (`NEGO_QUEUE_CAP`).
    /// 0 (o `Default`) significa «usa o default do código» — só o `SfuState`
    /// construído pelo `main` traz o valor da configuração.
    nego_cap: usize,
}

/// Config de ICE que o SFU usa para se tornar alcançável de fora do cluster.
#[derive(Default, Clone)]
pub struct IceConfig {
    /// IP externo/alcançável a anunciar (NAT 1:1). Vazio => só host candidates.
    pub external_ip: Option<String>,
    /// Host:porta do STUN/TURN (coturn). Ex.: `172.30.0.200:3478`.
    pub turn_host: String,
    /// Segredo partilhado com o coturn (use-auth-secret) para credenciais TURN.
    pub turn_secret: String,
    /// Força relay-only (a media do SFU passa sempre pelo TURN). Ver Config.
    pub force_relay: bool,
}

impl SfuState {
    /// Constrói o SFU com a config de ICE (a partir de `Config`) e os contadores.
    pub fn new(ice: IceConfig, metrics: Arc<crate::metrics::Metrics>, nego_cap: usize) -> Self {
        Self {
            ice,
            metrics,
            nego_cap,
            ..Default::default()
        }
    }

    /// Capacidade efectiva da fila de renegociação (default do código quando o
    /// `SfuState` foi construído por `Default`, como nos testes).
    fn nego_cap(&self) -> usize {
        if self.nego_cap == 0 {
            64
        } else {
            self.nego_cap
        }
    }

    /// Põe uma mensagem de negociação na fila LIMITADA do peer.
    ///
    /// Cheia significa que a `negotiation_loop` deste peer está tão atrasada
    /// que já tem dezenas de trocas SDP por processar — nesse ponto a sessão
    /// não é recuperável por acumular mais uma. Descarta-se e conta-se; a
    /// renegociação seguinte (ou o timeout, que já incrementa
    /// `sfu_renegotiations_failed_total`) trata da recuperação.
    fn enqueue_nego(&self, peer: &Arc<SfuPeer>, msg: NegoMsg) {
        if peer.nego_tx.try_send(msg).is_err() {
            crate::metrics::Metrics::bump(&self.metrics.nego_queue_dropped_total);
            tracing::warn!("fila de renegociação cheia — pedido descartado");
        }
    }
}

/// Portos UDP efémeros do SFU (para a media). Um intervalo FIXO e conhecido
/// permite expô-lo no K8s (Service LoadBalancer UDP). Ver deploy/k8s.
const SFU_UDP_MIN: u16 = 50000;
const SFU_UDP_MAX: u16 = 50200;

/// Servidores ICE que o próprio SFU usa para recolher candidatos srflx/relay
/// (via STUN/TURN). Sem isto, só há host candidates (IP interno do pod). As
/// credenciais TURN são de curta duração (mesmo esquema `use-auth-secret` que
/// o cliente recebe em rooms.rs `ice_servers`).
fn sfu_ice_servers(ice: &IceConfig) -> Vec<RTCIceServer> {
    if ice.turn_host.is_empty() {
        return vec![];
    }
    let mut servers = vec![RTCIceServer {
        urls: vec![format!("stun:{}", ice.turn_host)],
        ..Default::default()
    }];
    if !ice.turn_secret.is_empty() {
        use base64::Engine as _;
        use hmac::{Hmac, Mac};
        use sha1::Sha1;
        let expiry = (chrono::Utc::now().timestamp() + 3600).to_string();
        match Hmac::<Sha1>::new_from_slice(ice.turn_secret.as_bytes()) {
            Ok(mut mac) => {
                mac.update(expiry.as_bytes());
                let cred =
                    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
                servers.push(RTCIceServer {
                    urls: vec![
                        format!("turn:{}?transport=udp", ice.turn_host),
                        format!("turn:{}?transport=tcp", ice.turn_host),
                    ],
                    username: expiry,
                    credential: cred,
                    ..Default::default()
                });
            }
            // Sem candidato TURN, o SFU em relay-only (FORCE_TURN_RELAY) fica sem
            // NENHUM candidato → media morta silenciosa. Não engolir o erro.
            Err(e) => tracing::error!("SFU sem TURN: HMAC do turn_secret falhou: {e}"),
        }
    }
    servers
}

fn new_api(ice: &IceConfig) -> Result<webrtc::api::API> {
    let mut media = MediaEngine::default();
    media.register_default_codecs()?;
    // Nível de áudio por pacote (RFC 6464). Sem esta extensão negociada o SFU
    // não sabe quem está a falar e não pode selecionar oradores — teria de
    // reencaminhar todos os microfones para toda a gente.
    media.register_header_extension(
        RTCRtpHeaderExtensionCapability {
            uri: "urn:ietf:params:rtp-hdrext:ssrc-audio-level".to_owned(),
        },
        RTPCodecType::Audio,
        None,
    )?;
    // Header extensions necessárias para receber simulcast (mid + rid).
    for uri in [
        "urn:ietf:params:rtp-hdrext:sdes:mid",
        "urn:ietf:params:rtp-hdrext:sdes:rtp-stream-id",
        "urn:ietf:params:rtp-hdrext:sdes:repaired-rtp-stream-id",
    ] {
        media.register_header_extension(
            RTCRtpHeaderExtensionCapability {
                uri: uri.to_owned(),
            },
            RTPCodecType::Video,
            None,
        )?;
    }
    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut media)?;

    // SettingEngine: torna o SFU alcançável de fora do cluster.
    let mut settings = SettingEngine::default();
    if let Some(ip) = ice.external_ip.as_deref().filter(|s| !s.is_empty()) {
        // NAT 1:1 — anuncia o IP externo (LB/nó) em vez do IP interno do pod.
        settings.set_nat_1to1_ips(vec![ip.to_string()], RTCIceCandidateType::Host);
    }
    // Intervalo de portas UDP fixo e conhecido (para expor no K8s).
    settings.set_udp_network(webrtc::ice::udp_network::UDPNetwork::Ephemeral(
        webrtc::ice::udp_network::EphemeralUDP::new(SFU_UDP_MIN, SFU_UDP_MAX)
            .map_err(|e| webrtc::Error::new(e.to_string()))?,
    ));

    Ok(APIBuilder::new()
        .with_media_engine(media)
        .with_interceptor_registry(registry)
        .with_setting_engine(settings)
        .build())
}

impl SfuState {
    /// Inicia um listener UDP para pacotes RTP "PSTN" do FreeSWITCH (Fase 3 Stub)
    pub async fn spawn_phantom_listener(self: &Arc<Self>, room_id: Uuid) -> std::io::Result<u16> {
        if let Some(port) = self.phantom_listeners.get(&room_id) {
            return Ok(*port);
        }

        let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
        let port = socket.local_addr()?.port();
        self.phantom_listeners.insert(room_id, port);

        // O socket TEM de sobreviver a esta função: se ficasse como local, era
        // fechado no `return` e a porta anunciada ao FreeSWITCH estaria morta
        // (ICMP port unreachable em cada pacote). A task mantém-no vivo e
        // drena o que chega até a sala desaparecer.
        // Stub: a futura lógica criará o TrackLocalStaticRTP e bombeará
        // estes pacotes para dentro da sala.
        let state = self.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 1500];
            loop {
                match socket.recv_from(&mut buf).await {
                    Ok(_) => {
                        if !state.rooms.contains_key(&room_id) {
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(%room_id, error = %e, "PSTN phantom listener terminou");
                        break;
                    }
                }
            }
            state.phantom_listeners.remove(&room_id);
        });
        tracing::info!(%room_id, port, "PSTN Phantom RTP Listener bound");

        Ok(port)
    }

    pub async fn add_peer(
        self: &Arc<Self>,
        room_id: Uuid,
        peer_id: Uuid,
        out: crate::signaling::PeerTx,
    ) -> Result<()> {
        crate::metrics::Metrics::bump(&self.metrics.sfu_peers_total);
        let room = self
            .rooms
            .entry(room_id)
            .or_insert_with(|| Arc::new(SfuRoom::default()))
            .clone();

        // Uma task de seleção de oradores por sala (a primeira entrada arranca-a).
        if !room.selector_started.swap(true, Relaxed) {
            tokio::spawn(speaker_selector(self.clone(), room_id));
        }

        let api = new_api(&self.ice)?;
        // NAT 1:1 + STUN/TURN configurados via SettingEngine/RTCConfiguration:
        // o SFU recolhe candidatos host(externo)/srflx/relay → media direta
        // quando alcançável, TURN relay como fallback (ver sfu_ice_servers).
        let pc_config = RTCConfiguration {
            ice_servers: sfu_ice_servers(&self.ice),
            // Relay-only quando configurado: em K8s os host candidates do pod
            // não transportam media; obriga o SFU a usar só o relay TURN.
            ice_transport_policy: if self.ice.force_relay {
                webrtc::peer_connection::policy::ice_transport_policy::RTCIceTransportPolicy::Relay
            } else {
                webrtc::peer_connection::policy::ice_transport_policy::RTCIceTransportPolicy::All
            },
            ..Default::default()
        };
        let pc = Arc::new(api.new_peer_connection(pc_config).await?);

        let (nego_tx, nego_rx) = mpsc::channel(self.nego_cap());
        let peer = Arc::new(SfuPeer {
            pc: pc.clone(),
            sharing: AtomicBool::new(false),
            out: out.clone(),
            nego_tx,
            renegotiate_queued: AtomicBool::new(false),
            metrics: self.metrics.clone(),
            pending_ice: Mutex::new(Vec::new()),
            subscribed: Mutex::new(HashMap::new()),
            quality: Quality::default(),
            video_interest: Mutex::new(None),
        });
        room.peers.lock().await.insert(peer_id, peer.clone());

        // Trickle ICE: servidor -> cliente.
        {
            let out = out.clone();
            pc.on_ice_candidate(Box::new(move |c| {
                let out = out.clone();
                Box::pin(async move {
                    if let Some(c) = c {
                        if let Ok(init) = c.to_json() {
                            if let Ok(candidate) = serde_json::to_value(&init) {
                                let _ = out.send(ServerMsg::SfuIce { candidate });
                            }
                        }
                    }
                })
            }));
        }

        // Publicação: track recebida deste peer -> fan-out para os outros.
        {
            let state = self.clone();
            pc.on_track(Box::new(move |remote, receiver, _transceiver| {
                let state = state.clone();
                Box::pin(async move {
                    state
                        .handle_publish(room_id, peer_id, remote, receiver)
                        .await;
                })
            }));
        }

        {
            let state = self.clone();
            // Guarda para o gauge não driftar: só conta um inc (no 1º Connected)
            // e um dec (no 1º estado terminal), independentemente de repetições.
            let counted = Arc::new(AtomicBool::new(false));
            pc.on_peer_connection_state_change(Box::new(move |s| {
                tracing::info!(%room_id, %peer_id, state = %s, "sfu pc state");
                let state = state.clone();
                let counted = counted.clone();
                Box::pin(async move {
                    match s {
                        RTCPeerConnectionState::Connected => {
                            if !counted.swap(true, Relaxed) {
                                crate::metrics::Metrics::inc(&state.metrics.sfu_pc_connected);
                            }
                        }
                        RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed => {
                            if counted.swap(false, Relaxed) {
                                crate::metrics::Metrics::dec(&state.metrics.sfu_pc_connected);
                            }
                        }
                        _ => {}
                    }
                    if s == RTCPeerConnectionState::Failed {
                        // A gravação em curso (se esta era a última pessoa) NÃO
                        // pode ser descartada: fica no mapa de órfãs e o
                        // `remove_peer` do signaling.rs finaliza-a.
                        if let Some(session) = state.remove_peer(room_id, peer_id).await {
                            tracing::warn!(%room_id, "sala esvaziou por falha de ICE — gravação guardada para finalização");
                            crate::metrics::Metrics::bump(&state.metrics.sfu_recordings_orphaned_total);
                            state.orphan_recordings.insert(room_id, session);
                        }
                    }
                })
            }));
        }

        // Task de negociação serializada deste peer (ofertas do cliente E do
        // servidor pelo mesmo canal — ver `NegoMsg`). Guarda um `Weak`: com um
        // `Arc` o próprio `nego_tx` (que vive dentro do peer) nunca fecharia o
        // canal e a task ficava viva para sempre depois de o peer sair.
        tokio::spawn(negotiation_loop(
            self.clone(),
            room_id,
            peer_id,
            Arc::downgrade(&peer),
            nego_rx,
        ));

        // A sala cresceu: quem já cá estava pode ter de descer de camada.
        self.reevaluate_room(room_id).await;

        Ok(())
    }

    async fn peer(&self, room_id: Uuid, peer_id: Uuid) -> Option<Arc<SfuPeer>> {
        let room = self.rooms.get(&room_id)?.clone();
        let peers = room.peers.lock().await;
        peers.get(&peer_id).cloned()
    }

    pub async fn on_client_msg(
        self: &Arc<Self>,
        room_id: Uuid,
        peer_id: Uuid,
        msg: ClientMsg,
    ) -> Result<()> {
        let Some(peer) = self.peer(room_id, peer_id).await else {
            return Ok(());
        };
        match msg {
            // Oferta do cliente (publicação inicial, ecrã, câmara ligada).
            // NÃO é aplicada aqui: vai para a `negotiation_loop`, que a aplica
            // quando não houver oferta nossa por responder (glare — ver NegoMsg).
            ClientMsg::SfuOffer { sdp } => {
                self.enqueue_nego(&peer, NegoMsg::ClientOffer(sdp));
            }
            // Resposta do cliente a uma renegociação iniciada pelo servidor.
            ClientMsg::SfuAnswer { sdp } => {
                self.enqueue_nego(&peer, NegoMsg::ClientAnswer(sdp));
            }
            ClientMsg::SfuIce { candidate } => {
                let init: RTCIceCandidateInit =
                    serde_json::from_value(candidate).map_err(|_| webrtc::Error::ErrUnknownType)?;
                if peer.pc.remote_description().await.is_some() {
                    peer.pc.add_ice_candidate(init).await?;
                } else {
                    peer.pending_ice.lock().await.push(init);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Nova track publicada: cria subscrições em todos os outros peers e
    /// bombeia RTP até a track terminar.
    async fn handle_publish(
        self: Arc<Self>,
        room_id: Uuid,
        publisher: Uuid,
        remote: Arc<TrackRemote>,
        receiver: Arc<RTCRtpReceiver>,
    ) {
        let Some(room) = self.rooms.get(&room_id).map(|r| r.clone()) else {
            return;
        };
        let Some(pub_peer) = self.peer(room_id, publisher).await else {
            return;
        };
        // Partilha de ecrã: vídeo sem rid enquanto o peer anunciou partilha —
        // internamente é um "kind" próprio para ter slot/subscrição separados
        // da câmara (e ficheiro próprio na gravação). O áudio do sistema da
        // partilha (2º áudio do mesmo peer durante sharing) idem.
        let raw_kind = remote.kind().to_string();
        let raw_kind_is_audio = raw_kind == "audio";
        let sharing_now = pub_peer.sharing.load(Relaxed);
        // A câmara publica com rids de simulcast, o ecrã não. Mas um browser sem
        // `sendEncodings` (fallback em webrtc.ts) publica a CÂMARA sem rid: se
        // isso acontecesse durante uma partilha, a câmara era classificada como
        // ecrã. Só a PRIMEIRA track de vídeo sem rid durante a partilha é ecrã.
        let is_screen = raw_kind == "video"
            && remote.rid().is_empty()
            && sharing_now
            && !room
                .publications
                .lock()
                .await
                .iter()
                .any(|p| p.publisher == publisher && p.kind == "screen");
        let is_screen_audio = raw_kind == "audio" && sharing_now && {
            // já existe um áudio (microfone OU ecrã) deste publicador?
            let pubs = room.publications.lock().await;
            let mut has_audio = false;
            for p in pubs.iter() {
                if p.publisher == publisher && (p.kind == "audio" || p.kind == "screen-audio") {
                    has_audio = true;
                    break;
                }
            }
            has_audio
        };
        let kind = if is_screen {
            "screen".to_string()
        } else if is_screen_audio {
            "screen-audio".to_string()
        } else {
            raw_kind
        };
        let rid = match remote.rid() {
            "" => "f".to_string(),
            r => r.to_string(),
        };
        tracing::info!(%room_id, %publisher, kind, rid, "sfu track published");
        crate::metrics::Metrics::bump(&self.metrics.sfu_publications_total);

        // Id negociado da extensão de nível de áudio deste publicador.
        let audio_level_id = if raw_kind_is_audio {
            receiver
                .get_parameters()
                .await
                .header_extensions
                .iter()
                .find(|h| h.uri == "urn:ietf:params:rtp-hdrext:ssrc-audio-level")
                .and_then(|h| u8::try_from(h.id).ok())
                .unwrap_or(0)
        } else {
            0
        };

        let publication = Arc::new(Publication {
            publisher,
            kind: kind.clone(),
            rid,
            subscribers: Mutex::new(HashMap::new()),
            subs_version: AtomicU64::new(0),
            remote: remote.clone(),
            publisher_pc: pub_peer.pc.clone(),
            rec: Mutex::new(None),
            last_pli: std::sync::Mutex::new(None),
            audio_level_id,
            audio: AudioMeter::default(),
            // Arranca a reencaminhar: o seletor só suprime depois de ter
            // medições — nunca se corta áudio "por defeito".
            forwarding: AtomicBool::new(true),
        });
        room.publications.lock().await.push(publication.clone());

        // Gravação a decorrer? Anexa um writer a esta track nova.
        // Áudio grava sempre; vídeo grava UMA camada por publicador — a
        // melhor que aparecer (h/f; nem sempre o browser envia a "f").
        {
            let mut rec_guard = room.recording.lock().await;
            if rec_guard.is_some() && !recordable_codec(&publication) {
                tracing::error!(
                    %room_id, %publisher, kind = %publication.kind,
                    codec = %publication.remote.codec().capability.mime_type,
                    "codec não gravável — track EXCLUÍDA da gravação (ver recordable_codec)"
                );
            } else if rec_guard.is_some() {
                let attach = if publication.kind == "audio" || publication.kind == "screen-audio" {
                    true
                } else if publication.kind == "video" && publication.rid == "q" {
                    false
                } else {
                    // uma track gravada por (publicador, tipo) — evita duplicar
                    // camadas de câmara; o ecrã ("screen") tem slot próprio.
                    let pubs = room.publications.lock().await;
                    let mut already = false;
                    for p in pubs.iter() {
                        if p.kind == publication.kind
                            && p.publisher == publisher
                            && !Arc::ptr_eq(p, &publication)
                            && p.rec.lock().await.is_some()
                        {
                            already = true;
                            break;
                        }
                    }
                    !already
                };
                if attach {
                    if let Some(session) = rec_guard.as_mut() {
                        if let Some(w) = session.open_track(&publication.kind) {
                            *publication.rec.lock().await = Some(w);
                        }
                    }
                }
            }
        }

        // Diagnóstico (partilha de ecrã / novas tracks): que track publicou.
        // `kind=screen` confirma a deteção.
        tracing::info!(%room_id, %publisher, kind = %publication.kind, rid = %publication.rid, "sfu publish → fan-out");
        // Liga/ajusta as subscrições de todos os outros peers a esta camada.
        // A reavaliação escolhe a camada certa por peer (tamanho da sala +
        // saúde da rede dele) em vez de olhar só para esta publicação.
        self.reevaluate_room_except(room_id, Some(publisher)).await;

        // NOTA: não há PLI periódico. Antes havia um ticker de 3 s POR
        // publicação (=3 por câmara com simulcast) que forçava um keyframe a
        // cada 3 s para sempre — desperdício enorme de bitrate e "pumping"
        // visível — e que continuava vivo depois de a track terminar. Os
        // keyframes são agora pedidos SÓ quando fazem falta: subscrição nova,
        // troca de camada, ou PLI/FIR reencaminhado de um subscritor
        // (ver `subscribe_layer` → drenagem de RTCP).

        // Bomba de RTP: remota -> todas as tracks locais subscritas
        // (+ tee para o writer de gravação, se ativo).
        let this = self.clone();
        let is_audio = kind == "audio";

        tokio::spawn(async move {
            // Socket PSTN criado só quando há mesmo um destino registado —
            // antes era um socket UDP por CADA publicação de áudio da
            // instância, mesmo sem PSTN configurado.
            let mut pstn_socket: Option<tokio::net::UdpSocket> = None;
            // Snapshot dos subscritores: refrescado só quando a lista muda.
            let mut targets: Vec<Arc<TrackLocalStaticRTP>> = Vec::new();
            let mut targets_version = u64::MAX;
            let audio_level_id = publication.audio_level_id;
            loop {
                match publication.remote.read_rtp().await {
                    Ok((mut packet, _)) => {
                        // Nível de voz deste pacote → energia para o seletor de
                        // oradores (`speaker_selector`).
                        if is_audio && audio_level_id != 0 {
                            if let Some(mut raw) = packet.header.get_extension(audio_level_id) {
                                if let Ok(ext) = AudioLevelExtension::unmarshal(&mut raw) {
                                    publication.audio.observe_level(ext.level);
                                }
                            }
                        }

                        // A GRAVAÇÃO e o PSTN recebem SEMPRE tudo: a seleção de
                        // oradores é uma decisão de entrega ao vivo, não pode
                        // apagar ninguém da ata nem da chamada telefónica.
                        if let Some(w) = publication.rec.lock().await.as_mut() {
                            w.write_rtp(&packet);
                        }
                        if is_audio {
                            if let Some(dest) = this.pstn_outbounds.get(&room_id).map(|d| *d) {
                                if pstn_socket.is_none() {
                                    pstn_socket =
                                        tokio::net::UdpSocket::bind("0.0.0.0:0").await.ok();
                                }
                                if let Some(socket) = &pstn_socket {
                                    use webrtc::util::Marshal;
                                    if let Ok(buf) = packet.marshal() {
                                        let _ = socket.send_to(&buf, dest).await;
                                    }
                                }
                            }
                        }

                        // Microfone fora do top-N: não se reencaminha.
                        if !publication.forwarding.load(Relaxed) {
                            continue;
                        }
                        // Numeração própria e contígua no áudio — obrigatória
                        // por causa da supressão acima (ver `next_seq`).
                        if is_audio {
                            packet.header.sequence_number = publication.audio.next_seq();
                        }

                        let version = publication.subs_version.load(Relaxed);
                        if version != targets_version {
                            targets = publication
                                .subscribers
                                .lock()
                                .await
                                .values()
                                .map(|(track, _)| track.clone())
                                .collect();
                            targets_version = version;
                        }
                        // Escritas FORA do lock: um subscritor lento atrasa-se a
                        // si próprio, não à sala inteira.
                        for track in &targets {
                            let _ = track.write_rtp(&packet).await;
                        }
                    }
                    Err(_) => break, // track terminou
                }
            }
            if let Some(mut w) = publication.rec.lock().await.take() {
                w.close();
            }
            this.unpublish(room_id, &publication).await;
        });
    }

    /// Reavalia TODAS as subscrições de um peer: para cada (publicador, tipo)
    /// escolhe a camada certa para o tamanho atual da sala e para a saúde da
    /// rede DELE, ligando o que falta e trocando o que está errado.
    ///
    /// É o núcleo do escalonamento de downlink. Antes, a camada só era decidida
    /// quando chegava uma publicação nova: quem já estava na sala mantinha para
    /// sempre a camada com que entrou, e a sala crescer não reduzia nada para
    /// esses participantes (que são a maioria).
    ///
    /// Devolve um future **boxed** de propósito: `reevaluate_peer` →
    /// `subscribe_layer` → (drenagem RTCP) → `reevaluate_peer` é um ciclo, e
    /// sem a indireção o compilador não consegue inferir `Send` (recursão
    /// infinita na auto-trait).
    fn reevaluate_peer(
        self: Arc<Self>,
        room_id: Uuid,
        peer_id: Uuid,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        Box::pin(async move {
            let Some(room) = self.rooms.get(&room_id).map(|r| r.clone()) else {
                return;
            };
            let Some(peer) = self.peer(room_id, peer_id).await else {
                return;
            };
            // Peer ainda sem SDP remoto (acabou de entrar, ainda não ofertou):
            // subscrever agora produziria uma renegociação a apontar para o vazio.
            // O `apply_client_offer` chama isto assim que a oferta dele chega.
            if peer.pc.remote_description().await.is_none() {
                return;
            }
            let room_size = room.peers.lock().await.len();
            let shift = peer.quality.shift.load(Relaxed);
            let publications = room.publications.lock().await.clone();

            // Agrupar camadas por (publicador, tipo).
            let mut groups: HashMap<(Uuid, String), Vec<Arc<Publication>>> = HashMap::new();
            for p in publications {
                if p.publisher == peer_id {
                    continue;
                }
                groups
                    .entry((p.publisher, p.kind.clone()))
                    .or_default()
                    .push(p);
            }
            let current = peer.subscribed.lock().await.clone();
            let interest = peer.video_interest.lock().await.clone();
            for (key, layers) in groups {
                // Vídeo de câmara fora da página que o cliente está a ver: se
                // estava ligado, DESLIGA (é aqui que a paginação poupa banda a
                // sério). Áudio e ecrã partilhado passam sempre.
                let wanted_by_client =
                    key.1 != "video" || interest.as_ref().is_none_or(|set| set.contains(&key.0));
                if !wanted_by_client {
                    if let Some((_, sender)) = current.get(&key) {
                        let _ = peer.pc.remove_track(sender).await;
                        peer.subscribed.lock().await.remove(&key);
                        for p in &layers {
                            if p.subscribers.lock().await.remove(&peer_id).is_some() {
                                p.touch_subs();
                                crate::metrics::Metrics::dec(&self.metrics.sfu_subscriptions);
                            }
                        }
                        trigger_renegotiate(&peer);
                    }
                    continue;
                }
                let wanted = wanted_rid(&key.1, room_size, shift);
                let cur_rid = current.get(&key).map(|(rid, _)| rid.as_str());
                let Some(chosen) = pick_layer(&layers, wanted, cur_rid) else {
                    continue;
                };
                match current.get(&key) {
                    None => {
                        if let Err(e) =
                            subscribe_layer(&self, room_id, chosen, peer_id, &peer).await
                        {
                            tracing::warn!(%room_id, %peer_id, error = %e, "sfu subscribe failed");
                            continue;
                        }
                    }
                    Some((rid, old_sender)) if *rid != chosen.rid => {
                        tracing::info!(%room_id, %peer_id, publisher = %key.0, from = %rid, to = %chosen.rid, shift, room_size, "sfu layer switch");
                        crate::metrics::Metrics::bump(&self.metrics.sfu_layer_switches_total);
                        switch_layer(&self, room_id, &room, chosen, peer_id, &peer, old_sender)
                            .await;
                    }
                    Some(_) => continue,
                }
                // Keyframe imediato: sem ele o subscritor novo fica preto até ao
                // próximo keyframe natural do publicador.
                request_keyframe(chosen, &self.metrics).await;
            }
        })
    }

    /// Reavalia as subscrições de todos os peers da sala (entrada/saída de
    /// participantes muda a camada desejada de toda a gente).
    async fn reevaluate_room(self: &Arc<Self>, room_id: Uuid) {
        self.reevaluate_room_except(room_id, None).await;
    }

    async fn reevaluate_room_except(self: &Arc<Self>, room_id: Uuid, skip: Option<Uuid>) {
        let Some(room) = self.rooms.get(&room_id).map(|r| r.clone()) else {
            return;
        };
        let ids: Vec<Uuid> = room.peers.lock().await.keys().copied().collect();
        for id in ids {
            if Some(id) == skip {
                continue;
            }
            self.clone().reevaluate_peer(room_id, id).await;
        }
    }

    async fn unpublish(self: &Arc<Self>, room_id: Uuid, publication: &Arc<Publication>) {
        let Some(room) = self.rooms.get(&room_id).map(|r| r.clone()) else {
            return;
        };
        room.publications
            .lock()
            .await
            .retain(|p| !Arc::ptr_eq(p, publication));
        let subs: Vec<(Uuid, Arc<RTCRtpSender>)> = publication
            .subscribers
            .lock()
            .await
            .drain()
            .map(|(id, (_, sender))| (id, sender))
            .collect();
        publication.touch_subs();
        for _ in 0..subs.len() {
            crate::metrics::Metrics::dec(&self.metrics.sfu_subscriptions);
        }
        let peers = room.peers.lock().await.clone();
        let key = (publication.publisher, publication.kind.clone());
        // Camadas restantes deste publicador/tipo, para fallback.
        let remaining: Vec<Arc<Publication>> = room
            .publications
            .lock()
            .await
            .iter()
            .filter(|p| p.publisher == key.0 && p.kind == key.1)
            .cloned()
            .collect();
        for (sub_id, sender) in subs {
            if let Some(sub_peer) = peers.get(&sub_id) {
                let _ = sub_peer.pc.remove_track(&sender).await;
                sub_peer.subscribed.lock().await.remove(&key);
                // A camada que este peer via desapareceu — cai para a melhor
                // que sobra (ou renegocia a remoção se não sobrou nenhuma).
                match remaining.iter().max_by_key(|p| rid_rank(&p.rid)) {
                    Some(next) => {
                        if subscribe_layer(self, room_id, next, sub_id, sub_peer)
                            .await
                            .is_ok()
                        {
                            request_keyframe(next, &self.metrics).await;
                        }
                    }
                    None => trigger_renegotiate(sub_peer),
                }
            }
        }
        tracing::info!(%room_id, publisher = %publication.publisher, kind = %publication.kind, rid = %publication.rid, "sfu track unpublished");
    }

    /// Remove um peer. Se a sala ficar vazia, devolve a sessão de gravação
    /// em curso (se houver) para o caller finalizar — a sala é apagada aqui.
    pub async fn remove_peer(
        self: &Arc<Self>,
        room_id: Uuid,
        peer_id: Uuid,
    ) -> Option<crate::recorder::RecordingSession> {
        let Some(room) = self.rooms.get(&room_id).map(|r| r.clone()) else {
            // Sala já desaparecida: pode haver uma gravação órfã à espera (o
            // handler de `Failed` da PC deixou-a aqui — ver orphan_recordings).
            return self.orphan_recordings.remove(&room_id).map(|(_, s)| s);
        };
        let Some(peer) = room.peers.lock().await.remove(&peer_id) else {
            return self.orphan_recordings.remove(&room_id).map(|(_, s)| s);
        };

        // Remover as publicações deste peer dos restantes participantes.
        let gone: Vec<Arc<Publication>> = room
            .publications
            .lock()
            .await
            .iter()
            .filter(|p| p.publisher == peer_id)
            .cloned()
            .collect();
        for publication in gone {
            self.unpublish(room_id, &publication).await;
        }

        // Retirar as subscrições deste peer das publicações que ficam.
        for publication in room.publications.lock().await.iter() {
            if publication
                .subscribers
                .lock()
                .await
                .remove(&peer_id)
                .is_some()
            {
                publication.touch_subs();
                crate::metrics::Metrics::dec(&self.metrics.sfu_subscriptions);
            }
        }
        peer.subscribed.lock().await.clear();
        if peer.quality.shift.load(Relaxed) > 0 {
            crate::metrics::Metrics::dec(&self.metrics.sfu_degraded_subscribers);
        }

        let _ = peer.pc.close().await;

        let empty = room.peers.lock().await.is_empty();
        let mut orphan_recording = None;
        if empty {
            orphan_recording = room.recording.lock().await.take();
            if orphan_recording.is_some() {
                for publication in room.publications.lock().await.iter() {
                    if let Some(mut w) = publication.rec.lock().await.take() {
                        w.close();
                    }
                }
            }
            self.rooms.remove_if(&room_id, |_, _| true);
        }
        tracing::info!(%room_id, %peer_id, "sfu peer removed");
        if !empty {
            // A sala encolheu: quem fica pode voltar a subir de camada.
            self.reevaluate_room(room_id).await;
        }
        orphan_recording.or_else(|| self.orphan_recordings.remove(&room_id).map(|(_, s)| s))
    }

    // ---------- Gravação server-side ----------

    /// Inicia a gravação da sala: writers para as tracks já publicadas;
    /// as futuras anexam-se no handle_publish. Falha se já estiver a gravar.
    pub async fn start_recording(
        &self,
        room_id: Uuid,
        by_user: Uuid,
        by_name: &str,
        e2ee_key: Option<Vec<u8>>,
        recordings_dir: &std::path::Path,
    ) -> bool {
        let Some(room) = self.rooms.get(&room_id).map(|r| r.clone()) else {
            return false;
        };
        let mut rec = room.recording.lock().await;
        if rec.is_some() {
            return false;
        }
        let Ok(mut session) = crate::recorder::RecordingSession::new(
            by_user,
            by_name.to_string(),
            e2ee_key,
            recordings_dir,
        )
        .await
        else {
            return false;
        };
        // Áudios todos; vídeo = a melhor camada disponível por publicador
        // (f > h > q — o browser nem sempre envia a camada cheia).
        let rank = |rid: &str| match rid {
            "f" => 3,
            "h" => 2,
            _ => 1,
        };
        let pubs = room.publications.lock().await;
        let mut best_video: HashMap<(Uuid, String), &Arc<Publication>> = HashMap::new();
        for p in pubs.iter() {
            // Codec que a gravação não sabe escrever → fora (senão saía um
            // ficheiro corrompido sem qualquer erro). Ver `recordable_codec`.
            if !recordable_codec(p) {
                tracing::error!(
                    %room_id, publisher = %p.publisher, kind = %p.kind,
                    codec = %p.remote.codec().capability.mime_type,
                    "codec não gravável — track EXCLUÍDA da gravação"
                );
                continue;
            }
            if p.kind == "audio" {
                if let Some(w) = session.open_track("audio") {
                    *p.rec.lock().await = Some(w);
                }
            } else {
                // melhor camada por (publicador, tipo): câmara e ecrã à parte
                let key = (p.publisher, p.kind.clone());
                if best_video
                    .get(&key)
                    .map(|b| rank(&p.rid) > rank(&b.rid))
                    .unwrap_or(true)
                {
                    best_video.insert(key, p);
                }
            }
        }
        for ((_, kind), p) in &best_video {
            if let Some(w) = session.open_track(kind) {
                *p.rec.lock().await = Some(w);
            }
        }
        drop(pubs);
        tracing::info!(%room_id, %by_user, "server recording started");
        *rec = Some(session);
        true
    }

    /// Para a gravação: fecha os writers e devolve a sessão para o finalize
    /// (composição ffmpeg + inserção na biblioteca).
    pub async fn stop_recording(&self, room_id: Uuid) -> Option<crate::recorder::RecordingSession> {
        let room = self.rooms.get(&room_id).map(|r| r.clone())?;
        let session = room.recording.lock().await.take()?;
        for publication in room.publications.lock().await.iter() {
            if let Some(mut w) = publication.rec.lock().await.take() {
                w.close();
            }
        }
        tracing::info!(%room_id, "server recording stopped");
        Some(session)
    }

    /// Quem começou a gravação em curso, se houver.
    pub async fn recording_by(&self, room_id: Uuid) -> Option<String> {
        let room = self.rooms.get(&room_id).map(|r| r.clone())?;
        let rec = room.recording.lock().await;
        rec.as_ref().map(|s| s.by_name.clone())
    }

    /// O cliente diz de quem quer ver vídeo (paginação da grelha).
    ///
    /// Sem isto, uma sala de 40 obrigava cada participante a descarregar 39
    /// fluxos de vídeo para desenhar 24 tiles — o resto era decodificado e
    /// deitado fora. Aqui o SFU **deixa mesmo de enviar** o que não está no
    /// ecrã. Só afeta `video`: o áudio de toda a gente e o ecrã partilhado
    /// continuam sempre subscritos.
    pub async fn set_video_interest(
        self: &Arc<Self>,
        room_id: Uuid,
        peer_id: Uuid,
        peers: Vec<Uuid>,
    ) {
        let Some(peer) = self.peer(room_id, peer_id).await else {
            return;
        };
        *peer.video_interest.lock().await = Some(peers.into_iter().collect());
        self.clone().reevaluate_peer(room_id, peer_id).await;
    }

    /// Marca/desmarca o peer como "a partilhar ecrã" — a próxima track de
    /// vídeo sem rid dele será tratada como ecrã.
    pub async fn set_screen(&self, room_id: Uuid, peer_id: Uuid, on: bool) {
        if let Some(peer) = self.peer(room_id, peer_id).await {
            peer.sharing.store(on, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Sala sem peers? (para auto-parar a gravação quando todos saem)
    pub async fn is_room_empty(&self, room_id: Uuid) -> bool {
        match self.rooms.get(&room_id).map(|r| r.clone()) {
            Some(room) => room.peers.lock().await.is_empty(),
            None => true,
        }
    }
}

/// A gravação server-side só sabe escrever VP8 (IVF, com PTS reais do RTP) e
/// Opus (OGG). Se o browser negociar VP9/H264/AV1, o depacketizer VP8 devolve
/// lixo e o ficheiro sai corrompido — **sem** erro em lado nenhum, porque o
/// ffmpeg compõe na mesma e a gravação entra na biblioteca. Melhor não gravar
/// a track e dizê-lo alto no log.
fn recordable_codec(publication: &Arc<Publication>) -> bool {
    let mime = publication
        .remote
        .codec()
        .capability
        .mime_type
        .to_ascii_lowercase();
    if publication.kind.ends_with("audio") {
        mime == "audio/opus"
    } else {
        mime == "video/vp8"
    }
}

/// Escolhe, a cada `SPEAKER_TICK`, que microfones o SFU reencaminha.
///
/// Sem isto o áudio é O(n) no downlink: numa sala de 20, cada cliente descarrega
/// 19 fluxos de Opus em simultâneo (~2,4 Mbps só de voz) — inviável nas redes
/// que este produto tem por alvo. Reencaminhar só os oradores ativos torna-o
/// O(1) sem que se note: quem fala está sempre lá, quem está calado não custa
/// nada (e o DTX já tinha tratado do silêncio dentro de cada fluxo).
///
/// **Nunca** se aplica a salas pequenas, a vídeo, ao áudio de ecrã partilhado,
/// à gravação nem ao PSTN.
async fn speaker_selector(state: Arc<SfuState>, room_id: Uuid) {
    let mut ticker = tokio::time::interval(SPEAKER_TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut suppressed_now = 0i64;
    loop {
        ticker.tick().await;
        let Some(room) = state.rooms.get(&room_id).map(|r| r.clone()) else {
            break; // sala desapareceu
        };
        let room_size = room.peers.lock().await.len();
        let publications = room.publications.lock().await.clone();

        // Passa SEMPRE: tudo o que não é microfone, e os microfones cuja
        // extensão de nível (RFC 6464) não foi negociada — desses não sabemos
        // se estão a falar, e na dúvida ouve-se. Suprimir por energia 0
        // silenciaria pessoas ao acaso num browser que não envie a extensão.
        let mut mics: Vec<&Arc<Publication>> = Vec::new();
        for p in &publications {
            if p.kind == "audio" && p.audio_level_id != 0 {
                p.audio.decay();
                mics.push(p);
            } else {
                p.forwarding.store(true, Relaxed);
            }
        }

        let select = room_size >= SPEAKER_SELECTION_MIN_ROOM && mics.len() > MAX_ACTIVE_SPEAKERS;
        let mut suppressed = 0i64;
        if select {
            mics.sort_by_key(|p| std::cmp::Reverse(p.audio.energy()));
            for (i, p) in mics.iter().enumerate() {
                let on = i < MAX_ACTIVE_SPEAKERS;
                p.forwarding.store(on, Relaxed);
                if !on {
                    suppressed += 1;
                }
            }
        } else {
            for p in &mics {
                p.forwarding.store(true, Relaxed);
            }
        }
        // Gauge sem drift: só se aplica o delta.
        if suppressed != suppressed_now {
            state
                .metrics
                .sfu_audio_suppressed
                .fetch_add(suppressed - suppressed_now, Relaxed);
            suppressed_now = suppressed;
        }
    }
    if suppressed_now != 0 {
        state
            .metrics
            .sfu_audio_suppressed
            .fetch_sub(suppressed_now, Relaxed);
    }
    tracing::debug!(%room_id, "seletor de oradores terminado");
}

/// Pede um keyframe ao publicador (com rate-limit por publicação).
async fn request_keyframe(publication: &Arc<Publication>, metrics: &Arc<crate::metrics::Metrics>) {
    if publication.kind != "video" && publication.kind != "screen" {
        return;
    }
    if !publication.pli_allowed() {
        return;
    }
    crate::metrics::Metrics::bump(&metrics.sfu_keyframes_requested_total);
    let _ = publication
        .publisher_pc
        .write_rtcp(&[Box::new(PictureLossIndication {
            sender_ssrc: 0,
            media_ssrc: publication.remote.ssrc(),
        })])
        .await;
}

/// Pede uma renegociação ao peer, colapsando rajadas: várias subscrições
/// adicionadas em sequência produzem UMA oferta, não uma por track.
fn trigger_renegotiate(peer: &Arc<SfuPeer>) {
    let mut rejected = false;
    coalesce_renegotiate(&peer.renegotiate_queued, || {
        let ok = peer.nego_tx.try_send(NegoMsg::Renegotiate).is_ok();
        rejected = !ok;
        ok
    });
    if rejected {
        crate::metrics::Metrics::bump(&peer.metrics.nego_queue_dropped_total);
        tracing::warn!("fila de renegociação cheia — pedido de renegociação descartado");
    }
}

/// Coalescing do pedido de renegociação, com **reposição da bandeira em falha**.
///
/// A bandeira existe para colapsar rajadas: várias subscrições seguidas
/// produzem UMA oferta. Com a fila agora limitada, o envio pode falhar — e se
/// a bandeira ficasse a `true` com o pedido perdido, este peer **nunca mais**
/// renegociaria e ficava permanentemente sem receber media nova. Repor é o que
/// torna o limite seguro.
///
/// Devolve `true` só quando o pedido ficou mesmo em fila.
fn coalesce_renegotiate(queued: &AtomicBool, send: impl FnOnce() -> bool) -> bool {
    if queued.swap(true, Relaxed) {
        return false; // já havia um pedido por processar
    }
    if send() {
        return true;
    }
    queued.store(false, Relaxed);
    false
}

/// Liga uma publicação (camada concreta) a um subscritor e renegoceia.
async fn subscribe_layer(
    state: &Arc<SfuState>,
    room_id: Uuid,
    publication: &Arc<Publication>,
    sub_id: Uuid,
    sub_peer: &Arc<SfuPeer>,
) -> Result<()> {
    let key = (publication.publisher, publication.kind.clone());
    let mut subscribed = sub_peer.subscribed.lock().await;
    if subscribed.contains_key(&key) {
        return Ok(()); // já ligado a uma camada deste publicador/tipo
    }
    // stream_id = peer_id do publisher (o cliente mapeia a track ao
    // participante); a partilha de ecrã (vídeo E áudio do sistema) vai num
    // stream "<peer>-screen" — o mesmo elemento <video> reproduz ambos.
    let stream_id = if publication.kind == "screen" || publication.kind == "screen-audio" {
        format!("{}-screen", publication.publisher)
    } else {
        publication.publisher.to_string()
    };
    let local = Arc::new(TrackLocalStaticRTP::new(
        publication.remote.codec().capability.clone(),
        format!(
            "{}-{}-{}",
            publication.publisher, publication.kind, publication.rid
        ),
        stream_id,
    ));
    let sender = sub_peer
        .pc
        .add_track(Arc::clone(&local) as Arc<dyn TrackLocal + Send + Sync>)
        .await?;
    {
        let mut subs = publication.subscribers.lock().await;
        subs.insert(sub_id, (local, sender.clone()));
    }
    publication.touch_subs();
    crate::metrics::Metrics::inc(&state.metrics.sfu_subscriptions);
    subscribed.insert(key, (publication.rid.clone(), sender.clone()));
    drop(subscribed);

    // Drenar o RTCP do sender é OBRIGATÓRIO para os interceptors funcionarem —
    // e é aqui que o feedback do subscritor deixa de ser deitado fora:
    //  - PLI/FIR  → reencaminhado ao publicador (é assim que um subscritor
    //    recupera de um keyframe perdido; sem isto, a única recuperação era o
    //    antigo ticker de 3 s que queimava bitrate para toda a gente);
    //  - Receiver Report → alimenta a escolha de camada (`Quality`), para o
    //    SFU descer de camada a quem está a perder pacotes.
    {
        let sender = sender.clone();
        let publication = publication.clone();
        let peer_weak = Arc::downgrade(sub_peer);
        let state = state.clone();
        tokio::spawn(async move {
            while let Ok((packets, _)) = sender.read_rtcp().await {
                let mut requalify = false;
                for packet in packets {
                    let any = packet.as_any();
                    if any.downcast_ref::<PictureLossIndication>().is_some()
                        || any.downcast_ref::<FullIntraRequest>().is_some()
                    {
                        request_keyframe(&publication, &state.metrics).await;
                    } else if let Some(rr) = any.downcast_ref::<ReceiverReport>() {
                        let worst = rr
                            .reports
                            .iter()
                            .map(|r| r.fraction_lost)
                            .max()
                            .unwrap_or(0);
                        if let Some(peer) = peer_weak.upgrade() {
                            let before = peer.quality.shift.load(Relaxed);
                            if peer.quality.observe(worst) {
                                let after = peer.quality.shift.load(Relaxed);
                                if before == 0 && after > 0 {
                                    crate::metrics::Metrics::inc(
                                        &state.metrics.sfu_degraded_subscribers,
                                    );
                                } else if before > 0 && after == 0 {
                                    crate::metrics::Metrics::dec(
                                        &state.metrics.sfu_degraded_subscribers,
                                    );
                                }
                                requalify = true;
                            }
                        }
                    }
                }
                if requalify {
                    state.clone().reevaluate_peer(room_id, sub_id).await;
                }
            }
        });
    }

    tracing::info!(%sub_id, publisher = %publication.publisher, kind = %publication.kind, rid = %publication.rid, "sfu subscribe_layer → renegotiate subscritor");
    trigger_renegotiate(sub_peer);
    Ok(())
}

/// Troca a camada que um subscritor recebe de um dado (publicador, tipo):
/// solta a antiga em todas as publicações do grupo, liga a nova, renegoceia.
#[allow(clippy::too_many_arguments)]
async fn switch_layer(
    state: &Arc<SfuState>,
    room_id: Uuid,
    room: &Arc<SfuRoom>,
    chosen: &Arc<Publication>,
    sub_id: Uuid,
    sub_peer: &Arc<SfuPeer>,
    old_sender: &Arc<RTCRtpSender>,
) {
    let key = (chosen.publisher, chosen.kind.clone());
    {
        let publications = room.publications.lock().await.clone();
        for p in &publications {
            if p.publisher == key.0 && p.kind == key.1 && !Arc::ptr_eq(p, chosen) {
                if p.subscribers.lock().await.remove(&sub_id).is_some() {
                    p.touch_subs();
                    crate::metrics::Metrics::dec(&state.metrics.sfu_subscriptions);
                }
            }
        }
    }
    let _ = sub_peer.pc.remove_track(old_sender).await;
    sub_peer.subscribed.lock().await.remove(&key);
    if let Err(e) = subscribe_layer(state, room_id, chosen, sub_id, sub_peer).await {
        tracing::warn!(%room_id, %sub_id, error = %e, "sfu layer switch failed");
    }
}

async fn flush_pending_ice(peer: &Arc<SfuPeer>) {
    let pending: Vec<RTCIceCandidateInit> = peer.pending_ice.lock().await.drain(..).collect();
    for init in pending {
        let _ = peer.pc.add_ice_candidate(init).await;
    }
}

/// Aplica uma oferta do cliente e responde. Só é chamada com a PC em `stable`
/// (a `negotiation_loop` garante-o) — ver `NegoMsg` para o porquê.
async fn apply_client_offer(
    state: &Arc<SfuState>,
    room_id: Uuid,
    peer_id: Uuid,
    peer: &Arc<SfuPeer>,
    sdp: String,
) {
    let offer = match RTCSessionDescription::offer(sdp) {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!(%room_id, %peer_id, error = %e, "sfu oferta do cliente inválida");
            return;
        }
    };
    if let Err(e) = peer.pc.set_remote_description(offer).await {
        tracing::warn!(%room_id, %peer_id, error = %e, "sfu set_remote(offer) do cliente falhou");
        return;
    }
    flush_pending_ice(peer).await;
    let answer = match peer.pc.create_answer(None).await {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!(%room_id, %peer_id, error = %e, "sfu create_answer failed");
            return;
        }
    };
    if let Err(e) = peer.pc.set_local_description(answer).await {
        tracing::warn!(%room_id, %peer_id, error = %e, "sfu set_local(answer) failed");
        return;
    }
    if let Some(local) = peer.pc.local_description().await {
        let _ = peer.out.send(ServerMsg::SfuAnswer { sdp: local.sdp });
    }
    // Este peer passa a ter SDP remoto: liga-o ao que já se publica na sala
    // (idempotente) e acerta as camadas de todos para o tamanho atual.
    state.reevaluate_room(room_id).await;
}

/// Negociação serializada de um peer: ofertas do cliente, respostas do cliente
/// e renegociações do servidor passam TODAS por aqui, uma de cada vez.
///
/// Ver `NegoMsg`: o webrtc-rs não tem rollback, por isso uma oferta do cliente
/// que chegue enquanto a nossa está por responder é **adiada**, não descartada.
async fn negotiation_loop(
    state: Arc<SfuState>,
    room_id: Uuid,
    peer_id: Uuid,
    peer: Weak<SfuPeer>,
    mut rx: mpsc::Receiver<NegoMsg>,
) {
    let mut deferred: VecDeque<String> = VecDeque::new();
    while let Some(msg) = rx.recv().await {
        let Some(p) = peer.upgrade() else { break };
        match msg {
            // Resposta sem oferta nossa pendente (chegou tarde, após timeout).
            NegoMsg::ClientAnswer(_) => {}
            NegoMsg::ClientOffer(sdp) => {
                apply_client_offer(&state, room_id, peer_id, &p, sdp).await;
            }
            NegoMsg::Renegotiate => {
                p.renegotiate_queued.store(false, Relaxed);
                run_renegotiation(&p, &mut rx, &mut deferred, &state.metrics).await;
            }
        }
        // Ofertas adiadas durante uma renegociação: agora a PC está estável.
        while let Some(sdp) = deferred.pop_front() {
            apply_client_offer(&state, room_id, peer_id, &p, sdp).await;
        }
    }
    tracing::debug!(%room_id, %peer_id, "sfu negotiation loop terminado");
}

/// Uma renegociação server-driven completa: oferta → resposta do cliente.
/// Em timeout re-oferta (a transição `have-local-offer` →
/// `SetLocal(offer)` É válida no webrtc-rs, e é a única recuperação possível
/// sem rollback). Ofertas do cliente que cheguem entretanto ficam em `deferred`.
async fn run_renegotiation(
    peer: &Arc<SfuPeer>,
    rx: &mut mpsc::Receiver<NegoMsg>,
    deferred: &mut VecDeque<String>,
    metrics: &Arc<crate::metrics::Metrics>,
) {
    // Sem SDP remoto ainda não há nada que renegociar (a publicação inicial do
    // cliente pode estar em curso). NÃO dormir às cegas: a oferta do cliente
    // chega por ESTE canal, portanto uma espera passiva bloquearia justamente a
    // mensagem que se está à espera. Drena-se o canal e devolve-se o controlo
    // ao `negotiation_loop`, que aplica a oferta e reagenda.
    for _ in 0..50 {
        if peer.pc.remote_description().await.is_some() {
            break;
        }
        match tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
            Ok(Some(NegoMsg::ClientOffer(sdp))) => {
                deferred.push_back(sdp);
                trigger_renegotiate(peer);
                return;
            }
            Ok(Some(NegoMsg::Renegotiate)) => peer.renegotiate_queued.store(false, Relaxed),
            Ok(Some(NegoMsg::ClientAnswer(_))) => {}
            Ok(None) => return, // peer saiu
            Err(_) => {}        // 100 ms sem nada — volta a testar o SDP remoto
        }
    }

    let mut more = true;
    while more {
        more = false;
        let mut settled = false;
        for attempt in 0..3u8 {
            let offer = match peer.pc.create_offer(None).await {
                Ok(o) => o,
                Err(e) => {
                    tracing::warn!(error = %e, "sfu create_offer failed");
                    return;
                }
            };
            if let Err(e) = peer.pc.set_local_description(offer).await {
                tracing::warn!(error = %e, "sfu set_local failed");
                return;
            }
            let Some(local) = peer.pc.local_description().await else {
                return;
            };
            let _ = peer.out.send(ServerMsg::SfuOffer { sdp: local.sdp });

            let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
            loop {
                match tokio::time::timeout_at(deadline, rx.recv()).await {
                    Ok(Some(NegoMsg::ClientAnswer(sdp))) => {
                        match RTCSessionDescription::answer(sdp) {
                            Ok(answer) => {
                                if let Err(e) = peer.pc.set_remote_description(answer).await {
                                    tracing::warn!(error = %e, "sfu set_remote answer failed");
                                } else {
                                    flush_pending_ice(peer).await;
                                }
                            }
                            Err(e) => tracing::warn!(error = %e, "sfu bad answer"),
                        }
                        settled = true;
                        break;
                    }
                    // Não pode ser aplicada agora (estamos em have-local-offer):
                    // fica para quando a PC voltar a `stable`.
                    Ok(Some(NegoMsg::ClientOffer(sdp))) => {
                        crate::metrics::Metrics::bump(&metrics.sfu_offers_deferred_total);
                        deferred.push_back(sdp);
                    }
                    // Subscrição nova durante a espera: outra ronda a seguir.
                    Ok(Some(NegoMsg::Renegotiate)) => {
                        peer.renegotiate_queued.store(false, Relaxed);
                        more = true;
                    }
                    Ok(None) => return, // peer saiu
                    Err(_) => {
                        tracing::warn!(attempt, "sfu renegotiation timed out — a re-ofertar");
                        break;
                    }
                }
            }
            if settled {
                break;
            }
        }
        if !settled {
            crate::metrics::Metrics::bump(&metrics.sfu_renegotiations_failed_total);
            tracing::error!("sfu renegotiação falhou após 3 tentativas — peer sem media nova");
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A camada base depende do tamanho da sala — e o `shift` (perda de
    /// pacotes reportada pelo subscritor) desce-a mais. Antes do `shift`, um
    /// participante em rede fraca recebia a camada cheia até o vídeo colapsar.
    #[test]
    fn layer_follows_room_size_and_loss() {
        assert_eq!(wanted_rid("video", 2, 0), "f");
        assert_eq!(wanted_rid("video", 6, 0), "h");
        assert_eq!(wanted_rid("video", 12, 0), "q");
        // Rede má num 1:1 → desce mesmo com a sala pequena.
        assert_eq!(wanted_rid("video", 2, 1), "h");
        assert_eq!(wanted_rid("video", 2, 2), "q");
        // Nunca abaixo da camada mais leve.
        assert_eq!(wanted_rid("video", 12, 2), "q");
        // Áudio e ecrã não têm simulcast.
        assert_eq!(wanted_rid("audio", 12, 2), "f");
        assert_eq!(wanted_rid("screen", 12, 2), "f");
    }

    #[test]
    fn quality_downgrades_fast_and_upgrades_slow() {
        let q = Quality::default();
        // Perda alta: precisa de BAD_TO_DOWNGRADE relatórios seguidos.
        for _ in 0..BAD_TO_DOWNGRADE - 1 {
            assert!(!q.observe(60));
        }
        assert!(q.observe(60));
        assert_eq!(q.shift.load(Relaxed), 1);

        // Um relatório limpo isolado NÃO faz subir logo.
        assert!(!q.observe(0));
        assert_eq!(q.shift.load(Relaxed), 1);
        for _ in 0..GOOD_TO_UPGRADE - 2 {
            assert!(!q.observe(0));
        }
        assert!(q.observe(0));
        assert_eq!(q.shift.load(Relaxed), 0);
    }

    /// O nível RFC 6464 é invertido (0 dBov = mais alto, 127 = silêncio) e o
    /// seletor ordena por energia — trocar o sentido silenciaria justamente
    /// quem está a falar.
    #[test]
    fn audio_energy_is_inverted_and_holds() {
        let loud = AudioMeter::default();
        let quiet = AudioMeter::default();
        loud.observe_level(10); // quase no máximo
        quiet.observe_level(120); // quase silêncio
        assert!(loud.energy() > quiet.energy());

        // Ataque imediato: um pico entra já no topo.
        quiet.observe_level(0);
        assert!(quiet.energy() >= 127);

        // Decaimento no TEMPO: uma pausa curta entre palavras (≈600 ms = 3
        // ticks) não pode tirar ninguém do top-N — senão a fala corta-se a meio.
        for _ in 0..3 {
            quiet.decay();
        }
        assert!(
            quiet.energy() > 60,
            "decaimento demasiado rápido: cortaria a fala nas pausas"
        );
        // Mas um silêncio real (≈4 s = 20 ticks) liberta mesmo o lugar.
        for _ in 0..20 {
            quiet.decay();
        }
        assert!(
            quiet.energy() < 5,
            "silêncio prolongado tem de libertar o top-N"
        );

        // Com DTX não chegam pacotes: sem `decay()` a energia ficaria presa.
        let silent_with_dtx = AudioMeter::default();
        silent_with_dtx.observe_level(0);
        let peak = silent_with_dtx.energy();
        for _ in 0..20 {
            silent_with_dtx.decay();
        }
        assert!(silent_with_dtx.energy() < peak / 4);
    }

    /// A renumeração tem de ser contígua: é ela que impede que a supressão de
    /// oradores apareça como PERDA ao recetor (e baixe o vídeo por engano).
    #[test]
    fn out_seq_is_contiguous_and_wraps() {
        let p = AudioMeter::default();
        assert_eq!(p.next_seq(), 0);
        assert_eq!(p.next_seq(), 1);
        p.out_seq.store(65535, Relaxed);
        assert_eq!(p.next_seq(), 65535);
        assert_eq!(p.next_seq(), 0, "tem de dar a volta em u16");
    }

    #[test]
    fn quality_never_goes_below_max_shift() {
        let q = Quality::default();
        for _ in 0..100 {
            q.observe(200);
        }
        assert_eq!(q.shift.load(Relaxed), MAX_SHIFT);
    }

    /// `pick_layer` NÃO pode trocar de camada só porque a desejada ainda não
    /// chegou: no arranque de um publicador as camadas chegam `q`→`h`→`f` e
    /// isso provocaria uma renegociação por cada uma.
    #[test]
    fn renegotiate_flag_is_restored_when_the_queue_rejects() {
        let flag = AtomicBool::new(false);

        // Caminho normal: entra em fila, bandeira fica levantada.
        assert!(coalesce_renegotiate(&flag, || true));
        assert!(flag.load(Relaxed));

        // Segundo pedido enquanto o primeiro não foi processado: coalescido.
        assert!(!coalesce_renegotiate(&flag, || panic!(
            "não devia tentar enviar"
        )));

        // Processado — bandeira baixa (é o que a negotiation_loop faz).
        flag.store(false, Relaxed);

        // Fila cheia: o pedido perde-se MAS a bandeira tem de voltar a baixo,
        // senão o peer nunca mais renegoceia (fica sem media nova para sempre).
        assert!(!coalesce_renegotiate(&flag, || false));
        assert!(
            !flag.load(Relaxed),
            "bandeira presa a true após falha de envio = peer sem renegociação para sempre"
        );

        // E o pedido seguinte volta a poder ser enviado.
        assert!(coalesce_renegotiate(&flag, || true));
    }

    #[test]
    fn pick_layer_keeps_current_while_wanted_is_missing() {
        let layers = ["q", "h"];
        // Quer "f", já está em "h" e "f" ainda não existe → mantém.
        assert_eq!(pick_layer_idx(&layers, "f", Some("h")), None);
        // Quer "h" e "h" existe → troca.
        assert_eq!(pick_layer_idx(&layers, "h", Some("q")), Some(1));
        // Sem subscrição ainda → liga à melhor disponível.
        assert_eq!(pick_layer_idx(&layers, "f", None), Some(1));
        // A camada em uso desapareceu → cai para a melhor que sobra.
        assert_eq!(pick_layer_idx(&["q"], "f", Some("h")), Some(0));
        // Nada publicado → nada a escolher.
        assert_eq!(pick_layer_idx(&[], "f", None), None);
        // Camada exata presente ganha sempre.
        assert_eq!(pick_layer_idx(&["q", "h", "f"], "q", Some("f")), Some(0));
    }
}
