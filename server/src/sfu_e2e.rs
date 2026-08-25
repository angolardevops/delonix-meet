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
    rtp_transceiver::{rtp_codec::RTCRtpCodecCapability, RTCRtpTransceiverInit},
    track::track_local::{track_local_static_sample::TrackLocalStaticSample, TrackLocal},
};

use crate::{
    metrics::Metrics,
    sfu::{IceConfig, SfuState},
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
}

async fn client_api() -> webrtc::api::API {
    let mut media = MediaEngine::default();
    media.register_default_codecs().unwrap();
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
        let (tx, rx, _shutdown) =
            crate::signaling::PeerTx::new(512, Arc::new(Metrics::default()));
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
            pc.on_track(Box::new(move |remote, _r, _t| {
                let received = received.clone();
                let rtp_seen = rtp_seen.clone();
                Box::pin(async move {
                    let stream_id = remote.stream_id().to_string();
                    received
                        .lock()
                        .await
                        .push((stream_id.clone(), remote.kind().to_string()));
                    tokio::spawn(async move {
                        while remote.read_rtp().await.is_ok() {
                            *rtp_seen.lock().await.entry(stream_id.clone()).or_insert(0) += 1;
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
        let Ok(answer) = self.pc.create_answer(None).await else { return };
        if self.pc.set_local_description(answer).await.is_err() {
            return;
        }
        let Some(local) = self.pc.local_description().await else { return };
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
                if client
                    .hold_answer
                    .load(std::sync::atomic::Ordering::SeqCst)
                {
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

fn new_sfu() -> (Arc<SfuState>, Arc<Metrics>) {
    let metrics = Arc::new(Metrics::default());
    (
        Arc::new(SfuState::new(IceConfig::default(), metrics.clone(), 64)),
        metrics,
    )
}

/// Espera até `cond` ou falha com `label` — evita `sleep` fixos frágeis.
async fn eventually<F, Fut>(label: &str, timeout: Duration, mut cond: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if cond().await {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("timeout à espera de: {label}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
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
    eventually("B recebe áudio+vídeo de A", Duration::from_secs(30), || {
        let b = b.clone();
        let a_id = a.id.to_string();
        async move {
            let seen = b.streams_seen().await;
            seen.iter().filter(|(s, _)| *s == a_id).count() >= 2
        }
    })
    .await;

    // …e o inverso: sem isto, "media num só sentido" passaria despercebido.
    eventually("A recebe áudio+vídeo de B", Duration::from_secs(30), || {
        let a = a.clone();
        let b_id = b.id.to_string();
        async move {
            let seen = a.streams_seen().await;
            seen.iter().filter(|(s, _)| *s == b_id).count() >= 2
        }
    })
    .await;

    // Tracks negociadas não chegam: exige-se RTP mesmo a passar pelo fan-out.
    eventually("RTP real de A para B", Duration::from_secs(30), || {
        let b = b.clone();
        let a_id = a.id.to_string();
        async move {
            b.rtp_seen
                .lock()
                .await
                .get(&a_id)
                .copied()
                .unwrap_or(0)
                > 0
        }
    })
    .await;

    // Asserções explícitas: um `eventually` que passasse por engano deixaria
    // isto a zero e o teste seria decorativo.
    let a_to_b = b.rtp_seen.lock().await.get(&a.id.to_string()).copied().unwrap_or(0);
    let b_to_a = a.rtp_seen.lock().await.get(&b.id.to_string()).copied().unwrap_or(0);
    eprintln!(
        "media_flows_both_ways: tracks B<-A={:?} A<-B={:?} | RTP A->B={a_to_b} B->A={b_to_a}",
        b.streams_seen().await,
        a.streams_seen().await
    );
    assert!(a_to_b > 0, "nenhum RTP de A chegou a B");
    assert_eq!(
        b.streams_seen().await.iter().filter(|(s, _)| *s == a.id.to_string()).count(),
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

    eventually("ligação inicial estabelecida", Duration::from_secs(30), || {
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
    eventually(
        "servidor com oferta pendente para A",
        Duration::from_secs(30),
        || {
            let a = a.clone();
            async move { !a.held.lock().await.is_empty() }
        },
    )
    .await;

    // …e é NESTE instante que A publica o ecrã (oferta do cliente em glare).
    let _screen = a.publish(VP8, "a-screen").await;

    // O VEREDICTO: a oferta de A foi ADIADA, não descartada. Com o bug o
    // contador ficaria a 0 — o `set_remote_description` falhava, saía um `warn`
    // e a track perdia-se sem deixar rasto em lado nenhum.
    eventually("oferta de A adiada pelo servidor", Duration::from_secs(20), || {
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
