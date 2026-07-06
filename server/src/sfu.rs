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
        interceptor_registry::register_default_interceptors, media_engine::MediaEngine, APIBuilder,
    },
    ice_transport::ice_candidate::RTCIceCandidateInit,
    interceptor::registry::Registry,
    peer_connection::{
        peer_connection_state::RTCPeerConnectionState,
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
}

struct SfuPeer {
    pc: Arc<RTCPeerConnection>,
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
}

#[derive(Default)]
pub struct SfuState {
    rooms: DashMap<Uuid, Arc<SfuRoom>>,
}

fn new_api() -> Result<webrtc::api::API> {
    let mut media = MediaEngine::default();
    media.register_default_codecs()?;
    // Header extensions necessárias para receber simulcast (mid + rid).
    for uri in [
        "urn:ietf:params:rtp-hdrext:sdes:mid",
        "urn:ietf:params:rtp-hdrext:sdes:rtp-stream-id",
        "urn:ietf:params:rtp-hdrext:sdes:repaired-rtp-stream-id",
    ] {
        media.register_header_extension(
            RTCRtpHeaderExtensionCapability { uri: uri.to_owned() },
            RTPCodecType::Video,
            None,
        )?;
    }
    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut media)?;
    Ok(APIBuilder::new()
        .with_media_engine(media)
        .with_interceptor_registry(registry)
        .build())
}

impl SfuState {
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

        let api = new_api()?;
        // Host candidates chegam para dev/local; em produção configurar
        // SettingEngine com NAT 1:1 / porta UDP fixa.
        let pc = Arc::new(api.new_peer_connection(Default::default()).await?);

        let (renegotiate_tx, renegotiate_rx) = mpsc::unbounded_channel();
        let peer = Arc::new(SfuPeer {
            pc: pc.clone(),
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
    async fn handle_publish(self: Arc<Self>, room_id: Uuid, publisher: Uuid, remote: Arc<TrackRemote>) {
        let Some(room) = self.rooms.get(&room_id).map(|r| r.clone()) else {
            return;
        };
        let Some(pub_peer) = self.peer(room_id, publisher).await else {
            return;
        };
        let kind = remote.kind().to_string();
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
        });
        room.publications.lock().await.push(publication.clone());

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
        for (sub_id, sub_peer) in peers {
            if let Err(e) = consider_subscribe(&room, &publication, sub_id, &sub_peer, room_size).await {
                tracing::warn!(%room_id, %sub_id, error = %e, "sfu subscribe failed");
            }
        }

        // PLI periódico para vídeo: força keyframes para novos subscritores.
        if kind == "video" {
            let pc = pub_peer.pc.clone();
            let ssrc = remote.ssrc();
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_secs(3));
                loop {
                    ticker.tick().await;
                    if pc
                        .write_rtcp(&[Box::new(PictureLossIndication { sender_ssrc: 0, media_ssrc: ssrc })])
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }

        // Bomba de RTP: remota -> todas as tracks locais subscritas.
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                match publication.remote.read_rtp().await {
                    Ok((packet, _)) => {
                        let subs = publication.subscribers.lock().await;
                        for (track, _) in subs.values() {
                            let _ = track.write_rtp(&packet).await;
                        }
                    }
                    Err(_) => break, // track terminou
                }
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
            groups.entry((p.publisher, p.kind.clone())).or_default().push(p);
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

    pub async fn remove_peer(&self, room_id: Uuid, peer_id: Uuid) {
        let Some(room) = self.rooms.get(&room_id).map(|r| r.clone()) else {
            return;
        };
        let Some(peer) = room.peers.lock().await.remove(&peer_id) else {
            return;
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
        if empty {
            self.rooms.remove_if(&room_id, |_, _| true);
        }
        tracing::info!(%room_id, %peer_id, "sfu peer removed");
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
    let local = Arc::new(TrackLocalStaticRTP::new(
        publication.remote.codec().capability.clone(),
        format!("{}-{}-{}", publication.publisher, publication.kind, publication.rid),
        // stream_id = peer_id do publisher: é assim que o cliente mapeia
        // a track recebida para o participante certo.
        publication.publisher.to_string(),
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
                if p.publisher == publication.publisher && p.kind == publication.kind && p.rid == cur_rid {
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
            Ok(Ok(sdp)) => {
                match RTCSessionDescription::answer(sdp) {
                    Ok(answer) => {
                        if let Err(e) = peer.pc.set_remote_description(answer).await {
                            tracing::warn!(error = %e, "sfu set_remote answer failed");
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "sfu bad answer"),
                }
            }
            _ => tracing::warn!("sfu renegotiation timed out"),
        }
        *peer.answer_slot.lock().await = None;
    }
}
