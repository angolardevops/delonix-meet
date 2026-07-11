//! SFU (Selective Forwarding Unit) em Rust puro com `webrtc-rs`.
//!
//! Cada participante liga um único RTCPeerConnection ao servidor e publica
//! as suas tracks; o SFU encaminha os pacotes RTP para os restantes
//! participantes sem descodificar media (DTLS/SRTP termina aqui, o conteúdo
//! não é inspecionado). Escala O(n) uplinks por sala em vez do O(n²) do mesh.
//!
//! Negociação: o cliente faz a oferta inicial (publicação); a partir daí o
//! servidor é sempre o ofertante (subscrições novas/removidas), com as
//! renegociações serializadas por peer para evitar glare.

use dashmap::DashMap;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot, Mutex};
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
    rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication,
    rtp_transceiver::{
        rtp_codec::{RTCRtpHeaderExtensionCapability, RTPCodecType},
        rtp_sender::RTCRtpSender,
    },
    track::{
        track_local::{track_local_static_rtp::TrackLocalStaticRTP, TrackLocal, TrackLocalWriter},
        track_remote::TrackRemote,
    },
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
    remote: Arc<TrackRemote>,
    publisher_pc: Arc<RTCPeerConnection>,
    /// Writer da gravação server-side (ativo só durante uma gravação).
    rec: Mutex<Option<crate::recorder::RecWriter>>,
}

struct SfuPeer {
    pc: Arc<RTCPeerConnection>,
    /// Este peer anunciou partilha de ecrã: a próxima track de vídeo SEM rid
    /// é o ecrã (a câmara publica sempre com rids de simulcast).
    sharing: std::sync::atomic::AtomicBool,
    out: mpsc::UnboundedSender<ServerMsg>,
    renegotiate_tx: mpsc::UnboundedSender<()>,
    /// Slot onde o handler de `sfu-answer` entrega a resposta à renegociação.
    answer_slot: Arc<Mutex<Option<oneshot::Sender<String>>>>,
    pending_ice: Mutex<Vec<RTCIceCandidateInit>>,
    /// Subscrições ativas deste peer: (publisher, kind) -> (rid, sender).
    /// Garante 1 camada por publicador/tipo e permite trocar de camada.
    subscribed: Mutex<HashMap<(Uuid, String), (String, Arc<RTCRtpSender>)>>,
}

/// Camada desejada consoante o tamanho da sala: salas grandes recebem
/// camadas mais leves — é isto que faz o SFU escalar em downlink.
fn wanted_rid(kind: &str, room_size: usize) -> &'static str {
    if kind != "video" {
        return "f";
    }
    match room_size {
        n if n > 8 => "q",
        n if n > 4 => "h",
        _ => "f",
    }
}

#[derive(Default)]
struct SfuRoom {
    peers: Mutex<HashMap<Uuid, Arc<SfuPeer>>>,
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
    /// Config de conectividade ICE do SFU (IP externo, STUN/TURN).
    ice: IceConfig,
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
    /// Constrói o SFU com a config de ICE (a partir de `Config`).
    pub fn new(ice: IceConfig) -> Self {
        Self { ice, ..Default::default() }
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
    settings
        .set_udp_network(webrtc::ice::udp_network::UDPNetwork::Ephemeral(
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

        // Stub: A futura lógica criará o TrackLocalStaticRTP e bombeará
        // os pacotes do `socket` para dentro da sala.
        tracing::info!(%room_id, port, "PSTN Phantom RTP Listener bound");

        Ok(port)
    }

    pub async fn add_peer(
        self: &Arc<Self>,
        room_id: Uuid,
        peer_id: Uuid,
        out: mpsc::UnboundedSender<ServerMsg>,
    ) -> Result<()> {
        let room = self
            .rooms
            .entry(room_id)
            .or_insert_with(|| Arc::new(SfuRoom::default()))
            .clone();

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

        let (renegotiate_tx, renegotiate_rx) = mpsc::unbounded_channel();
        let peer = Arc::new(SfuPeer {
            pc: pc.clone(),
            sharing: std::sync::atomic::AtomicBool::new(false),
            out: out.clone(),
            renegotiate_tx,
            answer_slot: Arc::new(Mutex::new(None)),
            pending_ice: Mutex::new(Vec::new()),
            subscribed: Mutex::new(HashMap::new()),
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
            pc.on_track(Box::new(move |remote, _receiver, _transceiver| {
                let state = state.clone();
                Box::pin(async move {
                    state.handle_publish(room_id, peer_id, remote).await;
                })
            }));
        }

        {
            let state = self.clone();
            pc.on_peer_connection_state_change(Box::new(move |s| {
                tracing::info!(%room_id, %peer_id, state = %s, "sfu pc state");
                let state = state.clone();
                Box::pin(async move {
                    if s == RTCPeerConnectionState::Failed {
                        state.remove_peer(room_id, peer_id).await;
                    }
                })
            }));
        }

        // Task de renegociação serializada deste peer.
        tokio::spawn(renegotiation_loop(peer.clone(), renegotiate_rx));

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
            // Oferta inicial do cliente (publicação das suas tracks).
            ClientMsg::SfuOffer { sdp } => {
                let offer = RTCSessionDescription::offer(sdp)?;
                peer.pc.set_remote_description(offer).await?;
                flush_pending_ice(&peer).await;
                let answer = peer.pc.create_answer(None).await?;
                peer.pc.set_local_description(answer).await?;
                if let Some(local) = peer.pc.local_description().await {
                    let _ = peer.out.send(ServerMsg::SfuAnswer { sdp: local.sdp });
                }
                // Subscrever este peer a tudo o que já é publicado na sala.
                self.subscribe_to_existing(room_id, peer_id).await;
            }
            // Resposta do cliente a uma renegociação iniciada pelo servidor.
            ClientMsg::SfuAnswer { sdp } => {
                if let Some(tx) = peer.answer_slot.lock().await.take() {
                    let _ = tx.send(sdp);
                }
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
        let sharing_now = pub_peer.sharing.load(std::sync::atomic::Ordering::Relaxed);
        let is_screen = raw_kind == "video" && remote.rid().is_empty() && sharing_now;
        let is_screen_audio = raw_kind == "audio" && sharing_now && {
            // já existe um áudio (microfone) deste publicador?
            let pubs = room.publications.lock().await;
            let mut has_mic = false;
            for p in pubs.iter() {
                if p.publisher == publisher && p.kind == "audio" {
                    has_mic = true;
                    break;
                }
            }
            has_mic
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

        let publication = Arc::new(Publication {
            publisher,
            kind: kind.clone(),
            rid,
            subscribers: Mutex::new(HashMap::new()),
            remote: remote.clone(),
            publisher_pc: pub_peer.pc.clone(),
            rec: Mutex::new(None),
        });
        room.publications.lock().await.push(publication.clone());

        // Gravação a decorrer? Anexa um writer a esta track nova.
        // Áudio grava sempre; vídeo grava UMA camada por publicador — a
        // melhor que aparecer (h/f; nem sempre o browser envia a "f").
        {
            let mut rec_guard = room.recording.lock().await;
            if rec_guard.is_some() {
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

        // Subscrever todos os peers atuais (exceto o próprio publisher):
        // primeira camada a chegar liga já; se depois chegar a camada
        // desejada para o tamanho da sala, troca.
        let peers: Vec<(Uuid, Arc<SfuPeer>)> = room
            .peers
            .lock()
            .await
            .iter()
            .filter(|(id, _)| **id != publisher)
            .map(|(id, p)| (*id, p.clone()))
            .collect();
        let room_size = peers.len() + 1;
        // Diagnóstico (partilha de ecrã / novas tracks): que track publicou e
        // para quantos subscritores vai. `kind=screen` confirma a deteção.
        tracing::info!(%room_id, %publisher, kind = %publication.kind, rid = %publication.rid, subscribers = peers.len(), "sfu publish → fan-out");
        for (sub_id, sub_peer) in peers {
            if let Err(e) =
                consider_subscribe(&room, &publication, sub_id, &sub_peer, room_size).await
            {
                tracing::warn!(%room_id, %sub_id, error = %e, "sfu subscribe failed");
            }
        }

        // PLI periódico para vídeo: força keyframes para novos subscritores.
        if kind == "video" || kind == "screen" {
            let pc = pub_peer.pc.clone();
            let ssrc = remote.ssrc();
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_secs(3));
                loop {
                    ticker.tick().await;
                    if pc
                        .write_rtcp(&[Box::new(PictureLossIndication {
                            sender_ssrc: 0,
                            media_ssrc: ssrc,
                        })])
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }

        // Bomba de RTP: remota -> todas as tracks locais subscritas
        // (+ tee para o writer de gravação, se ativo).
        let this = self.clone();
        let is_audio = kind == "audio";

        tokio::spawn(async move {
            let pstn_socket = if is_audio {
                tokio::net::UdpSocket::bind("0.0.0.0:0").await.ok()
            } else {
                None
            };
            loop {
                match publication.remote.read_rtp().await {
                    Ok((packet, _)) => {
                        {
                            let subs = publication.subscribers.lock().await;
                            for (track, _) in subs.values() {
                                let _ = track.write_rtp(&packet).await;
                            }
                        }
                        if let Some(w) = publication.rec.lock().await.as_mut() {
                            w.write_rtp(&packet);
                        }

                        // Envio PSTN (Outbound) se for áudio e houver um destino registado
                        if is_audio {
                            if let (Some(socket), Some(dest)) =
                                (&pstn_socket, this.pstn_outbounds.get(&room_id))
                            {
                                use webrtc::util::Marshal;
                                if let Ok(buf) = packet.marshal() {
                                    let _ = socket.send_to(&buf, *dest).await;
                                }
                            }
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

    /// Subscreve `peer_id` às publicações existentes, escolhendo a melhor
    /// camada disponível por (publicador, tipo) para o tamanho atual da sala.
    async fn subscribe_to_existing(&self, room_id: Uuid, peer_id: Uuid) {
        let Some(room) = self.rooms.get(&room_id).map(|r| r.clone()) else {
            return;
        };
        let Some(peer) = self.peer(room_id, peer_id).await else {
            return;
        };
        let room_size = room.peers.lock().await.len();
        let publications = room.publications.lock().await.clone();

        // Agrupar camadas por (publicador, tipo) e escolher uma.
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
        for ((_, kind), layers) in groups {
            let wanted = wanted_rid(&kind, room_size);
            let chosen = layers
                .iter()
                .find(|p| p.rid == wanted)
                .or_else(|| layers.iter().find(|p| p.rid == "f"))
                .or_else(|| layers.iter().find(|p| p.rid == "h"))
                .or_else(|| layers.first());
            let Some(publication) = chosen else { continue };
            if let Err(e) = subscribe_layer(publication, peer_id, &peer).await {
                tracing::warn!(%room_id, %peer_id, error = %e, "sfu subscribe existing failed");
            }
            // Keyframe imediato para o novo subscritor.
            if publication.kind == "video" {
                let _ = publication
                    .publisher_pc
                    .write_rtcp(&[Box::new(PictureLossIndication {
                        sender_ssrc: 0,
                        media_ssrc: publication.remote.ssrc(),
                    })])
                    .await;
            }
        }
    }

    async fn unpublish(&self, room_id: Uuid, publication: &Arc<Publication>) {
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
                // A camada que este peer via desapareceu — cai para outra.
                let fallback = remaining
                    .iter()
                    .find(|p| p.rid == "f")
                    .or_else(|| remaining.iter().find(|p| p.rid == "h"))
                    .or_else(|| remaining.first());
                if let Some(next) = fallback {
                    let _ = subscribe_layer(next, sub_id, sub_peer).await;
                } else {
                    let _ = sub_peer.renegotiate_tx.send(());
                }
            }
        }
        tracing::info!(%room_id, publisher = %publication.publisher, kind = %publication.kind, rid = %publication.rid, "sfu track unpublished");
    }

    /// Remove um peer. Se a sala ficar vazia, devolve a sessão de gravação
    /// em curso (se houver) para o caller finalizar — a sala é apagada aqui.
    pub async fn remove_peer(
        &self,
        room_id: Uuid,
        peer_id: Uuid,
    ) -> Option<crate::recorder::RecordingSession> {
        let Some(room) = self.rooms.get(&room_id).map(|r| r.clone()) else {
            return None;
        };
        let Some(peer) = room.peers.lock().await.remove(&peer_id) else {
            return None;
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
            publication.subscribers.lock().await.remove(&peer_id);
        }
        peer.subscribed.lock().await.clear();

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
        orphan_recording
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

/// Liga uma publicação (camada concreta) a um subscritor e renegoceia.
async fn subscribe_layer(
    publication: &Arc<Publication>,
    sub_id: Uuid,
    sub_peer: &Arc<SfuPeer>,
) -> Result<()> {
    let key = (publication.publisher, publication.kind.clone());
    let mut subscribed = sub_peer.subscribed.lock().await;
    if subscribed.contains_key(&key) {
        return Ok(()); // já ligado a uma camada deste publicador/tipo
    }
    let mut subs = publication.subscribers.lock().await;
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
    // Drenar RTCP do sender (obrigatório para os interceptors funcionarem).
    {
        let sender = sender.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 1500];
            while sender.read(&mut buf).await.is_ok() {}
        });
    }
    subs.insert(sub_id, (local, sender.clone()));
    subscribed.insert(key, (publication.rid.clone(), sender));
    drop(subs);
    drop(subscribed);
    tracing::info!(%sub_id, publisher = %publication.publisher, kind = %publication.kind, "sfu subscribe_layer → renegotiate subscritor");
    let _ = sub_peer.renegotiate_tx.send(());
    Ok(())
}

/// Decide a subscrição quando chega uma camada nova: liga se ainda não há
/// nada deste (publicador, tipo); troca se esta é a camada desejada para o
/// tamanho atual da sala e a atual não é.
async fn consider_subscribe(
    room: &Arc<SfuRoom>,
    publication: &Arc<Publication>,
    sub_id: Uuid,
    sub_peer: &Arc<SfuPeer>,
    room_size: usize,
) -> Result<()> {
    let key = (publication.publisher, publication.kind.clone());
    let wanted = wanted_rid(&publication.kind, room_size);

    let current = sub_peer.subscribed.lock().await.get(&key).cloned();
    match current {
        None => subscribe_layer(publication, sub_id, sub_peer).await,
        Some((cur_rid, old_sender)) if publication.rid == wanted && cur_rid != wanted => {
            // Trocar de camada: solta a antiga, liga a nova, renegoceia uma vez.
            let publications = room.publications.lock().await.clone();
            for p in &publications {
                if p.publisher == publication.publisher
                    && p.kind == publication.kind
                    && p.rid == cur_rid
                {
                    p.subscribers.lock().await.remove(&sub_id);
                }
            }
            let _ = sub_peer.pc.remove_track(&old_sender).await;
            sub_peer.subscribed.lock().await.remove(&key);
            tracing::info!(publisher = %publication.publisher, from = %cur_rid, to = %publication.rid, "sfu layer switch");
            subscribe_layer(publication, sub_id, sub_peer).await
        }
        Some(_) => Ok(()),
    }
}

async fn flush_pending_ice(peer: &Arc<SfuPeer>) {
    let pending: Vec<RTCIceCandidateInit> = peer.pending_ice.lock().await.drain(..).collect();
    for init in pending {
        let _ = peer.pc.add_ice_candidate(init).await;
    }
}

/// Renegociações server-driven, uma de cada vez por peer: oferta -> espera
/// pela resposta do cliente (com timeout) -> próxima.
async fn renegotiation_loop(peer: Arc<SfuPeer>, mut rx: mpsc::UnboundedReceiver<()>) {
    while rx.recv().await.is_some() {
        // Colapsar pedidos acumulados numa só renegociação.
        while rx.try_recv().is_ok() {}

        // A oferta inicial do cliente pode ainda estar em curso.
        for _ in 0..50 {
            if peer.pc.remote_description().await.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        let offer = match peer.pc.create_offer(None).await {
            Ok(o) => o,
            Err(e) => {
                tracing::warn!(error = %e, "sfu create_offer failed");
                continue;
            }
        };
        if let Err(e) = peer.pc.set_local_description(offer).await {
            tracing::warn!(error = %e, "sfu set_local failed");
            continue;
        }
        let Some(local) = peer.pc.local_description().await else { continue };

        let (tx, answer_rx) = oneshot::channel();
        *peer.answer_slot.lock().await = Some(tx);
        let _ = peer.out.send(ServerMsg::SfuOffer { sdp: local.sdp });

        match tokio::time::timeout(Duration::from_secs(10), answer_rx).await {
            Ok(Ok(sdp)) => match RTCSessionDescription::answer(sdp) {
                Ok(answer) => {
                    if let Err(e) = peer.pc.set_remote_description(answer).await {
                        tracing::warn!(error = %e, "sfu set_remote answer failed");
                    }
                }
                Err(e) => tracing::warn!(error = %e, "sfu bad answer"),
            },
            _ => tracing::warn!("sfu renegotiation timed out"),
        }
        *peer.answer_slot.lock().await = None;
    }
}
