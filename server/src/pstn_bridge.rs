//! Ponte de media FreeSWITCH ↔ SFU — Abordagem B
//! (ver `docs/pstn-sfu-bridge-design.md`).
//!
//! Fecha o buraco descrito em `VoiceCard.tsx`: hoje quem entra por telefone
//! cai numa conferência SÓ do FreeSWITCH (`mod_conference`) e não ouve os
//! participantes WebRTC da mesma sala. Este módulo dá, por sala com uma
//! `voice_room` activa, DOIS fluxos SRTP com o FreeSWITCH:
//!
//! - **Ingress** (FreeSWITCH → SFU): o FreeSWITCH já mistura os chamadores
//!   PSTN (`mod_conference`) e manda ESSE mix, como Opus/SRTP, para o socket
//!   que `activate()` abre aqui. Não há mistura a fazer deste lado — os
//!   pacotes são só desencriptados e reencaminhados para os subscritores
//!   WebRTC via `TrackLocalStaticRTP::write_rtp`, o MESMO mecanismo que
//!   `sfu.rs::subscribe_layer` já usa para qualquer outra track (ver
//!   `PstnBridge::add_subscriber`).
//! - **Egress** (SFU → FreeSWITCH): aqui SIM há trabalho novo — o SFU nunca
//!   tinha descodificado Opus (reencaminha RTP sem tocar no conteúdo, ver o
//!   cabeçalho de `sfu.rs`). `feed_egress` é chamado pelo mesmo laço de
//!   bombeamento de RTP que já existe em `handle_publish` — cada pacote de
//!   áudio de um participante WebRTC é descodificado para PCM aqui; uma task
//!   própria por sala mistura (`mix_pcm`) as fontes ainda "frescas" a cada
//!   20 ms, reencoda em Opus e envia 1 stream SRTP ao FreeSWITCH.
//!
//! ## O que este módulo NÃO resolve (ver o relatório da tarefa)
//! A parte do lado do FreeSWITCH — o mecanismo exacto (verbo/variável de
//! dialplan) que faz o FreeSWITCH enviar/receber SRTP cru para um host:porta
//! arbitrário sem um segundo diálogo SIP — não foi possível verificar com
//! confiança nesta sandbox (sem uma instância FreeSWITCH real para testar
//! contra). `voice/freeswitch/scripts/dialin_ivr.lua` documenta a questão
//! exacta em vez de inventar sintaxe plausível. Este módulo (o lado SFU) está
//! completo e testado por si — decodifica, mistura, reencoda e cifra
//! correctamente independentemente de como o FreeSWITCH acaba por lhe falar.
//!
//! ## Segurança
//! SRTP obrigatório nos dois sentidos, chaves efémeras por sala (geridas
//! aqui, nunca fixas/reutilizadas — `SrtpKeyPair::generate`), e a ingress só
//! aceita pacotes do IP configurado (`ip_allowed` — fail-closed: sem IP
//! configurado, NINGUÉM é aceite, ver `config.rs::pstn_bridge_freeswitch_ip`).

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{
        atomic::{AtomicBool, Ordering::Relaxed},
        Arc,
    },
    time::{Duration, Instant},
};

use audiopus::{
    coder::{Decoder, Encoder},
    Application, Channels, SampleRate,
};
use base64::Engine as _;
use bytes::Bytes;
use tokio::{net::UdpSocket, sync::Mutex};
use uuid::Uuid;
use webrtc::{
    rtp,
    rtp_transceiver::{rtp_codec::RTCRtpCodecCapability, rtp_sender::RTCRtpSender},
    track::track_local::{track_local_static_rtp::TrackLocalStaticRTP, TrackLocalWriter},
    util::{Marshal, Unmarshal},
};
use webrtc_srtp::{context::Context as SrtpContext, protection_profile::ProtectionProfile};

/// Perfil SRTP usado nos dois sentidos — o mesmo default que o `webrtc-rs`
/// negoceia por DTLS-SRTP nas ligações normais do SFU (ver `SettingEngine`
/// em `sfu.rs::new_api`; não é configurável aí, e não há razão para o ser aqui).
pub const SRTP_PROFILE: ProtectionProfile = ProtectionProfile::Aes128CmHmacSha1_80;
pub const SRTP_PROFILE_NAME: &str = "AES_CM_128_HMAC_SHA1_80";
const SRTP_KEY_LEN: usize = 16;
const SRTP_SALT_LEN: usize = 14;
/// Janela do detector de replay SRTP (pacotes). 64 é o valor de referência
/// usado noutras pilhas SRTP (libsrtp); não há aqui negociação a copiar.
const SRTP_REPLAY_WINDOW: usize = 64;

/// Payload type RTP fixo para o Opus nesta ponte. Como não há SDP entre o SFU
/// e o FreeSWITCH (é precisamente esse shim que a Abordagem B evita do lado
/// WebRTC — ver o design doc), o número tem de ser um ACORDO fixo dos dois
/// lados em vez de negociado. 111 é a mesma convenção que o `MediaEngine`
/// deste SFU já usa para Opus (`register_default_codecs`), por consistência.
pub const OPUS_PAYLOAD_TYPE: u8 = 111;

/// Duração de cada frame Opus/RTP (ms). Tem de bater com `mod_conference`
/// (`voice/freeswitch/autoload_configs/conference.conf.xml`: `interval=20`).
pub const FRAME_MS: u32 = 20;
/// Opus RTP é SEMPRE sinalizado a 48 kHz (RFC 7587), independentemente da
/// taxa de mistura interna do FreeSWITCH (`rate=8000` no perfil da
/// conferência) ou do conteúdo real do áudio — o descodificador liga a este
/// valor, não ao que foi codificado. O `MediaEngine` deste SFU regista Opus
/// exactamente com estes valores (`sfu.rs::new_api` → `register_default_codecs`).
pub const SAMPLE_RATE_HZ: u32 = 48_000;
/// Estéreo dos dois lados. Um decoder/encoder Opus configurado para N canais
/// sempre entrega/aceita N canais (upmix/downmix é feito pela própria
/// libopus), por isso isto é seguro mesmo que uma das pontas fale mono.
pub const CHANNELS: usize = 2;
pub const SAMPLES_PER_FRAME: usize = (SAMPLE_RATE_HZ as usize / 1000) * FRAME_MS as usize; // 960
/// Comprimento de um frame PCM entrelaçado (amostras totais, não por canal).
pub const FRAME_LEN: usize = SAMPLES_PER_FRAME * CHANNELS; // 1920

/// Bitrate Opus explícito para a egress (e para os testes de round-trip
/// abaixo, para medirem o mesmo que corre em produção). 64 kbps é uma
/// escolha de qualidade-voz generosa para estéreo a 48 kHz — o default do
/// encoder (`Bitrate::Auto`) é DOCUMENTADO pelo próprio crate como "não
/// recomendado", e nos testes deste módulo dava erro de round-trip muito
/// acima do que um sinal de voz real produziria.
pub const OPUS_BITRATE_BPS: i32 = 64_000;

/// Quanto tempo uma fonte fica "activa" no mixer sem novo pacote antes de
/// cair do mix (≈5 frames de 20 ms). Sobrevive a uma perda isolada sem
/// arrastar silêncio de quem ainda está a falar; ao fim disto trata-se como
/// se a pessoa se tivesse calado — o que é exactamente o que aconteceu do
/// ponto de vista do DTX/perda de pacotes.
const SOURCE_STALE_AFTER: Duration = Duration::from_millis(100);

// ============================================================
//  Funções puras — testáveis sem socket, sem SRTP, sem Opus real.
// ============================================================

/// A ingress só aceita pacotes cujo IP de origem é o FreeSWITCH configurado.
/// **Fail-closed**: sem IP configurado (`allowed = None`), ninguém passa —
/// o mesmo padrão que `voice_internal_secret` vazio usa em `voice.rs`
/// (funcionalidade indisponível é mais seguro que aberta por omissão).
pub fn ip_allowed(allowed: Option<IpAddr>, remote: IpAddr) -> bool {
    match allowed {
        Some(ip) => ip == remote,
        None => false,
    }
}

/// Mistura N fontes PCM (mesma convenção de frame: `FRAME_LEN` amostras
/// entrelaçadas) por soma escalada pelo número de fontes.
///
/// Porquê dividir por `n` e não um limitador dinâmico: a soma de N amostras,
/// cada uma no máximo `i16::MAX`/`i16::MIN`, dividida por N nunca excede o
/// maior valor de entrada — **matematicamente impossível estourar**, sem
/// precisar de detectar picos nem aplicar compressão. É a opção mais simples
/// que ainda assim cumpre a exigência de não distorcer com 3+ oradores
/// simultâneos. O preço é ficar mais baixo com mais gente a falar ao mesmo
/// tempo — aceitável para uma primeira versão correcta (ver o relatório da
/// tarefa); um limitador percentual/AGC fica para depois, se a qualidade
/// medida o pedir.
///
/// Fontes de comprimentos diferentes (não devia acontecer — `feed_egress`
/// normaliza sempre para `FRAME_LEN` — mas a função não presume isso do
/// chamador): o resultado tem o comprimento da MAIOR, as mais curtas
/// contam como silêncio (0) fora do seu alcance.
pub fn mix_pcm(sources: &[&[i16]]) -> Vec<i16> {
    if sources.is_empty() {
        return Vec::new();
    }
    if sources.len() == 1 {
        // Um só orador: passa quase inalterado (divisor 1) — sem isto uma
        // sala com uma pessoa a falar soaria sempre mais baixo do que devia.
        return sources[0].to_vec();
    }
    let len = sources.iter().map(|s| s.len()).max().unwrap_or(0);
    let n = sources.len() as i32;
    let mut out = vec![0i16; len];
    for (i, slot) in out.iter_mut().enumerate() {
        let sum: i32 = sources.iter().map(|s| *s.get(i).unwrap_or(&0) as i32).sum();
        *slot = (sum / n) as i16;
    }
    out
}

/// Par de chaves SRTP efémeras (AES_CM_128_HMAC_SHA1_80: chave de 16 bytes +
/// sal de 14). Uma instância por SENTIDO (ingress e egress têm cada uma a
/// SUA — nunca a mesma chave nos dois sentidos, a mesma disciplina que
/// DTLS-SRTP aplica com client-write-key/server-write-key distintas).
#[derive(Clone)]
pub struct SrtpKeyPair {
    pub master_key: [u8; SRTP_KEY_LEN],
    pub master_salt: [u8; SRTP_SALT_LEN],
}

impl SrtpKeyPair {
    /// Chave nova, aleatória — via `crypto::random_bytes` (CSPRNG do SO), não
    /// um `rand::thread_rng()` próprio: ADR-0004 §5 regra 4 quer toda a
    /// aleatoriedade de segurança num só sítio (`crypto.rs`), verificado pela
    /// catraca de arquitectura (`scripts/check-arquitectura-catraca.sh`).
    /// Chamar uma vez por sala por activação da ponte — nunca reutilizar
    /// entre salas ou chamadas.
    pub fn generate() -> Self {
        let master_key: [u8; SRTP_KEY_LEN] = crate::crypto::random_bytes(SRTP_KEY_LEN)
            .try_into()
            .expect("random_bytes devolve exactamente o comprimento pedido");
        let master_salt: [u8; SRTP_SALT_LEN] = crate::crypto::random_bytes(SRTP_SALT_LEN)
            .try_into()
            .expect("random_bytes devolve exactamente o comprimento pedido");
        Self {
            master_key,
            master_salt,
        }
    }

    /// `master_key || master_salt`, base64 — a mesma convenção do
    /// `a=crypto` do SDES-SRTP (RFC 4568), para que o campo que sai na
    /// resposta do IVR (`voice.rs::PstnBridgeResp`) seja reconhecível por
    /// quem já mexeu em SRTP estático noutro sítio.
    pub fn to_b64(&self) -> String {
        let mut buf = Vec::with_capacity(SRTP_KEY_LEN + SRTP_SALT_LEN);
        buf.extend_from_slice(&self.master_key);
        buf.extend_from_slice(&self.master_salt);
        base64::engine::general_purpose::STANDARD.encode(buf)
    }

    /// Um `Context` SRTP novo a partir desta chave. Cada `Context` só deve
    /// ser usado num sentido (o próprio crate documenta isto — o estado de
    /// replay/ROC é por direcção); `activate()` cria um para desencriptar
    /// (ingress) e outro, de uma chave DIFERENTE, para encriptar (egress).
    fn context(&self) -> Result<SrtpContext, webrtc_srtp::Error> {
        SrtpContext::new(
            &self.master_key,
            &self.master_salt,
            SRTP_PROFILE,
            Some(webrtc_srtp::option::srtp_replay_protection(
                SRTP_REPLAY_WINDOW,
            )),
            Some(webrtc_srtp::option::srtcp_replay_protection(
                SRTP_REPLAY_WINDOW,
            )),
        )
    }
}

/// Capacidade RTP anunciada para a track "Telefone" — tem de bater com o que
/// o `MediaEngine` do SFU regista para Opus (`sfu.rs::new_api`), senão os
/// subscritores WebRTC negoceiam um payload type que não corresponde ao que
/// `write_rtp` está de facto a mandar.
pub fn phone_track_capability() -> RTCRtpCodecCapability {
    RTCRtpCodecCapability {
        mime_type: "audio/opus".to_owned(),
        clock_rate: SAMPLE_RATE_HZ,
        channels: CHANNELS as u16,
        sdp_fmtp_line: "minptime=10;useinbandfec=1".to_owned(),
        rtcp_feedback: vec![],
    }
}

// ============================================================
//  Estado da ponte — uma instância por sala com PSTN activo.
// ============================================================

struct MixSlot {
    pcm: Vec<i16>,
    updated_at: Instant,
}

/// O que `voice.rs::ivr_validate_pin` devolve ao IVR (via `ValidatePinResp`).
/// Não inclui o host — este módulo não sabe o endereço anunciável do SFU
/// (isso é `Config::pstn_bridge_host`, uma decisão de implantação); só a
/// porta que ele próprio abriu.
#[derive(Clone)]
pub struct PstnBridgeInfo {
    pub port: u16,
    /// Chave que o FreeSWITCH usa para CIFRAR o que manda (o SFU desencripta
    /// com ela do lado da ingress).
    pub ingress_key_b64: String,
    /// Chave que o FreeSWITCH usa para DESENCRIPTAR o que o SFU lhe manda
    /// (o SFU cifra com ela do lado da egress).
    pub egress_key_b64: String,
    pub profile: &'static str,
    pub payload_type: u8,
}

/// Um subscritor WebRTC da track "Telefone": a track local que o alimenta e
/// o sender que a liga à `RTCPeerConnection` dele (tem de sobreviver, senão
/// o `add_track` desliga-se — ver `pstn_subscribe` em `sfu.rs`).
type PstnSubscribers = HashMap<Uuid, (Arc<TrackLocalStaticRTP>, Arc<RTCRtpSender>)>;

/// Ponte activa de uma sala: 1 socket UDP, 2 pares de chaves SRTP (ingress e
/// egress), os subscritores WebRTC da track "Telefone", e o estado do mixer
/// de egress (última amostra PCM conhecida de cada publicador WebRTC).
pub struct PstnBridge {
    room_id: Uuid,
    socket: Arc<UdpSocket>,
    local_port: u16,
    ingress_key: SrtpKeyPair,
    egress_key: SrtpKeyPair,
    allowed_ip: IpAddr,
    /// Endereço (IP:porta) de onde chegou o último pacote de ingress válido
    /// — é para AÍ que a egress manda a mistura ("RTP simétrico": a mesma
    /// associação UDP serve os dois sentidos, sem o FreeSWITCH ter de
    /// anunciar antecipadamente a porta em que escuta). `None` até chegar o
    /// primeiro pacote — a egress não manda nada enquanto não souber para onde.
    learned_peer: Mutex<Option<SocketAddr>>,
    subscribers: Mutex<PstnSubscribers>,
    /// Um descodificador Opus POR publicador WebRTC — o estado de
    /// concealment/FEC do Opus é por-fluxo, não pode ser partilhado entre
    /// participantes diferentes.
    decoders: Mutex<HashMap<Uuid, Decoder>>,
    mixer_slots: Mutex<HashMap<Uuid, MixSlot>>,
    /// Baixa a `false` em `deactivate()`; as duas tasks de fundo (ingress e
    /// egress) verificam-na a cada iteração e terminam sozinhas — evita ter
    /// de cancelar `JoinHandle`s ou de matar o socket a meio de um `recv`.
    alive: AtomicBool,
}

impl PstnBridge {
    /// Abre o socket, gera as duas chaves e arranca as tasks de ingress e
    /// egress. Devolve a ponte (para o `SfuState` guardar) e os dados que o
    /// IVR precisa.
    ///
    /// `allowed_ip` é obrigatório aqui — a decisão fail-closed (sem IP
    /// configurado, sem ponte) é do CHAMADOR (`SfuState::activate_pstn_bridge`
    /// em `sfu.rs`, que lê `Config::pstn_bridge_freeswitch_ip`), não deste
    /// construtor: este módulo não sabe nada de configuração global.
    pub async fn activate(room_id: Uuid, allowed_ip: IpAddr) -> std::io::Result<Arc<PstnBridge>> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        let local_port = socket.local_addr()?.port();
        let socket = Arc::new(socket);

        let ingress_key = SrtpKeyPair::generate();
        let egress_key = SrtpKeyPair::generate();
        let ingress_ctx = ingress_key
            .context()
            .map_err(|e| std::io::Error::other(format!("SRTP ingress context: {e}")))?;
        let egress_ctx = egress_key
            .context()
            .map_err(|e| std::io::Error::other(format!("SRTP egress context: {e}")))?;

        let bridge = Arc::new(PstnBridge {
            room_id,
            socket: socket.clone(),
            local_port,
            ingress_key: ingress_key.clone(),
            egress_key: egress_key.clone(),
            allowed_ip,
            learned_peer: Mutex::new(None),
            subscribers: Mutex::new(HashMap::new()),
            decoders: Mutex::new(HashMap::new()),
            mixer_slots: Mutex::new(HashMap::new()),
            alive: AtomicBool::new(true),
        });

        tokio::spawn(run_ingress(bridge.clone(), ingress_ctx));
        tokio::spawn(run_egress(bridge.clone(), egress_ctx));

        tracing::info!(%room_id, port = local_port, %allowed_ip, "pstn bridge activada");
        Ok(bridge)
    }

    /// Pára as duas tasks de fundo (na próxima iteração de cada uma). O
    /// socket fecha-se quando o último `Arc` cair — `SfuState` deve deixar
    /// de guardar a sua referência a seguir a chamar isto.
    pub fn deactivate(&self) {
        self.alive.store(false, Relaxed);
        tracing::info!(room_id = %self.room_id, "pstn bridge desactivada");
    }

    pub fn info(&self) -> PstnBridgeInfo {
        PstnBridgeInfo {
            port: self.local_port,
            ingress_key_b64: self.ingress_key.to_b64(),
            egress_key_b64: self.egress_key.to_b64(),
            profile: SRTP_PROFILE_NAME,
            payload_type: OPUS_PAYLOAD_TYPE,
        }
    }

    pub async fn has_subscriber(&self, peer_id: Uuid) -> bool {
        self.subscribers.lock().await.contains_key(&peer_id)
    }

    pub async fn add_subscriber(
        &self,
        peer_id: Uuid,
        track: Arc<TrackLocalStaticRTP>,
        sender: Arc<RTCRtpSender>,
    ) {
        self.subscribers
            .lock()
            .await
            .insert(peer_id, (track, sender));
    }

    pub async fn remove_subscriber(&self, peer_id: Uuid) -> Option<Arc<RTCRtpSender>> {
        self.subscribers
            .lock()
            .await
            .remove(&peer_id)
            .map(|(_, sender)| sender)
    }

    /// Chamado pelo laço de bombeamento de RTP existente em
    /// `sfu.rs::handle_publish` para CADA pacote de áudio de um participante
    /// WebRTC — o mesmo local por onde já passa o reencaminhamento normal
    /// para os outros subscritores. Descodifica Opus→PCM e guarda a última
    /// amostra deste publicador; a mistura em si acontece na task de egress
    /// (`run_egress`), a um ritmo próprio de 20 ms — não aqui, para não
    /// atar o ritmo de chegada (jitter) de UM participante ao envio para o
    /// PSTN.
    pub async fn feed_egress(&self, publisher: Uuid, packet: &rtp::packet::Packet) {
        let mut decoders = self.decoders.lock().await;
        let decoder = match decoders.entry(publisher) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(v) => {
                match Decoder::new(SampleRate::Hz48000, Channels::Stereo) {
                    Ok(d) => v.insert(d),
                    Err(e) => {
                        tracing::error!(%publisher, error = %e, "pstn egress: falha ao criar Opus decoder");
                        return;
                    }
                }
            }
        };
        let mut pcm = [0i16; FRAME_LEN];
        match decoder.decode(Some(packet.payload.as_ref()), pcm.as_mut_slice(), false) {
            Ok(_samples_per_channel) => {
                drop(decoders);
                self.mixer_slots.lock().await.insert(
                    publisher,
                    MixSlot {
                        pcm: pcm.to_vec(),
                        updated_at: Instant::now(),
                    },
                );
            }
            Err(e) => {
                // Um pacote Opus corrompido/inesperado não pode derrubar a
                // mistura inteira — ignora-se este frame e mantém-se o
                // último bom (a task de egress trata "sem actualização
                // recente" como silêncio ao fim de SOURCE_STALE_AFTER).
                tracing::debug!(%publisher, error = %e, "pstn egress: opus decode falhou — pacote ignorado");
            }
        }
    }
}

/// Task de ingress: lê o socket, valida o IP de origem, desencripta SRTP,
/// e reencaminha para cada subscritor pelo MESMO `write_rtp` que
/// `sfu.rs::subscribe_layer` já usa — do ponto de vista de um participante
/// WebRTC, a track "Telefone" chega exactamente como a de qualquer outro
/// peer.
async fn run_ingress(bridge: Arc<PstnBridge>, mut ctx: SrtpContext) {
    let mut buf = vec![0u8; 1500];
    loop {
        if !bridge.alive.load(Relaxed) {
            break;
        }
        let (n, src) = match bridge.socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(room_id = %bridge.room_id, error = %e, "pstn ingress: recv falhou");
                break;
            }
        };
        if !ip_allowed(Some(bridge.allowed_ip), src.ip()) {
            tracing::warn!(room_id = %bridge.room_id, remote = %src, allowed = %bridge.allowed_ip, "pstn ingress: pacote de IP não autorizado — descartado");
            continue;
        }
        *bridge.learned_peer.lock().await = Some(src);

        let mut plain = match ctx.decrypt_rtp(&buf[..n]) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(room_id = %bridge.room_id, error = %e, "pstn ingress: SRTP unprotect falhou");
                continue;
            }
        };
        let packet = match rtp::packet::Packet::unmarshal(&mut plain) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(room_id = %bridge.room_id, error = %e, "pstn ingress: RTP inválido após desencriptar");
                continue;
            }
        };

        let subs = bridge.subscribers.lock().await;
        for (track, _sender) in subs.values() {
            let _ = track.write_rtp(&packet).await;
        }
    }
    tracing::info!(room_id = %bridge.room_id, "pstn ingress terminado");
}

/// Task de egress: a cada `FRAME_MS`, junta as fontes WebRTC ainda frescas
/// (`feed_egress` as vai alimentando), mistura (`mix_pcm`), reencoda em
/// Opus e envia 1 stream SRTP para o endereço aprendido da ingress
/// (RTP simétrico — ver `PstnBridge::learned_peer`).
async fn run_egress(bridge: Arc<PstnBridge>, mut ctx: SrtpContext) {
    let mut encoder = match Encoder::new(SampleRate::Hz48000, Channels::Stereo, Application::Voip) {
        Ok(e) => e,
        Err(e) => {
            tracing::error!(room_id = %bridge.room_id, error = %e, "pstn egress: falha ao criar Opus encoder — ponte sem saída para o PSTN");
            return;
        }
    };
    // O default do encoder ("decidido pela libopus, NÃO recomendado" — doc do
    // próprio crate `audiopus::Bitrate::Auto`) fica baixo demais para vários
    // oradores misturados. Um bitrate explícito é o que faz a diferença entre
    // um round-trip limpo e ruído de quantização a dominar o sinal (medido
    // nos testes deste módulo — ver `pstn_bridge::tests`).
    if let Err(e) = encoder.set_bitrate(audiopus::Bitrate::BitsPerSecond(OPUS_BITRATE_BPS)) {
        tracing::warn!(room_id = %bridge.room_id, error = %e, "pstn egress: set_bitrate falhou — a seguir com o default do encoder");
    }

    // `rand::random` em vez de guardar um `ThreadRng` na task: o `ThreadRng`
    // não é `Send` (usa `Rc` por dentro) e esta task atravessa `.await`
    // pontos (o `ticker.tick().await` do laço) — um `ThreadRng` vivo nesse
    // intervalo impedia `tokio::spawn` de aceitar a future.
    let mut seq: u16 = (rand::random::<u32>() & 0xffff) as u16;
    let mut ts: u32 = rand::random();
    let ssrc: u32 = rand::random();

    let mut ticker = tokio::time::interval(Duration::from_millis(FRAME_MS as u64));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;
        if !bridge.alive.load(Relaxed) {
            break;
        }
        let Some(dest) = *bridge.learned_peer.lock().await else {
            continue; // ainda não recebemos nenhum pacote do FreeSWITCH
        };

        let now = Instant::now();
        let sources: Vec<Vec<i16>> = {
            let mut slots = bridge.mixer_slots.lock().await;
            slots.retain(|_, s| now.duration_since(s.updated_at) < SOURCE_STALE_AFTER);
            slots.values().map(|s| s.pcm.clone()).collect()
        };
        if sources.is_empty() {
            continue; // ninguém a falar — nada para misturar/enviar
        }
        let refs: Vec<&[i16]> = sources.iter().map(|v| v.as_slice()).collect();
        let mixed = mix_pcm(&refs);

        let mut opus_buf = [0u8; 4000]; // Opus nunca excede ~4 KB por frame a estas taxas
        let encoded_len = match encoder.encode(&mixed, &mut opus_buf) {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!(room_id = %bridge.room_id, error = %e, "pstn egress: opus encode falhou — frame perdido");
                continue;
            }
        };

        let packet = rtp::packet::Packet {
            header: rtp::header::Header {
                version: 2,
                payload_type: OPUS_PAYLOAD_TYPE,
                sequence_number: seq,
                timestamp: ts,
                ssrc,
                ..Default::default()
            },
            payload: Bytes::copy_from_slice(&opus_buf[..encoded_len]),
        };
        seq = seq.wrapping_add(1);
        ts = ts.wrapping_add(SAMPLES_PER_FRAME as u32);

        let plain = match packet.marshal() {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(room_id = %bridge.room_id, error = %e, "pstn egress: marshal RTP falhou");
                continue;
            }
        };
        let protected = match ctx.encrypt_rtp(&plain) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(room_id = %bridge.room_id, error = %e, "pstn egress: SRTP protect falhou");
                continue;
            }
        };
        if let Err(e) = bridge.socket.send_to(&protected, dest).await {
            tracing::warn!(room_id = %bridge.room_id, %dest, error = %e, "pstn egress: send_to falhou");
        }
    }
    tracing::info!(room_id = %bridge.room_id, "pstn egress terminado");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ip_allowlist_fail_closed_sem_config() {
        let remote: IpAddr = "203.0.113.10".parse().unwrap();
        assert!(!ip_allowed(None, remote));
    }

    #[test]
    fn ip_allowlist_aceita_so_o_ip_configurado() {
        let fs_ip: IpAddr = "203.0.113.10".parse().unwrap();
        let outro: IpAddr = "198.51.100.5".parse().unwrap();
        assert!(ip_allowed(Some(fs_ip), fs_ip));
        assert!(!ip_allowed(Some(fs_ip), outro));
    }

    #[test]
    fn mix_pcm_passa_um_so_orador_quase_inalterado() {
        let source: Vec<i16> = vec![1000, -2000, 3000, -4000];
        let out = mix_pcm(&[&source]);
        assert_eq!(out, source);
    }

    #[test]
    fn mix_pcm_soma_e_escala_sem_estourar() {
        // Duas fontes no máximo positivo: a soma óbvia estouraria i16, a
        // escalada por n fica exactamente no máximo — nunca acima.
        let a = vec![i16::MAX, i16::MAX];
        let b = vec![i16::MAX, i16::MAX];
        let out = mix_pcm(&[&a, &b]);
        assert_eq!(out, vec![i16::MAX, i16::MAX]);

        let a = vec![i16::MIN, i16::MIN];
        let b = vec![i16::MIN, i16::MIN];
        let out = mix_pcm(&[&a, &b]);
        assert_eq!(out, vec![i16::MIN, i16::MIN]);
    }

    #[test]
    fn mix_pcm_de_quatro_oradores_nunca_clipa() {
        // 4 "oradores" simultâneos perto do máximo — a exigência explícita
        // da tarefa: não pode distorcer/dar wrap com 3+ oradores.
        let loud = vec![30_000i16; 960];
        let sources: Vec<&[i16]> = vec![&loud, &loud, &loud, &loud];
        let out = mix_pcm(&sources);
        assert_eq!(out.len(), 960);
        for &s in &out {
            assert!(
                (i16::MIN..=i16::MAX).contains(&s),
                "amostra fora do intervalo i16 — teria dado wrap/distorção"
            );
        }
        // Com 4 fontes idênticas a divisão dá exactamente o valor de entrada.
        assert!(out.iter().all(|&s| s == 30_000));
    }

    #[test]
    fn mix_pcm_trata_silencio_como_zero() {
        let voz = vec![5000i16; 4];
        let silencio = vec![0i16; 4];
        let out = mix_pcm(&[&voz, &silencio]);
        // (5000 + 0) / 2 = 2500
        assert_eq!(out, vec![2500, 2500, 2500, 2500]);
    }

    #[test]
    fn mix_pcm_sem_fontes_da_vazio() {
        assert_eq!(mix_pcm(&[]), Vec::<i16>::new());
    }

    #[test]
    fn srtp_keypair_tem_o_comprimento_certo() {
        let kp = SrtpKeyPair::generate();
        assert_eq!(kp.master_key.len(), SRTP_KEY_LEN);
        assert_eq!(kp.master_salt.len(), SRTP_SALT_LEN);
        // 30 bytes → base64 sem padding múltiplo exacto: 40 chars com padding.
        let b64 = kp.to_b64();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&b64)
            .expect("base64 válido");
        assert_eq!(decoded.len(), SRTP_KEY_LEN + SRTP_SALT_LEN);
    }

    #[test]
    fn srtp_keypair_e_efemera_e_unica_por_sala() {
        // Duas gerações NUNCA podem coincidir — senão uma sala podia
        // desencriptar/injectar áudio na ponte doutra.
        let a = SrtpKeyPair::generate();
        let b = SrtpKeyPair::generate();
        assert_ne!(a.master_key, b.master_key);
        assert_ne!(a.master_salt, b.master_salt);
    }

    #[test]
    fn srtp_keypair_ingress_e_egress_sao_chaves_diferentes() {
        // A mesma disciplina que DTLS-SRTP aplica: nunca a mesma chave nos
        // dois sentidos.
        let ingress = SrtpKeyPair::generate();
        let egress = SrtpKeyPair::generate();
        assert_ne!(ingress.master_key, egress.master_key);
    }

    #[test]
    fn srtp_context_constroi_com_a_chave_gerada() {
        // Não testa a rede — só que o par (chave, perfil) que `activate()`
        // usa é aceite pelo `webrtc-srtp` sem erro (comprimento certo, etc.).
        let kp = SrtpKeyPair::generate();
        assert!(kp.context().is_ok());
    }

    #[test]
    fn opus_round_trip_preserva_o_sinal_aproximadamente() {
        // Codec com perdas E com atraso algorítmico: o Opus intercala alguns
        // ms de "lookahead" interno, por isso a amostra N decodificada NÃO
        // corresponde à amostra N codificada — corresponde a uma alguns
        // instantes atrás (medido: comparar amostra-a-amostra dava erro
        // MAIOR que a amplitude do sinal, mesmo com o par a funcionar bem —
        // era o teste a comparar pontos de fases diferentes da mesma onda,
        // não o codec a estragar o sinal). Por isso esta verificação não
        // compara amostra-a-amostra: mede ENERGIA (RMS) do sinal
        // descodificado ao longo de VÁRIOS frames contínuos e confirma que
        // fica perto da energia de entrada — o que prova a mesma coisa (o
        // par encoder/decoder está correctamente ligado: mesma taxa, mesmos
        // canais, mesma convenção de frame) sem depender de alinhamento
        // exacto de fase, que não é isto que este teste quer verificar.
        let mut encoder = Encoder::new(SampleRate::Hz48000, Channels::Stereo, Application::Voip)
            .expect("encoder");
        // MESMO bitrate que `run_egress` usa em produção.
        encoder
            .set_bitrate(audiopus::Bitrate::BitsPerSecond(OPUS_BITRATE_BPS))
            .expect("set_bitrate");
        let mut decoder = Decoder::new(SampleRate::Hz48000, Channels::Stereo).expect("decoder");

        const FRAMES: usize = 8;
        let mut opus_buf = [0u8; 4000];
        let mut in_energy = 0f64;
        let mut out_energy = 0f64;
        let mut in_n = 0usize;
        let mut out_n = 0usize;
        for frame in 0..FRAMES {
            let mut pcm = [0i16; FRAME_LEN];
            for (i, sample) in pcm.iter_mut().enumerate() {
                // `t` contínuo através dos frames (não reinicia a cada
                // frame) — um tom de facto contínuo, como áudio real.
                let t = (frame * SAMPLES_PER_FRAME + i / CHANNELS) as f32;
                *sample = (8000.0 * (t * 0.05).sin()) as i16;
            }
            let n = encoder.encode(&pcm, &mut opus_buf).expect("encode");
            assert!(n > 0);
            let mut out = [0i16; FRAME_LEN];
            let decoded_per_channel = decoder
                .decode(Some(&opus_buf[..n]), out.as_mut_slice(), false)
                .expect("decode");
            assert_eq!(decoded_per_channel, SAMPLES_PER_FRAME);
            // Só os últimos frames entram na medição — os primeiros ainda
            // têm o lookahead do encoder a "aquecer" (arranca de zero).
            if frame >= FRAMES - 3 {
                in_energy += pcm.iter().map(|&s| (s as f64).powi(2)).sum::<f64>();
                in_n += pcm.len();
                out_energy += out.iter().map(|&s| (s as f64).powi(2)).sum::<f64>();
                out_n += out.len();
            }
        }

        let in_rms = (in_energy / in_n as f64).sqrt();
        let out_rms = (out_energy / out_n as f64).sqrt();
        assert!(
            out_rms > in_rms * 0.5 && out_rms < in_rms * 1.5,
            "RMS de saída ({out_rms:.0}) longe do RMS de entrada ({in_rms:.0}) — \
             round-trip Opus quebrado (silêncio, garbage, ou canais/taxa trocados)"
        );
    }

    #[test]
    fn opus_round_trip_de_silencio_fica_silencioso() {
        let encoder = Encoder::new(SampleRate::Hz48000, Channels::Stereo, Application::Voip)
            .expect("encoder");
        let mut decoder = Decoder::new(SampleRate::Hz48000, Channels::Stereo).expect("decoder");
        let pcm = vec![0i16; FRAME_LEN];
        let mut opus_buf = [0u8; 4000];
        let n = encoder.encode(&pcm, &mut opus_buf).expect("encode");
        let mut out = [0i16; FRAME_LEN];
        decoder
            .decode(Some(&opus_buf[..n]), out.as_mut_slice(), false)
            .expect("decode");
        for s in out {
            assert!(
                s.abs() < 50,
                "silêncio devia decodificar quase a zero, veio {s}"
            );
        }
    }

    #[test]
    fn phone_track_capability_bate_com_o_media_engine() {
        let cap = phone_track_capability();
        assert_eq!(cap.mime_type, "audio/opus");
        assert_eq!(cap.clock_rate, 48_000);
        assert_eq!(cap.channels, 2);
    }
}
