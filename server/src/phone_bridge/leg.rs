//! Uma perna da ponte: UMA chamada de fora da app ligada a UMA sala.
//!
//! ```text
//!   FreeSWITCH ──RTP G.711──▶ socket UDP ─▶ Ingress (G.711→Opus) ─▶ SFU: publicação de áudio
//!   FreeSWITCH ◀─RTP G.711── socket UDP ◀─ Mixer (mix-minus)     ◀─ SFU: microfones da sala
//! ```
//!
//! Quando o diálogo SIP negoceia Opus (ADR-0017) a perna é a mesma e os dois
//! sentidos mudam de forma — nenhum pacote é recodificado à entrada:
//!
//! ```text
//!   FreeSWITCH ──RTP Opus──▶ socket UDP ─▶ Passthrough (valida, mede) ─▶ SFU: o MESMO payload
//!   FreeSWITCH ◀─RTP Opus── socket UDP ◀─ Mixer a 16 kHz → MixEncoder ◀─ SFU: microfones da sala
//! ```
//!
//! Uma só tarefa por perna e um só socket (RTP simétrico: o endereço de onde
//! o FreeSWITCH envia é para onde a mistura volta). Não há SIP aqui — a
//! sinalização da chamada é do `sip.rs`; a perna só recebe e devolve media.
//!
//! **SRTP.** O que entra é desencriptado e o que sai é cifrado com o par de
//! contextos que o diálogo SIP negociou por SDES (`srtp.rs`). Uma chamada real
//! nunca chega aqui sem eles: o `sip.rs` recusa com `488` uma oferta sem
//! `a=crypto`. `None` existe para os testes do caminho de RTP (R221), que
//! provam a media e não a cifra — a cifra é provada em `srtp.rs::tests` e
//! contra o FreeSWITCH real (ADR-0010 §10).

use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;
use webrtc::rtp::{header::Header, packet::Packet};
use webrtc::util::{Marshal, Unmarshal};

use super::audio::{
    encode_mix, Ingress, MixEncoder, Mixer, Passthrough, FRAME_8K, OPUS_TS_PER_FRAME,
};
use super::g711::Law;
use super::quality::RtpQuality;
use super::srtp::{ip_allowed, SrtpSession};
use crate::sfu::{BridgePacket, SfuState};
use delonix_meet_domain::conferencing::channels::weak_link;

/// Tipo de payload dinâmico que a ponte usa para o Opus. O `TrackLocalStaticRTP`
/// reescreve-o para o PT negociado com cada subscritor.
const OPUS_PT: u8 = 111;

/// O que a perna conta, sem lock: lido pelas métricas e pelo teste de CPU.
#[derive(Default, Debug)]
pub struct LegStats {
    /// Pacotes RTP de voz aceites do telefone.
    pub packets_in: AtomicU64,
    /// Pacotes recusados: origem não autorizada, SRTP que não autentica, RTP
    /// inválido, Opus que o descodificador não lê.
    pub packets_rejected: AtomicU64,
    /// Pacotes recusados SÓ pelo SRTP (chave errada, repetição, truncados).
    /// Separado de `packets_rejected` porque diz outra coisa: não é um vizinho
    /// a varrer portas, é alguém a falar a cifra errada.
    pub packets_srtp_failed: AtomicU64,
    /// Tempo de CPU só da cifra e da decifra (ns) — para saber quanto do custo
    /// da ponte é SRTP e quanto são codecs.
    pub srtp_nanos: AtomicU64,
    /// Pacotes Opus entregues à sala.
    pub frames_published: AtomicU64,
    /// Pacotes da mistura devolvidos ao telefone.
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

/// O codec que o diálogo SIP negociou para a perna (ADR-0017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegCodec {
    /// G.711 a 8 kHz: a perna transcodifica nos dois sentidos (ADR-0010).
    G711(Law),
    /// Opus (RFC 7587) no tipo de payload que a oferta escolheu: o que vem do
    /// telefone passa intacto, e a mistura volta em banda larga.
    Opus { payload_type: u8 },
}

impl LegCodec {
    /// O relógio RTP da perna — é nele que o jitter se mede.
    fn clock_rate(self) -> u32 {
        match self {
            LegCodec::G711(_) => 8_000,
            LegCodec::Opus { .. } => 48_000,
        }
    }
}

/// Configuração de uma perna.
#[derive(Debug, Clone)]
pub struct LegConfig {
    pub room_id: Uuid,
    pub leg_id: Uuid,
    /// IPs de onde se aceita RTP (os FreeSWITCH). Vazio = recusa tudo: uma
    /// porta UDP aberta que publica numa sala o que lhe chegar seria uma porta
    /// para dentro de qualquer reunião.
    pub allowed_sources: Vec<IpAddr>,
    /// O codec negociado. Em G.711 é a lei a usar para a mistura antes de o
    /// telefone mandar o primeiro pacote (depois segue a dele); as operadoras
    /// angolanas usam lei A.
    pub codec: LegCodec,
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
    muted: Arc<AtomicBool>,
    /// Contadores da perna. Fora dos testes ninguém os lê ainda — o laço
    /// regista-os no log quando a perna fecha, e quem os vai expor é a consola
    /// dos canais, que vem com a frente da telefonia.
    #[cfg_attr(not(test), allow(dead_code))]
    pub stats: Arc<LegStats>,
    stop: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

impl LegHandle {
    /// O interruptor do silêncio desta perna.
    ///
    /// Silenciar NA PONTE: o áudio do telefone deixa de entrar na sala e o
    /// telefone continua a ouvir a reunião — é o que o `ForceMute` de um
    /// anfitrião faz a quem não tem cliente para o honrar (R224). Devolve-se o
    /// `Arc` em vez de um `set_muted(&self)` porque quem o acciona está a
    /// tratar uma mensagem do WebSocket e não pode esperar por `await` nenhum:
    /// a `SipBridge` guarda-o num registo síncrono (ver `PhoneControl`).
    pub fn mute_flag(&self) -> Arc<AtomicBool> {
        self.muted.clone()
    }

    /// Termina a perna e espera que a sala a largue.
    pub async fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.task.await;
    }
}

/// Arranca uma perna num socket JÁ ABERTO, publica na sala e começa a
/// misturar. O socket vem de fora porque quem escolhe a porta (o `sip.rs`,
/// que percorre o intervalo de RTP) tem de saber que ela abriu ANTES de gastar
/// a sessão SRTP da chamada — um `bind` falhado a meio não pode deixar a
/// chamada sem cifra.
pub async fn start(
    sfu: Arc<SfuState>,
    cfg: LegConfig,
    socket: UdpSocket,
    srtp: Option<SrtpSession>,
    events: mpsc::Sender<LegEvent>,
) -> std::io::Result<LegHandle> {
    let local_addr = socket.local_addr()?;
    let stats = Arc::new(LegStats::default());
    let muted = Arc::new(AtomicBool::new(false));
    let (stop, stop_rx) = oneshot::channel();
    let publish = sfu.publish_bridge_audio(cfg.room_id, cfg.leg_id).await;
    let taps = sfu.tap_room_audio(cfg.room_id, cfg.leg_id);
    // Os codificadores abrem-se ANTES de a tarefa arrancar: uma perna que não
    // consegue codificar não chega a responder `200`.
    let media = match cfg.codec {
        LegCodec::G711(law) => Media::G711 {
            ingress: Ingress::new().map_err(std::io::Error::other)?,
            mixer: Mixer::new(cfg.leg_id),
            law,
        },
        LegCodec::Opus { payload_type } => Media::Opus {
            pass: Passthrough::new(),
            mixer: Mixer::wideband(cfg.leg_id),
            encoder: MixEncoder::new().map_err(std::io::Error::other)?,
            payload_type,
            was_muted: false,
        },
    };
    tracing::info!(room = %cfg.room_id, leg = %cfg.leg_id, %local_addr, codec = ?cfg.codec, "ponte: perna aberta");
    let task = tokio::spawn(run(Run {
        sfu,
        cfg,
        socket,
        srtp,
        publish,
        taps,
        media,
        stats: stats.clone(),
        muted: muted.clone(),
        stop: stop_rx,
        events,
    }));
    Ok(LegHandle {
        local_addr,
        muted,
        stats,
        stop,
        task,
    })
}

struct Run {
    sfu: Arc<SfuState>,
    cfg: LegConfig,
    socket: UdpSocket,
    srtp: Option<SrtpSession>,
    publish: mpsc::Sender<BridgePacket>,
    taps: mpsc::Receiver<crate::sfu::TapPacket>,
    media: Media,
    stats: Arc<LegStats>,
    muted: Arc<AtomicBool>,
    stop: oneshot::Receiver<()>,
    events: mpsc::Sender<LegEvent>,
}

/// Os dois sentidos da media, conforme o codec da perna.
enum Media {
    G711 {
        ingress: Ingress,
        mixer: Mixer,
        /// A lei em uso: começa na da oferta e segue a do telefone.
        law: Law,
    },
    Opus {
        pass: Passthrough,
        mixer: Mixer,
        encoder: MixEncoder,
        payload_type: u8,
        /// O pacote anterior foi retido por a perna estar silenciada.
        was_muted: bool,
    },
}

impl Media {
    fn mixer(&mut self) -> &mut Mixer {
        match self {
            Media::G711 { mixer, .. } | Media::Opus { mixer, .. } => mixer,
        }
    }
}

async fn run(mut r: Run) {
    let mut quality = RtpQuality::new(r.cfg.codec.clock_rate());
    let mut weak = false;
    let mut remote: Option<SocketAddr> = r.cfg.initial_remote;
    // Numeração e relógio próprios nos dois sentidos, a partir de valores
    // aleatórios (RFC 3550 §5.1).
    let mut out_seq: u16 = rand::random();
    // Em Opus a sequência segue a do telefone: esta é só a base a que se soma.
    let out_seq_base: u16 = out_seq;
    let out_ts_base: u32 = rand::random();
    let out_ssrc: u32 = rand::random();
    let mix_ssrc: u32 = rand::random();
    let mut mix_seq: u16 = rand::random();
    let mut mix_ts: u32 = rand::random();
    let mut first_media = false;
    let mut buf = vec![0u8; 1500];
    let mut mix_out = Vec::with_capacity(FRAME_8K * 2);
    let mut tick = tokio::time::interval(Duration::from_millis(20));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut quality_tick = tokio::time::interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            _ = &mut r.stop => break,
            recv = r.socket.recv_from(&mut buf) => {
                let Ok((n, from)) = recv else { break };
                let arrived = Instant::now();
                if !ip_allowed(&r.cfg.allowed_sources, from.ip()) {
                    r.stats.packets_rejected.fetch_add(1, Relaxed);
                    continue;
                }
                // SRTP primeiro: um pacote que não autentica não chega ao
                // parser de RTP, e muito menos à sala.
                let claro: Option<bytes::Bytes> = match r.srtp.as_mut() {
                    Some(s) => {
                        let t = Instant::now();
                        let out = s.inbound.decrypt_rtp(&buf[..n]);
                        r.stats.srtp_nanos.fetch_add(t.elapsed().as_nanos() as u64, Relaxed);
                        match out {
                            Ok(b) => Some(b),
                            Err(_) => {
                                r.stats.packets_srtp_failed.fetch_add(1, Relaxed);
                                r.stats.packets_rejected.fetch_add(1, Relaxed);
                                continue;
                            }
                        }
                    }
                    None => None,
                };
                let mut raw: &[u8] = match &claro {
                    Some(b) => &b[..],
                    None => &buf[..n],
                };
                let Ok(pkt) = Packet::unmarshal(&mut raw) else {
                    r.stats.packets_rejected.fetch_add(1, Relaxed);
                    continue;
                };
                // DTMF (RFC 4733, PT dinâmico) e conforto de ruído chegam pelo
                // mesmo socket: não são voz, ignoram-se sem contar como erro.
                // Uma perna só aceita o codec que negociou.
                let pkt_law = match &mut r.media {
                    Media::G711 { law, .. } => {
                        let Some(l) = Law::from_payload_type(pkt.header.payload_type) else {
                            continue;
                        };
                        *law = l;
                        Some(l)
                    }
                    Media::Opus { payload_type, .. } => {
                        if pkt.header.payload_type != *payload_type {
                            continue;
                        }
                        None
                    }
                };
                // RTP simétrico: a mistura volta para onde veio o áudio. Se
                // o FreeSWITCH mudar de porta (re-INVITE), segue-se.
                remote = Some(from);
                r.stats.packets_in.fetch_add(1, Relaxed);
                quality.observe_at(pkt.header.sequence_number, pkt.header.timestamp, arrived);
                if !first_media {
                    first_media = true;
                    let _ = r.events.try_send(LegEvent::FirstMedia);
                }
                // Silenciado: o pacote CONTA (chegou, mede-se a qualidade e o
                // RTP simétrico segue-o) mas não entra na sala. Descartar antes
                // da estatística faria a perna parecer morta a quem a observa.
                let muted = r.muted.load(Relaxed);
                if let Media::Opus { pass, was_muted, .. } = &mut r.media {
                    // O que se reteve não é perda: ao voltar, a saída continua
                    // de onde ficou em vez de abrir um buraco na sequência.
                    if *was_muted && !muted {
                        pass.discontinuity();
                    }
                    *was_muted = muted;
                }
                if muted {
                    continue;
                }
                let t = Instant::now();
                match (&mut r.media, pkt_law) {
                    (Media::G711 { ingress, .. }, Some(pkt_law)) => {
                        let frames = ingress.push(pkt_law, &pkt.payload, pkt.header.timestamp);
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
                    }
                    (Media::Opus { pass, .. }, _) => {
                        // O payload sai como entrou; só o cabeçalho é nosso.
                        let passed = pass.push(
                            pkt.header.sequence_number,
                            pkt.header.timestamp,
                            &pkt.payload,
                        );
                        r.stats.codec_nanos.fetch_add(t.elapsed().as_nanos() as u64, Relaxed);
                        let Some(f) = passed else {
                            r.stats.packets_rejected.fetch_add(1, Relaxed);
                            continue;
                        };
                        let packet = Packet {
                            header: Header {
                                version: 2,
                                marker: f.seq == 0,
                                payload_type: OPUS_PT,
                                sequence_number: out_seq_base.wrapping_add(f.seq),
                                timestamp: out_ts_base.wrapping_add(f.timestamp),
                                ssrc: out_ssrc,
                                ..Default::default()
                            },
                            payload: pkt.payload.clone(),
                        };
                        if r.publish.try_send(BridgePacket { packet, level: f.level }).is_ok() {
                            r.stats.frames_published.fetch_add(1, Relaxed);
                        }
                    }
                    // Uma perna G.711 sem lei no pacote não chega aqui.
                    (Media::G711 { .. }, None) => {}
                }
                let us = arrived.elapsed().as_micros() as u64;
                r.stats.ingress_max_micros.fetch_max(us, Relaxed);
            }
            Some(tap) = r.taps.recv() => {
                let t = Instant::now();
                r.media.mixer().push(tap.publisher, tap.seq, &tap.payload);
                r.stats.codec_nanos.fetch_add(t.elapsed().as_nanos() as u64, Relaxed);
            }
            _ = tick.tick() => {
                let Some(dest) = remote else { continue };
                let t = Instant::now();
                // O tique consome SEMPRE um bloco da mistura, codifique ou não:
                // é ele que marca o passo dos buffers das fontes.
                let (payload_type, ts_step) = match &mut r.media {
                    Media::G711 { mixer, law, .. } => {
                        encode_mix(*law, mixer.tick(), &mut mix_out);
                        (law.payload_type(), FRAME_8K as u32)
                    }
                    Media::Opus { mixer, encoder, payload_type, .. } => {
                        mix_out.clear();
                        if let Some(opus) = encoder.encode(mixer.tick()) {
                            mix_out.extend_from_slice(opus);
                        }
                        (*payload_type, OPUS_TS_PER_FRAME)
                    }
                };
                r.stats.codec_nanos.fetch_add(t.elapsed().as_nanos() as u64, Relaxed);
                let this_ts = mix_ts;
                mix_ts = mix_ts.wrapping_add(ts_step);
                // Um bloco que o codificador não deu avança o relógio e não
                // gasta número de sequência: o telefone vê silêncio, não perda.
                if mix_out.is_empty() {
                    continue;
                }
                let packet = Packet {
                    header: Header {
                        version: 2,
                        payload_type,
                        sequence_number: mix_seq,
                        timestamp: this_ts,
                        ssrc: mix_ssrc,
                        ..Default::default()
                    },
                    payload: bytes::Bytes::copy_from_slice(&mix_out),
                };
                mix_seq = mix_seq.wrapping_add(1);
                let Ok(claro) = packet.marshal() else { continue };
                let saida = match r.srtp.as_mut() {
                    Some(s) => {
                        let t = Instant::now();
                        let out = s.outbound.encrypt_rtp(&claro);
                        r.stats.srtp_nanos.fetch_add(t.elapsed().as_nanos() as u64, Relaxed);
                        match out {
                            Ok(b) => b,
                            Err(e) => {
                                // Não se manda em claro o que devia ir cifrado.
                                tracing::warn!(leg = %r.cfg.leg_id, error = %e, "ponte: SRTP de saída falhou — pacote descartado");
                                continue;
                            }
                        }
                    }
                    None => claro,
                };
                if r.socket.send_to(&saida, dest).await.is_ok() {
                    r.stats.packets_out.fetch_add(1, Relaxed);
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
    let opus_rejected = match &r.media {
        Media::Opus { pass, .. } => pass.rejected,
        Media::G711 { .. } => 0,
    };
    let decode_errors = r.media.mixer().decode_errors;
    tracing::info!(
        room = %r.cfg.room_id,
        leg = %r.cfg.leg_id,
        packets_in = r.stats.packets_in.load(Relaxed),
        packets_out = r.stats.packets_out.load(Relaxed),
        rejected = r.stats.packets_rejected.load(Relaxed),
        srtp_failed = r.stats.packets_srtp_failed.load(Relaxed),
        decode_errors,
        opus_rejected,
        "ponte: perna fechada"
    );
}
