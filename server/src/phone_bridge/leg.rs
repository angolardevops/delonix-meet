//! Uma perna da ponte: UMA chamada de fora da app ligada a UMA sala.
//!
//! ```text
//!   FreeSWITCH ──RTP G.711──▶ socket UDP ─▶ Ingress (G.711→Opus) ─▶ SFU: publicação de áudio
//!   FreeSWITCH ◀─RTP G.711── socket UDP ◀─ Mixer (mix-minus)     ◀─ SFU: microfones da sala
//! ```
//!
//! Uma só tarefa por perna e um só socket (RTP simétrico: o endereço de onde
//! o FreeSWITCH envia é para onde a mistura volta). Não há SIP aqui — a
//! sinalização da chamada é do FreeSWITCH e da telefonia (ADR-0009); a ponte
//! só recebe e devolve media.

use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;
use webrtc::rtp::{header::Header, packet::Packet};
use webrtc::util::{Marshal, Unmarshal};

use super::audio::{encode_mix, Ingress, Mixer, FRAME_8K};
use super::g711::Law;
use super::quality::RtpQuality;
use crate::sfu::{BridgePacket, SfuState};
use delonix_meet_domain::conferencing::channels::weak_link;

/// Tipo de payload dinâmico que a ponte usa para o Opus. O `TrackLocalStaticRTP`
/// reescreve-o para o PT negociado com cada subscritor.
const OPUS_PT: u8 = 111;

/// O que a perna conta, sem lock: lido pelas métricas e pelo teste de CPU.
#[derive(Default, Debug)]
pub struct LegStats {
    /// Pacotes RTP G.711 aceites do telefone.
    pub packets_in: AtomicU64,
    /// Pacotes recusados: origem não autorizada, RTP inválido, codec não G.711.
    pub packets_rejected: AtomicU64,
    /// Pacotes Opus entregues à sala.
    pub frames_published: AtomicU64,
    /// Pacotes G.711 misturados devolvidos ao telefone.
    pub packets_out: AtomicU64,
    /// Tempo de CPU gasto nos codecs e na mistura (ns). É o custo da ponte por
    /// chamada, fora o do SFU.
    pub codec_nanos: AtomicU64,
    /// Pior tempo entre receber um pacote do telefone e entregá-lo à sala (µs).
    pub ingress_max_micros: AtomicU64,
    /// Jitter medido (µs) e perda (partes por milhão) — para o crachá.
    pub jitter_micros: AtomicU64,
    pub loss_ppm: AtomicU64,
}

/// Configuração de uma perna.
#[derive(Debug, Clone)]
pub struct LegConfig {
    pub room_id: Uuid,
    pub leg_id: Uuid,
    /// Onde escutar. Em produção, o IP da rede interna do FreeSWITCH.
    pub bind: SocketAddr,
    /// IPs de onde se aceita RTP (os FreeSWITCH). Vazio = recusa tudo: uma
    /// porta UDP aberta que publica numa sala o que lhe chegar seria uma porta
    /// para dentro de qualquer reunião.
    pub allowed_sources: Vec<IpAddr>,
    /// Lei G.711 a usar para a mistura antes de o telefone mandar o primeiro
    /// pacote (depois segue a dele). As operadoras angolanas usam lei A.
    pub default_law: Law,
    /// Para onde mandar a mistura antes do primeiro pacote chegar (o endereço
    /// do SDP). Depois segue a origem real do RTP (RTP simétrico).
    pub initial_remote: Option<SocketAddr>,
}

/// Evento da perna para quem a gere (canais da sala).
#[derive(Debug, Clone, PartialEq)]
pub enum LegEvent {
    /// Chegou o primeiro áudio do telefone.
    FirstMedia,
    /// A qualidade medida cruzou o limiar da «ligação fraca» (com histerese).
    Quality {
        weak: bool,
        jitter_ms: f64,
        loss: f64,
    },
}

/// Pega numa perna a correr.
pub struct LegHandle {
    pub local_addr: SocketAddr,
    pub stats: Arc<LegStats>,
    muted: Arc<AtomicBool>,
    stop: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

impl LegHandle {
    /// Silenciar NA PONTE: o áudio do telefone deixa de entrar na sala. O
    /// telefone continua a ouvir a reunião.
    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Relaxed);
    }

    pub fn muted(&self) -> bool {
        self.muted.load(Relaxed)
    }

    /// Termina a perna e espera que a sala a largue.
    pub async fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.task.await;
    }
}

/// Arranca uma perna: abre o socket, publica na sala e começa a misturar.
pub async fn start(
    sfu: Arc<SfuState>,
    cfg: LegConfig,
    events: mpsc::Sender<LegEvent>,
) -> std::io::Result<LegHandle> {
    let socket = UdpSocket::bind(cfg.bind).await?;
    let local_addr = socket.local_addr()?;
    let stats = Arc::new(LegStats::default());
    let muted = Arc::new(AtomicBool::new(false));
    let (stop, stop_rx) = oneshot::channel();
    let publish = sfu.publish_bridge_audio(cfg.room_id, cfg.leg_id).await;
    let taps = sfu.tap_room_audio(cfg.room_id, cfg.leg_id);
    let ingress = Ingress::new().map_err(std::io::Error::other)?;
    tracing::info!(room = %cfg.room_id, leg = %cfg.leg_id, %local_addr, "ponte: perna aberta");
    let task = tokio::spawn(run(Run {
        sfu,
        cfg,
        socket,
        publish,
        taps,
        ingress,
        stats: stats.clone(),
        muted: muted.clone(),
        stop: stop_rx,
        events,
    }));
    Ok(LegHandle {
        local_addr,
        stats,
        muted,
        stop,
        task,
    })
}

struct Run {
    sfu: Arc<SfuState>,
    cfg: LegConfig,
    socket: UdpSocket,
    publish: mpsc::Sender<BridgePacket>,
    taps: mpsc::Receiver<crate::sfu::TapPacket>,
    ingress: Ingress,
    stats: Arc<LegStats>,
    muted: Arc<AtomicBool>,
    stop: oneshot::Receiver<()>,
    events: mpsc::Sender<LegEvent>,
}

async fn run(mut r: Run) {
    let mut mixer = Mixer::new(r.cfg.leg_id);
    let mut quality = RtpQuality::new(8000);
    let mut weak = false;
    let mut remote: Option<SocketAddr> = r.cfg.initial_remote;
    let mut law = r.cfg.default_law;
    // Numeração e relógio próprios nos dois sentidos, a partir de valores
    // aleatórios (RFC 3550 §5.1).
    let mut out_seq: u16 = rand::random();
    let out_ts_base: u32 = rand::random();
    let out_ssrc: u32 = rand::random();
    let mix_ssrc: u32 = rand::random();
    let mut mix_seq: u16 = rand::random();
    let mut mix_ts: u32 = rand::random();
    let mut first_media = false;
    let mut buf = vec![0u8; 1500];
    let mut g711_out = Vec::with_capacity(FRAME_8K);
    let mut tick = tokio::time::interval(Duration::from_millis(20));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut quality_tick = tokio::time::interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            _ = &mut r.stop => break,
            recv = r.socket.recv_from(&mut buf) => {
                let Ok((n, from)) = recv else { break };
                let arrived = Instant::now();
                if !r.cfg.allowed_sources.contains(&from.ip()) {
                    r.stats.packets_rejected.fetch_add(1, Relaxed);
                    continue;
                }
                let mut raw = &buf[..n];
                let Ok(pkt) = Packet::unmarshal(&mut raw) else {
                    r.stats.packets_rejected.fetch_add(1, Relaxed);
                    continue;
                };
                // DTMF (RFC 4733, PT dinâmico) e conforto de ruído chegam pelo
                // mesmo socket: não são voz, ignoram-se sem contar como erro.
                let Some(pkt_law) = Law::from_payload_type(pkt.header.payload_type) else {
                    continue;
                };
                // RTP simétrico: a mistura volta para onde veio o áudio. Se
                // o FreeSWITCH mudar de porta (re-INVITE), segue-se.
                remote = Some(from);
                law = pkt_law;
                r.stats.packets_in.fetch_add(1, Relaxed);
                quality.observe_at(pkt.header.sequence_number, pkt.header.timestamp, arrived);
                if !first_media {
                    first_media = true;
                    let _ = r.events.try_send(LegEvent::FirstMedia);
                }
                if r.muted.load(Relaxed) {
                    continue;
                }
                let t = Instant::now();
                let frames = r.ingress.push(pkt_law, &pkt.payload, pkt.header.timestamp);
                r.stats.codec_nanos.fetch_add(t.elapsed().as_nanos() as u64, Relaxed);
                for f in frames {
                    let packet = Packet {
                        header: Header {
                            version: 2,
                            marker: out_seq == 0,
                            payload_type: OPUS_PT,
                            sequence_number: out_seq,
                            timestamp: out_ts_base.wrapping_add(f.timestamp),
                            ssrc: out_ssrc,
                            ..Default::default()
                        },
                        payload: bytes::Bytes::from(f.payload),
                    };
                    out_seq = out_seq.wrapping_add(1);
                    if r.publish.try_send(BridgePacket { packet, level: f.level }).is_ok() {
                        r.stats.frames_published.fetch_add(1, Relaxed);
                    }
                }
                let us = arrived.elapsed().as_micros() as u64;
                r.stats.ingress_max_micros.fetch_max(us, Relaxed);
            }
            Some(tap) = r.taps.recv() => {
                let t = Instant::now();
                mixer.push(tap.publisher, tap.seq, &tap.payload);
                r.stats.codec_nanos.fetch_add(t.elapsed().as_nanos() as u64, Relaxed);
            }
            _ = tick.tick() => {
                let Some(dest) = remote else { continue };
                let t = Instant::now();
                let mixed = mixer.tick();
                encode_mix(law, mixed, &mut g711_out);
                r.stats.codec_nanos.fetch_add(t.elapsed().as_nanos() as u64, Relaxed);
                let packet = Packet {
                    header: Header {
                        version: 2,
                        payload_type: law.payload_type(),
                        sequence_number: mix_seq,
                        timestamp: mix_ts,
                        ssrc: mix_ssrc,
                        ..Default::default()
                    },
                    payload: bytes::Bytes::copy_from_slice(&g711_out),
                };
                mix_seq = mix_seq.wrapping_add(1);
                mix_ts = mix_ts.wrapping_add(FRAME_8K as u32);
                if let Ok(bytes) = packet.marshal() {
                    if r.socket.send_to(&bytes, dest).await.is_ok() {
                        r.stats.packets_out.fetch_add(1, Relaxed);
                    }
                }
            }
            _ = quality_tick.tick() => {
                if !first_media {
                    continue;
                }
                let (jitter_ms, loss) = (quality.jitter_ms(), quality.loss());
                r.stats.jitter_micros.store((jitter_ms * 1000.0) as u64, Relaxed);
                r.stats.loss_ppm.store((loss * 1_000_000.0) as u64, Relaxed);
                let now_weak = weak_link(weak, jitter_ms, loss);
                if now_weak != weak {
                    weak = now_weak;
                    let _ = r.events.try_send(LegEvent::Quality { weak, jitter_ms, loss });
                }
            }
        }
    }
    drop(r.publish);
    r.sfu.end_bridge(r.cfg.room_id, r.cfg.leg_id).await;
    tracing::info!(
        room = %r.cfg.room_id,
        leg = %r.cfg.leg_id,
        packets_in = r.stats.packets_in.load(Relaxed),
        packets_out = r.stats.packets_out.load(Relaxed),
        rejected = r.stats.packets_rejected.load(Relaxed),
        decode_errors = mixer.decode_errors,
        "ponte: perna fechada"
    );
}
