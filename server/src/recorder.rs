//! Gravação server-side: o SFU alimenta estes writers com os pacotes RTP de
//! cada publicador (VP8 → IVF com PTS reais; Opus → OGG), e ao parar o
//! ffmpeg compõe tudo num único `.webm`:
//!  - 1 publicador  → o vídeo em cópia (zero reencode), o áudio recodificado;
//!  - N publicadores → grelha xstack em VP9 CRF 30 + Opus 128k (melhor
//!    rácio qualidade/tamanho sem perda percetível).
//!
//! O áudio passa SEMPRE por `AUDIO_GAP_FILL` antes de mais nada: uma pista
//! gravada tem buracos de PTS onde o participante se calou (DTX) ou onde se
//! perdeu um pacote, e sem os encher a fala seguinte recua (R295).
//!
//! A gravação entra na biblioteca LOGO ao parar, em `processing`, com o
//! progresso da composição lido do `-progress` do ffmpeg; passa a `ready`
//! quando o ficheiro existe e depois é medida (`media_probe`). A qualidade
//! pedida na reunião (`rooms.record_quality`) decide a grelha, a redução de
//! resolução e o «só áudio».

use aes_gcm::{
    aead::{Aead, Payload},
    Aes256Gcm, KeyInit,
};
use delonix_meet_domain::content::composition::{
    self as compose_rules, ComposeManifest, ComposeTrack,
};
use std::{
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};
use uuid::Uuid;
use webrtc::media::io::{ogg_writer::OggWriter, Writer as MediaWriter};
use webrtc::rtp::{codecs::vp8::Vp8Packet, packetizer::Depacketizer};

use crate::AppState;

/// Desencripta um frame E2EE do cliente: [header claro | ct+tag(16) | IV(12)],
/// AES-256-GCM com o header como additionalData (ver web/src/e2ee.ts).
/// Devolve header‖plaintext; None se não autenticar. Frames minúsculos
/// passaram em claro no emissor e voltam tal-qual.
fn decrypt_e2ee(key: &Aes256Gcm, data: &[u8], offset: usize) -> Option<Vec<u8>> {
    if data.len() <= offset + 12 + 16 {
        return Some(data.to_vec());
    }
    let (header, rest) = data.split_at(offset);
    let (ct, iv) = rest.split_at(rest.len() - 12);
    let nonce = aes_gcm::Nonce::try_from(iv).ok()?;
    let pt = key
        .decrypt(
            &nonce,
            Payload {
                msg: ct,
                aad: header,
            },
        )
        .ok()?;
    let mut out = Vec::with_capacity(offset + pt.len());
    out.extend_from_slice(header);
    out.extend_from_slice(&pt);
    Some(out)
}

/// Este pacote é o INÍCIO de um quadro VP8 — o primeiro pacote da primeira
/// partição (bit S do descritor e PID a zero, RFC 7741 §4.2)?
///
/// Só nesse pacote o primeiro byte do payload é o cabeçalho do quadro. Em
/// todos os outros é dado comprimido, e lê-lo como cabeçalho é ler ruído.
fn vp8_frame_start(depack: &Vp8Packet) -> bool {
    depack.s == 1 && depack.pid == 0
}

/// Este pacote abre um KEYFRAME VP8? `vp8` é o payload já sem o descritor.
///
/// O bit de keyframe é o bit 0 do cabeçalho do quadro, a zero, e um keyframe
/// traz logo a seguir o código de início `9d 01 2a` (RFC 6386 §9.1) — em claro
/// também numa sala E2EE, que deixa os 10 bytes do cabeçalho por cifrar. Tudo
/// isto só existe no pacote que inicia o quadro. A guarda antiga lia o bit em
/// qualquer pacote: uma continuação de um quadro delta com o primeiro byte par
/// passava por keyframe, e a pista abria a meio de um quadro que nenhum
/// descodificador aceita (R299).
fn vp8_starts_keyframe(depack: &Vp8Packet, vp8: &[u8]) -> bool {
    vp8_frame_start(depack)
        && vp8.len() >= 6
        && vp8[0] & 0x01 == 0
        && vp8[3..6] == [0x9d, 0x01, 0x2a]
}

/// O mesmo, a partir do pacote RTP inteiro — é o que o `RecWriter` usa para
/// saber, do lado de quem entrega, se a pista já tem o keyframe que a abre.
fn rtp_starts_vp8_keyframe(pkt: &webrtc::rtp::packet::Packet) -> bool {
    let mut depack = Vp8Packet::default();
    depack
        .depacketize(&pkt.payload)
        .is_ok_and(|vp8| vp8_starts_keyframe(&depack, &vp8))
}

/// IVF (VP8) com PTS em milissegundos derivados do timestamp RTP (90 kHz) —
/// o writer da lib usa um contador de frames, o que acelera/atrasa o vídeo
/// quando o fps varia; este mantém o tempo real.
pub struct Vp8IvfWriter {
    /// `BufWriter` e não `File` directo: cada pacote RTP fazia uma `write(2)`
    /// própria, e essa chamada é SÍNCRONA dentro da task async que reencaminha
    /// RTP — com uma gravação a 30 fps por publicador são milhares de syscalls
    /// por segundo a bloquear um worker do Tokio. Com buffer, é uma escrita a
    /// cada 64 KiB. O `close()` faz `seek` para corrigir o cabeçalho, e o
    /// `BufWriter` esvazia o buffer antes de qualquer `seek` — por isso o
    /// cabeçalho continua a ser corrigido correctamente.
    w: std::io::BufWriter<std::fs::File>,
    count: u32,
    first_ts: Option<u32>,
    frame: Vec<u8>,
    seen_key: bool,
    /// Aceso enquanto a pista não tiver o seu primeiro quadro ESCRITO. É a
    /// mesma bandeira que o `RecWriter` lê em `wants_keyframe`: quem entrega
    /// apaga-a quando põe um início de keyframe na fila, e esta thread volta a
    /// acendê-la se esse keyframe afinal não serviu (ver `reopen`).
    awaiting_key: Arc<std::sync::atomic::AtomicBool>,
    /// Número de sequência do último pacote junto ao quadro de ABERTURA.
    open_seq: Option<u16>,
    /// Dimensões reais lidas do primeiro keyframe (corrigidas no close).
    dims: Option<(u16, u16)>,
    /// Chave E2EE da sala (cedida pelo anfitrião) — desencripta cada frame.
    key: Option<Arc<Aes256Gcm>>,
}

impl Vp8IvfWriter {
    pub fn new(w: std::fs::File) -> std::io::Result<Self> {
        let mut w = std::io::BufWriter::with_capacity(64 * 1024, w);
        // Cabeçalho IVF de 32 bytes; timebase 1/1000 => PTS em ms.
        w.write_all(b"DKIF")?;
        w.write_all(&0u16.to_le_bytes())?; // versão
        w.write_all(&32u16.to_le_bytes())?; // tamanho do header
        w.write_all(b"VP80")?;
        w.write_all(&1280u16.to_le_bytes())?; // dimensões nominais; o VP8
        w.write_all(&720u16.to_le_bytes())?; //  real vem do bitstream
        w.write_all(&1000u32.to_le_bytes())?; // timebase denominador
        w.write_all(&1u32.to_le_bytes())?; // timebase numerador
        w.write_all(&0u32.to_le_bytes())?; // nº de frames (corrigido no close)
        w.write_all(&0u32.to_le_bytes())?;
        Ok(Self {
            w,
            count: 0,
            first_ts: None,
            frame: Vec::new(),
            seen_key: false,
            awaiting_key: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            open_seq: None,
            dims: None,
            key: None,
        })
    }

    pub fn write_rtp(&mut self, pkt: &webrtc::rtp::packet::Packet) -> std::io::Result<()> {
        if pkt.payload.is_empty() {
            return Ok(());
        }
        let mut depack = Vp8Packet::default();
        let Ok(payload) = depack.depacketize(&pkt.payload) else {
            return Ok(());
        };
        if payload.is_empty() {
            return Ok(());
        }
        // O quadro de abertura tem de chegar INTEIRO e seguido. Se lhe faltar
        // um pacote, ou se o seu primeiro pacote for uma retransmissão que
        // chegou a meio de outro quadro, o que se escrevia tinha cara de
        // keyframe (bit, código de início, dimensões) e era lixo — e ninguém
        // voltava a pedir outro. Um salto na sequência deita-o fora e a pista
        // volta a esperar.
        if self.count == 0 && !self.frame.is_empty() {
            if let Some(prev) = self.open_seq {
                if pkt.header.sequence_number != prev.wrapping_add(1) {
                    self.reopen();
                }
            }
        }
        // A pista só abre num keyframe verdadeiro: tudo o que chega antes —
        // quadros delta inteiros e os seus pacotes de continuação — fica de fora.
        if !self.seen_key {
            if !vp8_starts_keyframe(&depack, &payload) {
                return Ok(());
            }
            self.seen_key = true;
        }
        if self.frame.is_empty() && !vp8_frame_start(&depack) {
            return Ok(()); // meio de um frame que não começámos
        }
        self.frame.extend_from_slice(&payload);
        if self.count == 0 {
            self.open_seq = Some(pkt.header.sequence_number);
        }
        if !pkt.header.marker {
            return Ok(());
        }
        // Sala E2EE: o frame remontado é [header claro|ct|IV] — desencriptar
        // antes de escrever (frames que não autenticam são descartados).
        if let Some(key) = &self.key {
            let offset = if self.frame[0] & 0x01 == 0 { 10 } else { 3 };
            match decrypt_e2ee(key, &self.frame, offset) {
                Some(clear) => self.frame = clear,
                None => {
                    self.frame.clear();
                    // O keyframe de abertura não autenticou: a pista não abriu.
                    if self.count == 0 {
                        self.reopen();
                    }
                    return Ok(());
                }
            }
        }
        // Keyframe VP8 traz as dimensões (sync 9d 01 2a + 2×u14 LE).
        if self.dims.is_none()
            && self.frame.len() >= 10
            && self.frame[0] & 0x01 == 0
            && self.frame[3..6] == [0x9d, 0x01, 0x2a]
        {
            let w = u16::from_le_bytes([self.frame[6], self.frame[7]]) & 0x3fff;
            let h = u16::from_le_bytes([self.frame[8], self.frame[9]]) & 0x3fff;
            if w > 0 && h > 0 {
                self.dims = Some((w, h));
            }
        }
        let first = *self.first_ts.get_or_insert(pkt.header.timestamp);
        let pts_ms = (pkt.header.timestamp.wrapping_sub(first) as u64) / 90;
        self.w.write_all(&(self.frame.len() as u32).to_le_bytes())?;
        self.w.write_all(&pts_ms.to_le_bytes())?;
        self.w.write_all(&self.frame)?;
        self.frame.clear();
        if self.count == 0 {
            // Agora sim, a pista abriu.
            self.awaiting_key
                .store(false, std::sync::atomic::Ordering::Relaxed);
        }
        self.count += 1;
        Ok(())
    }

    /// O keyframe de abertura não serviu: deita-se fora o que dele houver e a
    /// pista volta a esperar — e o SFU volta a pedir (`RecWriter::wants_keyframe`).
    fn reopen(&mut self) {
        self.frame.clear();
        self.open_seq = None;
        self.seen_key = false;
        self.awaiting_key
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn close(&mut self) -> std::io::Result<()> {
        if let Some((w, h)) = self.dims {
            self.w.seek(SeekFrom::Start(12))?;
            self.w.write_all(&w.to_le_bytes())?;
            self.w.write_all(&h.to_le_bytes())?;
        }
        self.w.seek(SeekFrom::Start(24))?;
        self.w.write_all(&self.count.to_le_bytes())?;
        self.w.flush()
    }
}

/// A ordem e o relógio de uma pista Opus em gravação: recebe os pacotes pela
/// ordem em que chegam e entrega-os pela ordem em que foram ditos, com um
/// timestamp que o `OggWriter` aguenta.
///
/// O `OggWriter` (webrtc-media 0.17.2) avança a posição do grânulo com
/// `timestamp - anterior`, uma subtracção de `u32` sem `wrapping`. Um pacote
/// que lhe chegue com o timestamp para trás soma perto de 2^32 amostras à
/// pista em release (24 h 51 min a 48 kHz) e deita abaixo a thread de escrita
/// em debug. A bomba do SFU entrega pela ordem de chegada, por isso é aqui que
/// se garante: **o `OggWriter` só vê timestamps a crescer.**
///
/// - **Um atrasado escreve-se no sítio.** Os últimos `OPUS_REORDER_WINDOW`
///   pacotes ficam à espera, por ordem de timestamp, e só o mais antigo sai
///   quando chega mais um. Numa gravação o atraso não custa nada, e descartar
///   custava 20 ms de fala por pacote (R294, medido no ficheiro composto).
/// - **O que chega para lá da janela, ou repetido, não se escreve** — e
///   conta-se.
/// - **Um recuo que não passa é um relógio novo.** Uma origem que recomece o
///   timestamp para trás (a perna de um telefone depois de uma transferência)
///   põe todos os pacotes seguintes atrás do último escrito. Em vez de os
///   deitar fora, guardam-se: `OPUS_RESYNC_AFTER` seguidos que avançam entre si
///   são o relógio novo, e escrevem-se todos, a seguir ao último escrito. Um
///   pacote em dia pelo meio mostra que eram só atrasados, e saem. A rede não
///   reordena um segundo inteiro por ordem. Um recuo mais curto do que isso
///   não chega a ser relógio novo: perde-se até o relógio alcançar o ponto
///   onde ia, como uma rajada de atrasados.
/// - **As comparações são distâncias com sinal em 32 bits**, para a volta
///   legítima do relógio (os browsers começam o timestamp num valor ao acaso)
///   não ser um recuo. E o timestamp entregue conta a partir do primeiro
///   pacote escrito: o `OggWriter` só usa diferenças, e assim a subtracção
///   dele não vê a volta — que em release dá certo por acaso, e em debug é
///   pânico. Só voltaria a vê-la numa pista cujo relógio avançasse mais de
///   2^32 amostras do primeiro pacote ao último.
#[derive(Default)]
struct OpusTrack {
    /// `(primeiro timestamp escrito, último timestamp escrito)`.
    written: Option<(u32, u32)>,
    /// À frente do último escrito, por ordem de timestamp, à espera de vez.
    window: Vec<webrtc::rtp::packet::Packet>,
    /// Atrás do último escrito, seguidos e a avançar entre si: ou são um
    /// relógio novo, ou são atrasados.
    stale: Vec<webrtc::rtp::packet::Packet>,
}

/// Quantos pacotes esperam pela sua vez: 200 ms de Opus contínuo em pacotes de
/// 20 ms. Um receptor ao vivo já teria desistido de um pacote mais atrasado.
const OPUS_REORDER_WINDOW: usize = 10;

/// Quantos pacotes seguidos atrás do último escrito fazem um relógio novo. O
/// último escrito anda `OPUS_REORDER_WINDOW` pacotes atrás do mais novo que
/// chegou, por isso são 50 atrás do mais novo: 1 s de Opus contínuo em
/// pacotes de 20 ms (com DTX são os mesmos pacotes, e mais tempo).
const OPUS_RESYNC_AFTER: usize = 50 - OPUS_REORDER_WINDOW;
// O passo do relógio novo mede-se entre vizinhos da série.
const _: () = assert!(OPUS_RESYNC_AFTER >= 2);

/// O que aconteceu na pista ao receber um pacote (ou ao fechar).
#[derive(Debug, Default, PartialEq, Eq)]
struct OpusOutcome {
    /// Pacotes que não se escreveram: atrasados para lá da janela, ou repetidos.
    late: u32,
    /// O relógio da origem recuou de vez, e a pista continuou com o novo.
    resynced: bool,
    /// Amostras de salto do relógio do áudio que ficaram por encher, porque
    /// não passou tempo que as explique (`OpusGap::Refused`). Nunca é
    /// silencioso: vai a contador e a aviso, como a fila cheia (R18).
    unfilled: u32,
}

impl OpusTrack {
    /// `to` está à frente de `from`, mesmo com a volta do relógio pelo meio.
    fn ahead(to: u32, from: u32) -> bool {
        to.wrapping_sub(from) as i32 > 0
    }

    /// Recebe um pacote e põe em `out` os que já se podem escrever, por ordem
    /// e com o timestamp final.
    fn push(
        &mut self,
        pkt: webrtc::rtp::packet::Packet,
        out: &mut Vec<webrtc::rtp::packet::Packet>,
    ) -> OpusOutcome {
        let ts = pkt.header.timestamp;
        let mut did = OpusOutcome::default();
        if self.written.is_some_and(|(_, last)| !Self::ahead(ts, last)) {
            // Um que não avança sobre o anterior desfaz a série: recomeça nele.
            let breaks = |p: &webrtc::rtp::packet::Packet| !Self::ahead(ts, p.header.timestamp);
            if self.stale.last().is_some_and(breaks) {
                did.late += self.stale.len() as u32;
                self.stale.clear();
            }
            self.stale.push(pkt);
            if self.stale.len() >= OPUS_RESYNC_AFTER {
                did.late += self.resync(out);
                did.resynced = true;
            }
            return did;
        }
        // Um pacote em dia: os que esperavam para ser um relógio novo eram só
        // atrasados.
        did.late += self.stale.len() as u32;
        self.stale.clear();
        // Depois dos que não estão à frente dele: um repetido fica a seguir ao
        // original, e é o original que se escreve (o `emit` recusa o segundo).
        let at = self
            .window
            .partition_point(|p| !Self::ahead(p.header.timestamp, ts));
        self.window.insert(at, pkt);
        if self.window.len() > OPUS_REORDER_WINDOW {
            let oldest = self.window.remove(0);
            did.late += self.emit(oldest, out);
        }
        did
    }

    /// O fecho: o que esperava na janela escreve-se; o que esperava para ser
    /// um relógio novo não chegou a sê-lo, e descarta-se.
    fn flush(&mut self, out: &mut Vec<webrtc::rtp::packet::Packet>) -> OpusOutcome {
        let mut did = OpusOutcome::default();
        for pkt in std::mem::take(&mut self.window) {
            did.late += self.emit(pkt, out);
        }
        did.late += self.stale.len() as u32;
        self.stale.clear();
        did
    }

    /// Entrega um pacote com o timestamp contado do primeiro escrito. Devolve
    /// 1 se não o entregou, porque não avança sobre o último escrito: é um
    /// repetido, ou os primeiros pacotes da pista vieram a meio relógio uns
    /// dos outros e a janela não os soube ordenar. O `OggWriter` nunca vê um
    /// timestamp que não cresce.
    fn emit(
        &mut self,
        mut pkt: webrtc::rtp::packet::Packet,
        out: &mut Vec<webrtc::rtp::packet::Packet>,
    ) -> u32 {
        let ts = pkt.header.timestamp;
        let first = match &mut self.written {
            None => {
                self.written = Some((ts, ts));
                ts
            }
            Some((first, last)) => {
                if !Self::ahead(ts, *last) {
                    return 1;
                }
                *last = ts;
                *first
            }
        };
        pkt.header.timestamp = ts.wrapping_sub(first);
        out.push(pkt);
        0
    }

    /// O relógio novo entra na pista: primeiro sai o que o antigo ainda tinha
    /// à espera, e depois os pacotes guardados, com o primeiro um passo depois
    /// do último escrito. O passo é o MENOR entre vizinhos da série: um
    /// silêncio logo a seguir ao primeiro pacote (DTX) não pode empurrar o
    /// resto da pista. Devolve os que não se escreveram.
    fn resync(&mut self, out: &mut Vec<webrtc::rtp::packet::Packet>) -> u32 {
        let mut late = 0;
        for pkt in std::mem::take(&mut self.window) {
            late += self.emit(pkt, out);
        }
        let stale = std::mem::take(&mut self.stale);
        let t0 = stale[0].header.timestamp;
        let step = stale
            .windows(2)
            .map(|w| w[1].header.timestamp.wrapping_sub(w[0].header.timestamp))
            .min()
            .unwrap_or(960);
        if let Some((first, last)) = &mut self.written {
            let written = last.wrapping_sub(*first);
            *first = t0.wrapping_sub(written.wrapping_add(step));
            *last = t0.wrapping_sub(step);
        }
        for pkt in stale {
            late += self.emit(pkt, out);
        }
        late
    }
}

/// 20 ms de silêncio em Opus: o TOC `0xF8` (CELT, banda inteira, mono, um
/// quadro de 20 ms) e o quadro que qualquer descodificador lê como silêncio.
const OPUS_SILENCE: [u8; 3] = [0xf8, 0xff, 0xfe];

/// As amostras (a 48 kHz) de um `OPUS_SILENCE`.
const OPUS_SILENCE_SAMPLES: u32 = 960;

/// De quanto em quanto se escreve um `OPUS_SILENCE` dentro de um buraco: 5 s,
/// metade do salto a partir do qual o ffmpeg deixa de ver um buraco e passa a
/// ver uma descontinuidade (`-dts_delta_threshold`, 10 s), que tira. É também
/// o silêncio que o `aresample` da composição chega a guardar de uma vez.
const OPUS_GAP_MARK_EVERY: u32 = 5 * 48_000;

/// O salto a partir do qual o ffmpeg tira o buraco em vez de o deixar ao
/// filtro (`-dts_delta_threshold`, 10 s). Até aqui, o silêncio que o gravador
/// escreve não muda quanto silêncio a composição tem — muda só onde fica.
const OPUS_GAP_DISCONTINUITY: u32 = 2 * OPUS_GAP_MARK_EVERY;

/// Quanto pode o relógio de uma pista andar à frente do tempo que passou sem
/// um salto de mais de `OPUS_GAP_DISCONTINUITY` deixar de ser enchido. Cobre o
/// atraso de rede do primeiro pacote e a deriva do relógio de quem envia.
const OPUS_GAP_SLACK: std::time::Duration = std::time::Duration::from_secs(10);

/// As amostras (a 48 kHz) que um pacote Opus ocupa, lidas do TOC (RFC 6716
/// §3.1). `None` se o pacote não o diz: vazio, código 3 sem a contagem de
/// quadros, zero quadros, ou mais do que os 120 ms que um pacote pode levar.
fn opus_packet_samples(payload: &[u8]) -> Option<u32> {
    let toc = *payload.first()?;
    let frame = match toc >> 3 {
        config @ 0..=11 => [480, 960, 1920, 2880][usize::from(config & 3)],
        config @ 12..=15 => [480, 960][usize::from(config & 1)],
        config => 120 << (config & 3),
    };
    let frames = match toc & 3 {
        0 => 1,
        1 | 2 => 2,
        _ => u32::from(*payload.get(1)? & 0x3f),
    };
    Some(frame * frames).filter(|n| (1..=5760).contains(n))
}

/// O silêncio que falta numa pista Opus, decidido pacote a pacote.
///
/// Uma pista gravada só tem os pacotes que chegaram. Onde o participante se
/// calou (DTX: um pacote a cada 400 ms), ficou sem rede, ou foi calado na
/// origem (um telefone com `ForceMute` não publica nada), o timestamp salta e
/// a pista guarda o salto nos grânulos, sem amostra nenhuma no meio. O
/// `AUDIO_GAP_FILL` da composição enche esses saltos, com dois restos que só
/// se fecham aqui, ao gravar:
///
///  - o demuxer OGG do ffmpeg dá a cada pacote o grânulo da página ANTERIOR,
///    por isso o primeiro pacote depois de um salto cola-se ao último que
///    chegou: 20 ms do início da fala tocavam antes do silêncio. Um pacote de
///    silêncio escrito antes dele recebe esse grânulo em vez dele. Fica à
///    distância de um pacote DELE, para o seguinte lhe ficar encostado;
///  - um salto de mais de 10 s é uma descontinuidade para o ffmpeg, que o tira
///    antes de o filtro o ver. Com um pacote de silêncio a cada
///    `OPUS_GAP_MARK_EVERY`, nenhum salto lá chega.
///
/// Só há buraco onde cabe um `OPUS_SILENCE` inteiro, medido com a duração que
/// cada pacote diz ter: uma pista em pacotes de 60 ms salta 2880 amostras de
/// cada vez e não tem buraco nenhum.
///
/// **O tecto é o tempo que passou.** Um timestamp pode saltar sem ter havido
/// silêncio: corrompido, de uma origem que recomeçou o relógio lá à frente, ou
/// de quem queira esticar a gravação dos outros. Um silêncio de N segundos
/// demora N segundos a chegar — por isso um salto de mais de
/// `OPUS_GAP_DISCONTINUITY` só se enche se a pista, com ele, não andar mais do
/// que o tempo decorrido desde o seu primeiro pacote, mais `OPUS_GAP_SLACK`.
/// O salto que passa disso fica como sempre ficou (o ffmpeg tira-o), é
/// contado, e não conta como caminho andado: os silêncios de mais de 10 s que
/// o gravador escreve numa pista nunca somam mais do que o tempo que ela
/// durou. Os saltos até 10 s enchem-se sempre: a composição já os enchia.
#[derive(Default)]
struct OpusGapFill(Option<OpusGapTrack>);

struct OpusGapTrack {
    /// O último pacote escrito, fala ou silêncio: timestamp e amostras.
    last_ts: u32,
    last_samples: u32,
    /// De onde se conta o tecto: quando chegou o primeiro pacote, e quantas
    /// amostras a pista andou desde ele (sem os saltos recusados).
    anchored: Instant,
    walked: u64,
}

/// O que o `OpusGapFill` decide antes de um pacote ser escrito.
#[derive(Debug, PartialEq, Eq)]
enum OpusGap {
    /// Não há buraco onde caiba silêncio.
    None,
    /// Há: silêncio de `OPUS_GAP_MARK_EVERY` em `OPUS_GAP_MARK_EVERY` a contar
    /// de `after` (o último pacote escrito), e um último em `last`.
    Fill { after: u32, last: u32 },
    /// Há um salto destas amostras e não passou tempo que o explique.
    Refused(u32),
}

impl OpusGapFill {
    /// `ts` é o timestamp com que o pacote vai ser escrito (o que o
    /// `OpusTrack` deu por pronto, já ordenado), `samples` o que ele dura e
    /// `arrived` o instante em que chegou ao gravador.
    fn before(&mut self, ts: u32, samples: Option<u32>, arrived: Instant) -> OpusGap {
        // Um pacote que não diz quanto dura não se rodeia de silêncio; para o
        // que vier a seguir conta como um quadro de 20 ms.
        let dur = samples.unwrap_or(OPUS_SILENCE_SAMPLES);
        let Some(track) = &mut self.0 else {
            self.0 = Some(OpusGapTrack {
                last_ts: ts,
                last_samples: dur,
                anchored: arrived,
                walked: 0,
            });
            return OpusGap::None;
        };
        let (after, step) = (track.last_ts, ts.wrapping_sub(track.last_ts));
        // O buraco vai do fim do último pacote ao início deste; o último
        // silêncio fica um pacote DESTE antes dele, e tem de caber depois do
        // último escrito.
        let hole = i64::from(step) - i64::from(track.last_samples);
        let room = i64::from(step) - i64::from(dur);
        (track.last_ts, track.last_samples) = (ts, dur);
        let walked = track.walked + u64::from(step);
        let fits = i64::from(OPUS_SILENCE_SAMPLES);
        if samples.is_none() || hole < fits || room < fits {
            track.walked = walked;
            return OpusGap::None;
        }
        let passed = arrived.saturating_duration_since(track.anchored) + OPUS_GAP_SLACK;
        if step > OPUS_GAP_DISCONTINUITY && u128::from(walked) > passed.as_millis() * 48 {
            return OpusGap::Refused(hole as u32);
        }
        track.walked = walked;
        OpusGap::Fill {
            after,
            last: ts.wrapping_sub(dur),
        }
    }
}

impl OpusGap {
    /// Os timestamps onde se escreve silêncio, por ordem.
    fn silences(&self) -> impl Iterator<Item = u32> {
        let (after, last) = match *self {
            OpusGap::Fill { after, last } => (after, Some(last)),
            _ => (0, None),
        };
        // Os de 5 em 5 s que caibam antes do último, contados à partida: o
        // número de voltas não depende de nenhuma soma chegar ao fim.
        let room = last.map_or(0, |last| last.wrapping_sub(after));
        let marks = room.saturating_sub(OPUS_SILENCE_SAMPLES) / OPUS_GAP_MARK_EVERY;
        (1..=marks)
            .map(move |k| after.wrapping_add(k * OPUS_GAP_MARK_EVERY))
            .chain(last)
    }
}

/// O que viaja na fila do writer: o pacote e o instante em que chegou.
///
/// O instante vem de quem ENTREGA e não de quem escreve: a thread de escrita
/// pode estar atrasada, e o `OpusGapFill` usa-o como tecto («o tecto é o tempo
/// que passou») — medi-lo no fim da fila daria por explicado um salto que
/// ninguém esperou.
struct Queued {
    pkt: webrtc::rtp::packet::Packet,
    arrived: Instant,
}

/// O que escreve mesmo no disco. Vive numa thread dedicada — ver `RecWriter`.
enum RecSink {
    Video(Vp8IvfWriter),
    Audio {
        w: OggWriter<std::io::BufWriter<std::fs::File>>,
        key: Option<Arc<Aes256Gcm>>,
        track: OpusTrack,
        /// O silêncio que falta na pista, decidido pacote a pacote.
        gap: OpusGapFill,
        /// Os pacotes que a `track` deu por prontos, entre a chegada de um e
        /// a escrita (um `Vec` que se reaproveita).
        ready: Vec<webrtc::rtp::packet::Packet>,
    },
}

impl RecSink {
    /// A bandeira «à espera do keyframe de abertura» desta pista. Numa pista
    /// de áudio nasce apagada e ninguém lhe toca.
    fn awaiting_key(&self) -> Arc<std::sync::atomic::AtomicBool> {
        match self {
            RecSink::Video(w) => w.awaiting_key.clone(),
            RecSink::Audio { .. } => Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn audio(
        file: std::fs::File,
        key: Option<Arc<Aes256Gcm>>,
    ) -> Result<Self, webrtc::media::Error> {
        Ok(RecSink::Audio {
            w: OggWriter::new(std::io::BufWriter::with_capacity(64 * 1024, file), 48000, 2)?,
            key,
            track: OpusTrack::default(),
            gap: OpusGapFill::default(),
            ready: Vec::new(),
        })
    }

    fn write_rtp(&mut self, pkt: webrtc::rtp::packet::Packet, arrived: Instant) -> OpusOutcome {
        match self {
            RecSink::Video(w) => {
                let _ = w.write_rtp(&pkt);
                OpusOutcome::default()
            }
            RecSink::Audio {
                w,
                key,
                track,
                gap,
                ready,
            } => {
                // Um payload vazio não chega a ser escrito pelo `OggWriter`:
                // não pode ocupar lugar numa pista em que não entra.
                if pkt.payload.is_empty() {
                    return OpusOutcome::default();
                }
                let mut did = track.push(pkt, ready);
                did.unfilled = Self::write_opus(w, key.as_deref(), gap, ready, arrived);
                did
            }
        }
    }

    /// Escreve os pacotes que a pista deu por prontos. Opus: 1 frame por
    /// pacote — desencripta o payload (offset 1); o que não autentica não se
    /// escreve, e o seguinte cobre o tempo dele como uma perda.
    fn write_opus(
        w: &mut OggWriter<std::io::BufWriter<std::fs::File>>,
        key: Option<&Aes256Gcm>,
        gap: &mut OpusGapFill,
        ready: &mut Vec<webrtc::rtp::packet::Packet>,
        arrived: Instant,
    ) -> u32 {
        let mut unfilled: u32 = 0;
        for mut pkt in ready.drain(..) {
            if let Some(key) = key {
                let Some(clear) = decrypt_e2ee(key, &pkt.payload, 1) else {
                    continue;
                };
                pkt.payload = clear.into();
            }
            // O silêncio que falta ANTES deste pacote, sobre o timestamp com
            // que ele vai ser escrito e a duração que o payload já em claro
            // diz ter. Escreve-se em claro também numa sala cifrada: não é
            // media de ninguém, e o resto da pista já está decifrado ao lado.
            let plan = gap.before(
                pkt.header.timestamp,
                opus_packet_samples(&pkt.payload),
                arrived,
            );
            for at in plan.silences() {
                let mut silence = webrtc::rtp::packet::Packet::default();
                silence.header.timestamp = at;
                silence.payload = bytes::Bytes::from_static(&OPUS_SILENCE);
                let _ = w.write_rtp(&silence);
            }
            if let OpusGap::Refused(samples) = plan {
                unfilled = unfilled.saturating_add(samples);
            }
            let _ = w.write_rtp(&pkt);
        }
        unfilled
    }

    fn close(&mut self) -> OpusOutcome {
        match self {
            RecSink::Video(w) => {
                let _ = w.close();
                OpusOutcome::default()
            }
            RecSink::Audio {
                w,
                key,
                track,
                gap,
                ready,
            } => {
                // O que esperava a vez na janela ainda é da gravação.
                let mut did = track.flush(ready);
                // `Instant::now()` e não o instante de cada um: no fecho já
                // não se sabe quando chegaram. Só pode SOBREstimar o tempo
                // que passou, e no máximo pela latência da própria fila
                // (`OPUS_REORDER_WINDOW`, 200 ms de Opus) — duas ordens de
                // grandeza abaixo do `OPUS_GAP_SLACK`, que é de 10 s.
                did.unfilled = Self::write_opus(w, key.as_deref(), gap, ready, Instant::now());
                let _ = w.close();
                did
            }
        }
    }
}

/// O instante, no relógio da sessão, do primeiro pacote que a pista TEM.
///
/// O zero de uma pista é o primeiro quadro (ou pacote) que lá ficou escrito: o
/// `Vp8IvfWriter` dá PTS 0 ao primeiro quadro, o `OpusTrack` conta a partir do
/// primeiro pacote. Esse instante NÃO é o de quando o writer foi ligado — uma
/// pista de vídeo ligada a meio do fluxo espera pelo keyframe que o SFU pede
/// (até 1 s, se o pedido cair no intervalo mínimo entre PLI, e mais se se
/// perder), e uma de áudio em silêncio espera pelo pacote seguinte do DTX (até
/// 400 ms). Posta na linha do tempo pelo instante da ligação, a pista entrava
/// adiantada o tempo que esperou — a imagem à frente do som, ou o contrário.
///
/// Marca-o quem ENTREGA (`RecWriter::write_rtp`), à chegada do pacote, e lê-o
/// a composição (`RecTrackMeta::starts_at_ms`).
#[derive(Debug, Clone)]
struct FirstPacket {
    session_started: Instant,
    /// Milissegundos desde o início da sessão; `NOT_YET` enquanto não chegou.
    at_ms: Arc<std::sync::atomic::AtomicU64>,
}

impl FirstPacket {
    const NOT_YET: u64 = u64::MAX;

    fn new(session_started: Instant) -> Self {
        Self {
            session_started,
            at_ms: Arc::new(std::sync::atomic::AtomicU64::new(Self::NOT_YET)),
        }
    }

    /// A pista começa AGORA. Uma pista de vídeo cujo keyframe de abertura não
    /// serviu volta a marcar no seguinte: vale a última marca.
    fn mark(&self) {
        let ms = self.session_started.elapsed().as_millis() as u64;
        self.at_ms.store(ms, std::sync::atomic::Ordering::Relaxed);
    }

    fn get(&self) -> Option<u64> {
        match self.at_ms.load(std::sync::atomic::Ordering::Relaxed) {
            Self::NOT_YET => None,
            ms => Some(ms),
        }
    }
}

/// Durante quanto tempo se pede keyframe por uma pista de vídeo que ainda não
/// abriu (ver `RecWriter::wants_keyframe`). Ao ritmo do `pli_allowed` do SFU
/// são no máximo trinta pedidos por pista.
const KEYFRAME_ASK_FOR: std::time::Duration = std::time::Duration::from_secs(30);

/// Writer de uma track em gravação — um **handle** para uma thread de escrita.
///
/// Porquê uma thread e não escrita directa: o `write_rtp` era chamado de dentro
/// da task async que reencaminha RTP, e escrevia com `std::fs::File`, que é
/// SÍNCRONO. Com o volume de gravações lento ou cheio, uma escrita bloqueava um
/// worker do Tokio — e um worker bloqueado não serve só a gravação, serve todas
/// as salas que calharem naquela thread. O `BufWriter` (que já lá estava) reduziu
/// a frequência das syscalls; não tirou a escrita do executor.
///
/// A fila é LIMITADA (`REC_QUEUE_CAP`), pela mesma razão que todas as outras o
/// são: um disco que não acompanha não pode virar consumo de memória sem fim.
/// Cheia, PERDEM-SE pacotes — e isso é registado e contado, nunca silencioso:
/// uma gravação corrompida em silêncio é a R18, e é o pior resultado possível.
pub struct RecWriter {
    tx: Option<std::sync::mpsc::SyncSender<Box<Queued>>>,
    join: Option<std::thread::JoinHandle<()>>,
    dropped: Arc<std::sync::atomic::AtomicU64>,
    /// Aceso enquanto a pista de VÍDEO não tiver o keyframe que a abre (ver
    /// `wants_keyframe`). Partilhado com a thread de escrita, que o volta a
    /// acender se o keyframe não serviu. Numa pista de áudio nasce apagado.
    awaiting_key: Arc<std::sync::atomic::AtomicBool>,
    /// Quando o writer foi ligado — para dizer quanto a pista esperou pelo keyframe.
    opened: Instant,
    /// Pista de vídeo: começa no keyframe que a abre. As de áudio começam no
    /// primeiro pacote.
    video: bool,
    /// Quando chegou o primeiro pacote que a pista tem (ver `FirstPacket`).
    first: FirstPacket,
    /// Até quando se pede keyframe por esta pista (`KEYFRAME_ASK_FOR`).
    ask_until: Instant,
    /// Já se avisou de que se desistiu de pedir.
    gave_up: std::sync::atomic::AtomicBool,
    metrics: Arc<crate::metrics::Metrics>,
    label: String,
}

impl RecWriter {
    fn spawn(
        sink: RecSink,
        cap: usize,
        metrics: Arc<crate::metrics::Metrics>,
        label: String,
        first: FirstPacket,
    ) -> Self {
        let awaiting_key = sink.awaiting_key();
        let video = matches!(sink, RecSink::Video(_));
        let (tx, rx) = std::sync::mpsc::sync_channel::<Box<Queued>>(cap);
        let (thread_metrics, thread_label) = (metrics.clone(), label.clone());
        let join = std::thread::Builder::new()
            .name(format!("dlx-rec-{label}"))
            .spawn(move || {
                let mut sink = sink;
                let mut late: u64 = 0;
                // Contado e avisado como a fila cheia: um pacote que não entra
                // na gravação nunca é silencioso (R18).
                let mut account = |did: OpusOutcome| {
                    if did.late > 0 {
                        let n = u64::from(did.late);
                        thread_metrics
                            .recording_audio_late_dropped_total
                            .fetch_add(n, std::sync::atomic::Ordering::Relaxed);
                        // Um aviso ao primeiro e a cada 500.
                        if late == 0 || (late + n) / 500 > late / 500 {
                            tracing::warn!(
                                track = %thread_label,
                                descartados = late + n,
                                "gravação: áudio atrasado para lá da janela, ou repetido — não se escreve"
                            );
                        }
                        late += n;
                    }
                    if did.resynced {
                        tracing::warn!(
                            track = %thread_label,
                            "gravação: o relógio do áudio recuou de vez — a pista continua com o relógio novo"
                        );
                    }
                    if did.unfilled > 0 {
                        thread_metrics
                            .recording_audio_gap_unfilled_total
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        // O salto vai no aviso em ms, que é o que se lê num
                        // incidente — as amostras obrigam a dividir por 48.
                        tracing::warn!(
                            track = %thread_label,
                            salto_ms = did.unfilled / 48,
                            "gravação: salto do relógio do áudio sem tempo que o explique — fica por encher"
                        );
                    }
                };
                // O laço termina quando TODOS os emissores caem (o `close`
                // larga o `tx`), e só então se fecha o ficheiro. É isto que
                // garante que o que estava em fila chega ao disco antes de o
                // ffmpeg abrir o ficheiro.
                while let Ok(q) = rx.recv() {
                    let Queued { pkt, arrived } = *q;
                    account(sink.write_rtp(pkt, arrived));
                }
                account(sink.close());
            })
            .expect("thread de gravação");
        Self {
            tx: Some(tx),
            join: Some(join),
            dropped: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            awaiting_key,
            opened: Instant::now(),
            video,
            first,
            ask_until: Instant::now() + KEYFRAME_ASK_FOR,
            gave_up: std::sync::atomic::AtomicBool::new(false),
            metrics,
            label,
        }
    }

    /// Entrega um pacote à thread de escrita. NUNCA bloqueia o executor.
    pub fn write_rtp(&self, pkt: &webrtc::rtp::packet::Packet) {
        let Some(tx) = &self.tx else { return };
        use std::sync::atomic::Ordering::Relaxed;
        // Decidido AQUI, com o mesmo predicado que o `Vp8IvfWriter` aplica na
        // thread de escrita, e só dado por certo se o pacote entrar na fila:
        // assim quem entrega sabe na hora se a pista já abriu, sem esperar
        // que a thread lá chegue — e não pede um keyframe que já tem em mão.
        let opens = self.awaiting_key.load(Relaxed) && rtp_starts_vp8_keyframe(pkt);
        // O áudio começa no primeiro pacote que o `OggWriter` escreve (um
        // payload vazio não chega lá — ver `RecSink::write_rtp`).
        let starts = if self.video {
            opens
        } else {
            self.first.get().is_none() && !pkt.payload.is_empty()
        };
        if tx
            .try_send(Box::new(Queued {
                pkt: pkt.clone(),
                arrived: Instant::now(),
            }))
            .is_ok()
        {
            if starts {
                // É ESTE o instante em que a pista entra na linha do tempo da
                // gravação, não o da ligação do writer (ver `FirstPacket`).
                self.first.mark();
            }
            if opens {
                self.awaiting_key.store(false, Relaxed);
                tracing::debug!(
                    track = %self.label,
                    espera_ms = self.opened.elapsed().as_millis() as u64,
                    "gravação: a pista de vídeo abriu no seu keyframe"
                );
            } else if starts {
                tracing::debug!(
                    track = %self.label,
                    espera_ms = self.opened.elapsed().as_millis() as u64,
                    "gravação: a pista de áudio começa no seu primeiro pacote"
                );
            }
        } else {
            // Fila cheia (o disco não acompanha) ou thread morta. Perde-se o
            // pacote — a alternativa era bloquear o executor, que é pior. Conta-se
            // SEMPRE: é isto que transforma «a gravação saiu estranha» num número.
            let n = self
                .dropped
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            crate::metrics::Metrics::bump(&self.metrics.recording_packets_dropped_total);
            // Um aviso por cada 500 perdidos: o primeiro diz que começou, e os
            // seguintes dão a escala sem encher o log a milhares de linhas.
            if n.is_multiple_of(500) {
                tracing::warn!(
                    track = %self.label,
                    perdidos = n + 1,
                    "gravação: fila de escrita cheia — o disco não acompanha"
                );
            }
        }
    }

    /// A pista de vídeo ainda espera pelo keyframe que a abre?
    ///
    /// O gravador é um consumidor de vídeo como outro qualquer: só começa num
    /// keyframe, e o codificador do browser só manda um quando lho pedem. Um
    /// subscritor pede-o sozinho (PLI) enquanto não descodifica; o gravador não
    /// tem essa via de volta, por isso é o SFU que pergunta aqui e pede por ele
    /// — ao ligar o writer e, na bomba de RTP, enquanto a resposta for `true`
    /// (o pedido pode ter sido travado pelo intervalo mínimo entre PLI, ou
    /// ter-se perdido na rede, que é UDP).
    ///
    /// Pede-se durante `KEYFRAME_ASK_FOR` e não mais: um publicador que não
    /// responde em meio minuto não vai responder ao pedido seguinte, e um PLI
    /// por segundo para sempre é o ticker que a R14 tirou. A pista continua a
    /// poder abrir — com um keyframe que outro consumidor peça.
    pub fn wants_keyframe(&self) -> bool {
        use std::sync::atomic::Ordering::Relaxed;
        if !self.awaiting_key.load(Relaxed) {
            return false;
        }
        if Instant::now() < self.ask_until {
            return true;
        }
        if !self.gave_up.swap(true, Relaxed) {
            tracing::warn!(
                track = %self.label,
                segundos = KEYFRAME_ASK_FOR.as_secs(),
                "gravação: a pista de vídeo continua sem keyframe — deixa-se de o pedir"
            );
        }
        false
    }

    /// Pacotes perdidos por fila cheia nesta track. `> 0` = gravação degradada.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Fecha o writer e **espera** que a thread esvazie a fila e feche o ficheiro.
    ///
    /// É `async` de propósito. O `join()` bloqueia, e bloquear o executor é
    /// exactamente o que esta mudança existe para evitar — por isso o `join`
    /// corre em `spawn_blocking`. E é preciso ESPERAR: o `finalize` invoca o
    /// ffmpeg logo a seguir, e um ficheiro ainda por esvaziar dá uma gravação
    /// truncada sem um único erro pelo caminho.
    pub async fn close(mut self) -> u64 {
        let perdidos = self.dropped();
        drop(self.tx.take()); // fecha o canal → o laço da thread termina
        if let Some(h) = self.join.take() {
            let _ = tokio::task::spawn_blocking(move || h.join()).await;
        }
        // Uma pista de vídeo que nunca abriu não tem um quadro: a composição
        // deixa-a de fora, e esse participante fica sem imagem na gravação.
        // Nunca em silêncio (R18).
        if self.awaiting_key.load(std::sync::atomic::Ordering::Relaxed) {
            tracing::warn!(
                track = %self.label,
                esperou_ms = self.opened.elapsed().as_millis() as u64,
                "gravação: a pista de vídeo fechou sem ter recebido um keyframe — fica SEM imagem na gravação"
            );
        }
        perdidos
    }
}

impl Drop for RecWriter {
    fn drop(&mut self) {
        // Rede de segurança: se alguém largar o writer sem `close().await` (um
        // caminho de erro, um `?` pelo meio), o canal fecha e a thread ainda
        // esvazia o que tem e fecha o ficheiro. Não se faz `join` aqui — o
        // `Drop` pode correr no executor, e bloqueá-lo é o problema original.
        drop(self.tx.take());
    }
}

// O vocabulário dos `kind` vive no DOMÍNIO, ao lado do `ComposeTrack::kind` que
// os transporta (`delonix-meet-domain::content::composition`) — não aqui, que é
// o módulo de IO. O `sfu` e o `recordings` importam-nos do mesmo sítio.
pub(crate) use delonix_meet_domain::content::composition::{
    KIND_AUDIO, KIND_SCREEN, KIND_SCREEN_AUDIO, KIND_VIDEO,
};

/// O prefixo da pasta das pistas ISO. Deliberadamente DIFERENTE de `tmp-`: o
/// `sweep_orphan_segments` apaga qualquer `tmp-*` que nenhuma composição
/// reclame, e deixar as pistas lá era perdê-las no momento em que passam a ser
/// úteis.
const ISO_PREFIX: &str = "iso-";

/// O tamanho acima do qual uma pista tem conteúdo a sério. NÃO é zero: o
/// `open_track` escreve o cabeçalho ao criar o ficheiro (32 bytes de DKIF no
/// IVF, as páginas de identificação no OGG), logo uma pista que nunca recebeu
/// um pacote RTP tem dezenas de bytes. Um limiar de zero deixava passar
/// exactamente a pista vazia que ele diz querer travar.
const HEADER_ONLY: u64 = 1024;

/// O que fazer com os segmentos depois de perguntar à base se o estúdio pediu
/// gravação ISO.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum IsoDecision {
    /// Registar as pistas e guardar os ficheiros.
    Register,
    /// Apagar como sempre: não é um estúdio, ou o interruptor está desligado.
    /// Guardar ficheiros que ninguém pediu é uma fuga de disco silenciosa.
    Delete,
    /// Não apagar AGORA. Os segmentos ficam no `tmp-<sessão>` e é o varredor de
    /// órfãos que os recolhe depois da folga se ninguém os reclamar.
    Keep,
}

/// A decisão, separada da consulta para poder ser PROVADA nos três estados —
/// um deles é um erro da base, que não se sabe provocar contra Postgres real.
///
/// `None` significa «a base não respondeu». Antes, a leitura era
/// `.ok().flatten()`, o que tornava um erro indistinguível de «interruptor
/// desligado» e fazia o chamador apagar TODAS as pistas por fonte de um
/// estúdio que as pediu, sem um aviso — a mesma classe de perda silenciosa que
/// esta frente existe para fechar. Um atraso de horas a libertar disco é barato
/// ao lado disso.
pub(crate) fn decide_iso(leitura: Option<Option<bool>>) -> IsoDecision {
    match leitura {
        Some(Some(true)) => IsoDecision::Register,
        Some(_) => IsoDecision::Delete,
        None => IsoDecision::Keep,
    }
}

/// Onde vivem as pistas ISO de uma gravação. É UMA função porque este caminho é
/// construído em três sítios — quem renomeia, quem serve e quem apaga por
/// retenção — e um deles a divergir é, dependendo de qual, uma rota que serve o
/// vazio ou uma pasta que nunca ninguém apaga.
pub fn iso_dir(recordings_dir: &Path, rec_id: Uuid) -> PathBuf {
    recordings_dir.join(format!("{ISO_PREFIX}{rec_id}"))
}

/// Metadados de uma track gravada (o writer vive na Publication do SFU).
#[derive(Debug, Clone)]
pub struct RecTrackMeta {
    pub path: PathBuf,
    /// Um de [`TRACK_KINDS`].
    pub kind: String,
    /// Quem publicou esta pista: o `publisher` do SFU. Já estava em âmbito no
    /// sítio da chamada (`sfu.rs`) e era desperdiçado — sem ele, os ficheiros
    /// da gravação ISO saíam anónimos (`00-video.ivf`) e ninguém sabia que
    /// câmara era qual.
    pub publisher: Uuid,
    /// Quando o writer foi LIGADO, em ms desde o início da sessão. Não é onde
    /// a pista começa — isso é `starts_at_ms`.
    pub offset_ms: u64,
    first: FirstPacket,
}

impl RecTrackMeta {
    /// Onde a pista começa na linha do tempo da gravação, em ms desde o início
    /// da sessão: o instante do primeiro pacote que ela tem. É este o valor que
    /// a composição usa. Uma pista que nunca recebeu nada fica no instante da
    /// ligação (e a composição deixa-a de fora, por estar vazia).
    pub fn starts_at_ms(&self) -> u64 {
        self.first.get().unwrap_or(self.offset_ms)
    }

    /// A pista como vai no manifesto: o `starts_at_ms` já resolvido. É a ÚNICA
    /// passagem de memória para manifesto — o `starts_at_ms()` acima, com o seu
    /// fallback, fica coberto por quem testa a composição.
    pub fn to_compose(&self) -> ComposeTrack {
        ComposeTrack {
            path: self.path.clone(),
            kind: self.kind.clone(),
            starts_at_ms: self.starts_at_ms(),
            publisher: self.publisher,
        }
    }
}

/// Sessão de gravação de uma sala.
pub struct RecordingSession {
    pub id: Uuid,
    pub dir: PathBuf,
    pub started: Instant,
    pub by_user: Uuid,
    pub by_name: String,
    pub tracks: Vec<RecTrackMeta>,
    /// Chave E2EE da sala, cedida pelo anfitrião só para esta gravação
    /// (vive apenas em memória; morre com a sessão).
    pub e2ee_key: Option<Arc<Aes256Gcm>>,
}

impl RecordingSession {
    /// O manifesto desta sessão: o que a composição precisa de saber sem a
    /// sessão em memória. Guardado na linha `recordings` por
    /// `insert_processing`, e é o que faz a composição ser retomável.
    ///
    /// `starts_at_ms()` resolve-se AQUI: o instante do primeiro pacote de cada
    /// pista é um dado de memória, e sem ele guardado o `-itsoffset` de uma
    /// segunda tentativa não daria o mesmo resultado.
    pub fn manifest(&self) -> ComposeManifest {
        ComposeManifest {
            dir: self.dir.clone(),
            tracks: self.tracks.iter().map(RecTrackMeta::to_compose).collect(),
            expected_ms: self.started.elapsed().as_millis() as i64,
            duration_secs: i32::try_from(self.started.elapsed().as_secs()).ok(),
            by_user: self.by_user,
        }
    }

    /// `recordings_dir` vem de `state.config.recordings_dir` (lido uma vez no arranque).
    pub async fn new(
        by_user: Uuid,
        by_name: String,
        e2ee_key: Option<Vec<u8>>,
        recordings_dir: &Path,
    ) -> std::io::Result<Self> {
        let id = Uuid::new_v4();
        let dir = recordings_dir.join(format!("tmp-{id}"));
        tokio::fs::create_dir_all(&dir).await?;
        // Os bytes crus da chave passam a `Aes256Gcm` (cuja tabela interna é
        // limpa no Drop, via a feature `zeroize` do `cipher`) e o `Vec` de
        // origem é sobrescrito à mão — sem isto ficaria a chave AES-256 em
        // claro numa alocação libertada, à espera de quem leia a heap.
        let e2ee_key = {
            use zeroize::Zeroize;
            let mut raw = e2ee_key;
            let k = raw
                .as_deref()
                .and_then(|r| Aes256Gcm::new_from_slice(r).ok())
                .map(Arc::new);
            if let Some(v) = raw.as_mut() {
                v.zeroize();
            }
            k
        };
        Ok(Self {
            id,
            dir,
            started: Instant::now(),
            by_user,
            by_name,
            tracks: Vec::new(),
            e2ee_key,
        })
    }

    /// Cria o writer para uma track nova e regista os metadados.
    ///
    /// `kind`: um de [`TRACK_KINDS`] (no domínio) — e a lista é lá, não aqui. Foi um
    /// comentário desta linha, a enumerar três dos quatro valores, que fez o
    /// `CHECK` da migração 0124 nascer sem o `screen-audio`.
    pub fn open_track(
        &mut self,
        kind: &str,
        publisher: Uuid,
        cap: usize,
        metrics: Arc<crate::metrics::Metrics>,
    ) -> Option<RecWriter> {
        let n = self.tracks.len();
        let offset_ms = self.started.elapsed().as_millis() as u64;
        let is_audio = kind.ends_with("audio");
        let ext = if is_audio { "ogg" } else { "ivf" };
        let path = self.dir.join(format!("{n:02}-{kind}.{ext}"));
        let file = match std::fs::File::create(&path) {
            Ok(f) => f,
            Err(e) => {
                tracing::error!(path = %path.display(), error = %e, "falha a criar ficheiro de gravação");
                return None;
            }
        };
        let sink = if is_audio {
            match RecSink::audio(file, self.e2ee_key.clone()) {
                Ok(sink) => sink,
                Err(e) => {
                    tracing::error!(path = %path.display(), error = %e, "falha a criar OggWriter");
                    return None;
                }
            }
        } else {
            match Vp8IvfWriter::new(file) {
                Ok(mut w) => {
                    w.key = self.e2ee_key.clone();
                    RecSink::Video(w)
                }
                Err(e) => {
                    tracing::error!(path = %path.display(), error = %e, "falha a criar IvfWriter");
                    return None;
                }
            }
        };
        let first = FirstPacket::new(self.started);
        let writer = RecWriter::spawn(sink, cap, metrics, format!("{n:02}-{kind}"), first.clone());
        self.tracks.push(RecTrackMeta {
            path,
            kind: kind.to_string(),
            publisher,
            offset_ms,
            first,
        });
        Some(writer)
    }
}

/// Registra as pistas por fonte de uma gravação ISO. Devolve `true` se os
/// ficheiros devem FICAR no disco (e portanto não ser apagados).
///
/// Só numa sala que é de um estúdio com `iso_recording`. Fora disso devolve
/// `false` e o chamador apaga como sempre — guardar ficheiros que ninguém
/// pediu é uma fuga de disco silenciosa, e é por isso que o teste mede os
/// dois estados do interruptor.
///
/// **Os ficheiros MUDAM de sítio, e é obrigatório.** Os segmentos vivem em
/// `recordings_dir/tmp-<id da sessão>`, e o `sweep_orphan_segments` apaga
/// QUALQUER pasta `tmp-*` que não esteja referenciada por uma gravação em
/// `processing` e já tenha passado a folga. Deixá-los lá era perder as pistas
/// em silêncio exactamente quando deixam de estar a processar e passam a ser
/// úteis. Então renomeia-se a pasta para `iso-<id da gravação>`, que o
/// varredor não olha e que torna o caminho da rota determinístico sem
/// precisar de uma coluna para ele.
///
/// **O que esta fatia NÃO resolve, e fica dito:** o número e a etiqueta da
/// câmara resolvem-se do registo EM MEMÓRIA (`source_of_peer`), que o
/// `disconnect` limpa. Uma fonte que se desligue ANTES de o operador parar a
/// gravação deixa a pista sem nome — o `publisher_id` fica sempre, logo é
/// recuperável depois, mas o nome não se inventa. Na prática o operador para
/// com as câmaras ligadas; quando isso não for verdade, a saída é persistir o
/// par ↔ fonte em vez de o ter só em memória.
///
/// **A invariante, e é a razão desta função existir assim:** quando devolve
/// `true`, a pasta `iso-<gravação>` contém EXACTAMENTE os ficheiros que têm
/// linha em `recording_tracks`. Um ficheiro guardado sem linha não é
/// alcançável por rota nenhuma — é uma fuga de disco que ninguém vê, e foi o
/// que aconteceu à pista `screen-audio` que o `CHECK` da 0124 recusava. Por
/// isso o registo acaba com uma reconciliação da pasta contra a base, e não
/// com a contagem dos `INSERT` que correram bem.
async fn registar_pistas_iso(
    db: &sqlx::PgPool,
    recordings_dir: &Path,
    studio: &crate::studio_realtime::StudioHub,
    room_id: Uuid,
    manifest: &ComposeManifest,
    rec_id: Option<Uuid>,
    metrics: &crate::metrics::Metrics,
) -> bool {
    let Some(rec_id) = rec_id else {
        // Sem linha na biblioteca não há a que pendurar as pistas.
        return false;
    };
    let iso =
        sqlx::query_scalar::<_, bool>("SELECT s.iso_recording FROM studios s WHERE s.room_id = $1")
            .bind(room_id)
            .fetch_optional(db)
            .await;
    if let Err(e) = &iso {
        tracing::error!(%rec_id, %room_id, error = %e,
            "gravação ISO: a base não disse se o estúdio a pediu — os segmentos ficam para o varredor");
    }
    match decide_iso(iso.ok()) {
        IsoDecision::Register => {}
        IsoDecision::Delete => return false,
        IsoDecision::Keep => return true,
    }

    let destino = iso_dir(recordings_dir, rec_id);
    // A PASTA ENTRA NO REGISTO ANTES DE EXISTIR, e é isto que fecha a fuga que
    // esta função deixava aberta: o `rename` abaixo tira os ficheiros do
    // `tmp-<sessão>` (que o `sweep_orphan_segments` recolhe) e um processo
    // morto entre ele e os `INSERT` das pistas deixava a pasta sem NENHUMA
    // linha que a nomeasse — nem em `recording_tracks`, nem em sítio nenhum.
    // Com a linha escrita primeiro, essa janela não encolhe: deixa de existir.
    // E não se infere nada do nome nem da idade da pasta, que foi a abordagem
    // medida e revertida (ver o comentário do `sweep_orphan_segments`).
    //
    // Se o registo falhar, NÃO se renomeia: os ficheiros ficam no `tmp-` e o
    // varredor de segmentos recolhe-os depois da folga. Um ficheiro fora do
    // `tmp-` e sem linha é a fuga; dentro do `tmp-` tem sempre quem o leve.
    if !crate::recording_artifacts::register(
        db,
        rec_id,
        crate::recording_artifacts::KIND_ISO_TRACKS,
        recordings_dir,
        &destino,
    )
    .await
    {
        tracing::error!(%rec_id, para = %destino.display(),
            "gravação ISO: a pasta não ficou registada — as pistas ficam no tmp- para o varredor");
        return true;
    }
    // Para fora do `tmp-`: se a mudança falhar não se escreve nada mais, e o
    // chamador apaga como sempre — melhor não ter pistas do que ter linhas na
    // base a apontar para ficheiros que o varredor vai levar. É uma renomeação
    // no mesmo sistema de ficheiros, logo O(1).
    if let Err(e) = tokio::fs::rename(&manifest.dir, &destino).await {
        tracing::error!(%rec_id, error = %e, de = %manifest.dir.display(), para = %destino.display(),
            "gravação ISO: falha a mover as pistas para fora do tmp- — não se registam");
        // A LINHA FICA, e é deliberado. A tentação é tirá-la («a pasta nunca
        // chegou a existir»), e está errada: a razão mais provável de este
        // `rename` falhar é o destino JÁ existir — duas composições da mesma
        // gravação a correr (uma reserva que expirou com o pod ainda vivo, e o
        // `resume_due` a reivindicá-la noutro), ou uma pasta que ficou de
        // antes. Nesse caso a linha nomeia uma pasta REAL e cheia, da MESMA
        // gravação, e apagá-la era deixá-la órfã — a fuga 1 reaberta dentro do
        // seu próprio remendo. Se a pasta de facto não existe, a linha é
        // inofensiva: quem recolhe apaga o que não encontra e segue.
        //
        // O que não se faz aqui é apagar a pasta: ela pode ser da outra
        // composição, e essa tem pistas registadas.
        return false;
    }

    // Os nomes que ficaram com linha, e as duas razões de um ficheiro não
    // ficar: `vazias` é operação NORMAL (uma câmara que se ligou e nunca
    // publicou), `perdidas` é DEFEITO (a base recusou a pista). Só a segunda
    // vai ao contador — ver o comentário na reconciliação.
    let mut registadas: Vec<String> = Vec::new();
    let mut vazias = 0u64;
    let mut perdidas = 0u64;
    for t in &manifest.tracks {
        // O nome do ficheiro, nunca o caminho: quem serve junta-o ao
        // `recordings_dir`. Um caminho absoluto na base era uma travessia de
        // directório à espera de acontecer (a migração também o recusa).
        let Some(filename) = t.path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // Uma pista que nunca recebeu nada não vai para a lista: um ficheiro
        // que o browser não consegue abrir é pior do que ficheiro nenhum.
        // Também não fica no disco — a reconciliação lá abaixo leva-o.
        //
        // **O limiar NÃO é zero**, e isso foi medido: o `open_track` escreve o
        // cabeçalho no instante em que cria o ficheiro (as 32 bytes do DKIF no
        // `Vp8IvfWriter::new`, as páginas de identificação no OGG), logo uma
        // pista sem um único pacote RTP tem dezenas de bytes, nunca `len() == 0`.
        // Um `m.len() > 0` deixava passar exactamente a pista vazia que diz
        // querer travar, e servia-a como um ficheiro que não toca.
        let bytes = match tokio::fs::metadata(destino.join(filename)).await {
            Ok(m) if m.len() > HEADER_ONLY => m.len() as i64,
            Ok(_) => {
                vazias += 1;
                continue;
            }
            Err(_) => continue,
        };
        let fonte = studio.source_of_peer(room_id, t.publisher);
        let (numero, etiqueta) = match &fonte {
            Some((_, n, l)) => (Some(*n), Some(l.clone())),
            None => (None, None),
        };
        // `ON CONFLICT DO NOTHING`: uma composição repetida volta a passar
        // por aqui, e duplicar pistas punha a mesma câmara duas vezes no ecrã.
        let r = sqlx::query(
            "INSERT INTO recording_tracks
               (recording_id, publisher_id, kind, filename, size_bytes, starts_at_ms, source_number, source_label)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (recording_id, filename) DO NOTHING",
        )
        .bind(rec_id)
        .bind(t.publisher)
        .bind(&t.kind)
        .bind(filename)
        .bind(bytes)
        .bind(t.starts_at_ms as i64)
        .bind(numero)
        .bind(etiqueta)
        .execute(db)
        .await;
        match r {
            // `Ok` cobre o `ON CONFLICT DO NOTHING`: ou a linha nasceu agora, ou
            // já existia desta pasta — nos dois casos há linha a servir o
            // ficheiro, que é a única coisa que esta lista afirma.
            Ok(_) => registadas.push(filename.to_string()),
            Err(e) => {
                // O ficheiro NÃO fica: a reconciliação abaixo leva-o, porque
                // esta pista não entrou na base. Antes, este `tracing::error!`
                // era o único sinal e o ficheiro ficava no disco para sempre.
                perdidas += 1;
                tracing::error!(%rec_id, kind = %t.kind, error = %e,
                    "gravação ISO: a base recusou a pista — o ficheiro vai ser descartado");
            }
        }
    }

    // RECONCILIAÇÃO, NOS DOIS SENTIDOS. A invariante é «pasta e base dizem o
    // mesmo», e tem duas metades que falham de maneiras opostas:
    //
    //   · ficheiro sem linha → ninguém o consegue pedir (a fuga do `screen-audio`);
    //   · linha sem ficheiro → a listagem promete uma pista e o download dá 404.
    //
    // A segunda metade é a que a primeira versão desta função CRIAVA: os
    // `INSERT` já estão commitados quando se chega aqui (não há transacção à
    // volta do ciclo), e ela relia `SELECT filename` e apagava a pasta inteira
    // se a releitura falhasse — deixando as linhas commitadas a apontar para
    // ficheiros que acabara de apagar. A releitura também era errada por outro
    // motivo: trazia as linhas de TODA a gravação, incluindo as de uma
    // composição anterior com outros nomes, e com elas uma pasta recém-mudada
    // podia ser inteiramente apagada «por não casar». Agora a lista é o que
    // ESTE ciclo registou, e não há releitura nenhuma.
    let descartados = discard_rowless(&destino, &registadas).await;
    if perdidas > 0 || descartados > 0 {
        // Contador, não aviso perdido no log: um ficheiro de gravação que se
        // descarta é matéria-prima que não volta, e tem de ser alertável.
        // Conta-se o que indica DEFEITO — a pista que a base recusou e o
        // ficheiro estranho — e NÃO a pista deliberadamente vazia (uma câmara
        // ligada que nunca publicou), que é operação normal. Um contador que
        // sobe em quase todas as gravações é um alarme que se desliga, e com
        // ele perdia-se o único sinal da fuga que este commit fecha.
        let defeito = perdidas + descartados.saturating_sub(vazias);
        if defeito > 0 {
            metrics
                .recording_iso_tracks_discarded_total
                .fetch_add(defeito, std::sync::atomic::Ordering::Relaxed);
            tracing::warn!(%rec_id, %room_id, defeito, vazias,
                "gravação ISO: ficheiros sem linha na base foram descartados");
        }
    }
    // E a outra metade: linhas desta gravação que já não têm ficheiro na pasta
    // (uma composição anterior com outros nomes, ou a releitura que a versão
    // antiga desta função podia deixar). Sem isto, a listagem mostrava pistas
    // que o download não serve, e nada as limpava — não há rota de `DELETE`.
    let limpas = sqlx::query(
        "DELETE FROM recording_tracks
          WHERE recording_id = $1 AND filename <> ALL($2)",
    )
    .bind(rec_id)
    .bind(&registadas)
    .execute(db)
    .await;
    match limpas {
        Ok(r) if r.rows_affected() > 0 => {
            tracing::warn!(%rec_id, linhas = r.rows_affected(),
                "gravação ISO: linhas sem ficheiro na pasta foram removidas");
        }
        Err(e) => tracing::error!(%rec_id, error = %e,
            "gravação ISO: não foi possível limpar linhas sem ficheiro"),
        _ => {}
    }

    if registadas.is_empty() {
        // Nada registado: não se guardam ficheiros que ninguém vai conseguir
        // pedir. Dizê-lo, porque é um estado que o operador vai estranhar.
        tracing::warn!(%rec_id, %room_id, "gravação ISO pedida e NENHUMA pista registada — os segmentos são apagados");
        // A pasta já saiu do `tmp-` e é nossa (o `rename` acima correu bem):
        // apaga-se, e a linha sai SÓ SE ela sair. O `discard` é que garante a
        // ordem — um `remove_dir_all` recusado (`EACCES`, `EBUSY`, um
        // identificador NFS velho) mantém a linha, conta no
        // `recording_artifacts_stuck_total` e volta a ser tentado, em vez de
        // deixar a pasta no disco com a linha apagada. Era este o defeito da
        // primeira versão: um `let _ = remove_dir_all` seguido de um `forget`
        // incondicional.
        crate::recording_artifacts::discard(db, recordings_dir, metrics, &[destino]).await;
        return false;
    }
    tracing::info!(%rec_id, pistas = registadas.len(), "gravação ISO: pistas por fonte registadas");
    true
}

/// Apaga de `dir` tudo o que não está em `registadas`. Devolve quantas entradas
/// foram descartadas.
///
/// Um erro de leitura do directório devolve 0 e não apaga nada — é o único
/// caminho em que um ficheiro pode sobreviver sem linha, e nesse caso o
/// `sweep_orphan_segments` ainda o apanha pela gravação (ou pela falta dela).
async fn discard_rowless(dir: &Path, registadas: &[String]) -> u64 {
    let mut entradas = match tokio::fs::read_dir(dir).await {
        Ok(d) => d,
        Err(e) => {
            tracing::error!(dir = %dir.display(), error = %e,
                "gravação ISO: não foi possível reconciliar a pasta com a base");
            return 0;
        }
    };
    let mut n = 0;
    while let Ok(Some(e)) = entradas.next_entry().await {
        let nome = e.file_name();
        // Um nome que não é UTF-8 nunca pode ter linha (a coluna é `TEXT` e o
        // `INSERT` vem de um `&str`), logo é sempre para descartar.
        let tem_linha = nome
            .to_str()
            .is_some_and(|n| registadas.iter().any(|r| r == n));
        if tem_linha {
            continue;
        }
        let caminho = e.path();
        let apagado = if e.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
            tokio::fs::remove_dir_all(&caminho).await.is_ok()
        } else {
            tokio::fs::remove_file(&caminho).await.is_ok()
        };
        if apagado {
            tracing::warn!(ficheiro = %caminho.display(),
                "gravação ISO: ficheiro sem linha em recording_tracks — descartado");
            n += 1;
        }
    }
    n
}

/// Compõe a gravação num único webm (em background) e insere-a na biblioteca.
pub fn finalize(state: Arc<AppState>, room_id: Uuid, session: RecordingSession) {
    let manifest = session.manifest();
    tokio::spawn(async move {
        // A linha nasce JÁ, em `processing`, COM o manifesto e a primeira
        // tentativa reservada: quem parou a gravação vê-a na biblioteca a
        // compor, e se este processo morrer a seguir outro retoma de onde
        // ficou (`resume_due`) em vez de a dar por perdida.
        let rec_id = insert_processing(&state, room_id, &session, &manifest).await;
        compose_once(&state, room_id, &manifest, rec_id).await;
    });
}

/// Uma passagem de composição: compõe, e em caso de falha decide se a gravação
/// volta à fila ou se fica `failed`. É o corpo partilhado pelo primeiro
/// `finalize` e por cada retoma de `resume_due` — sem isto, a retoma era uma
/// cópia das regras de falha, que é o que a catraca da arquitectura persegue.
async fn compose_once(
    state: &Arc<AppState>,
    room_id: Uuid,
    manifest: &ComposeManifest,
    rec_id: Option<Uuid>,
) {
    match finalize_inner(state, room_id, manifest, rec_id).await {
        Ok(()) => {
            // GRAVAÇÃO ISO (ADR-0014 §2.3): numa sala de estúdio que a pediu,
            // os ficheiros por fonte FICAM e passam a ser servidos. Até aqui
            // eram escritos durante a gravação e apagados neste `remove_dir_all`
            // — a matéria-prima existia e era deitada fora, enquanto o
            // interruptor `studios.iso_recording` (NOT NULL DEFAULT TRUE) e o
            // ecrã prometiam pistas por câmara que nenhuma linha servia.
            // `registar_pistas_iso` RENOMEIA a pasta quando guarda, por isso o
            // `remove_dir_all` abaixo só faz sentido quando ela devolve false —
            // e nesse caso o `manifest.dir` ainda existe com o nome antigo.
            let guardar = registar_pistas_iso(
                &state.db,
                &state.config.recordings_dir,
                &state.studio,
                room_id,
                manifest,
                rec_id,
                &state.metrics,
            )
            .await;
            if !guardar {
                // Só o caminho feliz apaga os segmentos. Uma falha que vai ser
                // repetida PRECISA deles — apagá-los era o que fazia de qualquer
                // falha uma perda definitiva.
                let _ = tokio::fs::remove_dir_all(&manifest.dir).await;
            }
        }
        Err(e) => {
            tracing::error!(%room_id, error = %e, "server recording finalize failed");
            let tentativas = match rec_id {
                Some(id) => attempts_so_far(state, id).await,
                // Sem linha não há contador: não se repete o que não se conta.
                None => i32::MAX,
            };
            let repetivel = compose_rules::is_retryable(&e.to_string());
            if compose_rules::should_retry(repetivel, tentativas)
                && dir_ainda_existe(manifest).await
            {
                // Larga a reserva e MANTÉM `processing`: a próxima volta do
                // varredor pega nela. Os segmentos ficam.
                if let Some(id) = rec_id {
                    liberar_reserva(state, id, &e).await;
                }
                tracing::warn!(%room_id, tentativas, "composição falhou — volta à fila");
                return;
            }
            // A falha passa a ser VISÍVEL. Antes ficava só no log, o
            // directório temporário era apagado, e a biblioteca não mostrava
            // nada — do lado de quem carregou em «gravar» e viu o indicador
            // aceso a reunião inteira, isso é indistinguível de nunca ter
            // gravado.
            registar_falha(state, room_id, manifest, rec_id, &e).await;
            let _ = tokio::fs::remove_dir_all(&manifest.dir).await;
        }
    }
}

/// Quantas tentativas esta gravação já gastou. `i32::MAX` se não se souber —
/// não se repete o que não se consegue contar.
async fn attempts_so_far(state: &Arc<AppState>, rec_id: Uuid) -> i32 {
    sqlx::query_scalar::<_, i32>("SELECT compose_attempts FROM recordings WHERE id = $1")
        .bind(rec_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .unwrap_or(i32::MAX)
}

/// Os segmentos ainda estão no disco? Num pod novo com disco efémero não
/// estão, e aí retomar é impossível — mais vale falhar depressa e dizê-lo.
async fn dir_ainda_existe(manifest: &ComposeManifest) -> bool {
    tokio::fs::metadata(&manifest.dir)
        .await
        .map(|m| m.is_dir())
        .unwrap_or(false)
}

/// Larga a reserva sem fechar a gravação: fica `processing` e reivindicável.
/// O `failure_reason` escreve-se mesmo assim — quem está a olhar para o
/// progresso merece saber que houve um tropeção, e a próxima tentativa
/// reescreve-o.
async fn liberar_reserva(state: &Arc<AppState>, rec_id: Uuid, erro: &anyhow::Error) {
    let _ = sqlx::query(
        "UPDATE recordings
            SET compose_lease_token = NULL, compose_lease_expires_at = NULL,
                progress_pct = 0, progress_at = now(), failure_reason = $2
          WHERE id = $1 AND status = 'processing'",
    )
    .bind(rec_id)
    .bind(causa_legivel(erro))
    .execute(&state.db)
    .await;
}

/// O que o gravador precisa de saber da sala.
struct RoomRecInfo {
    code: String,
    format: String,
    quality: Option<String>,
}

async fn room_rec_info(state: &AppState, room_id: Uuid) -> RoomRecInfo {
    let row: Option<(String, String, Option<String>)> =
        sqlx::query_as("SELECT code, format, record_quality FROM rooms WHERE id = $1")
            .bind(room_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    let (code, format, quality) = row.unwrap_or_default();
    RoomRecInfo {
        code,
        format,
        quality,
    }
}

fn recording_filename(code: &str) -> String {
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M");
    format!("Reunião {code} — servidor — {stamp}.webm")
}

/// Insere a gravação em `processing` COM o manifesto e a primeira tentativa já
/// reservada. `None` se a base falhar — a composição segue na mesma e a linha é
/// inserida no fim, como antes (mas então não é retomável: sem linha não há
/// manifesto).
async fn insert_processing(
    state: &Arc<AppState>,
    room_id: Uuid,
    session: &RecordingSession,
    manifest: &ComposeManifest,
) -> Option<Uuid> {
    let info = room_rec_info(state, room_id).await;
    // O id nasce AQUI e não do `DEFAULT` da coluna: o `<id>.webm` e o
    // `<id>.jpg` derivam dele, e as linhas do registo de artefactos escrevem-se
    // na MESMA transacção que a da gravação — a linha antes do ficheiro, que é
    // a regra do `recording_artifacts` (migração 0126). Numa transacção só, não
    // há o caso de «a gravação existe e os ficheiros dela não estão
    // registados», que era a fuga a entrar por outra porta.
    let rec_id = Uuid::new_v4();
    let r = async {
        let mut tx = state.db.begin().await?;
        sqlx::query(
            "INSERT INTO recordings (id, room_id, uploader_id, filename, size_bytes, status,
                                     progress_pct, progress_at, kind,
                                     compose_manifest, compose_lease_token,
                                     compose_lease_expires_at, compose_attempts)
             VALUES ($1, $2, $3, $4, 0, 'processing', 0, now(), $5,
                     $6::jsonb, $7, now() + make_interval(secs => $8), 1)",
        )
        .bind(rec_id)
        .bind(room_id)
        .bind(session.by_user)
        .bind(recording_filename(&info.code))
        .bind(crate::recordings::kind_from_room_format(&info.format))
        .bind(serde_json::to_string(manifest).unwrap_or_else(|_| "null".into()))
        .bind(delonix_meet_core::crypto::random_hex(24))
        .bind(compose_rules::LEASE.as_secs() as f64)
        .execute(&mut *tx)
        .await?;
        crate::recording_artifacts::register_canonical(
            &mut tx,
            &state.config.recordings_dir,
            rec_id,
        )
        .await?;
        tx.commit().await
    }
    .await;
    match r {
        Ok(()) => Some(rec_id),
        Err(e) => {
            tracing::error!(%room_id, error = %e, "não foi possível registar a gravação em processamento");
            None
        }
    }
}

/// Caixa de resolução de uma qualidade pedida. `audio` e desconhecidas: `None`.
pub(crate) fn quality_box(quality: &str) -> Option<(u32, u32)> {
    match quality {
        "2160p" => Some((3840, 2160)),
        "1080p" => Some((1920, 1080)),
        "720p" => Some((1280, 720)),
        _ => None,
    }
}

/// Tamanho de cada mosaico da grelha para `n` vídeos.
///
/// Sem qualidade pedida, a composição de sempre: mosaicos de 640×360 e a tela
/// cresce com o número de pessoas. Com qualidade, a TELA é a caixa pedida e os
/// mosaicos dividem-na (dimensões pares, que o yuv420p exige).
pub(crate) fn grid_tile(quality: Option<&str>, n: usize) -> (u32, u32) {
    let Some((w, h)) = quality.and_then(quality_box) else {
        return (640, 360);
    };
    let n = n.max(1);
    let cols = (n as f64).sqrt().ceil() as u32;
    let rows = (n as u32).div_ceil(cols);
    let even = |v: u32| (v / 2) * 2;
    (even(w / cols), even(h / rows))
}

/// Percentagem da composição, presa a 0–99 (o 100 é o `ready`).
pub(crate) fn composition_pct(done_ms: i64, expected_ms: i64) -> i16 {
    if expected_ms <= 0 || done_ms <= 0 {
        return 0;
    }
    ((done_ms.saturating_mul(100) / expected_ms).clamp(0, 99)) as i16
}

/// Grava o progresso lido do ffmpeg, no máximo a cada 2 s.
fn spawn_progress_writer(
    state: Arc<AppState>,
    rec_id: Uuid,
    expected_ms: i64,
    mut rx: tokio::sync::watch::Receiver<i64>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut renovacao = tokio::time::interval(compose_rules::RENEW_EVERY);
        renovacao.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        renovacao.tick().await; // o primeiro é imediato e a reserva já está posta
        loop {
            // Duas razões para escrever, e a diferença importa: o progresso
            // vem do ffmpeg e pode PARAR (um input malformado deixa-o calado);
            // a renovação vem do relógio e só para quando a tarefa morre. Se a
            // posse dependesse do progresso, um ffmpeg estancado perdia a
            // reserva com o processo ainda vivo — e dois ffmpeg escreviam o
            // mesmo `out.webm`.
            let avancou = tokio::select! {
                mudou = rx.changed() => {
                    if mudou.is_err() {
                        return; // o ffmpeg acabou e largou o Sender
                    }
                    true
                }
                _ = renovacao.tick() => false,
            };
            let pct = avancou.then(|| composition_pct(*rx.borrow_and_update(), expected_ms));
            let _ = sqlx::query(
                "UPDATE recordings
                    SET progress_pct = COALESCE($2, progress_pct),
                        progress_at = now(),
                        compose_lease_expires_at = now() + make_interval(secs => $3)
                  WHERE id = $1 AND status = 'processing'",
            )
            .bind(rec_id)
            .bind(pct)
            .bind(compose_rules::LEASE.as_secs() as f64)
            .execute(&state.db)
            .await;
            if avancou {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    })
}

/// De quanto em quanto tempo uma composição à espera de vaga dá sinal de vida.
/// Tem de ser muito menor do que o tecto da varredura (`ffmpeg_timeout_secs` +
/// 600 s em `fail_stale_processing`): sem batimento, uma gravação que espera
/// há mais do que isso era marcada como falhada com o pod vivo e a fila a andar.
const COMPOSE_QUEUE_BEAT: std::time::Duration = std::time::Duration::from_secs(60);

/// Sobe um gauge enquanto vive e desce-o ao cair — inclusive quando o future
/// que o guarda é cancelado (shutdown), que é onde um `fetch_sub` à mão se perde.
struct GaugeGuard<'a>(&'a std::sync::atomic::AtomicI64);

impl<'a> GaugeGuard<'a> {
    fn up(g: &'a std::sync::atomic::AtomicI64) -> Self {
        g.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self(g)
    }
}

impl Drop for GaugeGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Espera por uma vaga de composição para `tenant`, chamando `on_beat` a cada
/// `beat_every` enquanto espera. O `acquire` fica fixo e é re-sondado: largar o
/// future perdia o lugar na fila. As vagas repartem-se por inquilino
/// (`fair_slots`): uma organização com muitas composições à espera não deixa as
/// outras atrás de si.
pub(crate) async fn wait_for_slot<F, Fut>(
    slots: crate::fair_slots::FairSlots,
    tenant: Uuid,
    beat_every: std::time::Duration,
    mut on_beat: F,
) -> anyhow::Result<crate::fair_slots::Slot>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let acquire = slots.acquire(tenant);
    tokio::pin!(acquire);
    let mut tick = tokio::time::interval(beat_every);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tick.tick().await; // o primeiro tick é imediato; o batimento só conta ao fim de um período
    loop {
        tokio::select! {
            slot = &mut acquire => {
                return slot.map_err(|_| anyhow::anyhow!("vagas de composição fechadas"));
            }
            _ = tick.tick() => on_beat().await,
        }
    }
}

/// Gravações presas em `processing` há mais do que o tecto do ffmpeg (com
/// folga) passam a `failed`: o pod que as compunha morreu a meio.
///
/// E as presas em `transcribing` sem sinal de vida há 6 h voltam a `ready`
/// (há ficheiro; a transcrição simplesmente não acabou): sem ai-worker vivo
/// que as reclame, ficavam a dizer «a transcrever» para sempre.
/// **Retoma as composições abandonadas.** Devolve quantas DESPACHOU — não
/// quantas acabaram: cada uma segue na sua tarefa, porque uma composição
/// demora até uma hora e este ciclo corre a cada cinco minutos.
///
/// É o que faltava: uma gravação em `processing` com manifesto e sem reserva em
/// vigor é trabalho que alguém começou e não acabou — o pod morreu, o rollout
/// levou-o, ou a tentativa anterior falhou de forma repetível. Reivindica-se
/// com `FOR UPDATE SKIP LOCKED` (dois pods nunca pegam na mesma) e compõe-se
/// outra vez a partir do manifesto.
///
/// Corre em TODOS os nós, como o `retry_due` dos webhooks. O limite por volta
/// é baixo de propósito: a composição é cara em CPU e as vagas são repartidas
/// por inquilino (`fair_slots`) — encher a fila de uma vez não a faria andar
/// mais depressa, só atrasaria as chamadas vivas do mesmo pod.
/// A fila da composição, declarada uma vez.
///
/// Era o sétimo `FOR UPDATE SKIP LOCKED` à mão do servidor, e nasceu DEPOIS do
/// desenho da peça comum (#268) — a oitava cópia do padrão a entrar enquanto o
/// trabalho de a acabar estava em curso, que é exactamente o que a catraca não
/// apanha e o desenho avisou.
///
/// `tenant_column: None`: a repartição entre inquilinos desta fila já existe a
/// jusante, nas vagas de composição (`fair_slots`), que é onde o recurso
/// disputado (CPU do ffmpeg) está. Pôr justiça também aqui seria contá-la duas
/// vezes.
const FILA: delonix_meet_core::jobs::Queue = delonix_meet_core::jobs::Queue {
    name: "recording_compose",
    table: "recordings",
    id_column: "id",
    ready_when: "status = 'processing' AND compose_manifest IS NOT NULL \
                 AND compose_attempts < {max_attempts} \
                 AND (compose_lease_expires_at IS NULL \
                      OR compose_lease_expires_at < now())",
    claim_set: "compose_lease_token = md5(random()::text || clock_timestamp()::text), \
                compose_lease_expires_at = now() + make_interval(secs => {lease_secs}), \
                compose_attempts = compose_attempts + 1, \
                progress_at = now()",
    returning: "id, room_id, compose_manifest, compose_attempts",
    order_by: "created_at",
    tenant_column: None,
    batch: 2,
};

/// Uma composição reivindicada para retomar.
#[derive(sqlx::FromRow)]
struct Retomavel {
    id: Uuid,
    room_id: Uuid,
    compose_manifest: serde_json::Value,
    compose_attempts: i32,
}

pub async fn resume_due(state: &Arc<AppState>) -> usize {
    let claimed: Vec<Retomavel> = match crate::jobs::claim(
        state,
        &crate::jobs::Worker {
            queue: FILA,
            retry: delonix_meet_core::jobs::Retry {
                max_attempts: compose_rules::MAX_ATTEMPTS,
                delays: &[],
                jitter: 0.0,
            },
            lease: Some(delonix_meet_core::jobs::Lease {
                duration: compose_rules::LEASE,
            }),
        },
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "retoma de composições: reivindicação falhou");
            return 0;
        }
    };
    let mut retomadas = 0;
    for Retomavel {
        id: rec_id,
        room_id,
        compose_manifest: raw,
        compose_attempts: tentativa,
    } in claimed
    {
        let manifest: ComposeManifest = match serde_json::from_value(raw) {
            Ok(m) => m,
            Err(e) => {
                // Manifesto que não se lê (de uma versão anterior do formato):
                // não se retoma às cegas, fecha-se com a causa.
                tracing::warn!(%rec_id, error = %e, "retoma: manifesto ilegível");
                let _ = sqlx::query(
                    "UPDATE recordings SET status = 'failed', compose_manifest = NULL,
                            compose_lease_token = NULL, compose_lease_expires_at = NULL,
                            progress_pct = NULL, progress_at = NULL,
                            failure_reason = $2
                      WHERE id = $1 AND status = 'processing'",
                )
                .bind(rec_id)
                .bind(FALHA_INTERROMPIDA)
                .execute(&state.db)
                .await;
                continue;
            }
        };
        if !dir_ainda_existe(&manifest).await {
            // Num pod novo com disco efémero os segmentos não viajaram. É uma
            // perda real: dizê-la depressa vale mais do que tentar três vezes
            // o que não pode funcionar.
            tracing::warn!(%rec_id, dir = %manifest.dir.display(), "retoma: segmentos já não existem");
            let _ = sqlx::query(
                "UPDATE recordings SET status = 'failed', compose_manifest = NULL,
                        compose_lease_token = NULL, compose_lease_expires_at = NULL,
                        progress_pct = NULL, progress_at = NULL, failure_reason = $2
                  WHERE id = $1 AND status = 'processing'",
            )
            .bind(rec_id)
            .bind(FALHA_SEGMENTOS_PERDIDOS)
            .execute(&state.db)
            .await;
            continue;
        }
        tracing::info!(%rec_id, %room_id, tentativa, "retoma de composição");
        // DESPACHA, não espera. A composição demora até
        // `FFMPEG_TIMEOUT_SECS` (1 h) e esta função é chamada do ciclo de 5
        // min que também fecha as gravações paradas e gera capítulos:
        // esperar aqui prendia esse ciclo horas a fio. A reserva é o que
        // impede a volta seguinte de pegar na mesma gravação, e as vagas
        // (`fair_slots`) é que limitam quantos ffmpeg correm ao mesmo tempo.
        let state = state.clone();
        tokio::spawn(async move {
            compose_once(&state, room_id, &manifest, Some(rec_id)).await;
        });
        retomadas += 1;
    }
    retomadas
}

/// Apaga os directórios `tmp-*` que já não pertencem a nenhuma composição.
///
/// **Não varre as pastas `iso-<gravação>`, e é uma decisão — não um esquecimento.**
/// Uma versão deste commit varria-as pela ausência da linha em `recordings`, e a
/// revisão mostrou que estava errada por três razões independentes, todas
/// medidas:
///
///  1. **Não cobria o caso para que foi escrita.** A pasta nasce de um `rename`
///     feito DEPOIS de o `insert_processing` ter escrito a linha, logo num
///     processo morto entre a renomeação e o `INSERT` das pistas a linha
///     EXISTE — e o varredor saltava a pasta para sempre.
///  2. **A folga não protegia nada.** O `rename(2)` preserva o mtime do
///     directório (medido): a pasta `iso-` nasce com a idade do `tmp-`, que
///     numa reunião longa já passou a folga.
///  3. **O modo de falha era catastrófico.** `vivas.is_empty()` não é um erro,
///     logo uma base restaurada para um ponto anterior — ou um ambiente apontado
///     ao mesmo volume `ReadWriteMany`, que sobrevive à base — autorizava
///     `remove_dir_all` em TODAS as pastas de pistas do volume, no primeiro
///     tique de arranque, sem tecto nem quarentena.
///
/// A fuga que ela queria fechar (uma pasta que um processo morto deixa a meio do
/// registo) é REAL e continua aberta. Fecha-se com um registo explícito do
/// artefacto — uma coluna `recordings.iso_dir`, ou uma tabela de ficheiros por
/// gravação que a retenção percorra — e não por inferência a partir do nome da
/// pasta. Fica dito aqui em vez de num ficheiro de notas.
///
/// Antes, o `remove_dir_all` só corria no caminho feliz: cada processo morto a
/// meio deixava os segmentos RTP no volume para sempre. Agora a composição
/// retomável PRECISA deles enquanto a linha viver — por isso a regra é: apaga-se
/// o que nenhuma gravação em `processing` reclama no manifesto, e só depois de
/// uma folga que cubra a reserva mais longa.
pub async fn sweep_orphan_segments(state: &Arc<AppState>) -> u64 {
    let raiz = &state.config.recordings_dir;
    let mut dir = match tokio::fs::read_dir(raiz).await {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(error = %e, "varredura de segmentos: não foi possível ler o directório");
            return 0;
        }
    };
    // Os que ainda têm dono. Uma consulta só, não uma por directório.
    let vivos: Vec<String> = sqlx::query_scalar(
        "SELECT compose_manifest->>'dir' FROM recordings
          WHERE status = 'processing' AND compose_manifest IS NOT NULL",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    // A folga NÃO é a reserva (essa são minutos): um directório pode ser de
    // uma composição a correr há quase o tecto inteiro do ffmpeg, ou de uma que
    // arrancou neste instante e cuja linha ainda não foi escrita.
    let folga = compose_rules::orphan_grace(state.config.ffmpeg_timeout_secs);
    let mut apagados = 0;
    while let Ok(Some(e)) = dir.next_entry().await {
        let path = e.path();
        if !path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("tmp-"))
        {
            continue;
        }
        if vivos.iter().any(|d| std::path::Path::new(d) == path) {
            continue;
        }
        let recente = e
            .metadata()
            .await
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|idade| idade < folga);
        if recente {
            continue;
        }
        if tokio::fs::remove_dir_all(&path).await.is_ok() {
            tracing::info!(dir = %path.display(), "segmentos órfãos apagados");
            apagados += 1;
        }
    }
    apagados
}

/// As duas causas que uma retoma pode ter de escrever. Texto para pessoas: o
/// detalhe técnico fica no log, onde serve a quem opera.
const FALHA_INTERROMPIDA: &str = "O processamento foi interrompido (o servidor reiniciou a meio) e não foi possível retomá-lo. A equipa de operação tem o detalhe no registo.";
const FALHA_SEGMENTOS_PERDIDOS: &str = "O processamento foi interrompido e os ficheiros temporários da gravação já não estavam disponíveis para o retomar. A equipa de operação tem o detalhe no registo.";

pub async fn fail_stale_processing(state: &Arc<AppState>) -> u64 {
    if let Err(e) = sqlx::query(
        "UPDATE recordings SET status = 'ready', progress_pct = NULL, progress_at = NULL
         WHERE status = 'transcribing' AND transcribed_at IS NULL
           AND COALESCE(progress_at, created_at) < now() - interval '6 hours'",
    )
    .execute(&state.db)
    .await
    {
        tracing::warn!(error = %e, "varredura de transcrições paradas falhou");
    }
    // Só o que a RETOMA já não vai apanhar: sem manifesto (linhas nascidas
    // antes da 0099, ou cuja base falhou no `insert_processing`) ou com as
    // tentativas esgotadas. Uma gravação retomável fica em `processing` de
    // propósito — é assim que o `resume_due` a encontra —, e marcá-la `failed`
    // aqui era o que fazia de qualquer rollout uma perda definitiva.
    let limit = state.config.ffmpeg_timeout_secs as i64 + 600;
    match sqlx::query(
        "UPDATE recordings SET status = 'failed', progress_pct = NULL,
                compose_lease_token = NULL, compose_lease_expires_at = NULL,
                failure_reason = $2
         WHERE status = 'processing' AND COALESCE(progress_at, created_at) < now() - make_interval(secs => $1)
           AND (compose_manifest IS NULL OR compose_attempts >= $3)",
    )
    .bind(limit as f64)
    .bind(FALHA_INTERROMPIDA)
    .bind(compose_rules::MAX_ATTEMPTS)
    .execute(&state.db)
    .await
    {
        Ok(r) => {
            if r.rows_affected() > 0 {
                tracing::warn!(n = r.rows_affected(), "gravações presas em processamento marcadas como falhadas");
            }
            r.rows_affected()
        }
        Err(e) => {
            tracing::warn!(error = %e, "varredura de processamento falhou");
            0
        }
    }
}

/// A sala pede gravação automática, não é E2EE (sem chave cedida o gravador
/// só escreveria ruído cifrado), e ainda não tem nenhuma gravação — parar à
/// mão e voltar a entrar não recomeça.
pub(crate) async fn auto_record_wanted(state: &AppState, room_id: Uuid) -> bool {
    sqlx::query_scalar::<_, bool>(
        "SELECT r.auto_record AND NOT r.e2ee
                AND NOT EXISTS(SELECT 1 FROM recordings x WHERE x.room_id = r.id)
         FROM rooms r WHERE r.id = $1",
    )
    .bind(room_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .unwrap_or(false)
}

/// Traduz um erro técnico para uma causa que se possa mostrar a uma pessoa.
///
/// O texto do erro NÃO é usado directamente: o stderr do ffmpeg traz caminhos
/// do servidor e nomes de ficheiros temporários, que não têm nada que fazer no
/// ecrã de um utilizador. O detalhe fica no log, onde é útil a quem opera.
fn causa_legivel(e: &anyhow::Error) -> &'static str {
    let t = e.to_string();
    if t.contains("storage.quota_exceeded") {
        "A organização atingiu a quota de armazenamento, por isso a gravação não foi guardada. Liberte espaço e grave de novo."
    } else if t.contains("ffmpeg-ausente") {
        // Causa de OPERAÇÃO, não do utilizador. Dizê-lo pelo nome poupa a quem
        // recebe a queixa uma investigação inteira — e a correcção é instalar
        // o ffmpeg, não voltar a gravar.
        "O servidor não tem o ffmpeg instalado, e sem ele não consegue compor gravações. É uma configuração em falta no servidor — comunica-o a quem o administra."
    } else if t.contains("nothing recorded") {
        "Não chegou media suficiente para gravar. A gravação pode ter sido parada demasiado cedo, ou ninguém tinha câmara nem microfone ligados."
    } else if t.contains("excedeu") {
        "A composição do vídeo excedeu o tempo máximo e foi interrompida."
    } else if t.contains("ffprobe") {
        "O vídeo final não passou na validação e não foi publicado. A equipa de operação tem o detalhe no registo."
    } else if t.contains("ffmpeg") {
        "O servidor não conseguiu compor o vídeo final. A equipa de operação tem o detalhe no registo."
    } else if t.contains("No space") || t.contains("space left") {
        "Não havia espaço em disco para guardar a gravação."
    } else {
        "A gravação não pôde ser finalizada. A equipa de operação tem o detalhe no registo."
    }
}

async fn registar_falha(
    state: &Arc<AppState>,
    room_id: Uuid,
    manifest: &ComposeManifest,
    rec_id: Option<Uuid>,
    erro: &anyhow::Error,
) {
    if let Some(id) = rec_id {
        let r = sqlx::query(
            // O manifesto e a reserva saem: a decisão de não repetir já foi
            // tomada em `compose_once`, e os segmentos são apagados a seguir.
            "UPDATE recordings SET status = 'failed', failure_reason = $2, size_bytes = 0,
                    progress_pct = NULL, progress_at = NULL,
                    compose_manifest = NULL, compose_lease_token = NULL,
                    compose_lease_expires_at = NULL
             WHERE id = $1",
        )
        .bind(id)
        .bind(causa_legivel(erro))
        .execute(&state.db)
        .await;
        match r {
            Ok(done) if done.rows_affected() > 0 => return,
            Ok(_) => {}
            Err(e) => {
                tracing::error!(%room_id, error = %e, "não foi possível marcar a gravação como falhada")
            }
        }
    }
    let nome = format!(
        "Gravação falhada · {}",
        chrono::Utc::now().format("%d/%m/%Y %H:%M")
    );
    // `size_bytes = 0` e `status = failed`: a linha existe para ser VISTA, não
    // para ser descarregada. O `download` recusa-a explicitamente.
    let r = sqlx::query(
        "INSERT INTO recordings (room_id, uploader_id, filename, size_bytes, status, failure_reason)
         VALUES ($1, $2, $3, 0, 'failed', $4)",
    )
    .bind(room_id)
    .bind(manifest.by_user)
    .bind(&nome)
    .bind(causa_legivel(erro))
    .execute(&state.db)
    .await;
    if let Err(e) = r {
        // Falhar a registar a falha é o fim da linha: não há mais onde a pôr.
        tracing::error!(%room_id, error = %e, "não foi possível registar a gravação falhada");
    }
}

/// Corre um processo externo com **tecto de tempo**, matando-o se o exceder.
///
/// Existe separado para ser testável sem um `ffmpeg` instalado: o
/// comportamento que interessa — não ficar pendurado para sempre, e matar o
/// processo em vez de o deixar órfão — é o mesmo seja qual for o binário.
#[cfg(test)]
async fn run_bounded(
    cmd: &mut tokio::process::Command,
    limit: std::time::Duration,
) -> anyhow::Result<std::process::ExitStatus> {
    run_bounded_progress(cmd, limit, None).await
}

/// `run_bounded` que, com `progress`, lê o `-progress pipe:1` do ffmpeg e
/// publica cada `out_time_us` (em ms). O stdout é lido até ao fim: um pipe
/// que ninguém esvazia bloqueia o processo.
pub(crate) async fn run_bounded_progress(
    cmd: &mut tokio::process::Command,
    limit: std::time::Duration,
    progress: Option<tokio::sync::watch::Sender<i64>>,
) -> anyhow::Result<std::process::ExitStatus> {
    if progress.is_some() {
        cmd.stdout(std::process::Stdio::piped());
    }
    // Contexto na ORIGEM. Sem isto, um ffmpeg em falta chega ao utilizador como
    // «No such file or directory (os error 2)» — indistinguível de um ficheiro
    // de track em falta, e a causa real (uma instalação incompleta do servidor)
    // fica escondida atrás de uma mensagem genérica.
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!("ffmpeg-ausente: não foi encontrado no PATH do servidor")
        } else {
            anyhow::Error::from(e)
        }
    })?;
    if let (Some(tx), Some(out)) = (progress, child.stdout.take()) {
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = tokio::io::BufReader::new(out).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                if let Some(ms) = l
                    .strip_prefix("out_time_us=")
                    .and_then(|v| v.trim().parse::<i64>().ok())
                    .filter(|v| *v > 0)
                    .map(|us| us / 1000)
                {
                    let _ = tx.send(ms);
                }
            }
        });
    }
    match tokio::time::timeout(limit, child.wait()).await {
        Ok(res) => Ok(res?),
        Err(_) => {
            // `kill` e depois `wait`: sem colher o filho ficava zombie.
            let _ = child.kill().await;
            let _ = child.wait().await;
            anyhow::bail!(
                "processo excedeu {}s e foi terminado (sobe FFMPEG_TIMEOUT_SECS \
                 se as gravações forem legitimamente mais longas)",
                limit.as_secs()
            )
        }
    }
}

async fn finalize_inner(
    state: &Arc<AppState>,
    room_id: Uuid,
    manifest: &ComposeManifest,
    rec_id: Option<Uuid>,
) -> anyhow::Result<()> {
    // Duração (G4): relógio de parede da sessão, medido quando a gravação
    // PAROU e guardado no manifesto. Não é a duração do media (não se sonda o
    // ficheiro com ffprobe), por isso pode divergir alguns segundos; é o que
    // se sabe sem mais um processo. Vem do manifesto e não de um `elapsed()`
    // porque numa retoma o relógio da sessão já não existe — e o progresso
    // tem de ter o mesmo denominador na segunda tentativa que na primeira.
    let duration_secs = manifest.duration_secs;
    let info = room_rec_info(state, room_id).await;
    let quality = info.quality.as_deref();
    let expected_ms = manifest.expected_ms;
    // Tracks com conteúdo real (ficheiros ~vazios ficam de fora).
    let mut videos: Vec<&ComposeTrack> = Vec::new();
    let mut audios: Vec<&ComposeTrack> = Vec::new();
    for t in &manifest.tracks {
        let is_big = tokio::fs::metadata(&t.path)
            .await
            .map(|m| m.len() > 4096)
            .unwrap_or(false);
        if !is_big {
            continue;
        }
        if t.kind.ends_with("audio") {
            audios.push(t);
        } else {
            videos.push(t);
        }
    }
    if quality == Some("audio") {
        // «Só áudio» pedido na reunião: o vídeo nem entra na composição.
        videos.clear();
    }
    if videos.is_empty() && audios.is_empty() {
        anyhow::bail!("nothing recorded");
    }

    let out = manifest.dir.join("out.webm");
    let mut cmd = tokio::process::Command::new(&state.config.ffmpeg_bin);
    cmd.arg("-y").arg("-loglevel").arg("error");
    // `-nostdin`: sem isto o ffmpeg herda o stdin do servidor e pode ficar à
    // espera de input que nunca chega. `-threads`: travão de CPU — a
    // composição de uma gravação não pode degradar as chamadas VIVAS do mesmo
    // pod. `kill_on_drop`: se este future for cancelado, o processo morre com
    // ele em vez de ficar órfão a consumir o nó.
    cmd.arg("-nostdin");
    cmd.args(["-threads", &state.config.ffmpeg_threads.to_string()]);
    cmd.args(["-progress", "pipe:1", "-nostats"]);
    cmd.kill_on_drop(true);

    // Resolução (G4): na composição em grelha é a da grelha; no remux é a do
    // cabeçalho IVF, que o `Vp8IvfWriter` corrige no fecho com as dimensões
    // do primeiro keyframe. Só áudio: sem resolução.
    let dims = if videos.len() == 1 && audios.len() <= 1 {
        ivf_dims(&videos[0].path).await
    } else {
        grid_dims(videos.len())
    };
    // Resolução acima da pedida → reduz-se. Nunca se amplia: aumentar não
    // acrescenta detalhe, só bytes.
    let downscale_to: Option<u32> = match (videos.len(), quality.and_then(quality_box)) {
        (1, Some((_, target_h))) => crate::media_probe::probe(
            &state.config.ffprobe_bin,
            &state.config.ffmpeg_bin,
            &videos[0].path,
        )
        .await
        .ok()
        .and_then(|m| m.height)
        .filter(|h| *h as u32 > target_h)
        .map(|_| target_h),
        _ => None,
    };

    cmd.args(compose_args(&videos, &audios, quality, downscale_to));
    cmd.arg(&out);

    tracing::info!(%room_id, tracks = manifest.tracks.len(), "server recording: a compor webm…");
    // Tecto de tempo. Um input malformado (ou um codec inesperado — ver R18)
    // pendurava o ffmpeg indefinidamente: o directório `tmp-<uuid>` ficava no
    // volume, a gravação nunca chegava à biblioteca, e não havia erro nenhum
    // para ver. Falhar em tempo limitado é a única resposta honesta.
    let limit = std::time::Duration::from_secs(state.config.ffmpeg_timeout_secs);
    // Vaga de composição ANTES de arrancar o ffmpeg e de o relógio do tecto
    // começar: esperar a vez não conta como tempo de composição.
    let waiting = GaugeGuard::up(&state.metrics.recording_compose_queued);
    let queued_at = std::time::Instant::now();
    // De quem é a composição: o inquilino da sala (a organização do dono), para
    // que as vagas se repartam entre organizações e não por ordem de chegada.
    let tenant = crate::signaling::resolve_tenant(state, room_id)
        .await
        .unwrap_or(crate::fair_slots::UNKNOWN_TENANT);
    let _slot = wait_for_slot(
        state.compose_slots.clone(),
        tenant,
        COMPOSE_QUEUE_BEAT,
        || async {
            if let Some(id) = rec_id {
                // Dá sinal de vida E renova a posse: uma composição que espera
                // vaga há mais do que a reserva não pode ser reivindicada por
                // outro nó só por ainda não ter começado.
                let _ = sqlx::query(
                    "UPDATE recordings
                        SET progress_at = now(),
                            compose_lease_expires_at = now() + make_interval(secs => $2)
                      WHERE id = $1 AND status = 'processing'",
                )
                .bind(id)
                .bind(compose_rules::LEASE.as_secs() as f64)
                .execute(&state.db)
                .await;
            }
        },
    )
    .await?;
    drop(waiting);
    let _running = GaugeGuard::up(&state.metrics.recording_compose_running);
    if queued_at.elapsed() > std::time::Duration::from_secs(1) {
        tracing::info!(
            %room_id,
            waited_secs = queued_at.elapsed().as_secs(),
            "server recording: vaga de composição obtida após espera"
        );
    }
    let (tx, rx) = tokio::sync::watch::channel(0i64);
    let writer = rec_id.map(|id| spawn_progress_writer(state.clone(), id, expected_ms, rx));
    let status = run_bounded_progress(&mut cmd, limit, Some(tx)).await;
    if let Some(w) = writer {
        // O leitor do stdout larga o `Sender` no EOF e o escritor sai sozinho;
        // se o ffmpeg foi morto por tempo, corta-se aqui.
        w.abort();
    }
    let status = status?;
    if !status.success() {
        anyhow::bail!("ffmpeg exited with {status}");
    }
    let size = tokio::fs::metadata(&out).await?.len() as i64;
    // Quota de armazenamento (RFC-0001, B7): o upload do cliente já a impunha
    // (`recordings.rs`); a gravação do servidor passava ao lado. Só se sabe o
    // tamanho depois de compor, por isso a recusa vem aqui — e segue a regra da
    // casa, «recusar o novo, nunca apagar o existente»: o ficheiro composto é
    // descartado e a linha fica `failed` com a causa. Um erro de leitura da
    // quota NÃO descarta a gravação (um soluço da base não pode custar uma
    // reunião); regista-se e segue.
    match crate::usage::enforce_recording_quota(state, manifest.by_user, size).await {
        Ok(()) => {}
        Err(crate::error::ApiError::Domain(d)) if d.code == "storage.quota_exceeded" => {
            let _ = tokio::fs::remove_file(&out).await;
            anyhow::bail!("storage.quota_exceeded: a gravação não cabe na quota da organização");
        }
        Err(e) => {
            tracing::warn!(%room_id, error = ?e, "não foi possível verificar a quota; a gravação segue");
        }
    }
    let kind = crate::recordings::kind_from_room_format(&info.format);

    // A linha normalmente já existe (`insert_processing`, chamado por
    // `finalize` antes de compor — a biblioteca mostra-a "a compor" em vez de
    // ficar vazia). `rec_id: None` é só o fallback de quem chamar sem esse
    // passo: nasce aqui com os dados finais já prontos.
    let (rec_id, filename): (Uuid, String) = match rec_id {
        Some(id) => {
            sqlx::query_as("SELECT id, filename FROM recordings WHERE id = $1")
                .bind(id)
                .fetch_one(&state.db)
                .await?
        }
        None => {
            let filename = recording_filename(&info.code);
            // Mesma regra do `insert_processing`: o id vem de cá e o registo dos
            // artefactos vai na mesma transacção, senão esta porta de recuo
            // criava gravações cujos ficheiros ninguém apaga.
            let id = Uuid::new_v4();
            let mut tx = state.db.begin().await?;
            sqlx::query(
                "INSERT INTO recordings (id, room_id, uploader_id, filename, size_bytes, status, kind)
                 VALUES ($1, $2, $3, $4, 0, 'processing', $5)",
            )
            .bind(id)
            .bind(room_id)
            .bind(manifest.by_user)
            .bind(&filename)
            .bind(kind)
            .execute(&mut *tx)
            .await?;
            crate::recording_artifacts::register_canonical(
                &mut tx,
                &state.config.recordings_dir,
                id,
            )
            .await?;
            tx.commit().await?;
            (id, filename)
        }
    };

    let final_path =
        crate::recording_artifacts::composed_path(&state.config.recordings_dir, rec_id);
    // rename falha com EXDEV (errno 18) se out e final_path estiverem em
    // filesystems diferentes (e.g., tmp num volume separado). Fallback: copy+delete.
    match tokio::fs::rename(&out, &final_path).await {
        Ok(()) => {}
        Err(e) if e.raw_os_error() == Some(18) => {
            tokio::fs::copy(&out, &final_path).await?;
            let _ = tokio::fs::remove_file(&out).await;
        }
        Err(e) => return Err(e.into()),
    }
    // Só é `ready` depois de o ficheiro final ser VALIDADO (RFC-0001, B5): o
    // `ffprobe` tem de o reconhecer e achar pelo menos uma pista. Antes, o
    // `ready` precedia a medição e um ficheiro ilegível ficava «disponível».
    // `probe_and_store` grava os metadados com a linha ainda em `processing`.
    let media = crate::media_probe::probe_and_store(state, rec_id, &final_path).await;
    if media.video_codec.is_none() && media.audio_codec.is_none() {
        let _ = tokio::fs::remove_file(&final_path).await;
        anyhow::bail!(
            "o ficheiro final não foi reconhecido pelo ffprobe (sem pista de vídeo nem de áudio)"
        );
    }
    // Só é `ready` depois de o ficheiro estar no sítio final: um `ready` sem
    // ficheiro era um download partido.
    //
    // `duration_secs`/`width`/`height` (G4) vêm da estimativa feita ANTES do
    // remux/composição (relógio de parede + cabeçalho IVF/grelha) — mais
    // barata que o ffprobe abaixo (`media_probe::probe_and_store`, que mede
    // `duration_ms`/`width`/`height`/`fps`/codecs de novo, a partir do
    // ficheiro final). Gravadas as duas: os consumidores do schema simples
    // (`RecordingItem.duration_secs`) e os do schema rico (`duration_ms`)
    // ficam ambos servidos, sem um a apagar o outro.
    // A passagem a `ready` e o REGISTO das entregas do `recording.ready` numa
    // só transacção (trabalho nº3). Antes, o `fire` disparava numa tarefa à
    // parte e entre este commit e o `INSERT` da entrega havia uma janela em que
    // um SIGTERM apagava o aviso sem deixar rasto nenhum — nem entrega, nem
    // linha falhada, nem nada para reenviar na consola. Ou saem as duas, ou
    // nenhuma; o envio vem depois do commit.
    // ANTES da transacção: a consulta das organizações usa a pool, e segurar
    // uma transacção enquanto se pede outra ligação é a armadilha que a
    // `delonix-meet-backend` avisa pelo nome.
    let orgs = crate::org::orgs_of_user(state, manifest.by_user).await;
    let mut tx = state.db.begin().await?;
    sqlx::query(
        // `compose_*` a NULL: a gravação saiu da fila da composição e a
        // reserva não pode ficar a apontar para um trabalho que já acabou.
        // O manifesto também sai — os segmentos vão ser apagados a seguir e um
        // manifesto que aponta para um directório que já não existe só serviria
        // para enganar quem o lesse.
        "UPDATE recordings SET size_bytes = $2, status = 'ready', progress_pct = NULL,
                progress_at = NULL, failure_reason = NULL,
                duration_secs = $3, width = $4, height = $5,
                compose_manifest = NULL, compose_lease_token = NULL,
                compose_lease_expires_at = NULL
         WHERE id = $1",
    )
    .bind(rec_id)
    .bind(size)
    .bind(duration_secs)
    .bind(dims.map(|d| d.0))
    .bind(dims.map(|d| d.1))
    .execute(&mut *tx)
    .await?;
    tracing::info!(%room_id, %rec_id, size, "server recording pronta na biblioteca");
    let fila = crate::recordings::enqueue_recording_ready(
        orgs,
        &mut tx,
        crate::recordings::ReadyRecording {
            id: rec_id,
            uploader: manifest.by_user,
            filename: &filename,
            size,
            room_code: &info.code,
            kind,
            media: &media,
            source: "server",
        },
    )
    .await?;
    tx.commit().await?;

    // DEPOIS do commit, e por esta ordem: a notificação e o envio só fazem
    // sentido com a gravação mesmo pronta. Antes, a notificação era escrita
    // antes do commit — avisava uma pessoa de uma gravação que ainda podia não
    // ficar.
    let code: String = sqlx::query_scalar("SELECT code FROM rooms WHERE id = $1")
        .bind(room_id)
        .fetch_optional(&state.db)
        .await?
        .unwrap_or_default();
    crate::notifications::recording_ready(state, manifest.by_user, rec_id, &filename, &code).await;
    // Segurar a transacção durante um HTTP para fora seria trocar um defeito
    // por outro pior. O que falhar aqui fica `pending` e o varredor dos
    // webhooks reagenda-o.
    crate::webhooks::envia(state, "recording.ready", fila).await;
    Ok(())
}

/// Preenche com silêncio os buracos de PTS de uma pista de áudio (R295).
///
/// Quem se cala deixa de enviar (o cliente pede `usedtx=1`: um pacote a cada
/// 400 ms), e um pacote perdido também não chega. O `OggWriter` avança o
/// grânulo pelo timestamp RTP, por isso a pista fica com as amostras que
/// chegaram e o salto nos PTS — e mais nada no meio. O `adelay` e o `amix`
/// contam amostras, não PTS, e um codificador ou um `-c copy` levam o buraco
/// para o contentor, onde o Chromium o ignora: toca as amostras seguidas e a
/// fala depois de cada silêncio recua. Este filtro vai SEMPRE antes de
/// qualquer outro, em todos os caminhos.
///
/// `async=1` só enche a partir de 100 ms (`min_hard_comp`, o valor por
/// omissão): um pacote perdido isolado desloca 20 ms e só é reposto quando a
/// soma passa disso. `first_pts=0` fixa o início da pista no zero dela.
///
/// O que ele NÃO enche, e **fecha-se a montante, no `OpusGapFill`**:
///
///  - mais de 10 s sem um único pacote. O ffmpeg trata esse salto como
///    descontinuidade (`-dts_delta_threshold`, 10 s) e tira-o antes de o
///    filtro o ver. Subir o limiar não é saída: o `aresample` guarda o
///    silêncio inteiro em memória antes de o entregar (medido: 440 MB para
///    6 min, 3,2 GB para 1 h). Só se fecha a escrever o silêncio na própria
///    pista, e é o que o gravador passou a fazer;
///  - o PRIMEIRO pacote depois de um buraco ficava antes do silêncio, não
///    depois. O demuxer OGG do ffmpeg dá a cada pacote o grânulo da página
///    anterior, e o filtro enche a seguir a ele: 20 ms do início da fala
///    tocavam colados ao último pacote que chegou (até 380 ms antes, com DTX).
///
/// Este filtro CONTINUA a ir sempre à frente, e os dois dividem o trabalho:
/// o gravador põe **um** pacote de silêncio imediatamente antes de cada
/// retoma — é isso que acerta o grânulo —, e mais um de 5 em 5 s dentro de um
/// salto grande; **o silêncio do buraco em si continua a ser deste filtro**.
/// Um buraco de 400 ms do DTX leva uma página de 20 ms do gravador e os
/// restantes 380 ms vêm daqui.
///
/// (A primeira versão deste comentário dizia que os buracos pequenos não
/// levavam nada do gravador. Dois testes do gravador mostraram que levam: há
/// silêncio onde cabe um `OPUS_SILENCE` inteiro, e 20 ms cabem.)
const AUDIO_GAP_FILL: &str = "aresample=async=1:first_pts=0";

/// O Opus de qualquer áudio que saia do ffmpeg recodificado.
const OPUS_ARGS: [&str; 6] = ["-c:a", "libopus", "-b:a", "128k", "-ar", "48000"];

/// Os argumentos de saída do caminho de um publicador (entrada 0 é o vídeo, a
/// 1 o áudio, se houver). O vídeo vai em cópia, ou reduzido a `downscale_to`
/// linhas; o áudio é SEMPRE recodificado depois do `AUDIO_GAP_FILL` — em
/// cópia, os silêncios ficavam como buracos no contentor.
fn single_publisher_args(has_audio: bool, downscale_to: Option<u32>) -> Vec<String> {
    let mut args: Vec<String> = vec!["-map".into(), "0:v:0".into()];
    if has_audio {
        args.extend(["-map".into(), "1:a:0".into()]);
    }
    match downscale_to {
        Some(h) => {
            args.extend(["-vf".into(), format!("scale=-2:{h}")]);
            args.extend(VP9_ARGS.iter().map(|a| a.to_string()));
        }
        None => args.extend(["-c:v".into(), "copy".into()]),
    }
    if has_audio {
        args.extend(["-af".into(), AUDIO_GAP_FILL.into()]);
        args.extend(OPUS_ARGS.iter().map(|a| a.to_string()));
    }
    args
}

/// Os argumentos do ffmpeg que compõem a gravação: as entradas (cada pista no
/// seu instante), os filtros e os codecs — tudo menos o ficheiro de saída.
/// `videos` e `audios` são as pistas com conteúdo, pela ordem da sessão.
///
/// Um vídeo com um áudio no máximo: o vídeo vai em cópia (ou reduzido a
/// `downscale_to` linhas). Mais do que isso: grelha e mistura.
fn compose_args(
    videos: &[&ComposeTrack],
    audios: &[&ComposeTrack],
    quality: Option<&str>,
    downscale_to: Option<u32>,
) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = Vec::new();
    let mut input = |shift_ms: u64, path: &Path| {
        args.extend(input_shift(shift_ms).into_iter().map(Into::into));
        args.push("-i".into());
        args.push(path.into());
    };
    if videos.len() == 1 && audios.len() <= 1 {
        // Caso simples: o vídeo vai em cópia, sem reencode; o áudio não (R295).
        // Cada pista entra no seu instante: a que começou depois leva a
        // diferença em `-itsoffset`, que vale também para o vídeo em cópia.
        let (video_ms, audio_ms) = single_publisher_shifts(
            videos[0].starts_at_ms,
            audios.first().map(|a| a.starts_at_ms),
        );
        input(video_ms, &videos[0].path);
        if let Some(a) = audios.first() {
            input(audio_ms, &a.path);
        }
        args.extend(
            single_publisher_args(!audios.is_empty(), downscale_to)
                .into_iter()
                .map(Into::into),
        );
        return args;
    }
    // Composição em grelha + mistura de áudio (VP9 CRF 30 + Opus 128k).
    for t in videos.iter().chain(audios) {
        input(0, &t.path);
    }
    let n = videos.len();
    let cols = (n as f64).sqrt().ceil() as usize;
    let (tw, th) = grid_tile(quality, n);
    let mut fc = String::new();
    for (i, v) in videos.iter().enumerate() {
        fc.push_str(&grid_cell_chain(i, tw, th, v.starts_at_ms));
    }
    let vout = if n > 1 {
        let layout = (0..n)
            .map(|i| format!("{}_{}", (i % cols) * tw as usize, (i / cols) * th as usize))
            .collect::<Vec<_>>()
            .join("|");
        let ins = (0..n).map(|i| format!("[v{i}]")).collect::<String>();
        fc.push_str(&format!(
            "{ins}xstack=inputs={n}:layout={layout}:fill=black[vout];"
        ));
        "[vout]"
    } else if n == 1 {
        "[v0]"
    } else {
        ""
    };
    let starts: Vec<u64> = audios.iter().map(|a| a.starts_at_ms).collect();
    let (afc, aout) = audio_mix_graph(n, &starts);
    fc.push_str(&afc);
    args.push("-filter_complex".into());
    args.push(fc.trim_end_matches(';').into());
    if !vout.is_empty() {
        args.extend(["-map", vout].map(Into::into));
        args.extend(VP9_ARGS.map(Into::into));
    }
    if !aout.is_empty() {
        args.push("-map".into());
        args.push(aout.into());
        args.extend(OPUS_ARGS.map(Into::into));
    }
    args
}

/// Quanto uma pista de áudio entra ADIANTADA na composição, em ms.
///
/// O `OggWriter` declara 3840 amostras de `pre-skip` no cabeçalho de cada
/// pista, e o ffmpeg desconta-as ao início: os primeiros 80 ms de som são
/// deitados fora e tudo o resto recua 80 ms. Todo o instante de áudio que a
/// composição usa leva este acerto — sem ele o som chega 80 ms antes da
/// imagem, e acertar só o lado do vídeo deixava a gravação PIOR do que estava
/// (um avanço do som nota-se a partir de 45 ms; um atraso, só aos 125).
const OGG_PRE_SKIP_MS: u64 = 80;

/// A cadência da grelha. Cada vídeo é posto nesta cadência antes de entrar no
/// `xstack` (ver `grid_cell_chain`).
const GRID_FPS: u64 = 30;

/// Quanto se atrasa cada entrada no caminho de um publicador, em ms: `(vídeo,
/// áudio)`. Recebe o instante em que cada pista começa (`starts_at_ms`); a que
/// começa primeiro fica no zero do ficheiro e a outra leva a diferença. O
/// áudio conta com o `OGG_PRE_SKIP_MS`.
fn single_publisher_shifts(video_at_ms: u64, audio_at_ms: Option<u64>) -> (u64, u64) {
    let Some(audio_at_ms) = audio_at_ms.map(|ms| ms + OGG_PRE_SKIP_MS) else {
        return (0, 0);
    };
    let zero = video_at_ms.min(audio_at_ms);
    (video_at_ms - zero, audio_at_ms - zero)
}

/// Os argumentos que atrasam `ms` a entrada do ffmpeg que vier a seguir.
///
/// É `-itsoffset` e não um PTS inicial diferente de zero na pista: o ffmpeg
/// põe cada entrada a começar no zero dela (desconta-lhe o `start_time`), em
/// cópia e em filtros — medido no 6.1.1, um IVF com o primeiro quadro aos
/// 400 ms sai com ele aos 0 ms.
fn input_shift(ms: u64) -> Vec<String> {
    if ms == 0 {
        return Vec::new();
    }
    vec!["-itsoffset".into(), format!("{:.3}", ms as f64 / 1000.0)]
}

/// A cadeia de uma célula da grelha: o vídeo da entrada `i`, numa célula de
/// `tw`×`th`, a começar aos `at_ms` da gravação (`starts_at_ms`), com preto
/// até lá.
///
/// **`fps` antes de tudo, e o preto contado em QUADROS.** Duas coisas medidas
/// com pistas a sério (ffmpeg 6.1.1), que o `tpad=start_duration` sobre a pista
/// tal como vem fazia mal:
/// - o `tpad` gera o preto à cadência que o ffmpeg ADIVINHA para a pista e
///   avança cada quadro um passo arredondado à base de tempo dela (1/1000): a
///   30 fps são 33 ms em vez de 33,33, e a imagem ficava adiantada 1 % do
///   offset — 0,3 s para quem ligou a câmara aos 30 s de gravação, 6 s aos 10
///   minutos;
/// - o `xstack` dá um quadro por cada quadro de cada entrada, a instantes que
///   não coincidem, e o codificador (cadência fixa) empurrava os que caíam na
///   mesma casa para a seguinte: a imagem da grelha saía 30 a 70 ms atrasada.
///
/// Com todas as entradas na mesma cadência, o preto são `n` quadros exactos, o
/// `xstack` dá um quadro por casa, e o erro de cada célula é o arredondamento
/// do `fps`: meio quadro, 17 ms. O que sobra do instante depois de tirar os
/// quadros inteiros (menos de um quadro) vai no `setpts`, antes do `fps`.
fn grid_cell_chain(i: usize, tw: u32, th: u32, at_ms: u64) -> String {
    let frames = at_ms * GRID_FPS / 1000;
    let rest_s = at_ms as f64 / 1000.0 - frames as f64 / GRID_FPS as f64;
    format!(
        "[{i}:v]setpts=PTS+{rest_s:.4}/TB,fps={GRID_FPS},\
         scale={tw}:{th}:force_original_aspect_ratio=decrease,\
         pad={tw}:{th}:(ow-iw)/2:(oh-ih)/2,setsar=1,\
         tpad=start={frames}:start_mode=add:color=black[v{i}];"
    )
}

/// A parte de áudio do `-filter_complex` da composição: cada pista é enchida
/// (`AUDIO_GAP_FILL`), atrasada até ao instante em que começa (`starts_ms`, o
/// `starts_at_ms` de cada uma, mais o `OGG_PRE_SKIP_MS`) e, havendo mais de
/// uma, misturam-se. `first_input` é o índice da primeira entrada de áudio do
/// ffmpeg (as de vídeo vêm antes). Devolve o grafo (cada cadeia acabada em
/// `;`) e o rótulo a mapear — vazios sem áudio.
fn audio_mix_graph(first_input: usize, starts_ms: &[u64]) -> (String, String) {
    let mut fc = String::new();
    if starts_ms.is_empty() {
        return (fc, String::new());
    }
    for (j, at) in starts_ms.iter().enumerate() {
        let idx = first_input + j;
        let ms = at + OGG_PRE_SKIP_MS;
        fc.push_str(&format!(
            "[{idx}:a]{AUDIO_GAP_FILL},adelay={ms}:all=1[a{j}];"
        ));
    }
    if starts_ms.len() == 1 {
        return (fc, "[a0]".into());
    }
    let ins = (0..starts_ms.len())
        .map(|j| format!("[a{j}]"))
        .collect::<String>();
    fc.push_str(&format!(
        "{ins}amix=inputs={}:normalize=0[aout];",
        starts_ms.len()
    ));
    (fc, "[aout]".into())
}

/// Dimensões da grelha que o `xstack` compõe: células de 640×360, `ceil(√n)`
/// colunas. `None` sem vídeo.
fn grid_dims(videos: usize) -> Option<(i32, i32)> {
    if videos == 0 {
        return None;
    }
    let cols = (videos as f64).sqrt().ceil() as usize;
    let rows = videos.div_ceil(cols);
    Some(((cols * 640) as i32, (rows * 360) as i32))
}

/// Largura e altura do cabeçalho IVF (bytes 12..16, LE). Um ficheiro ilegível
/// dá `None` — a gravação não falha por causa de um metadado.
async fn ivf_dims(path: &Path) -> Option<(i32, i32)> {
    use tokio::io::AsyncReadExt;
    let mut f = tokio::fs::File::open(path).await.ok()?;
    let mut h = [0u8; 16];
    f.read_exact(&mut h).await.ok()?;
    if &h[0..4] != b"DKIF" {
        return None;
    }
    let w = u16::from_le_bytes([h[12], h[13]]) as i32;
    let hh = u16::from_le_bytes([h[14], h[15]]) as i32;
    (w > 0 && hh > 0).then_some((w, hh))
}

/// Codificação VP9 da composição (CRF 30, sem tecto de débito).
const VP9_ARGS: [&str; 14] = [
    "-c:v",
    "libvpx-vp9",
    "-b:v",
    "0",
    "-crf",
    "30",
    "-deadline",
    "good",
    "-cpu-used",
    "4",
    "-row-mt",
    "1",
    "-pix_fmt",
    "yuv420p",
];

/// Cron de retenção (DLP-lite): apaga gravações mais antigas que
/// `organizations.retention_days` (>0) de cada org, ficheiro + registo.
pub async fn retention_sweep(state: &Arc<AppState>) -> usize {
    let rows: Vec<(Uuid, String)> = match sqlx::query_as(
        "SELECT r.id, r.filename FROM recordings r
         JOIN org_members m ON m.user_id = r.uploader_id
         JOIN organizations o ON o.id = m.org_id
         WHERE o.retention_days > 0
           AND r.created_at < now() - make_interval(days => o.retention_days)",
    )
    .fetch_all(&state.db)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "retention query failed");
            return 0;
        }
    };
    let mut n = 0;
    for (id, _fname) in rows {
        // UM caminho, e ele percorre o REGISTO (`recording_artifacts`, migração
        // 0126). Aqui estavam três `let _ = remove_file(...)` à mão — a
        // composta, a miniatura e a pasta das pistas — e o quarto derivado
        // ficou esquecido: os `clip-<id>.webm`/`.vtt` da 0118 nunca eram
        // apagados por ninguém. A lista do que uma gravação tem no disco deixa
        // de viver na memória de quem escreveu este ciclo.
        //
        // E a purga é uma CONDIÇÃO, não um passo: `None` é «a base não disse o
        // que esta gravação tem no disco». Apagar a linha nesse caso era perder
        // para sempre o único caminho que volta a esta gravação — a retenção
        // procura por `recordings`, logo sem a linha nunca mais cá volta, e o
        // disco ficava à espera do varredor de órfãos. Falha FECHADO: a
        // gravação fica, e a próxima volta tenta outra vez.
        if crate::recording_artifacts::purge(state, id).await.is_none() {
            tracing::warn!(%id,
                "retenção: o registo de artefactos não respondeu — a gravação fica para a próxima volta");
            continue;
        }
        if sqlx::query("DELETE FROM recordings WHERE id = $1")
            .bind(id)
            .execute(&state.db)
            .await
            .is_ok()
        {
            n += 1;
        }
    }
    if n > 0 {
        tracing::info!(deleted = n, "retention sweep");
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // ------------------------------------------------------------------
    //  Falha de gravação: causa legível, nunca o erro cru
    // ------------------------------------------------------------------

    #[test]
    fn a_causa_e_traduzida_e_nao_o_erro_cru() {
        // O stderr do ffmpeg traz caminhos do servidor e nomes de ficheiros
        // temporários — nada disso tem que fazer no ecrã de um utilizador.
        let e = anyhow::anyhow!(
            "ffmpeg exited with exit status: 183 (/tmp/dlx-recordings/tmp-9f2a/01-video.ivf)"
        );
        let causa = causa_legivel(&e);
        assert!(
            !causa.contains("/tmp"),
            "vazou um caminho do servidor: {causa}"
        );
        assert!(!causa.contains("183"), "vazou um código interno: {causa}");
        assert!(
            causa.contains("compor o vídeo"),
            "e tem de dizer o que se passou: {causa}"
        );
    }

    #[test]
    fn cada_falha_conhecida_tem_a_sua_explicacao() {
        // Um «falhou» genérico não ajuda ninguém a decidir o que fazer a
        // seguir. Cada causa que sabemos distinguir diz o que fazer.
        let casos = [
            (
                "ffmpeg-ausente: não foi encontrado no PATH do servidor",
                "não tem o ffmpeg instalado",
            ),
            ("nothing recorded", "parada demasiado cedo"),
            ("processo excedeu 3600s e foi terminado", "tempo máximo"),
            ("No space left on device", "espaço em disco"),
            ("storage.quota_exceeded: não cabe", "quota de armazenamento"),
        ];
        for (erro, esperado) in casos {
            let c = causa_legivel(&anyhow::anyhow!(erro.to_string()));
            assert!(
                c.contains(esperado),
                "para {erro:?} esperava mencionar {esperado:?}, deu {c:?}"
            );
        }
    }

    #[test]
    fn um_erro_desconhecido_nao_fica_sem_causa() {
        // Fail-safe: um erro que não sabemos classificar tem de dar na mesma
        // uma linha visível, não uma string vazia nem um panic.
        let c = causa_legivel(&anyhow::anyhow!("algo que nunca vimos"));
        assert!(!c.is_empty());
        assert!(
            c.contains("registo"),
            "e tem de dizer onde está o detalhe: {c}"
        );
    }

    // ------------------------------------------------------------------
    //  Formato E2EE: o Rust tem de decifrar o que o browser cifra
    // ------------------------------------------------------------------
    //
    // O cifrador vive em JavaScript (`web/src/e2ee.ts`, dentro de um worker) e
    // o decifrador em Rust (`decrypt_e2ee`). São duas implementações do MESMO
    // formato, em linguagens diferentes, sem nada que as obrigue a concordar.
    // Se divergirem, as gravações de salas E2EE saem em RUÍDO — e ninguém dá
    // por isso, porque o `Vp8IvfWriter` limita-se a descartar o que não
    // autentica e o ficheiro sai vazio ou truncado, sem erro nenhum.
    //
    // Estes testes reconstroem em Rust, byte a byte, o que o worker produz:
    //     [ header em claro | ciphertext+tag | IV(12) ]   AAD = header

    /// Cifra como o worker do browser cifra. Se este helper e o `e2ee.ts`
    /// divergirem, é sinal de que o formato mudou de um lado só.
    fn cifra_como_o_browser(
        key: &Aes256Gcm,
        header: &[u8],
        payload: &[u8],
        iv: &[u8; 12],
    ) -> Vec<u8> {
        use aes_gcm::aead::Aead;
        let nonce = aes_gcm::Nonce::try_from(&iv[..]).expect("nonce de 12 bytes");
        let ct = key
            .encrypt(
                &nonce,
                Payload {
                    msg: payload,
                    aad: header,
                },
            )
            .expect("cifrar");
        let mut out = Vec::with_capacity(header.len() + ct.len() + 12);
        out.extend_from_slice(header);
        out.extend_from_slice(&ct);
        out.extend_from_slice(iv);
        out
    }

    fn chave_de_teste(b: u8) -> Aes256Gcm {
        use aes_gcm::KeyInit;
        Aes256Gcm::new_from_slice(&[b; 32]).unwrap()
    }

    #[test]
    fn decifra_o_que_o_browser_cifrou_em_video_e_audio() {
        let key = chave_de_teste(7);
        // Os três offsets que o `cryptoOffset` do worker produz:
        // vídeo keyframe = 10, vídeo delta = 3, áudio = 1.
        for offset in [10usize, 3, 1] {
            let header: Vec<u8> = (0..offset as u8).collect();
            let payload: Vec<u8> = (0..200u8).collect();
            let frame = cifra_como_o_browser(&key, &header, &payload, &[9u8; 12]);

            let claro = decrypt_e2ee(&key, &frame, offset).expect("tem de autenticar");
            assert_eq!(&claro[..offset], &header[..], "o header sai intacto");
            assert_eq!(
                &claro[offset..],
                &payload[..],
                "o payload sai igual ao original"
            );
        }
    }

    #[test]
    fn o_header_vai_autenticado_nao_so_em_claro() {
        // O header fica legível de propósito (os packetizers precisam dele),
        // mas entra como AAD. Adulterá-lo tem de fazer a autenticação falhar —
        // senão um intermediário podia reescrever metadados de frame à vontade.
        let key = chave_de_teste(7);
        let header = vec![0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        let payload: Vec<u8> = (0..64u8).collect();
        let mut frame = cifra_como_o_browser(&key, &header, &payload, &[1u8; 12]);

        frame[2] ^= 0xff; // um bit trocado no header
        assert!(
            decrypt_e2ee(&key, &frame, 10).is_none(),
            "header adulterado TEM de falhar a autenticação"
        );
    }

    #[test]
    fn chave_errada_nao_decifra() {
        let header = vec![0u8, 1, 2];
        let payload: Vec<u8> = (0..64u8).collect();
        let frame = cifra_como_o_browser(&chave_de_teste(7), &header, &payload, &[2u8; 12]);
        assert!(decrypt_e2ee(&chave_de_teste(8), &frame, 3).is_none());
    }

    #[test]
    fn ciphertext_adulterado_nao_decifra() {
        let key = chave_de_teste(7);
        let header = vec![0u8, 1, 2];
        let payload: Vec<u8> = (0..64u8).collect();
        let mut frame = cifra_como_o_browser(&key, &header, &payload, &[3u8; 12]);
        let meio = frame.len() / 2;
        frame[meio] ^= 0x01;
        assert!(decrypt_e2ee(&key, &frame, 3).is_none());
    }

    #[test]
    fn iv_trocado_nao_decifra() {
        // O IV vai no FIM do frame, em claro. Trocá-lo tem de invalidar a tag.
        let key = chave_de_teste(7);
        let header = vec![0u8, 1, 2];
        let payload: Vec<u8> = (0..64u8).collect();
        let mut frame = cifra_como_o_browser(&key, &header, &payload, &[4u8; 12]);
        let n = frame.len();
        frame[n - 1] ^= 0xff;
        assert!(decrypt_e2ee(&key, &frame, 3).is_none());
    }

    #[test]
    fn frames_pequenos_demais_passam_intactos_dos_dois_lados() {
        // Abaixo de header+IV+tag não pode haver payload cifrado. O worker
        // também não os cifra — o importante é que as duas implementações
        // concordem no MESMO limiar, senão uma cifra e a outra não decifra.
        let key = chave_de_teste(7);
        let curto = vec![1u8, 2, 3, 4, 5];
        assert_eq!(decrypt_e2ee(&key, &curto, 3).unwrap(), curto);
    }

    // ------------------------------------------------------------------
    //  Integridade da gravação: a fila de escrita e o fecho
    // ------------------------------------------------------------------
    //
    // A escrita saiu do executor do Tokio para uma thread dedicada. Isso resolve
    // o bloqueio, mas abre a porta ao pior defeito possível numa gravação:
    // fechar o ficheiro ANTES de a fila estar esvaziada dá um vídeo truncado, sem
    // um único erro pelo caminho, e o ffmpeg compõe-no na mesma. É a família da
    // R18 — corrupção silenciosa. Estes testes existem para isso.

    /// Um pacote VP8 mínimo que o `Vp8IvfWriter` aceita como frame completo.
    ///
    /// `0x10` no descritor VP8 = início de partição (bit S). No payload, bit 0
    /// a zero = keyframe. `marker` fecha o frame.
    fn vp8_keyframe(seq: u16, ts: u32) -> webrtc::rtp::packet::Packet {
        let mut header = webrtc::rtp::header::Header {
            sequence_number: seq,
            timestamp: ts,
            marker: true,
            ..Default::default()
        };
        header.payload_type = 96;
        // descritor (S=1) + payload VP8 com bit0=0 e a assinatura de keyframe
        let payload: Vec<u8> = vec![
            0x10, // descritor VP8: S=1
            0x00, 0x00, 0x00, // tag do frame (bit0=0 ⇒ keyframe)
            0x9d, 0x01, 0x2a, // sync de keyframe
            0x40, 0x01, 0xf0, 0x00, // 320x240
            0xde, 0xad, 0xbe, 0xef,
        ];
        webrtc::rtp::packet::Packet {
            header,
            payload: payload.into(),
        }
    }

    /// Lê o contador de frames do cabeçalho IVF (u32 LE no offset 24).
    fn ivf_frame_count(path: &std::path::Path) -> u32 {
        let bytes = std::fs::read(path).expect("ficheiro de gravação");
        assert!(
            bytes.len() >= 32,
            "cabeçalho IVF incompleto: {} bytes",
            bytes.len()
        );
        assert_eq!(&bytes[0..4], b"DKIF", "não é um ficheiro IVF");
        u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]])
    }

    fn writer_de_teste(dir: &std::path::Path, cap: usize) -> (RecWriter, std::path::PathBuf) {
        let path = dir.join("teste.ivf");
        let file = std::fs::File::create(&path).unwrap();
        let sink = RecSink::Video(Vp8IvfWriter::new(file).unwrap());
        let w = RecWriter::spawn(
            sink,
            cap,
            Arc::new(crate::metrics::Metrics::default()),
            "teste".into(),
            FirstPacket::new(Instant::now()),
        );
        (w, path)
    }

    #[tokio::test]
    async fn close_espera_a_fila_esvaziar_ate_ao_ultimo_pacote() {
        let dir = std::env::temp_dir().join(format!("dlx-rec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (w, path) = writer_de_teste(&dir, 4096);

        const N: u16 = 300;
        for i in 0..N {
            w.write_rtp(&vp8_keyframe(i, i as u32 * 3000));
        }
        // Fecha IMEDIATAMENTE a seguir a enfileirar: a thread quase de certeza
        // ainda tem pacotes por escrever neste instante. Se o `close` não
        // esperasse, o ficheiro sairia truncado — e ninguém daria por isso.
        let perdidos = w.close().await;

        assert_eq!(perdidos, 0, "com fila folgada não se perde nada");
        assert_eq!(
            ivf_frame_count(&path),
            N as u32,
            "o ficheiro tem de conter TODOS os frames enfileirados antes do fecho"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn o_cabecalho_ivf_e_corrigido_no_fecho() {
        // O contador de frames e as dimensões só se sabem no fim: o `close`
        // volta atrás no ficheiro (`seek`) para os escrever. Se o `BufWriter`
        // não esvaziasse antes do `seek`, o cabeçalho ficaria por corrigir.
        let dir = std::env::temp_dir().join(format!("dlx-rec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (w, path) = writer_de_teste(&dir, 256);
        for i in 0..10u16 {
            w.write_rtp(&vp8_keyframe(i, i as u32 * 3000));
        }
        w.close().await;

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(ivf_frame_count(&path), 10);
        // Dimensões reais lidas do keyframe (320x240), não as nominais 1280x720.
        let w_px = u16::from_le_bytes([bytes[12], bytes[13]]);
        let h_px = u16::from_le_bytes([bytes[14], bytes[15]]);
        assert_eq!((w_px, h_px), (320, 240));
        // E é isso que a biblioteca recebe (G4).
        assert_eq!(ivf_dims(&path).await, Some((320, 240)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ------------------------------------------------------------------
    //  A pista de vídeo só abre num keyframe verdadeiro (R299)
    // ------------------------------------------------------------------
    //
    // Uma gravação arrancada a meio da chamada liga o writer a um fluxo que já
    // vai em quadros delta. O bit de keyframe só existe no pacote que INICIA o
    // quadro; a guarda antiga lia-o em qualquer pacote, e uma continuação com o
    // primeiro byte par (metade delas) abria a pista a meio de um quadro delta
    // — três pistas em três que o ffmpeg não descodificava.

    /// Um pacote VP8 qualquer: `descritor` é o byte do descritor do payload
    /// (`0x10` = S, início de partição; os 3 bits baixos são o PID) e
    /// `primeiro` o primeiro byte do que vem a seguir.
    fn vp8_pacote(
        seq: u16,
        ts: u32,
        descritor: u8,
        primeiro: u8,
        marker: bool,
    ) -> webrtc::rtp::packet::Packet {
        let header = webrtc::rtp::header::Header {
            sequence_number: seq,
            timestamp: ts,
            marker,
            payload_type: 96,
            ..Default::default()
        };
        webrtc::rtp::packet::Packet {
            header,
            payload: vec![descritor, primeiro, 0x00, 0x00, 0xaa, 0xbb, 0xcc, 0xdd].into(),
        }
    }

    /// Um quadro DELTA em três pacotes, como o Chromium os manda: o início
    /// (S=1, cabeçalho com o bit 0 aceso) e duas continuações cujo primeiro
    /// byte é PAR — dado comprimido que a guarda antiga lia como «keyframe».
    fn vp8_quadro_delta(seq: &mut u16, ts: u32) -> Vec<webrtc::rtp::packet::Packet> {
        let mut pacotes = Vec::new();
        for (descritor, primeiro, marker) in
            [(0x10, 0x11, false), (0x00, 0x00, false), (0x00, 0x42, true)]
        {
            pacotes.push(vp8_pacote(*seq, ts, descritor, primeiro, marker));
            *seq = seq.wrapping_add(1);
        }
        pacotes
    }

    /// Os quadros de um IVF, cada um com os seus bytes.
    fn ivf_quadros(path: &std::path::Path) -> Vec<Vec<u8>> {
        let b = std::fs::read(path).expect("ficheiro de gravação");
        let mut quadros = Vec::new();
        let mut i = 32;
        while i + 12 <= b.len() {
            let n = u32::from_le_bytes(b[i..i + 4].try_into().unwrap()) as usize;
            quadros.push(b[i + 12..i + 12 + n].to_vec());
            i += 12 + n;
        }
        quadros
    }

    fn e_keyframe(quadro: &[u8]) -> bool {
        quadro.len() >= 10 && quadro[0] & 0x01 == 0 && quadro[3..6] == [0x9d, 0x01, 0x2a]
    }

    #[tokio::test]
    async fn continuacoes_com_byte_par_nao_abrem_a_pista() {
        let dir = std::env::temp_dir().join(format!("dlx-rec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (w, path) = writer_de_teste(&dir, 4096);
        assert!(w.wants_keyframe(), "uma pista de vídeo nasce à espera");

        // A chamada já decorre: só chegam quadros delta.
        let mut seq = 0u16;
        for q in 0..30u32 {
            for p in vp8_quadro_delta(&mut seq, q * 3000) {
                w.write_rtp(&p);
            }
        }
        // E uma cabeça de partição que NÃO é o início do quadro (S=1, PID=1),
        // com byte par: também não é um keyframe.
        w.write_rtp(&vp8_pacote(seq, 30 * 3000, 0x11, 0x00, true));
        seq += 1;
        // E uma continuação cujos bytes imitam o cabeçalho de um keyframe
        // (bit 0 a zero e `9d 01 2a`), mas sem o bit S: é dado, não cabeçalho.
        let mut imitacao = vp8_keyframe(seq, 30 * 3000);
        imitacao.payload = {
            let mut p = imitacao.payload.to_vec();
            p[0] = 0x00;
            p.into()
        };
        w.write_rtp(&imitacao);
        seq += 1;
        assert!(
            w.wants_keyframe(),
            "sem keyframe a pista continua à espera — é isto que faz o SFU pedi-lo"
        );

        // O keyframe pedido chega, e a chamada continua em deltas.
        w.write_rtp(&vp8_keyframe(seq, 31 * 3000));
        seq += 1;
        assert!(
            !w.wants_keyframe(),
            "com o keyframe na fila já não se pede outro"
        );
        for q in 32..37u32 {
            for p in vp8_quadro_delta(&mut seq, q * 3000) {
                w.write_rtp(&p);
            }
        }
        w.close().await;

        let quadros = ivf_quadros(&path);
        assert_eq!(
            quadros.len(),
            6,
            "o keyframe e os cinco quadros depois dele — nada do que veio antes"
        );
        assert!(
            e_keyframe(&quadros[0]),
            "o primeiro quadro da pista tem de ser o keyframe: {:02x?}",
            &quadros[0][..quadros[0].len().min(10)]
        );
        assert!(
            quadros[1..]
                .iter()
                .all(|q| q[0] & 0x01 == 1 && q.len() == 21),
            "e depois dele os quadros delta, inteiros (3 pacotes de 7 bytes)"
        );
        // O cabeçalho deixou de levar as dimensões nominais (1280×720), que
        // eram a marca de uma pista que nunca viu um keyframe.
        assert_eq!(ivf_dims(&path).await, Some((320, 240)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Espera que a thread de escrita chegue ao que já lhe foi entregue.
    async fn ate_que(label: &str, mut cond: impl FnMut() -> bool) {
        for _ in 0..500 {
            if cond() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("ao fim de 5 s: {label}");
    }

    #[tokio::test]
    async fn um_keyframe_de_abertura_incompleto_nao_abre_a_pista() {
        // O primeiro pacote do keyframe perdeu-se e foi retransmitido: chega
        // sozinho, a meio de um quadro delta. Tem o bit, o código de início e
        // as dimensões — e o que se lhe segue não é dele. Escrito, era um
        // primeiro quadro com cara de keyframe e conteúdo de lixo, e ninguém
        // voltava a pedir outro.
        let dir = std::env::temp_dir().join(format!("dlx-rec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (w, path) = writer_de_teste(&dir, 4096);

        let mut inicio = vp8_keyframe(100, 3000);
        inicio.header.marker = false; // o keyframe tem mais pacotes
        w.write_rtp(&inicio);
        assert!(
            !w.wants_keyframe(),
            "com o início do keyframe na fila, quem entrega dá-o por recebido"
        );
        // …mas o pacote seguinte não é a continuação dele (salto na sequência).
        w.write_rtp(&vp8_pacote(140, 6000, 0x00, 0x42, true));
        ate_que("a pista volta a esperar pelo keyframe", || {
            w.wants_keyframe()
        })
        .await;

        // O keyframe pedido outra vez chega inteiro, em dois pacotes seguidos.
        let mut inicio = vp8_keyframe(200, 9000);
        inicio.header.marker = false;
        w.write_rtp(&inicio);
        w.write_rtp(&vp8_pacote(201, 9000, 0x00, 0x42, true));
        let mut seq = 202u16;
        for q in 4..7u32 {
            for p in vp8_quadro_delta(&mut seq, q * 3000) {
                w.write_rtp(&p);
            }
        }
        ate_que("a pista abriu", || !w.wants_keyframe()).await;
        w.close().await;

        let quadros = ivf_quadros(&path);
        assert_eq!(quadros.len(), 4, "o keyframe inteiro e três deltas");
        assert!(e_keyframe(&quadros[0]));
        assert_eq!(
            quadros[0].len(),
            14 + 7,
            "o keyframe de abertura são os seus dois pacotes, e só eles"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn os_pedidos_de_keyframe_tem_fim() {
        // Um publicador que nunca responde não leva um PLI por segundo para
        // sempre (R14): passado o prazo deixa-se de pedir. A pista continua a
        // abrir se o keyframe vier por outra via.
        let dir = std::env::temp_dir().join(format!("dlx-rec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (mut w, path) = writer_de_teste(&dir, 4096);
        assert!(w.wants_keyframe());
        w.ask_until = Instant::now(); // o prazo (`KEYFRAME_ASK_FOR`) passou
        assert!(!w.wants_keyframe(), "passado o prazo já não se pede");
        assert!(!w.wants_keyframe(), "nem na pergunta seguinte");

        w.write_rtp(&vp8_keyframe(1, 3000));
        w.close().await;
        assert_eq!(ivf_quadros(&path).len(), 1, "e a pista abre na mesma");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn so_o_video_espera_por_keyframe() {
        let dir = std::env::temp_dir().join(format!("dlx-rec-{}", uuid::Uuid::new_v4()));
        let mut session = RecordingSession::new(uuid::Uuid::new_v4(), "teste".into(), None, &dir)
            .await
            .unwrap();
        let metrics = Arc::new(crate::metrics::Metrics::default());
        // Publicadores DISTINTOS de propósito: a gravação ISO existe para
        // separar fontes, e um teste que lhes dê o mesmo id não veria a troca.
        let (p_audio, p_video, p_ecra) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let audio = session
            .open_track("audio", p_audio, 64, metrics.clone())
            .unwrap();
        let video = session
            .open_track("video", p_video, 64, metrics.clone())
            .unwrap();
        let ecra = session.open_track("screen", p_ecra, 64, metrics).unwrap();
        assert_eq!(session.tracks[0].publisher, p_audio);
        assert_eq!(session.tracks[1].publisher, p_video);
        assert_eq!(session.tracks[2].publisher, p_ecra);
        assert!(!audio.wants_keyframe(), "o áudio não tem keyframes");
        assert!(video.wants_keyframe());
        assert!(ecra.wants_keyframe(), "a partilha de ecrã é vídeo");
        for w in [audio, video, ecra] {
            w.close().await;
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_resolucao_da_grelha_segue_o_xstack() {
        assert_eq!(grid_dims(0), None);
        assert_eq!(grid_dims(1), Some((640, 360)));
        assert_eq!(grid_dims(2), Some((1280, 360)), "2 colunas, 1 linha");
        assert_eq!(grid_dims(3), Some((1280, 720)), "2 colunas, 2 linhas");
        assert_eq!(grid_dims(5), Some((1920, 720)), "3 colunas, 2 linhas");
    }

    #[tokio::test]
    async fn fila_cheia_perde_pacotes_mas_conta_os() {
        // Perder pacotes é aceitável; perdê-los EM SILÊNCIO não é.
        // Um disco que não acompanha tem de ser VISÍVEL. A alternativa —
        // bloquear o executor até ele alcançar — é pior, mas perder em silêncio
        // é o pior de todos: dá um ficheiro que parece bom e não é.
        let dir = std::env::temp_dir().join(format!("dlx-rec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (w, path) = writer_de_teste(&dir, 1);

        for i in 0..5_000u16 {
            w.write_rtp(&vp8_keyframe(i, i as u32 * 3000));
        }
        let perdidos = w.close().await;

        assert!(
            perdidos > 0,
            "com fila de 1 e 5000 pacotes, tem de haver perdas"
        );
        // E o que passou continua a ser um ficheiro válido — degradado, não corrompido.
        let escritos = ivf_frame_count(&path);
        assert!(escritos > 0);
        assert_eq!(
            escritos as u64 + perdidos,
            5_000,
            "todo o pacote ou foi escrito ou foi contado"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn write_rtp_nunca_bloqueia_quem_o_chama() {
        // É a razão de existir de toda esta mudança: o `write_rtp` corre dentro
        // da task async que reencaminha RTP. Se bloqueasse, prendia um worker do
        // Tokio — e um worker preso não serve só esta gravação, serve todas as
        // salas que calharem naquela thread.
        let dir = std::env::temp_dir().join(format!("dlx-rec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (w, _path) = writer_de_teste(&dir, 1);

        let inicio = std::time::Instant::now();
        for i in 0..20_000u16 {
            w.write_rtp(&vp8_keyframe(i, i as u32 * 3000));
        }
        let decorrido = inicio.elapsed();
        assert!(
            decorrido < Duration::from_secs(5),
            "20 000 escritas com a fila cheia demoraram {decorrido:?} — está a bloquear"
        );
        w.close().await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ------------------------------------------------------------------
    //  A ordem e o relógio de uma pista de áudio (`OpusTrack`)
    // ------------------------------------------------------------------
    //
    // O `OggWriter` avança a posição do grânulo com `timestamp - anterior`, uma
    // subtracção de `u32` sem `wrapping`. Um pacote com o timestamp para trás
    // (reordenação na rede, ou um duplicado) soma perto de 2^32 amostras à pista
    // em release — 24 h e 51 min — e deita abaixo a thread de escrita em debug.
    // E descartá-lo tirava 20 ms à fala (R294): escreve-se no sítio.

    type Pacote = webrtc::rtp::packet::Packet;

    /// Um pacote Opus mínimo, com o número de sequência no payload para se
    /// saber, no ficheiro, que pacote ficou em que página. O TOC `0xF8` é um
    /// frame CELT de 20 ms; o `OggWriter` não o descodifica.
    fn pacote_opus(seq: u16, ts: u32) -> Pacote {
        let header = webrtc::rtp::header::Header {
            sequence_number: seq,
            timestamp: ts,
            payload_type: 111,
            ..Default::default()
        };
        Pacote {
            header,
            payload: vec![0xf8, (seq >> 8) as u8, seq as u8].into(),
        }
    }

    /// `n` pacotes de 20 ms pela ordem em que foram ditos, a começar em `base`.
    fn falados(n: u16, base: u32) -> Vec<Pacote> {
        (0..n)
            .map(|i| pacote_opus(i, base.wrapping_add(u32::from(i) * 960)))
            .collect()
    }

    /// O pacote `k` chega `atraso` lugares depois do seu.
    fn atrasa(pacotes: &mut Vec<Pacote>, k: usize, atraso: usize) {
        let p = pacotes.remove(k);
        pacotes.insert(k + atraso, p);
    }

    /// `(tipo de cabeçalho, posição do grânulo, payload)` de cada página do
    /// OGG. Tipo `2` = início do fluxo, `4` = fim do fluxo.
    fn ogg_paginas(path: &std::path::Path) -> Vec<(u8, u64, Vec<u8>)> {
        let b = std::fs::read(path).expect("ficheiro de gravação");
        let mut paginas = Vec::new();
        let mut i = 0;
        while i + 27 <= b.len() {
            assert_eq!(&b[i..i + 4], b"OggS", "página OGG desalinhada em {i}");
            let granulo = u64::from_le_bytes(b[i + 6..i + 14].try_into().unwrap());
            let n_seg = b[i + 26] as usize;
            let corpo: usize = b[i + 27..i + 27 + n_seg].iter().map(|&x| x as usize).sum();
            let payload = b[i + 27 + n_seg..i + 27 + n_seg + corpo].to_vec();
            paginas.push((b[i + 5], granulo, payload));
            i += 27 + n_seg + corpo;
        }
        paginas
    }

    /// O que ficou de uma pista de áudio gravada num teste.
    #[derive(Debug)]
    struct PistaGravada {
        /// `(grânulo, payload)` das páginas de áudio, sem os dois cabeçalhos
        /// nem o fecho.
        paginas: Vec<(u64, Vec<u8>)>,
        /// A última página é a de fim do fluxo: a thread chegou ao `close`.
        fechada: bool,
        /// `recording_audio_late_dropped_total` no fim.
        atrasados: u64,
        /// `recording_audio_gap_unfilled_total` no fim.
        por_encher: u64,
    }

    impl PistaGravada {
        fn granulos(&self) -> Vec<u64> {
            self.paginas.iter().map(|p| p.0).collect()
        }
        /// Os números de sequência dos `pacote_opus`, pela ordem das páginas.
        fn seqs(&self) -> Vec<u16> {
            self.paginas
                .iter()
                .map(|(_, p)| u16::from_be_bytes([p[1], p[2]]))
                .collect()
        }
    }

    /// Os grânulos de `n` pacotes de 20 ms seguidos, sem nada em falta.
    fn granulos_seguidos(n: u64) -> Vec<u64> {
        (0..n).map(|i| 1 + i * 960).collect()
    }

    /// Grava pacotes com os timestamps dados, pela ordem dada (a sequência é
    /// a ordem de chegada), e fecha.
    async fn grava_audio(timestamps: &[u32]) -> PistaGravada {
        let pacotes = timestamps
            .iter()
            .enumerate()
            .map(|(i, ts)| pacote_opus(i as u16, *ts))
            .collect();
        grava_pacotes_de_audio(None, pacotes).await
    }

    /// Grava pelo caminho de produção: a thread do `RecWriter` e o `RecSink`.
    async fn grava_pacotes_de_audio(
        key: Option<Arc<Aes256Gcm>>,
        pacotes: Vec<Pacote>,
    ) -> PistaGravada {
        let dir = std::env::temp_dir().join(format!("dlx-rec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("teste.ogg");
        let sink = RecSink::audio(std::fs::File::create(&path).unwrap(), key).unwrap();
        let metrics = Arc::new(crate::metrics::Metrics::default());
        let w = RecWriter::spawn(
            sink,
            4096,
            metrics.clone(),
            "teste-audio".into(),
            FirstPacket::new(Instant::now()),
        );
        for pkt in &pacotes {
            w.write_rtp(pkt);
        }
        w.close().await;
        let paginas = ogg_paginas(&path);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(paginas.len() >= 2, "faltam os cabeçalhos do OGG");
        assert_eq!((paginas[0].0, paginas[0].1), (2, 0), "falta o OpusHead");
        PistaGravada {
            fechada: paginas.last().is_some_and(|p| p.0 == 4),
            paginas: paginas[2..]
                .iter()
                .filter(|p| p.0 != 4)
                .map(|p| (p.1, p.2.clone()))
                .collect(),
            atrasados: metrics
                .recording_audio_late_dropped_total
                .load(std::sync::atomic::Ordering::Relaxed),
            por_encher: metrics
                .recording_audio_gap_unfilled_total
                .load(std::sync::atomic::Ordering::Relaxed),
        }
    }

    /// Passa `chegada` pela `OpusTrack` e fecha: `(sequência, timestamp
    /// entregue)` de cada pacote escrito, e a soma do que aconteceu.
    fn pela_pista(chegada: Vec<Pacote>) -> (Vec<(u16, u32)>, OpusOutcome) {
        let (mut track, mut out) = (OpusTrack::default(), Vec::new());
        let mut total = OpusOutcome::default();
        let mut soma = |did: OpusOutcome| {
            total.late += did.late;
            total.resynced |= did.resynced;
        };
        for pkt in chegada {
            soma(track.push(pkt, &mut out));
        }
        soma(track.flush(&mut out));
        let escritos = out
            .iter()
            .map(|p| (p.header.sequence_number, p.header.timestamp))
            .collect();
        (escritos, total)
    }

    #[tokio::test]
    async fn um_pacote_de_audio_atrasado_escreve_se_no_sitio() {
        // 0, 1920, e só depois o 960 que a rede atrasou. 60 ms de áudio.
        let pista = grava_audio(&[0, 1920, 960]).await;
        let ultimo = pista.paginas.last().expect("páginas de áudio").0;
        assert!(
            ultimo <= 1 + 1920,
            "a pista de 60 ms ficou com {:.1} h (grânulo {ultimo}): {:?}",
            ultimo as f64 / 48000.0 / 3600.0,
            pista.granulos()
        );
        assert!(pista.fechada, "a thread de escrita morreu antes do fecho");
        assert_eq!(pista.granulos(), granulos_seguidos(3));
        assert_eq!(pista.seqs(), vec![0, 2, 1], "o atrasado fica no meio");
        assert_eq!(pista.atrasados, 0, "nada se perdeu");
    }

    #[tokio::test]
    async fn uma_pista_reordenada_fica_igual_a_que_chegou_em_ordem() {
        let em_ordem = falados(200, 90_000);
        let mut chegada = em_ordem.clone();
        chegada.swap(20, 21);
        atrasa(&mut chegada, 50, 3);
        // O limite: um pacote que chega depois dos `OPUS_REORDER_WINDOW`
        // seguintes ainda encontra o seu lugar.
        atrasa(&mut chegada, 100, OPUS_REORDER_WINDOW);
        chegada.swap(198, 199);
        let certa = grava_pacotes_de_audio(None, em_ordem).await;
        let pista = grava_pacotes_de_audio(None, chegada).await;
        assert!(pista.fechada, "a thread de escrita morreu antes do fecho");
        assert_eq!(pista.atrasados, 0);
        assert_eq!(certa.granulos(), granulos_seguidos(200));
        assert_eq!(pista.paginas, certa.paginas, "página a página");
    }

    #[tokio::test]
    async fn um_atrasado_para_la_da_janela_nao_se_escreve_e_conta_se() {
        let mut chegada = falados(60, 90_000);
        atrasa(&mut chegada, 20, OPUS_REORDER_WINDOW + 1);
        let pista = grava_pacotes_de_audio(None, chegada).await;
        assert!(pista.fechada, "a thread de escrita morreu antes do fecho");
        // O lugar dele leva um SILÊNCIO, e não um buraco — era a 1.ª metade
        // da R295: sem ele, o primeiro pacote depois do salto recebia o
        // grânulo da página anterior e a fala colava-se ao que veio antes.
        // O `seqs()` lê os bytes 1-2 do payload, e os do `OPUS_SILENCE`
        // (`[0xf8, 0xff, 0xfe]`) dão 0xfffe — é assim que ele aparece aqui.
        const SILENCIO_NO_SEQS: u16 = u16::from_be_bytes([OPUS_SILENCE[1], OPUS_SILENCE[2]]);
        let com_silencio: Vec<u16> = (0..60)
            .map(|i| if i == 20 { SILENCIO_NO_SEQS } else { i })
            .collect();
        assert_eq!(pista.seqs(), com_silencio);
        assert_eq!(pista.atrasados, 1, "descartado, mas contado");
        // O silêncio fica a um pacote do que retoma, para este lhe ficar
        // encostado: 20 ms de cada lado, e não os 40 de um buraco aberto.
        let g = pista.granulos();
        assert_eq!(g[20] - g[19], 960, "o silêncio entrou no lugar do 20");
        assert_eq!(g[21] - g[20], 960, "e o 21 ficou-lhe encostado");
        assert_eq!(*g.last().unwrap(), 1 + 59 * 960);
    }

    #[tokio::test]
    async fn um_repetido_nao_se_escreve_duas_vezes() {
        let em_ordem = falados(40, 90_000);
        let mut chegada = em_ordem.clone();
        // um repetido logo a seguir ao original, e outro de um pacote que já
        // foi escrito há muito
        chegada.insert(6, em_ordem[5].clone());
        chegada.insert(31, em_ordem[3].clone());
        let certa = grava_pacotes_de_audio(None, em_ordem).await;
        let pista = grava_pacotes_de_audio(None, chegada).await;
        assert!(pista.fechada, "a thread de escrita morreu antes do fecho");
        assert_eq!(pista.paginas, certa.paginas);
        assert_eq!(pista.atrasados, 2);
    }

    #[tokio::test]
    async fn a_volta_do_relogio_de_32_bits_nao_e_um_recuo() {
        // Os browsers começam o timestamp RTP num valor ao acaso, e a 48 kHz o
        // relógio de 32 bits dá a volta a cada 24 h 51 min: uma pista apanha a
        // volta a meio com a probabilidade da sua duração sobre esse tempo.
        // Aqui o pacote 20 tem o timestamp 0, e chega trocado com o 19.
        let mut chegada = falados(40, 0u32.wrapping_sub(20 * 960));
        assert_eq!(chegada[20].header.timestamp, 0);
        chegada.swap(19, 20);
        let pista = grava_pacotes_de_audio(None, chegada).await;
        assert!(pista.fechada, "a thread de escrita morreu antes do fecho");
        assert_eq!(pista.granulos(), granulos_seguidos(40));
        assert_eq!(pista.seqs(), (0..40).collect::<Vec<u16>>());
        assert_eq!(pista.atrasados, 0, "a volta não é um atraso");
    }

    #[test]
    fn o_relogio_conta_a_partir_do_primeiro_pacote_e_atravessa_a_volta() {
        // O que chega ao `OggWriter` nunca pode obrigar a subtracção dele a dar
        // a volta: em release dava certo por acaso, em debug é pânico — e o CI
        // corre em release, por isso é aqui que isto se guarda.
        let antes = u32::MAX - 959;
        let chegada = vec![
            pacote_opus(0, antes),
            pacote_opus(1, 0),
            pacote_opus(2, 960),
            // Um salto para a FRENTE é tempo que passou (DTX, um telefone
            // calado pelo anfitrião): aceita-se tal como vem.
            pacote_opus(3, 960 + 480_000),
        ];
        let (escritos, total) = pela_pista(chegada);
        assert_eq!(
            escritos,
            vec![(0, 0), (1, 960), (2, 1920), (3, 1920 + 480_000)]
        );
        assert_eq!(total, OpusOutcome::default());
    }

    /// 100 pacotes, e a origem recomeça o relógio 10 min atrás, de vez.
    fn com_recuo(depois: u16) -> Vec<Pacote> {
        let mut chegada = falados(100, 28_800_000);
        chegada.extend((0..depois).map(|j| pacote_opus(100 + j, u32::from(j) * 960)));
        chegada
    }

    #[test]
    fn um_recuo_que_nao_passa_e_um_relogio_novo_e_nao_se_perde_nada() {
        let n = OPUS_RESYNC_AFTER as u16 + 10;
        let (escritos, total) = pela_pista(com_recuo(n));
        // Todos, pela ordem em que foram ditos, e seguidos: o primeiro do
        // relógio novo fica um passo depois do último do antigo.
        let seguidos: Vec<(u16, u32)> = (0..100 + n).map(|i| (i, u32::from(i) * 960)).collect();
        assert_eq!(escritos, seguidos);
        assert_eq!(
            total,
            OpusOutcome {
                late: 0,
                resynced: true,
                unfilled: 0,
            }
        );
    }

    #[tokio::test]
    async fn depois_de_o_relogio_recuar_de_vez_a_pista_continua_inteira() {
        let n = OPUS_RESYNC_AFTER as u16 + 10;
        let pista = grava_pacotes_de_audio(None, com_recuo(n)).await;
        assert!(pista.fechada, "a thread de escrita morreu antes do fecho");
        assert_eq!(pista.granulos(), granulos_seguidos(100 + u64::from(n)));
        assert_eq!(pista.seqs(), (0..100 + n).collect::<Vec<u16>>());
        assert_eq!(pista.atrasados, 0);
    }

    #[test]
    fn o_relogio_novo_entra_ao_ultimo_pacote_da_serie_e_nao_antes() {
        // Exactamente `OPUS_RESYNC_AFTER`, e a pista fecha: já são um relógio
        // novo, e escrevem-se todos.
        let n = OPUS_RESYNC_AFTER as u16;
        let (escritos, total) = pela_pista(com_recuo(n));
        let seguidos: Vec<(u16, u32)> = (0..100 + n).map(|i| (i, u32::from(i) * 960)).collect();
        assert_eq!(escritos, seguidos);
        assert_eq!(
            total,
            OpusOutcome {
                late: 0,
                resynced: true,
                unfilled: 0,
            }
        );
    }

    #[test]
    fn um_silencio_no_inicio_do_relogio_novo_nao_empurra_a_pista() {
        // O relógio novo começa com um pacote, 400 ms de DTX, e depois fala.
        // O primeiro fica um passo de 20 ms depois do último escrito — o passo
        // da fala, não o do silêncio.
        let n = OPUS_RESYNC_AFTER as u16;
        let mut chegada = falados(100, 28_800_000);
        chegada.push(pacote_opus(100, 0));
        chegada.extend((0..n).map(|j| pacote_opus(101 + j, 19_200 + u32::from(j) * 960)));
        let (escritos, total) = pela_pista(chegada);
        assert!(total.resynced && total.late == 0, "{total:?}");
        assert_eq!(escritos[99], (99, 99 * 960));
        assert_eq!(escritos[100], (100, 100 * 960), "um passo de fala depois");
        assert_eq!(
            escritos[101],
            (101, 100 * 960 + 19_200),
            "e o silêncio dele"
        );
        assert_eq!(escritos[102], (102, 100 * 960 + 19_200 + 960));
    }

    /// 100 pacotes, e a origem recomeça o relógio `recuo` pacotes atrás do
    /// último, de vez; chegam `depois` pacotes do relógio novo.
    fn com_recuo_curto(recuo: u32, depois: u16) -> Vec<Pacote> {
        let base = 28_800_000 + (99 - recuo) * 960;
        let mut chegada = falados(100, 28_800_000);
        chegada.extend((0..depois).map(|j| pacote_opus(100 + j, base + u32::from(j) * 960)));
        chegada
    }

    #[test]
    fn um_recuo_de_um_segundo_ja_e_um_relogio_novo() {
        // 49 pacotes atrás do mais novo (0,98 s): os primeiros
        // `OPUS_RESYNC_AFTER` ficam atrás do último escrito, e chega. Nada se
        // perde. O número é literal de propósito: é o que a R294 já lia como
        // relógio novo, e subir o limiar ou a janela não o pode tirar daqui.
        let (escritos, total) = pela_pista(com_recuo_curto(49, 60));
        let seguidos: Vec<(u16, u32)> = (0..160).map(|i| (i, u32::from(i) * 960)).collect();
        assert_eq!(escritos, seguidos);
        assert_eq!(
            total,
            OpusOutcome {
                late: 0,
                resynced: true,
                unfilled: 0,
            }
        );
    }

    #[test]
    fn um_recuo_mais_curto_perde_se_ate_o_relogio_alcancar_o_ponto_onde_ia() {
        // Um pacote menos de recuo, e a série não chega ao fim: o relógio novo
        // alcança o último escrito antes disso. Perde-se tudo até ele passar o
        // mais novo do relógio antigo — é o que acontecia antes da janela, e
        // não se distingue de uma rajada de atrasados.
        let recuo = 48;
        let (escritos, total) = pela_pista(com_recuo_curto(recuo, 60));
        assert_eq!(
            total,
            OpusOutcome {
                late: recuo + 1,
                resynced: false,
                unfilled: 0,
            }
        );
        // Os 100 do relógio antigo, e os do novo a partir do primeiro que
        // passa à frente — seguidos, sem um passo fora do sítio.
        let primeiro_novo = 100 + recuo as u16 + 1;
        let mut esperados: Vec<(u16, u32)> = (0..100).map(|i| (i, u32::from(i) * 960)).collect();
        esperados
            .extend((primeiro_novo..160).map(|s| (s, u32::from(s - primeiro_novo + 100) * 960)));
        assert_eq!(escritos, esperados);
    }

    #[test]
    fn o_que_nao_se_escreve_quando_o_relogio_novo_entra_tambem_se_conta() {
        // Um repetido ainda à espera na janela quando o relógio novo entra: a
        // janela sai toda, ele é recusado — e tem de aparecer na conta (R18).
        let n = OPUS_RESYNC_AFTER as u16;
        let mut chegada = falados(100, 28_800_000);
        chegada.push(chegada[99].clone());
        chegada.extend((0..n).map(|j| pacote_opus(100 + j, u32::from(j) * 960)));
        let (escritos, total) = pela_pista(chegada);
        let seguidos: Vec<(u16, u32)> = (0..100 + n).map(|i| (i, u32::from(i) * 960)).collect();
        assert_eq!(escritos, seguidos);
        assert_eq!(
            total,
            OpusOutcome {
                late: 1,
                resynced: true,
                unfilled: 0,
            }
        );
    }

    #[test]
    fn dois_pacotes_a_meio_relogio_um_do_outro_nao_rebentam_a_escrita() {
        // A exactamente 2^31 nenhum está «à frente» do outro, e a janela não
        // tem como os ordenar. O que não avança sobre o último escrito não se
        // entrega (em debug, o `OggWriter` rebentava) — e conta-se.
        let (escritos, total) = pela_pista(vec![pacote_opus(0, 0), pacote_opus(1, 0x8000_0000)]);
        assert_eq!(escritos, vec![(0, 0)]);
        assert_eq!(
            total,
            OpusOutcome {
                late: 1,
                resynced: false,
                unfilled: 0,
            }
        );
    }

    #[test]
    fn os_que_esperavam_por_um_relogio_novo_ao_fechar_nao_se_escrevem() {
        // Menos do que `OPUS_RESYNC_AFTER` atrás do último escrito, e a pista
        // fecha: não chegaram a provar que eram um relógio novo.
        let n = OPUS_RESYNC_AFTER as u16 - 1;
        let (escritos, total) = pela_pista(com_recuo(n));
        assert_eq!(escritos.len(), 100);
        assert_eq!(
            total,
            OpusOutcome {
                late: u32::from(n),
                resynced: false,
                unfilled: 0,
            }
        );
    }

    #[test]
    fn uma_rajada_de_atrasados_nao_e_um_relogio_novo() {
        let n = OPUS_RESYNC_AFTER as u32;
        let mut chegada = falados(30, 1_000_000);
        let (mut seq, mut atrasados) = (30u16, 0u32);
        let mut junta = |chegada: &mut Vec<Pacote>, ts: u32| {
            chegada.push(pacote_opus(seq, ts));
            seq += 1;
        };
        // Atrasados que avançam SEMPRE, de volta para volta, mas com um pacote
        // em dia pelo meio: é ele que desfaz a contagem, e ela nunca chega ao
        // fim. Sem isso, 50 atrasos soltos ao longo de uma reunião davam um
        // relógio novo, e áudio velho escrito fora do sítio.
        for volta in 0..4u32 {
            for i in 1..n {
                junta(&mut chegada, (volta * n + i) * 960);
                atrasados += 1;
            }
            junta(&mut chegada, 1_000_000 + (30 + volta) * 960);
        }
        // E atrasados que NÃO avançam entre si (o mesmo pacote repetido, ou a
        // andar para trás) também não: cada um recomeça a série.
        for _ in 0..3 * n {
            junta(&mut chegada, 960);
            atrasados += 1;
        }
        for i in (1..3 * n).rev() {
            junta(&mut chegada, i * 960);
            atrasados += 1;
        }
        let (escritos, total) = pela_pista(chegada);
        assert_eq!(
            total,
            OpusOutcome {
                late: atrasados,
                resynced: false,
                unfilled: 0,
            }
        );
        let em_dia: Vec<u32> = (0..34).map(|i| i * 960).collect();
        assert_eq!(
            escritos.iter().map(|e| e.1).collect::<Vec<u32>>(),
            em_dia,
            "só o que chegou em dia, e seguido"
        );
    }

    #[tokio::test]
    async fn um_payload_vazio_nao_ocupa_lugar_na_pista() {
        // O `OggWriter` ignora-o sem mexer no relógio dele. Se entrasse na
        // pista, o pacote a sério com o mesmo timestamp era dado como repetido.
        let mut vazio = pacote_opus(1, 960);
        vazio.payload = Vec::new().into();
        let pacotes = vec![pacote_opus(0, 0), vazio, pacote_opus(2, 960)];
        let pista = grava_pacotes_de_audio(None, pacotes).await;
        assert_eq!(pista.granulos(), granulos_seguidos(2));
        assert_eq!(pista.seqs(), vec![0, 2]);
        assert_eq!(pista.atrasados, 0);
    }

    #[tokio::test]
    async fn o_audio_cifrado_passa_pela_mesma_pista_e_sai_decifrado() {
        // Com cifra ponta-a-ponta o payload é trocado pelo decifrado no mesmo
        // pacote a que se acerta o timestamp: os dois têm de chegar ao disco.
        let key = Arc::new(chave_de_teste(7));
        let frame = |n: u8| -> Vec<u8> { std::iter::once(0xf8).chain([n; 40]).collect() };
        let mut pacotes: Vec<_> = [(0u32, 1u8), (1920, 2), (960, 3), (2880, 4), (4800, 6)]
            .into_iter()
            .map(|(ts, n)| {
                let claro = frame(n);
                let mut pkt = pacote_opus(n as u16, ts);
                pkt.payload = cifra_como_o_browser(&key, &claro[..1], &claro[1..], &[n; 12]).into();
                pkt
            })
            .collect();
        // Um frame que não autentica (outra chave), em dia: não se escreve, não
        // conta como atrasado, e o seguinte cobre o tempo dele como uma perda.
        let claro = frame(5);
        let mut alheio = pacote_opus(5, 3840);
        alheio.payload =
            cifra_como_o_browser(&chave_de_teste(8), &claro[..1], &claro[1..], &[5; 12]).into();
        pacotes.insert(4, alheio);
        let pista = grava_pacotes_de_audio(Some(key), pacotes).await;
        assert!(pista.fechada, "a thread de escrita morreu antes do fecho");
        // O 3 chegou atrasado e está no sítio; o 5 não autentica e falta — e
        // o lugar dele leva um SILÊNCIO, escrito a um pacote do 6 para este
        // lhe ficar encostado (R295). O silêncio vai em CLARO numa pista
        // cifrada: não é media de ninguém, e sai do ficheiro como qualquer
        // outra página.
        let mut claros: Vec<(u64, Vec<u8>)> = [(1, 1u8), (961, 3), (1921, 2), (2881, 4)]
            .into_iter()
            .map(|(g, n)| (g, frame(n)))
            .collect();
        claros.push((3841, OPUS_SILENCE.to_vec()));
        claros.push((4801, frame(6)));
        assert_eq!(pista.paginas, claros);
        assert_eq!(pista.atrasados, 0);
    }

    #[test]
    fn grelha_sem_qualidade_e_a_de_sempre() {
        assert_eq!(grid_tile(None, 1), (640, 360));
        assert_eq!(grid_tile(None, 9), (640, 360));
        assert_eq!(grid_tile(Some("audio"), 4), (640, 360));
    }

    #[test]
    fn grelha_com_qualidade_divide_a_tela_pedida() {
        // 2 pessoas em 1080p: 2 colunas × 1 linha.
        assert_eq!(grid_tile(Some("1080p"), 2), (960, 1080));
        // 4 pessoas em 4K: 2×2 mosaicos de 1920×1080.
        assert_eq!(grid_tile(Some("2160p"), 4), (1920, 1080));
        // 5 em 720p: 3 colunas × 2 linhas, dimensões pares.
        let (w, h) = grid_tile(Some("720p"), 5);
        assert_eq!((w, h), (426, 360));
        assert_eq!(w % 2, 0);
    }

    #[test]
    fn progresso_nunca_chega_a_100_antes_do_ready() {
        assert_eq!(composition_pct(0, 10_000), 0);
        assert_eq!(composition_pct(5_000, 10_000), 50);
        assert_eq!(composition_pct(12_000, 10_000), 99);
        assert_eq!(composition_pct(5_000, 0), 0);
    }

    #[tokio::test]
    async fn run_bounded_progress_le_o_out_time_do_processo() {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.args(["-c", "echo out_time_us=1500000; echo progress=continue; echo out_time_us=N/A; echo out_time_us=3000000; echo progress=end"]);
        let (tx, mut rx) = tokio::sync::watch::channel(0i64);
        let st = run_bounded_progress(&mut cmd, Duration::from_secs(10), Some(tx))
            .await
            .unwrap();
        assert!(st.success());
        // O leitor termina no EOF; espera-se pelo último valor.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while *rx.borrow_and_update() != 3000 && std::time::Instant::now() < deadline {
            if rx.changed().await.is_err() {
                break;
            }
        }
        assert_eq!(*rx.borrow(), 3000);
    }

    #[tokio::test]
    async fn run_bounded_returns_when_the_process_finishes_in_time() {
        let mut cmd = tokio::process::Command::new("true");
        let st = run_bounded(&mut cmd, Duration::from_secs(30))
            .await
            .expect("devia ter terminado dentro do tecto");
        assert!(st.success());
    }

    #[tokio::test]
    async fn run_bounded_kills_a_process_that_overruns() {
        // O caso que já pendurou uma composição: o processo nunca termina.
        let mut cmd = tokio::process::Command::new("sleep");
        cmd.arg("60");
        let started = std::time::Instant::now();
        let err = run_bounded(&mut cmd, Duration::from_millis(150))
            .await
            .expect_err("tinha de falhar por exceder o tecto");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "voltou tarde demais — o tecto não está a ser imposto"
        );
        assert!(
            err.to_string().contains("excedeu"),
            "o erro tem de dizer o que aconteceu, e não um código opaco: {err}"
        );
    }

    #[tokio::test]
    async fn run_bounded_reports_a_failing_process_instead_of_hanging() {
        let mut cmd = tokio::process::Command::new("false");
        let st = run_bounded(&mut cmd, Duration::from_secs(30))
            .await
            .unwrap();
        assert!(!st.success(), "um ffmpeg que falha tem de ser visível");
    }

    #[tokio::test]
    async fn gauge_guard_sobe_e_desce_inclusive_ao_cancelar() {
        use std::sync::atomic::{AtomicI64, Ordering::Relaxed};
        let g = AtomicI64::new(0);
        {
            let _a = GaugeGuard::up(&g);
            let _b = GaugeGuard::up(&g);
            assert_eq!(g.load(Relaxed), 2);
        }
        assert_eq!(g.load(Relaxed), 0, "o gauge tem de voltar a zero ao largar");
    }

    #[tokio::test]
    async fn wait_for_slot_da_batimentos_enquanto_espera_e_entrega_a_vaga() {
        use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
        let slots = crate::fair_slots::FairSlots::new(1);
        let held = slots.acquire(Uuid::nil()).await.unwrap();
        let beats = Arc::new(AtomicUsize::new(0));
        let b = beats.clone();
        let waiter = tokio::spawn(wait_for_slot(
            slots.clone(),
            Uuid::nil(),
            Duration::from_millis(20),
            move || {
                let b = b.clone();
                async move {
                    b.fetch_add(1, Relaxed);
                }
            },
        ));
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(
            !waiter.is_finished(),
            "não havia vaga: não podia ter acabado"
        );
        assert!(
            beats.load(Relaxed) >= 2,
            "sem batimento a varredura marca como falhada uma gravação que só espera"
        );
        drop(held);
        let permit = tokio::time::timeout(Duration::from_secs(5), waiter)
            .await
            .expect("com a vaga livre tinha de acabar")
            .unwrap()
            .expect("a vaga tinha de ser entregue");
        assert_eq!(slots.available(), 0, "a vaga está agora com quem esperou");
        drop(permit);
        assert_eq!(slots.available(), 1);
    }

    // ------------------------------------------------------------------
    //  Buraco na pista de áudio (DTX, perda): a fala seguinte não recua
    // ------------------------------------------------------------------

    /// Uma sessão de gravação como a de produção, numa pasta de teste.
    async fn sessao_de_teste() -> RecordingSession {
        let dir = std::env::temp_dir().join(format!("dlx-buraco-{}", Uuid::new_v4()));
        RecordingSession::new(Uuid::new_v4(), "teste".into(), None, &dir)
            .await
            .unwrap()
    }

    /// Abre uma pista em `session` pelo caminho de PRODUÇÃO (`open_track`, a
    /// thread do `RecWriter`, o `RecSink`) e devolve o writer e o ficheiro.
    fn abre_pista(session: &mut RecordingSession, kind: &str) -> (RecWriter, PathBuf) {
        let metrics = Arc::new(crate::metrics::Metrics::default());
        let w = session
            .open_track(kind, Uuid::new_v4(), 4096, metrics)
            .unwrap();
        (w, session.tracks.last().unwrap().path.clone())
    }

    /// Grava uma pista de áudio. Dentro de `fala` (intervalos em ms) há um tom
    /// a `freq`; fora deles não sai pacote e o timestamp RTP salta, que é o
    /// que a perda faz. Com `dtx`, o silêncio leva um pacote de 20 ms a cada
    /// 400 ms, como o Opus do Chrome com `usedtx=1`.
    async fn pista_com_buraco(
        session: &mut RecordingSession,
        freq: f32,
        total_ms: u32,
        fala: &[(u32, u32)],
        dtx: bool,
    ) -> PathBuf {
        let (w, path) = abre_pista(session, "audio");
        let mut enc = opus_rs::OpusEncoder::new(48_000, 1, opus_rs::Application::Voip).unwrap();
        enc.bitrate_bps = 32_000;
        let mut out = vec![0u8; 1500];
        let (mut seq, mut calado) = (0u16, 0u32);
        for i in 0..total_ms / 20 {
            let t = i * 20;
            let fala_agora = fala.iter().any(|(a, b)| (*a..*b).contains(&t));
            let pcm: Vec<f32> = (0..960u32)
                .map(|k| {
                    if !fala_agora {
                        return 0.0;
                    }
                    let n = (i * 960 + k) as f32;
                    0.5 * (2.0 * std::f32::consts::PI * freq * n / 48_000.0).sin()
                })
                .collect();
            // O codificador vê todos os quadros; só alguns chegam ao gravador.
            let len = enc.encode(&pcm, 960, &mut out).unwrap();
            calado = if fala_agora { 0 } else { calado + 1 };
            if !fala_agora && !(dtx && calado % 20 == 0) {
                continue;
            }
            seq = seq.wrapping_add(1);
            w.write_rtp(&webrtc::rtp::packet::Packet {
                header: webrtc::rtp::header::Header {
                    sequence_number: seq,
                    timestamp: 90_000 + i * 960,
                    payload_type: 111,
                    ..Default::default()
                },
                payload: out[..len].to_vec().into(),
            });
        }
        assert_eq!(w.close().await, 0, "a fila de escrita perdeu pacotes");
        path
    }

    /// Corre o ffmpeg do servidor (`FFMPEG_BIN`) sobre `entradas` com os
    /// argumentos de saída `args`, para um webm, e devolve o áudio do
    /// resultado em PCM mono a 48 kHz, amostra atrás de amostra — sem olhar
    /// aos PTS, que é como o Chromium o toca. `None` se a máquina não tem ffmpeg.
    fn compor(dir: &Path, entradas: &[PathBuf], args: &[String]) -> Option<Vec<i16>> {
        let bin = std::env::var("FFMPEG_BIN").unwrap_or_else(|_| "ffmpeg".into());
        if std::process::Command::new(&bin)
            .arg("-version")
            .output()
            .is_err()
        {
            return None;
        }
        let corre = |cmd: &mut std::process::Command| {
            let res = cmd.output().unwrap();
            assert!(
                res.status.success(),
                "ffmpeg falhou: {}",
                String::from_utf8_lossy(&res.stderr)
            );
        };
        let (webm, pcm) = (dir.join("out.webm"), dir.join("out.pcm"));
        let mut cmd = std::process::Command::new(&bin);
        cmd.args(["-y", "-loglevel", "error", "-nostdin"]);
        for p in entradas {
            cmd.arg("-i").arg(p);
        }
        corre(cmd.args(args).arg(&webm));
        let mut cmd = std::process::Command::new(&bin);
        cmd.args(["-y", "-loglevel", "error", "-nostdin", "-i"]);
        cmd.arg(&webm);
        cmd.args(["-map", "0:a:0", "-ac", "1", "-ar", "48000", "-f", "s16le"]);
        corre(cmd.arg(&pcm));
        let bytes = std::fs::read(&pcm).unwrap();
        let (pares, _) = bytes.as_chunks::<2>();
        Some(pares.iter().map(|b| i16::from_le_bytes(*b)).collect())
    }

    /// Os argumentos de saída da composição só de áudio de `offsets_ms`.
    fn args_da_mistura(offsets_ms: &[u64]) -> Vec<String> {
        let (fc, aout) = audio_mix_graph(0, offsets_ms);
        let mut args: Vec<String> = vec![
            "-filter_complex".into(),
            fc.trim_end_matches(';').into(),
            "-map".into(),
            aout,
        ];
        args.extend(OPUS_ARGS.iter().map(|a| a.to_string()));
        args
    }

    /// Amplitude (0 a 1) do tom a `freq` entre `de_ms` e `ate_ms` do PCM.
    fn nivel(pcm: &[i16], freq: f32, de_ms: usize, ate_ms: usize) -> f64 {
        let (a, b) = (de_ms * 48, (ate_ms * 48).min(pcm.len()));
        if a >= b {
            return 0.0;
        }
        let w = 2.0 * std::f64::consts::PI * freq as f64 / 48_000.0;
        let (mut re, mut im) = (0f64, 0f64);
        for (i, s) in pcm[a..b].iter().enumerate() {
            let x = *s as f64 / 32768.0;
            re += x * (w * i as f64).cos();
            im += x * (w * i as f64).sin();
        }
        2.0 * (re * re + im * im).sqrt() / (b - a) as f64
    }

    /// O que a pista de 440 Hz (fala de 0 a 2 s e de 4 a 6 s) tem de dar depois
    /// de composta com `atraso_ms`: tom, dois segundos de silêncio, tom — e a
    /// duração inteira. As margens cobrem os 80 ms de `pre-skip` que o
    /// `OggWriter` declara e o ffmpeg desconta ao início de cada pista.
    fn exige_tom_silencio_tom(pcm: &[i16], atraso_ms: usize, caso: &str) {
        let (dur_ms, fim) = (pcm.len() / 48, atraso_ms + 6000);
        assert!(
            (fim - 150..=fim + 100).contains(&dur_ms),
            "{caso}: a composição tem {dur_ms} ms e a pista acaba aos {fim}"
        );
        let antes = nivel(pcm, 440.0, atraso_ms + 200, atraso_ms + 1800);
        let buraco = nivel(pcm, 440.0, atraso_ms + 2300, atraso_ms + 3700);
        let depois = nivel(pcm, 440.0, atraso_ms + 4200, atraso_ms + 5800);
        assert!(antes > 0.2, "{caso}: sem tom antes do buraco ({antes:.3})");
        assert!(
            buraco < 0.02,
            "{caso}: o buraco não ficou em silêncio ({buraco:.3}) — a fala seguinte recuou"
        );
        assert!(
            depois > 0.2,
            "{caso}: a fala depois do buraco não está onde foi dita ({depois:.3})"
        );
    }

    const SEM_FFMPEG: &str = "ffmpeg indisponível — o buraco de áudio NÃO foi verificado";
    const FALA: [(u32, u32); 2] = [(0, 2000), (4000, 6000)];

    #[tokio::test]
    async fn a_fala_depois_de_um_buraco_nao_recua_na_mistura() {
        for dtx in [false, true] {
            let mut s = sessao_de_teste().await;
            let a = pista_com_buraco(&mut s, 440.0, 6000, &FALA, dtx).await;
            let b = pista_com_buraco(&mut s, 1000.0, 6000, &[(0, 6000)], false).await;
            let pcm = compor(&s.dir, &[a, b], &args_da_mistura(&[500, 0]));
            let _ = std::fs::remove_dir_all(s.dir.parent().unwrap());
            let Some(pcm) = pcm else {
                eprintln!("{SEM_FFMPEG}");
                return;
            };
            let caso = if dtx {
                "mistura com DTX"
            } else {
                "mistura com perda"
            };
            exige_tom_silencio_tom(&pcm, 500, caso);
            let outro = nivel(&pcm, 1000.0, 200, 5800);
            assert!(outro > 0.2, "{caso}: a outra pista perdeu-se ({outro:.3})");
        }
    }

    #[tokio::test]
    async fn a_fala_depois_de_um_buraco_nao_recua_numa_pista_so() {
        let mut s = sessao_de_teste().await;
        let a = pista_com_buraco(&mut s, 440.0, 6000, &FALA, true).await;
        let pcm = compor(&s.dir, &[a], &args_da_mistura(&[500]));
        let _ = std::fs::remove_dir_all(s.dir.parent().unwrap());
        let Some(pcm) = pcm else {
            eprintln!("{SEM_FFMPEG}");
            return;
        };
        exige_tom_silencio_tom(&pcm, 500, "só áudio, uma pista");
    }

    /// O caminho de um publicador com os argumentos de produção. O vídeo vai
    /// em cópia e ninguém o descodifica: bastam-lhe quadros com cara de VP8.
    #[tokio::test]
    async fn a_fala_depois_de_um_buraco_nao_recua_com_um_publicador() {
        let mut s = sessao_de_teste().await;
        let (w, v) = abre_pista(&mut s, "video");
        for i in 0..60u32 {
            w.write_rtp(&vp8_keyframe(i as u16, i * 9000));
        }
        assert_eq!(w.close().await, 0);
        let a = pista_com_buraco(&mut s, 440.0, 6000, &FALA, true).await;
        let pcm = compor(&s.dir, &[v, a], &single_publisher_args(true, None));
        let _ = std::fs::remove_dir_all(s.dir.parent().unwrap());
        let Some(pcm) = pcm else {
            eprintln!("{SEM_FFMPEG}");
            return;
        };
        exige_tom_silencio_tom(&pcm, 0, "um publicador");
    }

    /// O que o CI vê (não tem ffmpeg): o enchimento está em TODAS as cadeias
    /// de áudio, antes do `adelay`, e nenhum caminho leva o áudio em cópia. O
    /// `adelay` é o instante da pista mais os 80 ms do `OGG_PRE_SKIP_MS`.
    #[test]
    fn o_audio_e_enchido_antes_de_qualquer_outro_filtro() {
        assert_eq!(AUDIO_GAP_FILL, "aresample=async=1:first_pts=0");
        let (fc, aout) = audio_mix_graph(2, &[500, 0]);
        assert_eq!(
            fc,
            format!(
                "[2:a]{AUDIO_GAP_FILL},adelay=580:all=1[a0];\
                 [3:a]{AUDIO_GAP_FILL},adelay=80:all=1[a1];\
                 [a0][a1]amix=inputs=2:normalize=0[aout];"
            )
        );
        assert_eq!(aout, "[aout]");
        assert_eq!(
            audio_mix_graph(0, &[7]),
            (
                format!("[0:a]{AUDIO_GAP_FILL},adelay=87:all=1[a0];"),
                "[a0]".to_string()
            )
        );
        assert_eq!(audio_mix_graph(1, &[]), (String::new(), String::new()));

        for reduz in [None, Some(720)] {
            let args = single_publisher_args(true, reduz);
            let par = |a: &str, b: &str| args.windows(2).any(|w| w[0] == a && w[1] == b);
            assert!(par("-af", AUDIO_GAP_FILL), "{args:?}");
            assert!(par("-c:a", "libopus"), "{args:?}");
            assert!(
                !par("-c", "copy") && !par("-c:a", "copy"),
                "áudio em cópia leva o buraco para o contentor: {args:?}"
            );
            assert_eq!(par("-c:v", "copy"), reduz.is_none(), "{args:?}");
        }
        assert_eq!(
            single_publisher_args(false, None),
            ["-map", "0:v:0", "-c:v", "copy"]
        );
    }

    // ------------------------------------------------------------------
    //  Cada pista entra na gravação no instante do seu primeiro pacote
    // ------------------------------------------------------------------
    //
    // O zero de uma pista é o primeiro quadro (ou pacote) que lá ficou. Uma
    // pista de vídeo ligada a meio do fluxo espera pelo keyframe que o SFU
    // pede; uma de áudio em silêncio, pelo pacote seguinte do DTX. Posta na
    // linha do tempo pelo instante em que o writer foi LIGADO, entrava
    // adiantada o tempo que esperou — a imagem à frente do som.

    /// Os milissegundos da sessão, agora.
    fn agora_ms(s: &RecordingSession) -> u64 {
        s.started.elapsed().as_millis() as u64
    }

    async fn dorme(ms: u64) {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }

    #[tokio::test]
    async fn a_pista_de_video_comeca_no_keyframe_e_nao_na_ligacao() {
        let mut s = sessao_de_teste().await;
        dorme(30).await;
        let (w, _) = abre_pista(&mut s, "video");
        let ligada = s.tracks[0].offset_ms;
        assert_eq!(
            s.tracks[0].starts_at_ms(),
            ligada,
            "sem pacotes, a pista fica no instante da ligação"
        );
        // A meio do fluxo: quadros delta enquanto o keyframe pedido não chega.
        let mut seq = 0u16;
        for q in 0..3u32 {
            for p in vp8_quadro_delta(&mut seq, q * 3000) {
                w.write_rtp(&p);
            }
        }
        dorme(120).await;
        assert_eq!(
            s.tracks[0].starts_at_ms(),
            ligada,
            "quadros delta não começam a pista"
        );
        let antes = agora_ms(&s);
        w.write_rtp(&vp8_keyframe(seq, 3 * 3000));
        let depois = agora_ms(&s);
        let comeca = s.tracks[0].starts_at_ms();
        assert!(
            (antes..=depois).contains(&comeca) && comeca >= ligada + 120,
            "a pista começa quando o keyframe chega ({comeca} ms; chegou entre {antes} e \
             {depois}), não quando o writer foi ligado ({ligada} ms)"
        );
        // O que vem a seguir já não mexe no início.
        dorme(40).await;
        seq = seq.wrapping_add(1);
        for q in 4..6u32 {
            for p in vp8_quadro_delta(&mut seq, q * 3000) {
                w.write_rtp(&p);
            }
        }
        w.write_rtp(&vp8_keyframe(seq, 6 * 3000));
        assert_eq!(s.tracks[0].starts_at_ms(), comeca);
        w.close().await;
        let _ = std::fs::remove_dir_all(s.dir.parent().unwrap());
    }

    #[tokio::test]
    async fn um_keyframe_de_abertura_que_nao_serviu_nao_fica_como_inicio_da_pista() {
        let mut s = sessao_de_teste().await;
        let (w, path) = abre_pista(&mut s, "video");
        // O início de um keyframe, e a seguir um pacote que não é dele: a
        // thread de escrita deita-o fora e a pista volta a esperar.
        let mut inicio = vp8_keyframe(100, 3000);
        inicio.header.marker = false;
        w.write_rtp(&inicio);
        let falhado = s.tracks[0].starts_at_ms();
        w.write_rtp(&vp8_pacote(140, 6000, 0x00, 0x42, true));
        ate_que("a pista volta a esperar pelo keyframe", || {
            w.wants_keyframe()
        })
        .await;
        dorme(120).await;
        let antes = agora_ms(&s);
        w.write_rtp(&vp8_keyframe(200, 9000));
        let depois = agora_ms(&s);
        ate_que("a pista abriu", || !w.wants_keyframe()).await;
        w.close().await;
        let comeca = s.tracks[0].starts_at_ms();
        assert_eq!(ivf_quadros(&path).len(), 1, "só o keyframe inteiro");
        assert!(
            (antes..=depois).contains(&comeca) && comeca >= falhado + 120,
            "o início é o do keyframe que ficou na pista ({comeca} ms; chegou entre {antes} \
             e {depois}), não o do que foi deitado fora ({falhado} ms)"
        );
        let _ = std::fs::remove_dir_all(s.dir.parent().unwrap());
    }

    /// Um pacote RTP de Opus para os testes do instante de cada pista. É igual
    /// ao `opus_silencio` dos testes do relógio, mas próprio: esses testes (e a
    /// função deles) saem com a correcção do pacote de áudio atrasado, e o
    /// instante de uma pista mede-se em qualquer dos dois mundos.
    fn opus_rtp(seq: u16, ts: u32) -> webrtc::rtp::packet::Packet {
        let header = webrtc::rtp::header::Header {
            sequence_number: seq,
            timestamp: ts,
            payload_type: 111,
            ..Default::default()
        };
        webrtc::rtp::packet::Packet {
            header,
            payload: vec![0xf8, 0xff, 0xfe].into(),
        }
    }

    #[tokio::test]
    async fn a_pista_de_audio_comeca_no_primeiro_pacote_e_nao_na_ligacao() {
        let mut s = sessao_de_teste().await;
        let (w, _) = abre_pista(&mut s, "audio");
        let ligada = s.tracks[0].offset_ms;
        // Um payload vazio não chega a ser escrito: não é o início de nada.
        let mut vazio = opus_rtp(1, 1000);
        vazio.payload = Vec::new().into();
        w.write_rtp(&vazio);
        // O microfone está em silêncio: o DTX só manda o pacote seguinte daqui a pouco.
        dorme(120).await;
        assert_eq!(s.tracks[0].starts_at_ms(), ligada);
        let antes = agora_ms(&s);
        w.write_rtp(&opus_rtp(2, 1000 + 19_200));
        let depois = agora_ms(&s);
        let comeca = s.tracks[0].starts_at_ms();
        assert!(
            (antes..=depois).contains(&comeca) && comeca >= ligada + 120,
            "a pista começa quando o primeiro pacote chega ({comeca} ms; chegou entre \
             {antes} e {depois}), não quando o writer foi ligado ({ligada} ms)"
        );
        dorme(40).await;
        w.write_rtp(&opus_rtp(3, 1000 + 20_160));
        assert_eq!(s.tracks[0].starts_at_ms(), comeca, "só o primeiro conta");
        w.close().await;
        let _ = std::fs::remove_dir_all(s.dir.parent().unwrap());
    }

    /// Os metadados de uma pista que começou aos `comeca_ms` da sessão (e cujo
    /// writer tinha sido ligado aos `ligada_ms`).
    /// Uma pista como a composição a vê. Passa pela MESMA conversão da
    /// produção (`RecTrackMeta::to_compose`), para que o fallback de
    /// `starts_at_ms()` — pista ligada mas sem pacote nenhum — continue a ser
    /// o que estes testes exercitam.
    fn pista(path: &str, kind: &str, ligada_ms: u64, comeca_ms: u64) -> ComposeTrack {
        let first = FirstPacket::new(Instant::now());
        if comeca_ms > 0 {
            first
                .at_ms
                .store(comeca_ms, std::sync::atomic::Ordering::Relaxed);
        }
        RecTrackMeta {
            path: PathBuf::from(path),
            kind: kind.into(),
            publisher: Uuid::new_v4(),
            offset_ms: ligada_ms,
            first,
        }
        .to_compose()
    }

    fn textos(args: &[std::ffi::OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    /// O que o CI vê (não tem ffmpeg): a composição põe cada pista no instante
    /// em que COMEÇA, nos dois caminhos — e nunca no da ligação do writer.
    #[test]
    fn a_composicao_poe_cada_pista_no_instante_em_que_comeca() {
        // Um publicador: a que começa depois leva a diferença; o áudio conta
        // com os 80 ms de pre-skip.
        assert_eq!(single_publisher_shifts(557, Some(10)), (467, 0));
        assert_eq!(single_publisher_shifts(5, Some(300)), (0, 375));
        assert_eq!(single_publisher_shifts(90, Some(10)), (0, 0));
        assert_eq!(single_publisher_shifts(1234, None), (0, 0));
        assert_eq!(input_shift(467), ["-itsoffset", "0.467"]);
        assert!(input_shift(0).is_empty());

        // O vídeo esperou 557 ms pelo keyframe; o áudio, 10 ms pelo pacote.
        let (v, a) = (
            pista("v.ivf", "video", 3, 560),
            pista("a.ogg", "audio", 3, 13),
        );
        let args = textos(&compose_args(&[&v], &[&a], None, None));
        assert_eq!(
            args[..6],
            ["-itsoffset", "0.467", "-i", "v.ivf", "-i", "a.ogg"],
            "o vídeo em cópia entra atrasado o que esperou a mais do que o áudio"
        );
        assert_eq!(args[6..], single_publisher_args(true, None));
        // Ao contrário: é o áudio que entra depois.
        let (v, a) = (
            pista("v.ivf", "video", 0, 20),
            pista("a.ogg", "audio", 0, 340),
        );
        let args = textos(&compose_args(&[&v], &[&a], None, None));
        assert_eq!(
            args[..6],
            ["-i", "v.ivf", "-itsoffset", "0.400", "-i", "a.ogg"]
        );
        // Sem áudio não há nada a acertar.
        let args = textos(&compose_args(&[&v], &[], None, None));
        assert_eq!(args[..2], ["-i", "v.ivf"]);

        // Grelha: o preto de cada célula conta-se em quadros da cadência da
        // grelha, com o `fps` antes; o resto (menos de um quadro) vai no `setpts`.
        assert_eq!(
            grid_cell_chain(1, 640, 360, 31_457),
            "[1:v]setpts=PTS+0.0237/TB,fps=30,\
             scale=640:360:force_original_aspect_ratio=decrease,\
             pad=640:360:(ow-iw)/2:(oh-ih)/2,setsar=1,\
             tpad=start=943:start_mode=add:color=black[v1];"
        );
        assert!(grid_cell_chain(0, 640, 360, 0).contains("setpts=PTS+0.0000/TB,fps=30,"));
        assert!(grid_cell_chain(0, 640, 360, 0).contains("tpad=start=0:"));
        let (v1, v2, a1, a2) = (
            pista("1.ivf", "video", 2, 580),
            pista("2.ivf", "video", 2, 31_457),
            pista("1.ogg", "audio", 2, 12),
            pista("2.ogg", "audio", 2, 31_000),
        );
        let args = textos(&compose_args(&[&v1, &v2], &[&a1, &a2], None, None));
        assert_eq!(
            args[..8],
            ["-i", "1.ivf", "-i", "2.ivf", "-i", "1.ogg", "-i", "2.ogg"],
            "na grelha os instantes vão nos filtros, não nas entradas"
        );
        assert_eq!(args[8], "-filter_complex");
        let fc = &args[9];
        for parte in [
            "[0:v]setpts=PTS+0.0133/TB,fps=30,",
            "tpad=start=17:start_mode=add:color=black[v0];",
            "tpad=start=943:start_mode=add:color=black[v1];",
            "[v0][v1]xstack=inputs=2:layout=0_0|640_0:fill=black[vout];",
            "adelay=92:all=1[a0];",
            "adelay=31080:all=1[a1];",
        ] {
            assert!(fc.contains(parte), "falta «{parte}» em {fc}");
        }
        assert!(
            !fc.contains("start_duration"),
            "o preto em segundos deriva 1 % do offset: {fc}"
        );
    }

    /// Um vídeo VP8 a sério, um quadro por pacote RTP: `total_ms` a 30 fps de
    /// preto, com um clarão branco de 100 ms aos `clarao_ms`. `None` se a
    /// máquina não tem ffmpeg.
    fn video_com_clarao(
        dir: &Path,
        clarao_ms: u32,
        total_ms: u32,
    ) -> Option<Vec<webrtc::rtp::packet::Packet>> {
        let bin = std::env::var("FFMPEG_BIN").unwrap_or_else(|_| "ffmpeg".into());
        let ivf = dir.join("clarao.ivf");
        let (de, ate) = (clarao_ms as f64 / 1000.0, (clarao_ms + 99) as f64 / 1000.0);
        let grafo = format!(
            "color=c=black:s=64x64:r=30:d={d}[p];color=c=white:s=64x64:r=30:d={d}[b];\
             [p][b]overlay=enable='between(t,{de},{ate})'",
            d = total_ms as f64 / 1000.0
        );
        let res = std::process::Command::new(&bin)
            .args(["-y", "-loglevel", "error", "-nostdin", "-f", "lavfi", "-i"])
            .arg(&grafo)
            .args(["-c:v", "libvpx", "-b:v", "200k", "-g", "300"])
            .args(["-auto-alt-ref", "0", "-lag-in-frames", "0", "-f", "ivf"])
            .arg(&ivf)
            .output()
            .ok()?;
        assert!(
            res.status.success(),
            "ffmpeg falhou: {}",
            String::from_utf8_lossy(&res.stderr)
        );
        let b = std::fs::read(&ivf).unwrap();
        let mut pacotes = Vec::new();
        let mut i = 32;
        while i + 12 <= b.len() {
            let n = u32::from_le_bytes(b[i..i + 4].try_into().unwrap()) as usize;
            let quadro = u64::from_le_bytes(b[i + 4..i + 12].try_into().unwrap()) as u32;
            let mut payload = vec![0x10u8]; // descritor: S=1, PID=0
            payload.extend_from_slice(&b[i + 12..i + 12 + n]);
            pacotes.push(webrtc::rtp::packet::Packet {
                header: webrtc::rtp::header::Header {
                    sequence_number: pacotes.len() as u16,
                    timestamp: 50_000 + quadro * 3000,
                    marker: true,
                    payload_type: 96,
                    ..Default::default()
                },
                payload: payload.into(),
            });
            i += 12 + n;
        }
        Some(pacotes)
    }

    /// Compõe com os argumentos de PRODUÇÃO (`compose_args`) e devolve o webm.
    fn compoe(dir: &Path, args: &[std::ffi::OsString]) -> PathBuf {
        let bin = std::env::var("FFMPEG_BIN").unwrap_or_else(|_| "ffmpeg".into());
        let webm = dir.join("out.webm");
        let res = std::process::Command::new(&bin)
            .args(["-y", "-loglevel", "error", "-nostdin"])
            .args(args)
            .arg(&webm)
            .output()
            .unwrap();
        assert!(
            res.status.success(),
            "ffmpeg falhou: {}",
            String::from_utf8_lossy(&res.stderr)
        );
        webm
    }

    /// O instante (ms, pelos PTS do ficheiro) em que a zona `recorte`
    /// (`w:h:x:y`) da imagem passa de escura a clara.
    fn clarao_aos(webm: &Path, recorte: &str) -> u64 {
        let bin = std::env::var("FFMPEG_BIN").unwrap_or_else(|_| "ffmpeg".into());
        let res = std::process::Command::new(&bin)
            .args(["-nostdin", "-loglevel", "info", "-i"])
            .arg(webm)
            .args(["-map", "0:v:0", "-vf"])
            .arg(format!("crop={recorte},scale=4:4,format=gray,showinfo"))
            .args(["-fps_mode", "passthrough", "-f", "null", "-"])
            .output()
            .unwrap();
        let log = String::from_utf8_lossy(&res.stderr);
        let campo = |l: &str, chave: &str| -> Option<f64> {
            let resto = &l[l.find(chave)? + chave.len()..];
            let fim = resto
                .find(|c: char| !(c.is_ascii_digit() || c == '.'))
                .unwrap_or(resto.len());
            resto[..fim].parse().ok()
        };
        log.lines()
            .filter_map(|l| Some((campo(l, "pts_time:")?, campo(l, "mean:[")?)))
            .find(|(_, luz)| *luz > 128.0)
            .map(|(t, _)| (t * 1000.0).round() as u64)
            .expect("a composição não tem o clarão")
    }

    /// O instante (ms, a contar amostras) em que o tom a `freq` começa.
    fn tom_aos(webm: &Path, freq: f32) -> u64 {
        let bin = std::env::var("FFMPEG_BIN").unwrap_or_else(|_| "ffmpeg".into());
        let res = std::process::Command::new(&bin)
            .args(["-nostdin", "-loglevel", "error", "-i"])
            .arg(webm)
            .args(["-map", "0:a:0", "-ac", "1", "-ar", "48000"])
            .args(["-f", "s16le", "-"])
            .output()
            .unwrap();
        let (pares, _) = res.stdout.as_chunks::<2>();
        let pcm: Vec<i16> = pares.iter().map(|b| i16::from_le_bytes(*b)).collect();
        (0..pcm.len() / 48)
            .step_by(5)
            .find(|t| nivel(&pcm, freq, *t, t + 10) > 0.1)
            .expect("a composição não tem o tom") as u64
    }

    /// Um publicador para os testes de sincronismo: áudio com um toque a
    /// `freq` ao primeiro segundo da pista, vídeo com um clarão ao primeiro
    /// segundo da pista. O áudio flui desde que o writer é ligado; o vídeo só
    /// recebe o keyframe `espera_ms` depois (quadros delta até lá). Devolve
    /// quanto a imagem ficou atrás do som NO RELÓGIO DA SESSÃO, em ms: é o que
    /// a gravação tem de mostrar.
    async fn publica(
        s: &mut RecordingSession,
        video: &[webrtc::rtp::packet::Packet],
        freq: f32,
        espera_ms: u64,
    ) -> i64 {
        let (wa, _) = abre_pista(s, "audio");
        let (wv, _) = abre_pista(s, "video");
        let mut enc = opus_rs::OpusEncoder::new(48_000, 1, opus_rs::Application::Voip).unwrap();
        enc.bitrate_bps = 32_000;
        let mut out = vec![0u8; 1500];
        let som_aos = agora_ms(s);
        for i in 0..150u32 {
            let pcm: Vec<f32> = (0..960u32)
                .map(|k| {
                    if !(50..55).contains(&i) {
                        return 0.0;
                    }
                    let n = (i * 960 + k) as f32;
                    0.5 * (2.0 * std::f32::consts::PI * freq * n / 48_000.0).sin()
                })
                .collect();
            let len = enc.encode(&pcm, 960, &mut out).unwrap();
            wa.write_rtp(&webrtc::rtp::packet::Packet {
                header: webrtc::rtp::header::Header {
                    sequence_number: i as u16,
                    timestamp: 90_000 + i * 960,
                    payload_type: 111,
                    ..Default::default()
                },
                payload: out[..len].to_vec().into(),
            });
        }
        let mut seq = 60_000u16;
        for p in vp8_quadro_delta(&mut seq, 1000) {
            wv.write_rtp(&p);
        }
        dorme(espera_ms).await;
        let imagem_aos = agora_ms(s);
        for p in video {
            wv.write_rtp(p);
        }
        assert_eq!(wa.close().await + wv.close().await, 0);
        imagem_aos as i64 - som_aos as i64
    }

    /// Os argumentos de produção para as pistas de `s`, como o `finalize_inner`
    /// os pede.
    fn args_de_producao(s: &RecordingSession) -> Vec<std::ffi::OsString> {
        let pistas: Vec<ComposeTrack> = s.tracks.iter().map(RecTrackMeta::to_compose).collect();
        let de = |audio: bool| -> Vec<&ComposeTrack> {
            pistas
                .iter()
                .filter(|t| t.kind.ends_with("audio") == audio)
                .collect()
        };
        compose_args(&de(false), &de(true), None, None)
    }

    /// Quanto a medição pode fugir: um quadro de vídeo a 30 fps (33 ms), meio
    /// quadro do `fps` da grelha (17 ms) e o passo da procura do tom (5 ms).
    const FOLGA_MS: i64 = 50;
    const SEM_FFMPEG_SINC: &str = "ffmpeg indisponível — o sincronismo NÃO foi verificado";

    /// O caminho de um publicador com os argumentos de produção: o vídeo em
    /// cópia tem de entrar atrasado o que esperou pelo keyframe.
    #[tokio::test]
    async fn o_som_e_a_imagem_ficam_juntos_com_um_publicador() {
        let mut s = sessao_de_teste().await;
        let Some(video) = video_com_clarao(&s.dir, 1000, 3000) else {
            eprintln!("{SEM_FFMPEG_SINC}");
            return;
        };
        let devido = publica(&mut s, &video, 440.0, 400).await;
        let webm = compoe(&s.dir, &args_de_producao(&s));
        let medido = clarao_aos(&webm, "64:64:0:0") as i64 - tom_aos(&webm, 440.0) as i64;
        let _ = std::fs::remove_dir_all(s.dir.parent().unwrap());
        assert!(
            (medido - devido).abs() <= FOLGA_MS,
            "a imagem chegou {devido} ms depois do som e na gravação está a {medido} ms dele"
        );
    }

    /// A grelha: cada célula no seu instante, com o seu som, e o segundo
    /// publicador — que entrou mais tarde — no sítio certo da gravação.
    #[tokio::test]
    async fn o_som_e_a_imagem_ficam_juntos_na_grelha() {
        let mut s = sessao_de_teste().await;
        let Some(video) = video_com_clarao(&s.dir, 1000, 3000) else {
            eprintln!("{SEM_FFMPEG_SINC}");
            return;
        };
        let devido_a = publica(&mut s, &video, 440.0, 300).await;
        dorme(1500).await;
        let entrou_b = agora_ms(&s) as i64;
        let devido_b = publica(&mut s, &video, 1000.0, 0).await;
        let webm = compoe(&s.dir, &args_de_producao(&s));
        let (som_a, som_b) = (tom_aos(&webm, 440.0) as i64, tom_aos(&webm, 1000.0) as i64);
        let medido_a = clarao_aos(&webm, "640:360:0:0") as i64 - som_a;
        let medido_b = clarao_aos(&webm, "640:360:640:0") as i64 - som_b;
        let _ = std::fs::remove_dir_all(s.dir.parent().unwrap());
        assert!(
            (medido_a - devido_a).abs() <= FOLGA_MS,
            "A: a imagem chegou {devido_a} ms depois do som e na gravação está a {medido_a} ms"
        );
        assert!(
            (medido_b - devido_b).abs() <= FOLGA_MS,
            "B: a imagem chegou {devido_b} ms depois do som e na gravação está a {medido_b} ms"
        );
        // O toque de B está ao primeiro segundo da pista dele, que começou
        // quando ele entrou.
        assert!(
            (som_b - (entrou_b + 1000)).abs() <= FOLGA_MS,
            "B entrou aos {entrou_b} ms e o toque dele, um segundo depois, está aos {som_b} ms"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn o_tecto_de_vagas_nunca_e_excedido() {
        use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
        const CAP: usize = 2;
        let slots = crate::fair_slots::FairSlots::new(CAP);
        let now = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for i in 0..8u128 {
            let (slots, now, peak) = (slots.clone(), now.clone(), peak.clone());
            tasks.push(tokio::spawn(async move {
                let _slot = wait_for_slot(
                    slots,
                    Uuid::from_u128(i % 2),
                    Duration::from_secs(60),
                    || async {},
                )
                .await
                .unwrap();
                let n = now.fetch_add(1, SeqCst) + 1;
                peak.fetch_max(n, SeqCst);
                tokio::time::sleep(Duration::from_millis(25)).await;
                now.fetch_sub(1, SeqCst);
            }));
        }
        for t in tasks {
            t.await.unwrap();
        }
        assert!(
            peak.load(SeqCst) <= CAP,
            "composições simultâneas acima do tecto: {}",
            peak.load(SeqCst)
        );
        assert_eq!(
            peak.load(SeqCst),
            CAP,
            "o tecto tem de ser usado, não só respeitado"
        );
    }
    // ------------------------------------------------------------------
    //  O silêncio que o gravador escreve nos buracos de uma pista de áudio
    // ------------------------------------------------------------------

    #[test]
    fn a_duracao_de_um_pacote_opus_le_se_do_toc() {
        // Configuração (5 bits), estéreo (1), código de quadros (2).
        assert_eq!(opus_packet_samples(&OPUS_SILENCE), Some(960));
        assert_eq!(opus_packet_samples(&[0x18]), Some(2880), "SILK, 60 ms");
        assert_eq!(opus_packet_samples(&[0x10]), Some(1920), "SILK, 40 ms");
        assert_eq!(
            opus_packet_samples(&[0x48]),
            Some(960),
            "SILK de banda larga"
        );
        assert_eq!(opus_packet_samples(&[0x60]), Some(480), "híbrido, 10 ms");
        assert_eq!(opus_packet_samples(&[0x78, 9, 9]), Some(960), "híbrido");
        assert_eq!(opus_packet_samples(&[0x80]), Some(120), "CELT, 2,5 ms");
        assert_eq!(
            opus_packet_samples(&[0xf9, 1, 1]),
            Some(1920),
            "dois quadros"
        );
        assert_eq!(
            opus_packet_samples(&[0xfa, 1, 1]),
            Some(1920),
            "dois, desiguais"
        );
        assert_eq!(
            opus_packet_samples(&[0xfb, 3]),
            Some(2880),
            "três, contados"
        );
        assert_eq!(
            opus_packet_samples(&[0xfb, 0x83]),
            Some(2880),
            "com os bits de VBR"
        );
        assert_eq!(
            opus_packet_samples(&[0xfb, 6]),
            Some(5760),
            "120 ms, o máximo"
        );
        assert_eq!(opus_packet_samples(&[0xfb, 7]), None, "mais de 120 ms");
        assert_eq!(opus_packet_samples(&[0x1b, 3]), None, "três de 60 ms");
        assert_eq!(opus_packet_samples(&[0xfb, 0]), None, "zero quadros");
        assert_eq!(opus_packet_samples(&[0xfb]), None, "falta a contagem");
        assert_eq!(opus_packet_samples(&[]), None);
    }

    /// O que o plano manda escrever antes de cada pacote de `pacotes`
    /// (`(timestamp, amostras, ms a que chegou)`).
    fn planos(pacotes: &[(u32, u32, u64)]) -> Vec<(OpusGap, Vec<u32>)> {
        let (mut gap, inicio) = (OpusGapFill::default(), Instant::now());
        pacotes
            .iter()
            .map(|(ts, amostras, ms)| {
                let chegou = inicio + Duration::from_millis(*ms);
                let plano = gap.before(*ts, Some(*amostras), chegou);
                let silencios = plano.silences().collect();
                (plano, silencios)
            })
            .collect()
    }

    #[test]
    fn so_ha_silencio_onde_falta_um_pacote_inteiro() {
        // Pacotes de 20 ms seguidos, e de 60 ms seguidos: nenhum buraco. Sem
        // ler a duração do pacote, uma pista em pacotes de 60 ms levava um
        // silêncio por cima de cada um.
        let seguidos = planos(&[(0, 960, 0), (960, 960, 20), (1920, 960, 40)]);
        assert!(
            seguidos.iter().all(|p| p.0 == OpusGap::None),
            "{seguidos:?}"
        );
        let de_60 = planos(&[(0, 2880, 0), (2880, 2880, 60), (5760, 2880, 120)]);
        assert!(de_60.iter().all(|p| p.0 == OpusGap::None), "{de_60:?}");
        // A origem muda o tamanho do pacote a meio, sem buraco.
        let muda = planos(&[
            (0, 2880, 0),
            (2880, 960, 60),
            (3840, 2880, 80),
            (6720, 960, 140),
        ]);
        assert!(muda.iter().all(|p| p.0 == OpusGap::None), "{muda:?}");
        // Menos de 20 ms em falta: não cabe um pacote de silêncio.
        let curto = planos(&[(0, 960, 0), (1440, 960, 30)]);
        assert_eq!(curto[1].0, OpusGap::None);
    }

    #[test]
    fn o_silencio_fica_um_pacote_antes_do_primeiro_depois_do_buraco() {
        // Um pacote perdido: o silêncio ocupa exactamente o lugar dele.
        let perda = planos(&[(0, 960, 0), (1920, 960, 40)]);
        assert_eq!(perda[1].1, vec![960]);
        // DTX: 380 ms sem nada, e o silêncio 20 ms antes do pacote seguinte.
        let dtx = planos(&[(0, 960, 0), (19_200, 960, 400)]);
        assert_eq!(dtx[1].1, vec![19_200 - 960]);
        // Se o pacote seguinte é de 60 ms, é a 60 ms dele que o silêncio fica:
        // o demuxer dá ao pacote o instante do silêncio, e o pacote a seguir
        // tem de lhe ficar encostado.
        let de_60 = planos(&[(0, 2880, 0), (48_000, 2880, 1000)]);
        assert_eq!(de_60[1].1, vec![48_000 - 2880]);
        // Falta um quadro de 20 ms antes de um pacote de 60 ms: o silêncio
        // não cabe depois do último escrito, e não se escreve.
        let nao_cabe = planos(&[(0, 960, 0), (1920, 2880, 40)]);
        assert_eq!(nao_cabe[1].0, OpusGap::None);
    }

    #[test]
    fn um_buraco_comprido_leva_um_silencio_a_cada_5_s() {
        let passo = OPUS_GAP_MARK_EVERY;
        // 12 s: aos 5 e aos 10 s do último pacote, e 20 ms antes do seguinte.
        let doze = planos(&[(0, 960, 0), (576_000, 960, 12_000)]);
        assert_eq!(doze[1].1, vec![passo, 2 * passo, 576_000 - 960]);
        // O último dos 5 s não se escreve se ficar em cima do silêncio final.
        let justo = planos(&[(0, 960, 0), (passo + 1440, 960, 5030)]);
        assert_eq!(justo[1].1, vec![passo + 480]);
        let cabe = planos(&[(0, 960, 0), (passo + 1920, 960, 5040)]);
        assert_eq!(cabe[1].1, vec![passo, passo + 960]);
        // Nenhum salto entre o que se escreve chega aos 10 s do ffmpeg.
        let hora = planos(&[(0, 960, 0), (3600 * 48_000, 960, 3_600_000)]);
        let escritos: Vec<u32> = std::iter::once(0)
            .chain(hora[1].1.iter().copied())
            .chain([3600 * 48_000])
            .collect();
        assert_eq!(hora[1].1.len(), 720);
        assert!(
            escritos
                .windows(2)
                .all(|w| w[1] > w[0] && w[1] - w[0] <= passo + 960),
            "saltos entre silêncios acima de 5 s"
        );
        // E atravessa a volta dos 32 bits sem se perder.
        let antes = u32::MAX - 959;
        let volta = planos(&[
            (antes, 960, 0),
            (antes.wrapping_add(12 * 48_000), 960, 12_000),
        ]);
        assert_eq!(
            volta[1].1,
            vec![
                antes.wrapping_add(passo),
                antes.wrapping_add(2 * passo),
                antes.wrapping_add(12 * 48_000 - 960)
            ]
        );
    }

    #[test]
    fn um_salto_maior_do_que_o_tempo_que_passou_nao_se_enche() {
        let hora = 3600 * 48_000u32;
        // Seis horas de salto num pacote que chega 20 ms depois do anterior.
        let corrompido = planos(&[(0, 960, 0), (960, 960, 20), (6 * hora, 960, 40)]);
        assert_eq!(corrompido[2].0, OpusGap::Refused(6 * hora - 1920));
        assert!(corrompido[2].1.is_empty());
        // Os mesmos 6 min de silêncio, a chegar 6 min depois, e a chegar já.
        let seis_min = 360 * 48_000;
        let calado = planos(&[(0, 960, 0), (seis_min, 960, 360_000)]);
        assert_eq!(calado[1].1.len(), 72);
        let falso = planos(&[(0, 960, 0), (seis_min, 960, 20)]);
        assert_eq!(falso[1].0, OpusGap::Refused(seis_min - 960));
        // A folga: 11 s de salto enchem-se se tiver passado 1,2 s, e não se
        // tiverem passado 20 ms.
        let folga = planos(&[(0, 960, 0), (528_000, 960, 1200)]);
        assert_eq!(folga[1].1.len(), 3);
        let sem_folga = planos(&[(0, 960, 0), (528_000, 960, 20)]);
        assert_eq!(sem_folga[1].0, OpusGap::Refused(528_000 - 960));
    }

    #[test]
    fn ate_10_s_o_salto_enche_se_sempre() {
        // Até aos 10 s a composição já enchia o salto sozinha: o silêncio do
        // gravador só muda onde fica o primeiro pacote, e o tecto não se
        // aplica. Aqui a pista vai 20 s à frente do relógio (mil pacotes
        // seguidos que chegam no mesmo instante — uma origem com o relógio
        // adiantado chega lá em horas) e um buraco de DTX enche-se na mesma.
        let mut pacotes: Vec<(u32, u32, u64)> = (0..1000u32).map(|i| (i * 960, 960, 0)).collect();
        let ultimo = 999 * 960;
        pacotes.push((ultimo + 19_200, 960, 400));
        pacotes.push((ultimo + 19_200 + 480_000, 960, 10_400));
        // Acima dos 10 s o tecto vale, e esta pista já o gastou.
        pacotes.push((ultimo + 19_200 + 480_000 + 576_000, 960, 22_400));
        let planos_todos = planos(&pacotes);
        assert_eq!(planos_todos[1000].1, vec![ultimo + 19_200 - 960]);
        assert_eq!(
            planos_todos[1001].1.len(),
            2,
            "10 s certos: aos 5 s e antes"
        );
        assert_eq!(planos_todos[1002].0, OpusGap::Refused(576_000 - 960));
    }

    #[test]
    fn os_saltos_recusados_nao_dao_folga_nova() {
        // Quem salte 12 s em cada pacote, a 50 pacotes por segundo: em 10 s
        // de relógio cabem, com a folga, 20 s de pista — UM salto enchido em
        // 499, e não um por cada recusa.
        let pacotes: Vec<(u32, u32, u64)> = (0..500u32)
            .map(|i| (i * 12 * 48_000, 960, u64::from(i) * 20))
            .collect();
        let planos_todos = planos(&pacotes);
        let enchidos = planos_todos.iter().filter(|p| !p.1.is_empty()).count();
        assert_eq!(enchidos, 1);
        let recusados = planos_todos
            .iter()
            .filter(|p| matches!(p.0, OpusGap::Refused(_)))
            .count();
        assert_eq!(recusados, 498);
        // Depois de um salto recusado, um silêncio a sério volta a ser enchido:
        // o salto não ficou a contar como tempo da pista.
        let hora = 3600 * 48_000u32;
        let depois = planos(&[
            (0, 960, 0),
            (hora, 960, 20),
            (hora + 960, 960, 40),
            (hora + 960 + 30 * 48_000, 960, 30_040),
        ]);
        assert_eq!(depois[1].0, OpusGap::Refused(hora - 960));
        assert_eq!(depois[3].1.len(), 6);
    }

    #[test]
    fn um_pacote_que_nao_diz_quanto_dura_nao_se_rodeia_de_silencio() {
        let (mut gap, t) = (OpusGapFill::default(), Instant::now());
        assert_eq!(gap.before(0, Some(960), t), OpusGap::None);
        assert_eq!(gap.before(48_000, None, t), OpusGap::None);
        // Para o seguinte, o ilegível contou como 20 ms.
        assert_eq!(gap.before(48_960, Some(960), t), OpusGap::None);
        assert_eq!(
            gap.before(50_880, Some(960), t),
            OpusGap::Fill {
                after: 48_960,
                last: 49_920
            }
        );
    }

    /// Um salto que o tempo não explica não se enche — e CONTA-SE.
    ///
    /// Seis horas de salto entre dois pacotes que chegaram a seguir um ao
    /// outro: um timestamp corrompido, uma origem que recomeçou o relógio lá à
    /// frente, ou quem queira esticar a gravação dos outros. Enchê-lo escrevia
    /// 4 320 páginas de silêncio (uma de 5 em 5 s) e seis horas de áudio num
    /// ficheiro de 80 ms de fala. Fica como sempre ficou, e vai a contador: um
    /// salto por encher nunca é silencioso (R18).
    ///
    /// Este teste existe porque uma mutação o pediu: tirar o
    /// `unfilled.saturating_add(samples)` do caminho de escrita não fazia
    /// falhar nada. Agora faz.
    #[tokio::test]
    async fn um_salto_que_o_tempo_nao_explica_conta_se_e_nao_se_enche() {
        let seis_horas = 6 * 3600 * 48_000;
        let pista = grava_audio(&[0, 960, seis_horas, seis_horas + 960]).await;
        assert!(pista.fechada, "a thread de escrita morreu antes do fecho");
        assert_eq!(
            pista.granulos().len(),
            4,
            "nem uma página a mais: {:?}",
            pista.granulos()
        );
        assert_eq!(pista.por_encher, 1, "o salto recusado foi contado");
        assert_eq!(pista.atrasados, 0, "nenhum pacote chegou atrasado");
    }
    // ===== Gravação ISO: o interruptor não mente (A-02 fatia 1) =====

    /// Uma org, uma sala e uma gravação. Com `iso` dá também um estúdio com o
    /// interruptor no estado pedido. Devolve `(room_id, rec_id)`.
    async fn semeia_sala_com_gravacao(db: &sqlx::PgPool, iso: Option<bool>) -> (Uuid, Uuid) {
        let (room_id, org_id, user_id): (Uuid, Uuid, Uuid) = sqlx::query_as(
            "WITH u AS (INSERT INTO users (email, username, password_hash)
                        VALUES ('iso@x.test', 'iso', 'x') RETURNING id),
                  o AS (INSERT INTO organizations (name, slug, created_by)
                        SELECT 'X', 'x-iso', id FROM u RETURNING id, created_by)
             INSERT INTO rooms (code, name, owner_id, topology, waiting_room, e2ee, format)
             SELECT 'iso-sala-1', 'Sala', created_by, 'sfu', false, false, 'normal' FROM o
             RETURNING id, (SELECT id FROM o), owner_id",
        )
        .fetch_one(db)
        .await
        .unwrap();
        if let Some(on) = iso {
            sqlx::query(
                "INSERT INTO studios (id, org_id, name, room_id, iso_recording, created_by)
                 VALUES (gen_random_uuid(), $1, 'Estúdio', $2, $3, $4)",
            )
            .bind(org_id)
            .bind(room_id)
            .bind(on)
            .bind(user_id)
            .execute(db)
            .await
            .unwrap();
        }
        let rec: Uuid = sqlx::query_scalar(
            "INSERT INTO recordings (room_id, uploader_id, filename, size_bytes)
             VALUES ($1, $2, 'composta.webm', 4) RETURNING id",
        )
        .bind(room_id)
        .bind(user_id)
        .fetch_one(db)
        .await
        .unwrap();
        (room_id, rec)
    }

    /// Dois ficheiros com conteúdo numa pasta `tmp-…`, como o recorder os
    /// deixa, e o manifesto que lhes corresponde.
    fn semeia_segmentos(raiz: &Path) -> (PathBuf, ComposeManifest) {
        semeia_segmentos_de(raiz, &[(KIND_VIDEO, 2048), (KIND_AUDIO, 2048)])
    }

    /// O mesmo, com os `kind` e os tamanhos escolhidos: é o que permite semear
    /// uma pista `screen-audio` (a que a base recusava) e uma pista VAZIA (a
    /// que fica no disco sem linha se ninguém reconciliar).
    fn semeia_segmentos_de(raiz: &Path, pistas: &[(&str, usize)]) -> (PathBuf, ComposeManifest) {
        let dir = raiz.join(format!("tmp-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut tracks = Vec::new();
        for (n, (kind, bytes)) in pistas.iter().enumerate() {
            // A mesma escolha do `open_track`, e pela mesma via.
            let is_audio = kind.ends_with("audio");
            let ext = if is_audio { "ogg" } else { "ivf" };
            // O MESMO nome que o `open_track` dá: `{n:02}-{kind}.{ext}`.
            let nome = format!("{n:02}-{kind}.{ext}");
            let caminho = dir.join(&nome);
            std::fs::write(&caminho, vec![7u8; *bytes]).unwrap();
            tracks.push(ComposeTrack {
                path: caminho,
                kind: (*kind).to_string(),
                starts_at_ms: 100 * (n as u64 + 1),
                publisher: Uuid::new_v4(),
            });
        }
        let m = ComposeManifest {
            dir: dir.clone(),
            tracks,
            expected_ms: 1000,
            duration_secs: Some(1),
            by_user: Uuid::new_v4(),
        };
        (dir, m)
    }

    /// Os nomes dos ficheiros que estão numa pasta, ordenados.
    fn ficheiros(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .map(|d| {
                d.filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }

    /// Com o interruptor LIGADO as pistas ficam registadas e os ficheiros saem
    /// do `tmp-` (que o varredor de órfãos apaga) para `iso-<gravação>`.
    #[sqlx::test(migrations = "./migrations")]
    async fn com_iso_ligado_as_pistas_ficam_e_saem_do_tmp(db: sqlx::PgPool) {
        let raiz = std::env::temp_dir().join(format!("iso-ligado-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&raiz).unwrap();
        let (room_id, rec) = semeia_sala_com_gravacao(&db, Some(true)).await;
        let (dir, manifest) = semeia_segmentos(&raiz);
        let studio = crate::studio_realtime::StudioHub::default();
        let m = crate::metrics::Metrics::default();

        let guardar =
            registar_pistas_iso(&db, &raiz, &studio, room_id, &manifest, Some(rec), &m).await;

        assert!(guardar, "com iso ligado os ficheiros têm de FICAR");
        let n: i64 =
            sqlx::query_scalar("SELECT count(*) FROM recording_tracks WHERE recording_id = $1")
                .bind(rec)
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(n, 2, "as duas pistas tinham de ser registadas");
        // Saiu do `tmp-`: é isto que impede o varredor de as apagar quando a
        // gravação deixa de estar em `processing`.
        assert!(
            !dir.exists(),
            "a pasta tmp- ainda existe — o varredor vai apagar as pistas"
        );
        let destino = iso_dir(&raiz, rec);
        assert!(
            destino.join("00-video.ivf").exists(),
            "o ficheiro não está em iso-<gravação>"
        );
        assert!(destino.join("01-audio.ogg").exists());
        std::fs::remove_dir_all(&raiz).ok();
    }

    /// A PASTA FICA REGISTADA, e é o que fecha a fuga do processo morto: a
    /// linha de `recording_artifacts` nomeia a pasta, logo a retenção e o
    /// `sweep_orphans` sabem o que apagar mesmo que os `INSERT` das pistas
    /// nunca cheguem a correr.
    #[sqlx::test(migrations = "./migrations")]
    async fn a_pasta_das_pistas_fica_registada(db: sqlx::PgPool) {
        let raiz = std::env::temp_dir().join(format!("iso-registada-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&raiz).unwrap();
        let (room_id, rec) = semeia_sala_com_gravacao(&db, Some(true)).await;
        let (_dir, manifest) = semeia_segmentos(&raiz);
        let studio = crate::studio_realtime::StudioHub::default();
        let m = crate::metrics::Metrics::default();

        assert!(registar_pistas_iso(&db, &raiz, &studio, room_id, &manifest, Some(rec), &m).await);

        let linhas: Vec<(String, String, bool)> = sqlx::query_as(
            "SELECT kind, path, is_dir FROM recording_artifacts WHERE recording_id = $1",
        )
        .bind(rec)
        .fetch_all(&db)
        .await
        .unwrap();
        assert_eq!(
            linhas,
            vec![(
                crate::recording_artifacts::KIND_ISO_TRACKS.to_string(),
                format!("iso-{rec}"),
                true
            )],
            "a pasta das pistas não ficou registada — um processo morto a meio \
             deixava-a sem nada que a nomeasse"
        );
        std::fs::remove_dir_all(&raiz).ok();
    }

    /// CONTROLO NEGATIVO da ORDEM, e é a prova de que a linha vem ANTES do
    /// `rename` e não depois: sem a tabela do registo, o `INSERT` falha — e os
    /// ficheiros têm de ficar no `tmp-<sessão>`, onde o `sweep_orphan_segments`
    /// os recolhe. Se o registo corresse depois da renomeação, a pasta já
    /// estaria em `iso-<gravação>` e este teste apanhava-o.
    ///
    /// Também é o único sítio onde se consegue provar o modo de falha: contra
    /// Postgres real não se sabe provocar um erro da base de outra maneira.
    #[sqlx::test(migrations = "./migrations")]
    async fn sem_registo_as_pistas_ficam_no_tmp(db: sqlx::PgPool) {
        let raiz = std::env::temp_dir().join(format!("iso-sem-registo-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&raiz).unwrap();
        let (room_id, rec) = semeia_sala_com_gravacao(&db, Some(true)).await;
        let (dir, manifest) = semeia_segmentos(&raiz);
        let studio = crate::studio_realtime::StudioHub::default();
        let m = crate::metrics::Metrics::default();
        sqlx::query("DROP TABLE recording_artifacts")
            .execute(&db)
            .await
            .unwrap();

        let guardar =
            registar_pistas_iso(&db, &raiz, &studio, room_id, &manifest, Some(rec), &m).await;

        // `true` = «não apagues»: os segmentos ficam para o varredor, que é o
        // recuo que não perde nada.
        assert!(
            guardar,
            "sem registo os segmentos não se apagam — ficam para o varredor"
        );
        assert!(
            dir.exists(),
            "a pasta saiu do tmp- sem estar registada — a ordem está invertida e a fuga está aberta"
        );
        assert!(
            !iso_dir(&raiz, rec).exists(),
            "a pasta foi renomeada sem registo: {:?}",
            ficheiros(&raiz)
        );
        std::fs::remove_dir_all(&raiz).ok();
    }

    /// CONTROLO NEGATIVO, e não é opcional: numa sala SEM estúdio, ou com o
    /// interruptor desligado, nada se registra e o chamador apaga como sempre.
    /// Guardar ficheiros que ninguém pediu é uma fuga de disco silenciosa.
    #[sqlx::test(migrations = "./migrations")]
    async fn sem_iso_nada_se_registra_e_os_ficheiros_sao_para_apagar(db: sqlx::PgPool) {
        let raiz = std::env::temp_dir().join(format!("iso-desligado-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&raiz).unwrap();
        let studio = crate::studio_realtime::StudioHub::default();

        // (a) estúdio com o interruptor DESLIGADO
        let (room_id, rec) = semeia_sala_com_gravacao(&db, Some(false)).await;
        let (dir, manifest) = semeia_segmentos(&raiz);
        let m = crate::metrics::Metrics::default();
        let guardar =
            registar_pistas_iso(&db, &raiz, &studio, room_id, &manifest, Some(rec), &m).await;
        assert!(!guardar, "com iso DESLIGADO os ficheiros não se guardam");
        assert!(
            dir.exists(),
            "a pasta não devia ter sido movida — o chamador vai apagá-la"
        );
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM recording_tracks")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(n, 0, "registou pistas de uma gravação que não as pediu");
        std::fs::remove_dir_all(&raiz).ok();
    }

    /// Sem linha na biblioteca não há a que pendurar pistas: não se guarda nada.
    #[sqlx::test(migrations = "./migrations")]
    async fn sem_gravacao_na_biblioteca_nao_se_guarda(db: sqlx::PgPool) {
        let raiz = std::env::temp_dir().join(format!("iso-sem-linha-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&raiz).unwrap();
        let (room_id, _rec) = semeia_sala_com_gravacao(&db, Some(true)).await;
        let (_dir, manifest) = semeia_segmentos(&raiz);
        let studio = crate::studio_realtime::StudioHub::default();
        let m = crate::metrics::Metrics::default();
        assert!(!registar_pistas_iso(&db, &raiz, &studio, room_id, &manifest, None, &m).await);
        std::fs::remove_dir_all(&raiz).ok();
    }

    /// Os TRÊS estados da leitura do interruptor, e o do meio é o que custava
    /// uma gravação: um erro da base NÃO é «iso desligado».
    #[test]
    fn um_erro_da_base_nao_autoriza_apagar_as_pistas() {
        // O estúdio pediu ISO.
        assert_eq!(decide_iso(Some(Some(true))), IsoDecision::Register);
        // Interruptor desligado, e sala sem estúdio: apaga-se como sempre —
        // guardar ficheiros que ninguém pediu é uma fuga de disco.
        assert_eq!(decide_iso(Some(Some(false))), IsoDecision::Delete);
        assert_eq!(decide_iso(Some(None)), IsoDecision::Delete);
        // A BASE NÃO RESPONDEU. Não se apaga: era isto que o `.ok().flatten()`
        // antigo fazia passar por «desligado», deitando fora as pistas de um
        // estúdio que as pediu sem deixar um aviso.
        assert_eq!(decide_iso(None), IsoDecision::Keep);
    }

    // ===== Fuga 1: a pista de áudio do ecrã desaparecia em silêncio =====

    /// O caso que o `CHECK` da 0124 recusava. Quem partilha ecrã com som
    /// publica DUAS pistas de áudio — o microfone e o som do sistema — e a
    /// segunda chega aqui com `kind == "screen-audio"` (`sfu.rs`). A base
    /// rejeitava o `INSERT`, o erro ia para o log, e o ficheiro ficava em
    /// `iso-<gravação>` para sempre sem linha que o pudesse pedir.
    ///
    /// A observação é o `SELECT count(*)`, não o `true` da função.
    #[sqlx::test(migrations = "./migrations")]
    async fn a_pista_de_audio_do_ecra_e_registada_e_fica_no_disco(db: sqlx::PgPool) {
        let raiz = std::env::temp_dir().join(format!("iso-screen-audio-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&raiz).unwrap();
        let (room_id, rec) = semeia_sala_com_gravacao(&db, Some(true)).await;
        // Exactamente o que o SFU produz numa partilha de ecrã com som.
        let (_dir, manifest) = semeia_segmentos_de(
            &raiz,
            &[
                (KIND_VIDEO, 2048),
                (KIND_AUDIO, 2048),
                (KIND_SCREEN, 2048),
                (KIND_SCREEN_AUDIO, 2048),
            ],
        );
        let studio = crate::studio_realtime::StudioHub::default();
        let m = crate::metrics::Metrics::default();

        let guardar =
            registar_pistas_iso(&db, &raiz, &studio, room_id, &manifest, Some(rec), &m).await;
        assert!(guardar);

        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM recording_tracks
              WHERE recording_id = $1 AND kind = 'screen-audio'",
        )
        .bind(rec)
        .fetch_one(&db)
        .await
        .unwrap();
        assert_eq!(n, 1, "a pista de áudio do ecrã NÃO foi registada");
        // As quatro, e o som do ecrã separado do microfone — é o propósito do
        // ISO (ADR-0014 §3: cada faixa antes de qualquer mistura).
        let kinds: Vec<String> = sqlx::query_scalar(
            "SELECT kind FROM recording_tracks WHERE recording_id = $1 ORDER BY kind",
        )
        .bind(rec)
        .fetch_all(&db)
        .await
        .unwrap();
        assert_eq!(kinds, ["audio", "screen", "screen-audio", "video"]);
        // E o ficheiro dessa pista está lá, servível.
        let destino = iso_dir(&raiz, rec);
        assert!(
            destino.join("03-screen-audio.ogg").exists(),
            "ficheiros na pasta: {:?}",
            ficheiros(&destino)
        );
        assert_eq!(
            m.recording_iso_tracks_discarded_total
                .load(std::sync::atomic::Ordering::Relaxed),
            0,
            "nada se devia ter descartado"
        );
        std::fs::remove_dir_all(&raiz).ok();
    }

    /// O vocabulário tem UMA fonte de verdade, e a base é a autoridade: cada
    /// valor de `TRACK_KINDS` tem de ser ACEITE, e um valor inventado tem
    /// de ser RECUSADO.
    ///
    /// O controlo negativo não é decoração: sem ele, este teste passava com um
    /// `CHECK` apagado — que é precisamente a maneira errada de fechar a fuga.
    #[sqlx::test(migrations = "./migrations")]
    async fn a_base_aceita_todos_os_kinds_e_recusa_um_inventado(db: sqlx::PgPool) {
        let (_room_id, rec) = semeia_sala_com_gravacao(&db, Some(true)).await;
        let insere = |kind: &'static str| {
            let db = db.clone();
            async move {
                sqlx::query(
                    "INSERT INTO recording_tracks
                       (recording_id, publisher_id, kind, filename, size_bytes, starts_at_ms)
                     VALUES ($1, gen_random_uuid(), $2, $3, 1, 0)",
                )
                .bind(rec)
                .bind(kind)
                .bind(format!("00-{kind}.bin"))
                .execute(&db)
                .await
            }
        };
        for kind in delonix_meet_domain::content::composition::TRACK_KINDS {
            assert!(
                insere(kind).await.is_ok(),
                "a base recusou «{kind}», que o SFU produz — falta na migração"
            );
        }
        // CONTROLO NEGATIVO: o `CHECK` continua a travar o que não conhece.
        assert!(
            insere("pata-de-elefante").await.is_err(),
            "o CHECK do `kind` deixou de recusar valores desconhecidos"
        );
    }

    // ===== Fuga 2: ficheiros no disco sem linha que os sirva =====

    /// A invariante do `registar_pistas_iso`: a pasta guardada contém
    /// EXACTAMENTE os ficheiros que têm linha. Aqui há três coisas que não
    /// podem ficar — uma pista vazia, um ficheiro que nunca esteve no
    /// manifesto, e uma pasta estranha — e uma que tem de ficar.
    #[sqlx::test(migrations = "./migrations")]
    async fn nenhum_ficheiro_fica_no_disco_sem_linha(db: sqlx::PgPool) {
        let raiz = std::env::temp_dir().join(format!("iso-sem-linha-no-disco-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&raiz).unwrap();
        let (room_id, rec) = semeia_sala_com_gravacao(&db, Some(true)).await;
        // A segunda pista tem SÓ O CABEÇALHO: uma câmara que se ligou e nunca
        // mandou um pacote. 48 bytes e não zero, porque é isso que o
        // `open_track` deixa no disco (o DKIF do IVF são 32 bytes) — um
        // ficheiro de zero bytes é um estado que a produção não sabe criar, e
        // uma fixture que o usasse provava o limiar errado. Não vai para a
        // lista, logo também não pode ficar no disco.
        let (dir, manifest) = semeia_segmentos_de(&raiz, &[(KIND_VIDEO, 2048), (KIND_SCREEN, 48)]);
        // Lixo que não está no manifesto: um `.part` do ffmpeg, e uma pasta.
        std::fs::write(dir.join("out.webm.part"), b"lixo").unwrap();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/x.ivf"), b"lixo").unwrap();
        let studio = crate::studio_realtime::StudioHub::default();
        let m = crate::metrics::Metrics::default();

        let guardar =
            registar_pistas_iso(&db, &raiz, &studio, room_id, &manifest, Some(rec), &m).await;
        assert!(guardar, "a pista com conteúdo tinha de ficar");

        // A OBSERVAÇÃO: o disco contra a base, não a contagem dos INSERT.
        let destino = iso_dir(&raiz, rec);
        let mut linhas: Vec<String> =
            sqlx::query_scalar("SELECT filename FROM recording_tracks WHERE recording_id = $1")
                .bind(rec)
                .fetch_all(&db)
                .await
                .unwrap();
        linhas.sort();
        assert_eq!(linhas, ["00-video.ivf"]);
        assert_eq!(
            ficheiros(&destino),
            linhas,
            "a pasta tem ficheiros que nenhuma linha serve"
        );
        // E o descarte é um CONTADOR, não um erro perdido no log — MAS só conta
        // o que é DEFEITO. Saíram três coisas do disco (a pista só-cabeçalho, o
        // `.part` e a pasta `sub`) e o contador diz DUAS: a pista de uma câmara
        // que nunca publicou é operação normal, e um contador que subisse em
        // quase todas as gravações seria um alarme que o operador desliga — e
        // com ele perdia-se o único sinal da fuga que isto fecha.
        assert_eq!(
            m.recording_iso_tracks_discarded_total
                .load(std::sync::atomic::Ordering::Relaxed),
            2,
            "o contador tem de contar o defeito e NÃO a pista legitimamente vazia"
        );
        std::fs::remove_dir_all(&raiz).ok();
    }
}
