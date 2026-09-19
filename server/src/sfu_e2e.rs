//! Testes ponta-a-ponta do SFU com clientes WebRTC **reais**.
//!
//! Porquê isto existe: as correções de negociação, subscrição e fan-out do
//! `sfu.rs` não são verificáveis por testes de unidade — dependem de DTLS/ICE,
//! da máquina de estados de SDP do webrtc-rs e de RTP a circular mesmo. Até
//! aqui a única validação possível era abrir dois browsers à mão, o que não
//! corre em CI e não protege ninguém de uma regressão.
//!
//! Estes testes levantam um `SfuState` verdadeiro e ligam-lhe `RTCPeerConnection`s
//! (o mesmo webrtc-rs que o servidor usa) no papel de browser: oferta, resposta,
//! trickle ICE, publicação de tracks e leitura de RTP. Tudo em loopback, sem
//! rede externa, sem TURN.
//!
//! Cobrem em particular a **R13** (glare do lado do servidor), que é a
//! regressão mais cara e a menos óbvia de reproduzir à mão: só acontece quando
//! o cliente oferta na janela em que o servidor tem uma oferta por responder.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;
use webrtc::{
    api::{
        interceptor_registry::register_default_interceptors, media_engine::MediaEngine, APIBuilder,
    },
    ice_transport::ice_candidate::RTCIceCandidateInit,
    interceptor::registry::Registry,
    media::Sample,
    peer_connection::{
        configuration::RTCConfiguration, sdp::session_description::RTCSessionDescription,
        RTCPeerConnection,
    },
    rtp_transceiver::{
        rtp_codec::{RTCRtpCodecCapability, RTCRtpHeaderExtensionCapability, RTPCodecType},
        RTCRtpTransceiverInit,
    },
    track::track_local::{
        track_local_static_rtp::TrackLocalStaticRTP,
        track_local_static_sample::TrackLocalStaticSample, TrackLocal, TrackLocalWriter,
    },
};

use crate::{
    metrics::Metrics,
    sfu::{CensusSnapshot, IceConfig, SfuState},
    signaling::{ClientMsg, ServerMsg},
};

const OPUS: &str = "audio/opus";
const VP8: &str = "video/VP8";

/// Cliente de teste: um `RTCPeerConnection` a fazer de browser, com a
/// sinalização ligada ao SFU por chamadas diretas a `on_client_msg`.
struct TestClient {
    id: Uuid,
    room: Uuid,
    pc: Arc<RTCPeerConnection>,
    sfu: Arc<SfuState>,
    /// Segura as respostas às ofertas do servidor — é assim que se provoca o
    /// glare da R13 de forma determinista, em vez de esperar por uma corrida.
    hold_answer: Arc<std::sync::atomic::AtomicBool>,
    held: Arc<Mutex<Vec<String>>>,
    /// Tracks recebidas do SFU: (stream_id, kind).
    received: Arc<Mutex<Vec<(String, String)>>>,
    /// Pacotes RTP efetivamente recebidos, por stream_id.
    rtp_seen: Arc<Mutex<std::collections::HashMap<String, usize>>>,
    /// Cada pacote de VÍDEO recebido, por ordem de chegada — é o que deixa
    /// provar numeração, relógio e camada de origem através das trocas.
    video_rx: Arc<std::sync::Mutex<Vec<RxVideo>>>,
}

/// Um pacote de vídeo tal como o subscritor o viu.
#[derive(Clone, Debug)]
struct RxVideo {
    stream_id: String,
    track_id: String,
    seq: u16,
    ts: u32,
    /// Primeiro byte do payload: a camada que o publicador marcou (`q`/`h`/`f`).
    layer: u8,
}

async fn client_api() -> webrtc::api::API {
    let mut media = MediaEngine::default();
    media.register_default_codecs().unwrap();
    // As MESMAS extensões, pela MESMA ordem, que o servidor regista em
    // `sfu::new_api` (e que o browser anuncia): sem mid+rid o cliente não
    // consegue ENVIAR simulcast.
    media
        .register_header_extension(
            RTCRtpHeaderExtensionCapability {
                uri: "urn:ietf:params:rtp-hdrext:ssrc-audio-level".to_owned(),
            },
            RTPCodecType::Audio,
            None,
        )
        .unwrap();
    for uri in [
        "urn:ietf:params:rtp-hdrext:sdes:mid",
        "urn:ietf:params:rtp-hdrext:sdes:rtp-stream-id",
        "urn:ietf:params:rtp-hdrext:sdes:repaired-rtp-stream-id",
    ] {
        media
            .register_header_extension(
                RTCRtpHeaderExtensionCapability {
                    uri: uri.to_owned(),
                },
                RTPCodecType::Video,
                None,
            )
            .unwrap();
    }
    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut media).unwrap();
    APIBuilder::new()
        .with_media_engine(media)
        .with_interceptor_registry(registry)
        .build()
}

impl TestClient {
    /// Liga um cliente novo à sala: regista-o no SFU e arranca a bomba de
    /// mensagens servidor→cliente.
    async fn join(sfu: &Arc<SfuState>, room: Uuid) -> Arc<Self> {
        let id = Uuid::new_v4();
        // Mesma fila limitada que a produção usa (ver signaling::PeerTx).
        let (tx, rx, _shutdown) = crate::signaling::PeerTx::new(512, Arc::new(Metrics::default()));
        sfu.add_peer(room, id, tx).await.expect("add_peer");

        let api = client_api().await;
        let pc = Arc::new(
            api.new_peer_connection(RTCConfiguration::default())
                .await
                .unwrap(),
        );

        let client = Arc::new(TestClient {
            id,
            room,
            pc: pc.clone(),
            sfu: sfu.clone(),
            hold_answer: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            held: Arc::new(Mutex::new(Vec::new())),
            received: Arc::new(Mutex::new(Vec::new())),
            rtp_seen: Arc::new(Mutex::new(std::collections::HashMap::new())),
            video_rx: Arc::new(std::sync::Mutex::new(Vec::new())),
        });

        // Trickle ICE cliente → SFU.
        {
            let sfu = sfu.clone();
            pc.on_ice_candidate(Box::new(move |c| {
                let sfu = sfu.clone();
                Box::pin(async move {
                    if let Some(c) = c {
                        if let Ok(init) = c.to_json() {
                            if let Ok(candidate) = serde_json::to_value(&init) {
                                let _ = sfu
                                    .on_client_msg(room, id, ClientMsg::SfuIce { candidate })
                                    .await;
                            }
                        }
                    }
                })
            }));
        }

        // Tracks que o SFU nos encaminha + contagem de RTP real.
        {
            let received = client.received.clone();
            let rtp_seen = client.rtp_seen.clone();
            let video_rx = client.video_rx.clone();
            pc.on_track(Box::new(move |remote, _r, _t| {
                let received = received.clone();
                let rtp_seen = rtp_seen.clone();
                let video_rx = video_rx.clone();
                Box::pin(async move {
                    let stream_id = remote.stream_id().to_string();
                    let is_video = remote.kind() == RTPCodecType::Video;
                    received
                        .lock()
                        .await
                        .push((stream_id.clone(), remote.kind().to_string()));
                    tokio::spawn(async move {
                        while let Ok((pkt, _)) = remote.read_rtp().await {
                            *rtp_seen.lock().await.entry(stream_id.clone()).or_insert(0) += 1;
                            if is_video {
                                video_rx.lock().unwrap().push(RxVideo {
                                    stream_id: stream_id.clone(),
                                    track_id: remote.id(),
                                    seq: pkt.header.sequence_number,
                                    ts: pkt.header.timestamp,
                                    layer: pkt.payload.first().copied().unwrap_or(0),
                                });
                            }
                        }
                    });
                })
            }));
        }

        tokio::spawn(pump(client.clone(), rx));
        client
    }

    /// Publica uma track e oferta ao SFU (é o que o browser faz ao entrar, ao
    /// ligar a câmara ou ao partilhar o ecrã).
    async fn publish(&self, mime: &str, id: &str) -> Arc<TrackLocalStaticSample> {
        let track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: mime.to_owned(),
                ..Default::default()
            },
            id.to_owned(),
            format!("stream-{id}"),
        ));
        self.pc
            .add_transceiver_from_track(
                Arc::clone(&track) as Arc<dyn TrackLocal + Send + Sync>,
                Some(RTCRtpTransceiverInit {
                    direction: webrtc::rtp_transceiver::rtp_transceiver_direction::RTCRtpTransceiverDirection::Sendrecv,
                    send_encodings: vec![],
                }),
            )
            .await
            .unwrap();
        self.offer().await;

        // Media contínua: o `on_track` do SFU só dispara com RTP a chegar.
        let t = track.clone();
        tokio::spawn(async move {
            loop {
                let ok = t
                    .write_sample(&Sample {
                        data: vec![0u8; 120].into(),
                        duration: Duration::from_millis(20),
                        ..Default::default()
                    })
                    .await
                    .is_ok();
                if !ok {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        });
        track
    }

    /// Publica a câmara em SIMULCAST a sério, como o `addSimulcastVideo` do
    /// `web/src/webrtc.ts`: UM transceiver `sendrecv` com três encodings
    /// (`q`/`h`/`f`), cada um com o seu SSRC.
    ///
    /// Cada camada tem numeração e relógio RTP PRÓPRIOS e muito afastados —
    /// como num browser, em que cada encoding é um fluxo independente — e marca
    /// a camada no primeiro byte do payload. Sem isso, uma troca que deixasse a
    /// numeração recuar, ou que servisse a camada errada, passaria despercebida.
    async fn publish_simulcast(&self, id: &str) -> tokio::task::JoinHandle<()> {
        let mk = |rid: &str| {
            Arc::new(TrackLocalStaticRTP::new_with_rid(
                RTCRtpCodecCapability {
                    mime_type: VP8.to_owned(),
                    ..Default::default()
                },
                id.to_owned(),
                rid.to_owned(),
                format!("stream-{id}"),
            ))
        };
        let layers: Vec<(Arc<TrackLocalStaticRTP>, u8, u16, u32)> = vec![
            (mk("q"), b'q', 1_000, 10_000),
            (mk("h"), b'h', 20_000, 1_500_000_000),
            (mk("f"), b'f', 40_000, 3_000_000_000),
        ];
        let tr = self
            .pc
            .add_transceiver_from_track(
                Arc::clone(&layers[0].0) as Arc<dyn TrackLocal + Send + Sync>,
                Some(RTCRtpTransceiverInit {
                    direction: webrtc::rtp_transceiver::rtp_transceiver_direction::RTCRtpTransceiverDirection::Sendrecv,
                    send_encodings: vec![],
                }),
            )
            .await
            .unwrap();
        let sender = tr.sender().await;
        for (track, ..) in &layers[1..] {
            sender
                .add_encoding(Arc::clone(track) as Arc<dyn TrackLocal + Send + Sync>)
                .await
                .unwrap();
        }
        self.offer().await;

        tokio::spawn(async move {
            let mut n: u32 = 0;
            loop {
                for (track, mark, seq0, ts0) in &layers {
                    let mut payload = vec![0u8; 100];
                    payload[0] = *mark;
                    payload[1..5].copy_from_slice(&n.to_be_bytes());
                    let pkt = webrtc::rtp::packet::Packet {
                        header: webrtc::rtp::header::Header {
                            version: 2,
                            marker: true,
                            payload_type: 96,
                            sequence_number: seq0.wrapping_add(n as u16),
                            // 30 fps a 90 kHz.
                            timestamp: ts0.wrapping_add(n.wrapping_mul(3_000)),
                            ..Default::default()
                        },
                        payload: payload.into(),
                    };
                    let _ = track.write_rtp(&pkt).await;
                }
                n = n.wrapping_add(1);
                // 200 pacotes/s por camada — a cadência de um vídeo a ~1 Mbps.
                // Mais devagar, a corrida da fronteira (um pacote da camada
                // antiga aceite e perdido a meio da troca) quase nunca se dá e
                // o teste deixava de a poder apanhar.
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
    }

    /// Envia a sugestão de camada pelo MESMO caminho do browser: a mensagem
    /// `video-interest` do WebSocket, desserializada para `ClientMsg` e entregue
    /// a `set_video_interest` como faz `signaling.rs`.
    async fn video_interest(&self, publisher: Uuid, quality: &str) {
        let raw = serde_json::json!({
            "type": "video-interest",
            "peers": [publisher],
            "quality": { publisher.to_string(): quality },
        });
        let Ok(ClientMsg::VideoInterest { peers, quality }) = serde_json::from_value(raw) else {
            panic!("a mensagem do browser deixou de desserializar para VideoInterest");
        };
        self.sfu
            .set_video_interest(self.room, self.id, peers, quality)
            .await;
    }

    /// Oferta do cliente para o servidor.
    async fn offer(&self) {
        let offer = self.pc.create_offer(None).await.unwrap();
        self.pc.set_local_description(offer).await.unwrap();
        let sdp = self.pc.local_description().await.unwrap().sdp;
        self.sfu
            .on_client_msg(self.room, self.id, ClientMsg::SfuOffer { sdp })
            .await
            .unwrap();
    }

    async fn release_held_answers(&self) {
        self.hold_answer
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let held: Vec<String> = self.held.lock().await.drain(..).collect();
        for sdp in held {
            self.answer_to(sdp).await;
        }
    }

    async fn answer_to(&self, sdp: String) {
        let offer = RTCSessionDescription::offer(sdp).unwrap();
        if self.pc.set_remote_description(offer).await.is_err() {
            return;
        }
        let Ok(answer) = self.pc.create_answer(None).await else {
            return;
        };
        if self.pc.set_local_description(answer).await.is_err() {
            return;
        }
        let Some(local) = self.pc.local_description().await else {
            return;
        };
        let sdp = local.sdp;
        let _ = self
            .sfu
            .on_client_msg(self.room, self.id, ClientMsg::SfuAnswer { sdp })
            .await;
    }

    async fn streams_seen(&self) -> Vec<(String, String)> {
        self.received.lock().await.clone()
    }
}

/// Bomba de mensagens servidor → cliente (o equivalente ao WebSocket).
async fn pump(client: Arc<TestClient>, mut rx: mpsc::Receiver<ServerMsg>) {
    while let Some(msg) = rx.recv().await {
        match msg {
            ServerMsg::SfuAnswer { sdp } => {
                let answer = RTCSessionDescription::answer(sdp).unwrap();
                let _ = client.pc.set_remote_description(answer).await;
            }
            ServerMsg::SfuOffer { sdp } => {
                if client.hold_answer.load(std::sync::atomic::Ordering::SeqCst) {
                    client.held.lock().await.push(sdp);
                    continue;
                }
                client.answer_to(sdp).await;
            }
            ServerMsg::SfuIce { candidate } => {
                if let Ok(init) = serde_json::from_value::<RTCIceCandidateInit>(candidate) {
                    let _ = client.pc.add_ice_candidate(init).await;
                }
            }
            _ => {}
        }
    }
}

/// Portas UDP para os testes, **abaixo do intervalo efémero do SO**.
///
/// O intervalo do produto (50000–50200) está inteiro dentro de
/// `ip_local_port_range` (32768–60999 por omissão no Linux): qualquer processo
/// do host — um browser aberto, os Chromium do Playwright — pode ficar com
/// essas portas, e então o SFU do teste não recolhe candidatos e falha por
/// TIMEOUT, que se lê como lentidão do runner e não como colisão (R57).
///
/// 20000+ nunca é entregue pelo SO como porta efémera, por isso o único
/// concorrente possível é outro processo de teste — e o PID separa-os.
fn portas_de_teste() -> (u16, u16) {
    // 60 portas chegam para os pares destes testes; 200 fatias distintas.
    let base = 20_000u16 + ((std::process::id() % 200) as u16 * 60);
    (base, base + 59)
}

fn new_sfu() -> (Arc<SfuState>, Arc<Metrics>) {
    new_sfu_com(None)
}

fn new_sfu_com(ice_timeouts: Option<(Duration, Duration)>) -> (Arc<SfuState>, Arc<Metrics>) {
    let metrics = Arc::new(Metrics::default());
    (
        Arc::new(SfuState::new(
            IceConfig {
                udp_ports: Some(portas_de_teste()),
                ice_timeouts,
                ..Default::default()
            },
            metrics.clone(),
            64,
            2048,
        )),
        metrics,
    )
}

/// Espera até `cond` ou falha com `label` — evita `sleep` fixos frágeis.
/// Orçamento base destas esperas, multiplicável pelo ambiente.
///
/// Estes testes montam `RTCPeerConnection`s a sério: ICE, DTLS e o primeiro
/// RTP. Nesta máquina de desenvolvimento resolvem-se em ~0,1 s; num runner de
/// CI partilhado com 2 vCPU chegaram a estourar 30 s e a falhar. Não é um bug
/// do produto — é o mesmo trabalho a correr numa máquina muito mais lenta.
///
/// Um portão que falha ao acaso perde a credibilidade toda: à terceira vez,
/// quem o vê vermelho assume flake e segue. Por isso o prazo passa a ser
/// generoso E ajustável (`E2E_TIMEOUT_FACTOR`), em vez de calibrado para o
/// portátil de quem o escreveu.
fn prazo(base_secs: u64) -> Duration {
    let fator: u64 = std::env::var("E2E_TIMEOUT_FACTOR")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|f| (1..=20).contains(f))
        .unwrap_or(1);
    Duration::from_secs(base_secs * fator)
}

async fn eventually<F, Fut>(label: &str, timeout: Duration, cond: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    eventually_com_diagnostico(label, timeout, cond, String::new).await
}

/// Como `eventually`, mas o `diag` é chamado **só quando o prazo estoura** e
/// junta-se à mensagem. Sem isto, «não chegou media» não distingue ICE que
/// nunca ligou, de subscrição que não aconteceu, de RTP a não fluir — e cada
/// uma delas manda investigar noutro sítio. Custa nada quando passa.
async fn eventually_com_diagnostico<F, Fut, D>(label: &str, timeout: Duration, mut cond: F, diag: D)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
    D: Fn() -> String,
{
    let inicio = tokio::time::Instant::now();
    let deadline = inicio + timeout;
    let mut tentativas = 0u32;
    loop {
        tentativas += 1;
        if cond().await {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            // O prazo e as tentativas entram na mensagem: sem eles, um timeout
            // não distingue «o produto está partido» de «a máquina é lenta», e
            // foi precisamente essa a dúvida que custou uma ida ao CI.
            let d = diag();
            // A DICA tem de depender do que se observou. Ela era incondicional
            // — «sobe o E2E_TIMEOUT_FACTOR» — e mandou investigar tempo numa
            // falha em que o ICE estava `Failed` nos DOIS pares, que é um
            // estado TERMINAL: mais prazo não liga um ICE que já desistiu.
            // Uma mensagem que aponta o remédio errado custa mais do que uma
            // mensagem que não aponta nenhum (R90).
            let desistiu = d.contains("Failed") || d.contains("Closed");
            let dica = if desistiu {
                "O ICE está em estado TERMINAL (Failed/Closed), não a meio: subir \
                 E2E_TIMEOUT_FACTOR não muda nada. Procura na REDE do ambiente — \
                 UDP bloqueado, sem interface com candidatos de host, ou o par a \
                 fechar antes de negociar."
            } else {
                "Se for lentidão do ambiente e não uma avaria, sobe E2E_TIMEOUT_FACTOR."
            };
            panic!(
                "timeout à espera de: {label} (prazo {:?}, {tentativas} tentativas em {:?}).{}\n{dica}",
                timeout,
                inicio.elapsed(),
                if d.is_empty() {
                    String::new()
                } else {
                    format!("\n  estado: {d}")
                }
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

impl TestClient {
    /// Retrato SÍNCRONO do estado da ligação, para entrar numa mensagem de
    /// timeout. Diz em que fase parou: ICE que nunca ligou aponta a rede;
    /// ICE ligado sem tracks aponta a subscrição; tracks sem RTP aponta ao
    /// fan-out. Sem isto, as três falhas dão a mesma mensagem.
    fn retrato(&self) -> String {
        let recebidas = self
            .received
            .try_lock()
            .map(|v| {
                v.iter()
                    .map(|(s, k)| format!("{}:{k}", &s[..s.len().min(8)]))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_else(|_| "<bloqueado>".into());
        let rtp = self
            .rtp_seen
            .try_lock()
            .map(|m| {
                m.iter()
                    .map(|(s, n)| format!("{}={n}", &s[..s.len().min(8)]))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_else(|_| "<bloqueado>".into());
        format!(
            "sinalização={:?} ice={:?} ligação={:?} tracks=[{recebidas}] rtp=[{rtp}]",
            self.pc.signaling_state(),
            self.pc.ice_connection_state(),
            self.pc.connection_state(),
        )
    }
}

/// Caminho feliz: dois participantes, media a fluir nos DOIS sentidos.
///
/// Cobre a cadeia toda — oferta do cliente, resposta do SFU, subscrição,
/// renegociação server-driven, fan-out de RTP e a renumeração do áudio.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn media_flows_both_ways() {
    let (sfu, _metrics) = new_sfu();
    let room = Uuid::new_v4();

    let a = TestClient::join(&sfu, room).await;
    a.publish(OPUS, "a-audio").await;
    a.publish(VP8, "a-video").await;

    let b = TestClient::join(&sfu, room).await;
    b.publish(OPUS, "b-audio").await;
    b.publish(VP8, "b-video").await;

    // B tem de receber as duas tracks de A, identificadas pelo peer_id de A.
    eventually_com_diagnostico(
        "B recebe áudio+vídeo de A",
        prazo(30),
        || {
            let b = b.clone();
            let a_id = a.id.to_string();
            async move {
                let seen = b.streams_seen().await;
                seen.iter().filter(|(s, _)| *s == a_id).count() >= 2
            }
        },
        || format!("A[{}] · B[{}]", a.retrato(), b.retrato()),
    )
    .await;

    // …e o inverso: sem isto, "media num só sentido" passaria despercebido.
    eventually_com_diagnostico(
        "A recebe áudio+vídeo de B",
        prazo(30),
        || {
            let a = a.clone();
            let b_id = b.id.to_string();
            async move {
                let seen = a.streams_seen().await;
                seen.iter().filter(|(s, _)| *s == b_id).count() >= 2
            }
        },
        || format!("A[{}] · B[{}]", a.retrato(), b.retrato()),
    )
    .await;

    // Tracks negociadas não chegam: exige-se RTP mesmo a passar pelo fan-out.
    eventually("RTP real de A para B", prazo(30), || {
        let b = b.clone();
        let a_id = a.id.to_string();
        async move { b.rtp_seen.lock().await.get(&a_id).copied().unwrap_or(0) > 0 }
    })
    .await;

    // Asserções explícitas: um `eventually` que passasse por engano deixaria
    // isto a zero e o teste seria decorativo.
    let a_to_b = b
        .rtp_seen
        .lock()
        .await
        .get(&a.id.to_string())
        .copied()
        .unwrap_or(0);
    let b_to_a = a
        .rtp_seen
        .lock()
        .await
        .get(&b.id.to_string())
        .copied()
        .unwrap_or(0);
    eprintln!(
        "media_flows_both_ways: tracks B<-A={:?} A<-B={:?} | RTP A->B={a_to_b} B->A={b_to_a}",
        b.streams_seen().await,
        a.streams_seen().await
    );
    assert!(a_to_b > 0, "nenhum RTP de A chegou a B");
    assert_eq!(
        b.streams_seen()
            .await
            .iter()
            .filter(|(s, _)| *s == a.id.to_string())
            .count(),
        2,
        "B tem de ver exatamente as 2 tracks de A (áudio + vídeo)"
    );

    sfu.remove_peer(room, a.id).await;
    sfu.remove_peer(room, b.id).await;
}

/// **R13 — glare do lado do servidor.**
///
/// Reproduz de forma determinista a janela que fazia a partilha de ecrã
/// desaparecer em silêncio: o cliente oferta enquanto o servidor tem uma oferta
/// por responder. Com o código antigo, o `set_remote_description` do servidor
/// falhava (`have-local-offer` + `SetRemote(offer)` não é transição válida no
/// webrtc-rs), o erro era um mero `warn` e a track do cliente ficava adicionada
/// mas nunca negociada.
///
/// **Âmbito:** este teste cobre o lado do SERVIDOR. O cliente aqui é webrtc-rs,
/// que — ao contrário do browser — **não suporta rollback** a partir de
/// `have-local-offer` (verificado em `signaling_state.rs`), logo não consegue
/// encenar a recuperação do lado do cliente. Essa metade vive em
/// `web/src/webrtc.ts` (rollback + RE-OFERTA) e está descrita na R13.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn client_offer_during_server_offer_is_deferred_not_dropped() {
    let (sfu, metrics) = new_sfu();
    let room = Uuid::new_v4();

    let a = TestClient::join(&sfu, room).await;
    a.publish(OPUS, "a-audio").await;

    let b = TestClient::join(&sfu, room).await;
    b.publish(OPUS, "b-audio").await;

    eventually("ligação inicial estabelecida", prazo(30), || {
        let b = b.clone();
        let a_id = a.id.to_string();
        async move { b.streams_seen().await.iter().any(|(s, _)| *s == a_id) }
    })
    .await;

    // A partir daqui A NÃO responde às ofertas do servidor: o servidor fica em
    // `have-local-offer` — exatamente a janela do glare.
    a.hold_answer
        .store(true, std::sync::atomic::Ordering::SeqCst);

    // B publica vídeo → o servidor tem de renegociar com A para lho entregar.
    b.publish(VP8, "b-video").await;
    eventually("servidor com oferta pendente para A", prazo(30), || {
        let a = a.clone();
        async move { !a.held.lock().await.is_empty() }
    })
    .await;

    // …e é NESTE instante que A publica o ecrã (oferta do cliente em glare).
    let _screen = a.publish(VP8, "a-screen").await;

    // O VEREDICTO: a oferta de A foi ADIADA, não descartada. Com o bug o
    // contador ficaria a 0 — o `set_remote_description` falhava, saía um `warn`
    // e a track perdia-se sem deixar rasto em lado nenhum.
    eventually("oferta de A adiada pelo servidor", prazo(20), || {
        let metrics = metrics.clone();
        async move {
            metrics
                .sfu_offers_deferred_total
                .load(std::sync::atomic::Ordering::Relaxed)
                >= 1
        }
    })
    .await;

    // E o servidor não desistiu da renegociação nem entrou em erro.
    assert_eq!(
        metrics
            .sfu_renegotiations_failed_total
            .load(std::sync::atomic::Ordering::Relaxed),
        0,
        "o servidor não pode desistir da renegociação durante o glare"
    );

    a.release_held_answers().await;
    sfu.remove_peer(room, a.id).await;
    sfu.remove_peer(room, b.id).await;
}

/// Prova que a DICA de um timeout depende do estado observado (R90).
///
/// Sem este teste, a distinção entre «ainda a tentar» e «desistiu» é uma
/// afirmação num comentário. A mensagem que manda subir o prazo num ICE
/// `Failed` custou uma investigação inteira na direcção errada.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dica_do_timeout_segue_o_estado_observado() {
    for (estado, esperado, proibido) in [
        (
            "A[ice=Failed] · B[ice=Failed]",
            "estado TERMINAL",
            "sobe E2E_TIMEOUT_FACTOR",
        ),
        (
            "A[ice=Checking] · B[ice=New]",
            "sobe E2E_TIMEOUT_FACTOR",
            "estado TERMINAL",
        ),
    ] {
        let e = estado.to_string();
        let r = tokio::spawn(async move {
            eventually_com_diagnostico(
                "condição que nunca acontece",
                Duration::from_millis(30),
                || async { false },
                move || e.clone(),
            )
            .await
        })
        .await;
        let msg = match r {
            Err(j) if j.is_panic() => {
                let p = j.into_panic();
                p.downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default()
            }
            _ => panic!("esperava-se que o prazo estourasse"),
        };
        assert!(
            msg.contains(esperado),
            "para o estado {estado:?} a dica devia conter {esperado:?}; veio: {msg}"
        );
        assert!(
            !msg.contains(proibido),
            "para o estado {estado:?} a dica NÃO devia conter {proibido:?}; veio: {msg}"
        );
    }
}

/// **R156 — troca de camada simulcast no mesmo sender, com RTP contínuo.**
///
/// O defeito: voltar a uma camada já usada (`f → h → f`) falhava SEMPRE
/// («new track must have the same envelope as previous»), o subscritor ficava
/// sem o vídeo desse participante e a PC acabava em `failed`. Depois da primeira
/// correcção, a numeração e o relógio RTP de cada camada chegavam crus ao
/// browser, que os via recuar e descartava os fotogramas: vídeo congelado.
///
/// Aqui o publicador envia simulcast de verdade (três SSRC, rid `q`/`h`/`f`,
/// numeração e relógio próprios, camada marcada no payload) e o subscritor pede
/// `f → h → f → q → f` pela mensagem do browser. Afirma-se, a cada troca:
/// (a) nenhuma falha; (b) o RTP continua a chegar e vem da camada pedida;
/// (c) numeração e relógio nunca recuam, e a fronteira da troca é contígua;
/// (d) nem transceivers nem tracks novas no subscritor — sem renegociação.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn troca_de_camada_simulcast_sem_renegociar_e_com_rtp_continuo() {
    use std::sync::atomic::Ordering::Relaxed;

    let (sfu, metrics) = new_sfu();
    let room = Uuid::new_v4();

    let a = TestClient::join(&sfu, room).await;
    let pump_a = a.publish_simulcast("a-cam").await;

    // B precisa de uma oferta própria antes de poder ser subscrito.
    let b = TestClient::join(&sfu, room).await;
    b.publish(OPUS, "b-audio").await;

    let a_id = a.id.to_string();
    let video_de_a = |b: &TestClient| -> Vec<RxVideo> {
        b.video_rx
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.stream_id == a_id)
            .cloned()
            .collect()
    };
    let diag = || {
        format!(
            "A[{}] · B[{}] · publicações={} trocas={} falhas={} degradados={}",
            a.retrato(),
            b.retrato(),
            metrics.sfu_publications_total.load(Relaxed),
            metrics.sfu_layer_switches_total.load(Relaxed),
            metrics.sfu_layer_switch_failures_total.load(Relaxed),
            metrics.sfu_degraded_subscribers.load(Relaxed),
        )
    };

    // As TRÊS camadas publicadas (+ o áudio de B) antes de pedir seja o que for:
    // sem isto a primeira escolha dependia da ordem de chegada das camadas.
    eventually_com_diagnostico(
        "SFU recebe as três camadas de A e o áudio de B",
        prazo(30),
        || {
            let m = metrics.clone();
            async move { m.sfu_publications_total.load(Relaxed) >= 4 }
        },
        diag,
    )
    .await;

    // Estado inicial: `f`, e media a fluir.
    b.video_interest(a.id, "f").await;
    let mut marca = 0usize;
    eventually_com_diagnostico(
        "B recebe a camada f de A",
        prazo(30),
        || {
            let v = video_de_a(&b);
            async move { v.iter().filter(|p| p.layer == b'f').count() >= 10 }
        },
        diag,
    )
    .await;
    let transceivers_iniciais = b.pc.get_transceivers().await.len();
    let tracks_iniciais = b.streams_seen().await.len();

    for (i, alvo) in ["h", "f", "q", "f"].into_iter().enumerate() {
        let alvo_b = alvo.as_bytes()[0];
        let falhas_antes = metrics.sfu_layer_switch_failures_total.load(Relaxed);
        let trocas_antes = metrics.sfu_layer_switches_total.load(Relaxed);
        let inicio = video_de_a(&b).len();
        b.video_interest(a.id, alvo).await;

        // (b) o RTP continua a chegar, e da camada pedida — ou a troca falhou,
        // e então sai JÁ com a razão em vez de esperar pelo prazo.
        eventually_com_diagnostico(
            &format!("troca #{} para {alvo}: RTP da camada nova chega a B", i + 1),
            prazo(30),
            || {
                let v = video_de_a(&b);
                let falhou = metrics.sfu_layer_switch_failures_total.load(Relaxed) > falhas_antes;
                async move {
                    falhou || v[inicio..].iter().filter(|p| p.layer == alvo_b).count() >= 10
                }
            },
            diag,
        )
        .await;

        // (a) nenhuma troca produz erro.
        assert_eq!(
            metrics.sfu_layer_switch_failures_total.load(Relaxed),
            falhas_antes,
            "troca #{} para {alvo} FALHOU («sfu layer switch failed»). {}",
            i + 1,
            diag()
        );
        assert!(
            metrics.sfu_layer_switches_total.load(Relaxed) > trocas_antes,
            "troca #{} para {alvo}: o SFU não chegou a trocar de camada — o teste \
             não estaria a medir nada. {}",
            i + 1,
            diag()
        );

        // (b) depois do primeiro pacote da camada nova, NENHUM de outra camada.
        let v = video_de_a(&b);
        let primeiro = inicio
            + v[inicio..]
                .iter()
                .position(|p| p.layer == alvo_b)
                .expect("a condição acima garante pelo menos um");
        let intrusos: Vec<char> = v[primeiro..]
            .iter()
            .filter(|p| p.layer != alvo_b)
            .map(|p| p.layer as char)
            .collect();
        assert!(
            intrusos.is_empty(),
            "troca #{} para {alvo}: chegaram pacotes de outra camada DEPOIS da nova: {intrusos:?}",
            i + 1
        );

        // (d) sem renegociação: nem transceiver nem track nova no subscritor.
        assert_eq!(
            b.pc.get_transceivers().await.len(),
            transceivers_iniciais,
            "troca #{} para {alvo}: o subscritor ganhou transceivers (renegociou)",
            i + 1
        );
        assert_eq!(
            b.streams_seen().await.len(),
            tracks_iniciais,
            "troca #{} para {alvo}: o subscritor recebeu uma track nova",
            i + 1
        );
        marca = v.len();
    }

    // (c) numeração e relógio através de TODAS as trocas.
    let v = video_de_a(&b);
    assert!(v.len() >= marca);
    let tracks: std::collections::BTreeSet<&str> = v.iter().map(|p| p.track_id.as_str()).collect();
    assert_eq!(
        tracks.len(),
        1,
        "o vídeo de A tem de chegar sempre pela MESMA track: {tracks:?}"
    );
    let mut fronteiras = 0;
    let mut lacunas = 0;
    for w in v.windows(2) {
        let (p, c) = (&w[0], &w[1]);
        let dseq = c.seq.wrapping_sub(p.seq);
        let dts = c.ts.wrapping_sub(p.ts);
        assert!(
            (1..0x8000).contains(&dseq),
            "sequence_number recuou ou repetiu: {} ({}) → {} ({})",
            p.seq,
            p.layer as char,
            c.seq,
            c.layer as char
        );
        assert!(
            dts < 0x8000_0000,
            "timestamp recuou: {} ({}) → {} ({})",
            p.ts,
            p.layer as char,
            c.ts,
            c.layer as char
        );
        if p.layer != c.layer {
            fronteiras += 1;
            assert_eq!(
                dseq, 1,
                "fronteira {} → {}: a numeração saltou {dseq} (tem de ser contígua)",
                p.layer as char, c.layer as char
            );
            assert!(
                dts > 0 && dts <= 90_000,
                "fronteira {} → {}: o relógio tem de avançar, e menos de 1 s (avançou {dts})",
                p.layer as char,
                c.layer as char
            );
        } else if dseq != 1 {
            lacunas += 1;
        }
    }
    // >= e não ==: antes do pedido explícito de `f` o SFU pode ter servido a
    // primeira camada que chegou e trocado para `f` sozinho — outra troca, que
    // também tem de ser contígua.
    assert!(
        fronteiras >= 4,
        "esperavam-se pelo menos 4 fronteiras de camada no fluxo, vieram {fronteiras}"
    );
    eprintln!(
        "troca_de_camada: {} pacotes de vídeo, {fronteiras} fronteiras contíguas, \
         {lacunas} lacunas dentro de camada, trocas={} falhas={} transceivers={transceivers_iniciais}",
        v.len(),
        metrics.sfu_layer_switches_total.load(Relaxed),
        metrics.sfu_layer_switch_failures_total.load(Relaxed),
    );

    pump_a.abort();
    sfu.remove_peer(room, a.id).await;
    sfu.remove_peer(room, b.id).await;
}

/// Resposta do SFU a uma verificação de conectividade encenada.
#[derive(Debug, PartialEq)]
enum RespostaStun {
    /// Nada chegou dentro do prazo — o pedido foi deitado fora.
    Silencio,
    /// Resposta de sucesso com MESSAGE-INTEGRITY válida.
    Sucesso,
    /// Resposta de erro autenticada, com o código.
    Erro(u16),
    /// Chegou alguma coisa, mas a integridade não bate: o browser descarta-a.
    NaoAutenticada,
}

/// Envia ao SFU, pelo par ICE que ESTE cliente tem seleccionado, um Binding
/// Request com as credenciais verdadeiras da sessão e o papel pedido — é a
/// verificação de consentimento (RFC 7675) que o browser manda a cada ~1 s.
///
/// Existe porque o browser que motivou a R157 não se consegue encenar com um
/// cliente webrtc-rs: é o libwebrtc que passa a CONTROLLED ao responder a uma
/// oferta do servidor. O que se pode medir, e é o que conta, é o que o SFU FAZ
/// quando recebe um pedido com o mesmo papel que o dele.
async fn sonda_de_papel(cliente: &TestClient, controlado: bool) -> RespostaStun {
    use webrtc::ice::control::{AttrControlled, AttrControlling};
    use webrtc::ice::priority::PriorityAttr;
    use webrtc::stun::{
        agent::TransactionId,
        attributes::ATTR_USERNAME,
        error_code::ErrorCodeAttribute,
        fingerprint::FINGERPRINT,
        integrity::MessageIntegrity,
        message::{
            Getter, Message, Setter, BINDING_REQUEST, CLASS_ERROR_RESPONSE, CLASS_SUCCESS_RESPONSE,
        },
        textattrs::Username,
    };

    let campo = |sdp: &str, chave: &str| -> String {
        sdp.lines()
            .find_map(|l| l.strip_prefix(chave))
            .map(|v| v.trim().to_string())
            .expect("SDP sem credenciais ICE")
    };
    let sdp_sfu = cliente
        .pc
        .remote_description()
        .await
        .expect("sem SDP do SFU")
        .sdp;
    let sdp_meu = cliente
        .pc
        .local_description()
        .await
        .expect("sem SDP local")
        .sdp;
    let (ufrag_sfu, pwd_sfu) = (
        campo(&sdp_sfu, "a=ice-ufrag:"),
        campo(&sdp_sfu, "a=ice-pwd:"),
    );
    let ufrag_meu = campo(&sdp_meu, "a=ice-ufrag:");

    // O `connected` da PC chega antes de o par seleccionado ficar visível no
    // `RTCIceTransport` — com os testes em paralelo apanhou-se essa janela.
    let transporte = cliente.pc.get_senders().await[0].transport();
    let mut par = None;
    for _ in 0..50 {
        par = transporte
            .ice_transport()
            .get_selected_candidate_pair()
            .await;
        if par.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let par = par.expect("o cliente não tem par ICE seleccionado (5 s)");
    // `SocketAddr::new` e não `format!("{ip}:{porta}")`: o par pode ser IPv6,
    // que sem parênteses rectos não se lê como endereço de socket.
    let destino = std::net::SocketAddr::new(
        par.remote
            .address
            .parse()
            .expect("endereço do candidato do SFU"),
        par.remote.port,
    );

    let papel: Box<dyn Setter> = if controlado {
        Box::new(AttrControlled(rand::random()))
    } else {
        Box::new(AttrControlling(rand::random()))
    };
    let mut pedido = Message::new();
    pedido
        .build(&[
            Box::new(BINDING_REQUEST),
            Box::new(TransactionId::new()),
            Box::new(Username::new(
                ATTR_USERNAME,
                format!("{ufrag_sfu}:{ufrag_meu}"),
            )),
            papel,
            Box::new(PriorityAttr(1_845_501_695)),
            Box::new(MessageIntegrity::new_short_term_integrity(pwd_sfu.clone())),
            Box::new(FINGERPRINT),
        ])
        .expect("pedido STUN");

    let socket = tokio::net::UdpSocket::bind(if destino.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    })
    .await
    .expect("socket da sonda");
    let mut buf = vec![0u8; 1500];
    // Três tentativas: é UDP, e um pacote perdido não pode dar «Silêncio».
    for _ in 0..3 {
        socket
            .send_to(&pedido.raw, destino)
            .await
            .expect("envio da sonda");
        let Ok(Ok((n, _))) =
            tokio::time::timeout(Duration::from_millis(700), socket.recv_from(&mut buf)).await
        else {
            continue;
        };
        let mut resposta = Message::new();
        resposta.raw = buf[..n].to_vec();
        if resposta.decode().is_err() || resposta.transaction_id != pedido.transaction_id {
            continue;
        }
        if MessageIntegrity::new_short_term_integrity(pwd_sfu.clone())
            .check(&mut resposta)
            .is_err()
        {
            return RespostaStun::NaoAutenticada;
        }
        if resposta.typ.class == CLASS_SUCCESS_RESPONSE {
            return RespostaStun::Sucesso;
        }
        if resposta.typ.class == CLASS_ERROR_RESPONSE {
            let mut codigo = ErrorCodeAttribute::default();
            let _ = codigo.get_from(&resposta);
            return RespostaStun::Erro(codigo.code.0);
        }
    }
    RespostaStun::Silencio
}

/// **R157 — o SFU tem de responder 487 a um pedido com papel ICE em conflito.**
///
/// O Chrome (151, medido) passa a CONTROLLED sempre que aplica a sua resposta a
/// uma oferta do SFU — e o SFU oferta logo a seguir a cada ligação, para
/// subscrever quem já está na sala. Com o webrtc-ice original, o SFU (também
/// controlled) deitava fora em silêncio todos os pedidos dele: sem respostas
/// de consentimento, o browser declarava `disconnected` ~5 s depois, e a
/// câmara «não aparecia» aos outros. A correcção (vendor/webrtc-ice,
/// `send_role_conflict`) responde 487 autenticado, que é o que manda o browser
/// voltar a CONTROLLING.
///
/// Sem a correcção este teste FALHA em `Silencio`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn conflito_de_papel_ice_recebe_487_autenticado() {
    let (sfu, _metrics) = new_sfu();
    let room = Uuid::new_v4();
    let a = TestClient::join(&sfu, room).await;
    a.publish(OPUS, "a-audio").await;
    eventually_com_diagnostico(
        "A liga ao SFU",
        prazo(30),
        || {
            let a = a.clone();
            async move {
                a.pc.connection_state()
                    == webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState::Connected
            }
        },
        || a.retrato(),
    )
    .await;

    // Controlo: a MESMA sonda com o papel certo é aceite. Sem isto, um pedido
    // mal formado daria «Silêncio» e passaria por prova do defeito.
    assert_eq!(
        sonda_de_papel(&a, false).await,
        RespostaStun::Sucesso,
        "a sonda com ICE-CONTROLLING tem de receber sucesso autenticado"
    );
    // O caso do browser depois de responder a uma oferta do SFU.
    assert_eq!(
        sonda_de_papel(&a, true).await,
        RespostaStun::Erro(487),
        "pedido com ICE-CONTROLLED a um SFU controlled: tem de vir 487 autenticado, \
         não silêncio (R157)"
    );

    sfu.remove_peer(room, a.id).await;
}

/// R157 no caminho completo: dois participantes, media nos DOIS sentidos durante
/// 24 s, com o SFU a renegociar logo a seguir a cada ligação (subscrição de
/// quem já estava) e outra vez a meio (A liga a câmara). A cada janela de 4 s
/// exige-se que: o RTP continue a chegar nos dois sentidos, as PCs continuem
/// `connected`, o SFU responda ao consentimento (sonda com o papel certo) e
/// responda 487 a um pedido com papel em conflito — o estado em que o Chrome
/// fica depois de cada resposta a uma oferta do SFU.
///
/// Âmbito: os clientes aqui são webrtc-rs, que NÃO muda de papel ao responder
/// (o libwebrtc muda). A inversão do browser é encenada pela sonda; a prova
/// com o browser real está na R157 (dois Chromium, 5 corridas de 60 s).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn media_e_consentimento_sobrevivem_as_renegociacoes_do_sfu() {
    use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState::Connected;

    let (sfu, _metrics) = new_sfu();
    let room = Uuid::new_v4();

    let a = TestClient::join(&sfu, room).await;
    a.publish(OPUS, "a-audio").await;
    let b = TestClient::join(&sfu, room).await;
    b.publish(OPUS, "b-audio").await;

    let rtp = |c: &TestClient, de: &TestClient| {
        c.rtp_seen
            .try_lock()
            .ok()
            .and_then(|m| m.get(&de.id.to_string()).copied())
            .unwrap_or(0)
    };
    eventually_com_diagnostico(
        "RTP nos dois sentidos",
        prazo(30),
        || {
            let (a2, b2) = (a.clone(), b.clone());
            async move { rtp(&a2, &b2) > 0 && rtp(&b2, &a2) > 0 }
        },
        || format!("A[{}] · B[{}]", a.retrato(), b.retrato()),
    )
    .await;

    let mut a_para_b = rtp(&b, &a);
    let mut b_para_a = rtp(&a, &b);
    for janela in 1..=6 {
        if janela == 2 {
            // Renegociação do SFU a meio: A liga a câmara → oferta nova a B.
            a.publish(VP8, "a-video").await;
        }
        tokio::time::sleep(Duration::from_secs(4)).await;
        let diag = format!("janela {janela}: A[{}] · B[{}]", a.retrato(), b.retrato());
        for c in [&a, &b] {
            assert_eq!(c.pc.connection_state(), Connected, "PC caiu — {diag}");
            assert_eq!(
                sonda_de_papel(c, false).await,
                RespostaStun::Sucesso,
                "o SFU deixou de responder ao consentimento — {diag}"
            );
            assert_eq!(
                sonda_de_papel(c, true).await,
                RespostaStun::Erro(487),
                "conflito de papel sem 487 — {diag}"
            );
        }
        let (ab, ba) = (rtp(&b, &a), rtp(&a, &b));
        assert!(ab > a_para_b, "RTP A→B parou ({a_para_b} → {ab}) — {diag}");
        assert!(ba > b_para_a, "RTP B→A parou ({b_para_a} → {ba}) — {diag}");
        (a_para_b, b_para_a) = (ab, ba);
    }
    assert!(
        b.streams_seen()
            .await
            .iter()
            .any(|(s, k)| *s == a.id.to_string() && k == "video"),
        "a câmara ligada a meio nunca chegou a B — a renegociação do meio não aconteceu"
    );
    eprintln!("media_e_consentimento: RTP A→B={a_para_b} B→A={b_para_a} em 24 s");

    sfu.remove_peer(room, a.id).await;
    sfu.remove_peer(room, b.id).await;
}

/// Espera que o SFU não tenha NADA vivo — nem PCs, nem peers, nem publicações,
/// nem tarefas. Falha com a fotografia, que diz o que ficou preso.
async fn esperar_censo_vazio(sfu: &Arc<SfuState>, label: &str, timeout: Duration) {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let c = sfu.census().await;
        if c == CensusSnapshot::default() {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("{label}: o SFU ainda mantém coisas vivas ao fim de {timeout:?}: {c:?}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// **Armadilha do webrtc-rs 0.17.1** que o SFU tem de contornar, fixada aqui.
///
/// `RTCRtpSender::read` espera por `Notify::notify_waiters()` e NÃO consulta a
/// bandeira de paragem: um `stop()` que aconteça quando ninguém está a ler é
/// perdido, e a leitura seguinte num sender que nunca enviou fica pendurada
/// para sempre. Qualquer tarefa que só termine quando `read_rtcp` falhar fica
/// viva — e com ela tudo o que segura (a `Publication` e a PC do publicador).
///
/// Se este teste começar a falhar é porque a biblioteca corrigiu a armadilha:
/// óptimo, mas o contorno em `subscribe_layer` continua a ser inofensivo.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn webrtc_rs_read_rtcp_depois_de_stop_nao_regressa() {
    use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
    let pc = client_api()
        .await
        .new_peer_connection(RTCConfiguration::default())
        .await
        .unwrap();
    let track = Arc::new(TrackLocalStaticRTP::new(
        RTCRtpCodecCapability {
            mime_type: VP8.to_owned(),
            ..Default::default()
        },
        "v".to_owned(),
        "s".to_owned(),
    ));
    let sender = pc
        .add_track(track as Arc<dyn TrackLocal + Send + Sync>)
        .await
        .unwrap();
    // Paragem ANTES de alguém estar a ler — é a janela que o SFU atravessa ao
    // subscrever e dessubscrever antes de a tarefa de RTCP ser escalonada.
    pc.remove_track(&sender).await.unwrap();
    let r = tokio::time::timeout(Duration::from_secs(2), sender.read_rtcp()).await;
    assert!(
        r.is_err(),
        "read_rtcp regressou depois de stop() — a biblioteca mudou, rever o contorno"
    );
    pc.close().await.unwrap();
}

/// **PC que falha por ICE tem de FECHAR e largar as portas UDP.**
///
/// Medido a 2026-09-17: com o host saturado (perda de 64%), um servidor parado
/// ficou >30 min com 359 sockets UDP abertos e zero peers. O caminho é o
/// `Failed`: o handler de estado chamava `remove_peer` → `pc.close()` DENTRO do
/// callback do webrtc-rs, que segura o mutex do handler durante a chamada; o
/// `close()` volta a pedir esse mutex para anunciar `Closed` e fica pendurado
/// para sempre — depois de o peer já ter saído da sala, por isso os gauges
/// liam zero.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pc_que_falha_por_ice_fecha_e_nao_fica_viva() {
    let (sfu, _metrics) = new_sfu_com(Some((
        Duration::from_millis(500),
        Duration::from_millis(1500),
    )));
    let room = Uuid::new_v4();

    let a = TestClient::join(&sfu, room).await;
    a.publish(OPUS, "a-audio").await;
    let b = TestClient::join(&sfu, room).await;
    b.publish(OPUS, "b-audio").await;

    eventually_com_diagnostico(
        "A e B ligados",
        prazo(30),
        || {
            let b = b.clone();
            let a_id = a.id.to_string();
            async move { b.rtp_seen.lock().await.get(&a_id).copied().unwrap_or(0) > 0 }
        },
        || format!("A[{}] · B[{}]", a.retrato(), b.retrato()),
    )
    .await;
    assert_eq!(sfu.census().await.pc_unclosed, 2);

    // B desaparece sem dizer nada ao SFU (browser morto, rede caída).
    b.pc.close().await.unwrap();

    // O handler de `Failed` tira-o da sala…
    eventually("B retirado da sala depois de Failed", prazo(30), || {
        let sfu = sfu.clone();
        async move { sfu.census().await.peers_in_rooms == 1 }
    })
    .await;

    // …e a PC dele tem de chegar ao fim do close() e deixar de existir.
    let prazo_fecho = prazo(15);
    let inicio = tokio::time::Instant::now();
    loop {
        let c = sfu.census().await;
        if c.pc_unclosed == 1 && c.pc_alive == 1 {
            break;
        }
        assert!(
            inicio.elapsed() < prazo_fecho,
            "a PC de B saiu da sala mas não fechou/libertou: {c:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    sfu.remove_peer(room, a.id).await;
    a.pc.close().await.unwrap();
    esperar_censo_vazio(&sfu, "depois de A sair", prazo(20)).await;
}

/// **Nada do SFU sobrevive à saída de toda a gente** — nem com subscrições
/// criadas e desfeitas antes de a renegociação acontecer.
///
/// Alternar o interesse de vídeo cria um sender, lança a tarefa de RTCP e
/// remove-o logo a seguir. Com a armadilha de `read_rtcp` (ver o teste acima),
/// a tarefa que perdesse o `stop()` ficava viva para sempre a segurar a
/// `Publication` de A — e com ela a `RTCPeerConnection` de A.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn churn_de_subscricoes_nao_deixa_nada_vivo() {
    let (sfu, _metrics) = new_sfu();
    let room = Uuid::new_v4();

    let a = TestClient::join(&sfu, room).await;
    a.publish(OPUS, "a-audio").await;
    a.publish(VP8, "a-video").await;
    let b = TestClient::join(&sfu, room).await;
    b.publish(OPUS, "b-audio").await;

    eventually_com_diagnostico(
        "B recebe o vídeo de A",
        prazo(30),
        || {
            let b = b.clone();
            let a_id = a.id.to_string();
            async move {
                b.streams_seen()
                    .await
                    .iter()
                    .any(|(s, k)| *s == a_id && k == "video")
            }
        },
        || format!("A[{}] · B[{}]", a.retrato(), b.retrato()),
    )
    .await;

    for _ in 0..200 {
        sfu.set_video_interest(room, b.id, vec![], None).await;
        sfu.set_video_interest(room, b.id, vec![a.id], None).await;
    }
    let depois = sfu.census().await;
    eprintln!("churn: censo com A e B na sala = {depois:?}");

    sfu.remove_peer(room, b.id).await;
    sfu.remove_peer(room, a.id).await;
    a.pc.close().await.unwrap();
    b.pc.close().await.unwrap();
    esperar_censo_vazio(&sfu, "depois do churn", prazo(40)).await;
}
