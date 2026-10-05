//! Processamento de áudio da ponte telefone↔sala, sem rede nem SFU.
//!
//! Dois sentidos, e são assimétricos de propósito:
//!
//! - **Telefone → sala** ([`Ingress`]): G.711 a 8 kHz entra, Opus sai. O Opus é
//!   codificado a 8 kHz (banda estreita — é tudo o que a rede telefónica tem)
//!   mas com o relógio RTP de 48 kHz que o RFC 7587 impõe, para qualquer
//!   browser o descodificar como um participante normal. Um só codificador por
//!   chamada.
//! - **Sala → telefone** ([`Mixer`]): cada microfone da sala chega em Opus e é
//!   descodificado a 8 kHz (o descodificador Opus faz a reamostragem), e o
//!   misturador soma todos MENOS a própria chamada (mix-minus — sem isto quem
//!   liga ouve-se a si próprio com atraso) num só fluxo G.711. O telefone só
//!   tem um canal de áudio: a mistura TEM de ser feita aqui.
//!
//! O Opus é o `opus-rs` (Rust puro). A interoperabilidade com a libopus de
//! referência foi medida nos dois sentidos antes de o escolher (ADR-0010).
//!
//! **Quando a perna negoceia Opus** (ADR-0017) os dois sentidos mudam de forma,
//! e continuam assimétricos:
//!
//! - **Telefone → sala** ([`Passthrough`]): o pacote já é Opus e passa INTACTO.
//!   Só se descodifica para o validar e para lhe medir o nível.
//! - **Sala → telefone**: o mesmo [`Mixer`], a 16 kHz em vez de 8, e um
//!   [`MixEncoder`] que devolve a mistura em Opus de banda larga.

use std::collections::{HashMap, VecDeque};

use uuid::Uuid;

use super::g711::Law;

/// 20 ms a 8 kHz.
pub const FRAME_8K: usize = 160;
/// Maior pacote que se aceita do lado Opus: 120 ms a 8 kHz.
const MAX_DECODED_8K: usize = 960;
/// Taxa a que a sala é descodificada e somada, e a que a mistura segue para
/// uma perna em Opus (ADR-0017): banda larga. A 48 kHz o custo por microfone
/// triplicava para dar ao telefone uma banda que o auscultador dele não
/// reproduz.
pub const WIDEBAND_RATE: u32 = 16_000;
/// 20 ms a essa taxa.
const MIX_FRAME: usize = 320;
/// 20 ms de Opus no relógio RTP de 48 kHz (RFC 7587), seja qual for a banda.
pub const OPUS_TS_PER_FRAME: u32 = 960;

/// Nível de áudio RFC 6464 (0 = 0 dBov, o mais alto; 127 = silêncio) de um
/// bloco de PCM. É o que o seletor de oradores do SFU lê das publicações dos
/// browsers; a ponte calcula-o do PCM porque o telefone não manda a extensão.
pub fn audio_level_dbov(pcm: &[i16]) -> u8 {
    if pcm.is_empty() {
        return 127;
    }
    let energy: f64 = pcm.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / pcm.len() as f64;
    if energy <= 0.0 {
        return 127;
    }
    let rms = energy.sqrt() / 32768.0;
    let dbov = -20.0 * rms.log10();
    dbov.clamp(0.0, 127.0) as u8
}

// ============================================================
//  Telefone → sala
// ============================================================

/// Um pacote Opus pronto a entrar na sala.
#[derive(Debug, Clone)]
pub struct OpusFrame {
    pub payload: Vec<u8>,
    /// Timestamp RTP no relógio de 48 kHz.
    pub timestamp: u32,
    /// Nível RFC 6464 do bloco codificado.
    pub level: u8,
}

/// Transcodificador G.711 → Opus de UMA chamada.
pub struct Ingress {
    encoder: opus_rs::OpusEncoder,
    /// Amostras à espera de completar um bloco de 20 ms.
    pending: Vec<i16>,
    /// Timestamp (relógio de 8 kHz do telefone) da primeira amostra pendente.
    pending_ts: Option<u32>,
    /// Primeiro timestamp visto do telefone; a saída conta a partir dele.
    base_ts: Option<u32>,
    scratch_pcm: Vec<i16>,
    scratch_f32: Vec<f32>,
    out: Vec<u8>,
}

impl Ingress {
    pub fn new() -> Result<Self, &'static str> {
        let mut encoder = opus_rs::OpusEncoder::new(8000, 1, opus_rs::Application::Voip)?;
        // Banda estreita: 16 kbps já é transparente para voz a 8 kHz. Mais é
        // largura de banda de upload que o telefone não tinha para dar.
        encoder.bitrate_bps = 16_000;
        encoder.complexity = 5;
        Ok(Self {
            encoder,
            pending: Vec::with_capacity(FRAME_8K * 2),
            pending_ts: None,
            base_ts: None,
            scratch_pcm: Vec::with_capacity(FRAME_8K * 2),
            scratch_f32: vec![0.0; FRAME_8K],
            out: vec![0u8; 512],
        })
    }

    /// Entra um pacote G.711 (payload e timestamp RTP do telefone). Sai zero ou
    /// mais pacotes Opus de 20 ms.
    ///
    /// O timestamp de saída DERIVA do de entrada (×6, de 8 para 48 kHz): um
    /// pacote perdido ou um silêncio suprimido pelo tronco avançam o relógio
    /// da sala na mesma medida, e o browser toca o resto no instante certo em
    /// vez de colar os pedaços.
    pub fn push(&mut self, law: Law, payload: &[u8], rtp_ts: u32) -> Vec<OpusFrame> {
        let base = *self.base_ts.get_or_insert(rtp_ts);
        self.scratch_pcm.clear();
        law.decode(payload, &mut self.scratch_pcm);
        // Salto no relógio do telefone (perda, DTX do tronco): o que estava
        // pendente fica para trás — completa-se com silêncio e sai.
        let mut frames = Vec::new();
        if let Some(pts) = self.pending_ts {
            let expected = pts.wrapping_add(self.pending.len() as u32);
            if expected != rtp_ts && !self.pending.is_empty() {
                self.pending.resize(FRAME_8K, 0);
                if let Some(f) = self.encode_pending(base) {
                    frames.push(f);
                }
            }
        }
        if self.pending.is_empty() {
            self.pending_ts = Some(rtp_ts);
        }
        let mut samples = std::mem::take(&mut self.scratch_pcm);
        let mut offset = 0;
        while offset < samples.len() {
            let take = (FRAME_8K - self.pending.len()).min(samples.len() - offset);
            self.pending
                .extend_from_slice(&samples[offset..offset + take]);
            offset += take;
            if self.pending.len() == FRAME_8K {
                if let Some(f) = self.encode_pending(base) {
                    frames.push(f);
                }
                self.pending_ts = Some(rtp_ts.wrapping_add(offset as u32));
            }
        }
        samples.clear();
        self.scratch_pcm = samples;
        frames
    }

    fn encode_pending(&mut self, base: u32) -> Option<OpusFrame> {
        let ts8 = self.pending_ts.unwrap_or(base);
        for (dst, &s) in self.scratch_f32.iter_mut().zip(&self.pending) {
            *dst = s as f32 / 32768.0;
        }
        let level = audio_level_dbov(&self.pending);
        self.pending.clear();
        match self
            .encoder
            .encode(&self.scratch_f32, FRAME_8K, &mut self.out)
        {
            Ok(n) => Some(OpusFrame {
                payload: self.out[..n].to_vec(),
                timestamp: ts8.wrapping_sub(base).wrapping_mul(6),
                level,
            }),
            Err(e) => {
                tracing::warn!(error = e, "ponte: codificação Opus falhou — bloco perdido");
                None
            }
        }
    }
}

// ============================================================
//  Telefone → sala, quando a perna já fala Opus (ADR-0017)
// ============================================================

/// Um pacote Opus do telefone, pronto a entrar na sala tal como chegou.
#[derive(Debug, Clone, PartialEq)]
pub struct PassedFrame {
    /// Número de sequência RELATIVO: quem publica soma-lhe a sua base aleatória.
    pub seq: u16,
    /// Timestamp RTP RELATIVO, no relógio de 48 kHz.
    pub timestamp: u32,
    /// Nível RFC 6464 do que o pacote contém.
    pub level: u8,
}

/// De onde se conta: um pacote de entrada e a posição de saída que lhe coube.
#[derive(Clone, Copy)]
struct Anchor {
    in_seq: u16,
    in_ts: u32,
    out_seq: u16,
    out_ts: u32,
}

/// Um salto maior do que isto na numeração é outro fluxo (re-INVITE, o
/// FreeSWITCH a recomeçar o RTP), não perda: volta-se a ancorar em vez de
/// publicar um buraco de minutos.
const MAX_SEQ_JUMP: i32 = 1_000;
/// O mesmo, no relógio: dez segundos a 48 kHz.
const MAX_TS_JUMP: i64 = 480_000;

/// Passagem de Opus de UMA chamada: não recodifica, só valida e mede.
///
/// **A numeração e o relógio seguem os da origem.** Um pacote perdido entre o
/// FreeSWITCH e a ponte chega ao browser como um buraco na sequência — é o que
/// faz o FEC e o PLC dele trabalharem. Renumerar de um em um esconderia a
/// perda atrás de um salto de relógio, que o browser lê como silêncio.
pub struct Passthrough {
    mono: Option<opus_rs::OpusDecoder>,
    stereo: Option<opus_rs::OpusDecoder>,
    decoded: Vec<f32>,
    anchor: Option<Anchor>,
    /// A posição a seguir ao pacote mais avançado que já saiu.
    next: (u16, u32),
    /// Pacotes que o descodificador recusou — e que por isso não entraram.
    pub rejected: u64,
}

impl Default for Passthrough {
    fn default() -> Self {
        Self::new()
    }
}

impl Passthrough {
    pub fn new() -> Self {
        Self {
            mono: None,
            stereo: None,
            decoded: vec![0.0; MAX_DECODED_8K * 2],
            anchor: None,
            next: (0, 0),
            rejected: 0,
        }
    }

    /// Houve um intervalo em que nada foi publicado de propósito (a perna
    /// esteve silenciada). O pacote seguinte continua de onde a saída ficou,
    /// em vez de aparecer na sala como segundos de perda.
    pub fn discontinuity(&mut self) {
        self.anchor = None;
    }

    /// Entra um pacote Opus do telefone. `None` se o descodificador o recusar:
    /// o que a ponte não consegue ler não é publicado numa sala.
    pub fn push(&mut self, seq: u16, rtp_ts: u32, payload: &[u8]) -> Option<PassedFrame> {
        if payload.is_empty() {
            self.rejected += 1;
            return None;
        }
        // Bit «s» do TOC: 1 = estéreo (RFC 6716 §3.1).
        let stereo = payload[0] & 0x04 != 0;
        let decoder = if stereo {
            &mut self.stereo
        } else {
            &mut self.mono
        };
        if decoder.is_none() {
            *decoder = opus_rs::OpusDecoder::new(8000, if stereo { 2 } else { 1 }).ok();
        }
        let n = match decoder
            .as_mut()
            .map(|d| d.decode(payload, MAX_DECODED_8K, &mut self.decoded))
        {
            Some(Ok(n)) if n > 0 => n,
            _ => {
                self.rejected += 1;
                return None;
            }
        };
        let channels = if stereo { 2 } else { 1 };
        let level = audio_level_dbov_f32(&self.decoded[..n * channels]);
        // O pacote dura `n` amostras a 8 kHz: ×6 no relógio de 48 kHz.
        let duration = (n as u32).wrapping_mul(6);

        let anchor = match self.anchor {
            Some(a) => {
                let dseq = seq.wrapping_sub(a.in_seq) as i16 as i32;
                let dts = rtp_ts.wrapping_sub(a.in_ts) as i32 as i64;
                if dseq.abs() > MAX_SEQ_JUMP || dts.abs() > MAX_TS_JUMP {
                    None
                } else {
                    Some(a)
                }
            }
            None => None,
        };
        let anchor = *self.anchor.insert(anchor.unwrap_or(Anchor {
            in_seq: seq,
            in_ts: rtp_ts,
            out_seq: self.next.0,
            out_ts: self.next.1,
        }));
        let out_seq = anchor.out_seq.wrapping_add(seq.wrapping_sub(anchor.in_seq));
        let out_ts = anchor
            .out_ts
            .wrapping_add(rtp_ts.wrapping_sub(anchor.in_ts));
        // Só um pacote mais avançado move a posição seguinte: um atrasado
        // (reordenação) sai com o seu número e não puxa a saída para trás.
        if out_seq.wrapping_sub(self.next.0) as i16 >= 0 {
            self.next = (out_seq.wrapping_add(1), out_ts.wrapping_add(duration));
        }
        Some(PassedFrame {
            seq: out_seq,
            timestamp: out_ts,
            level,
        })
    }
}

/// O mesmo nível RFC 6464, de amostras em vírgula flutuante (−1..1).
fn audio_level_dbov_f32(pcm: &[f32]) -> u8 {
    if pcm.is_empty() {
        return 127;
    }
    let energy: f64 = pcm.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / pcm.len() as f64;
    if energy <= 0.0 {
        return 127;
    }
    let dbov = -10.0 * energy.log10();
    dbov.clamp(0.0, 127.0) as u8
}

// ============================================================
//  Sala → telefone
// ============================================================

/// Blocos de 20 ms acumulados antes de uma fonte começar a contar para a
/// mistura: 40 ms absorvem o jitter normal de uma rede sem acrescentar atraso
/// audível.
const PREBUFFER_FRAMES: usize = 2;
/// Tecto por fonte: 120 ms. Acima disto a fonte está a chegar mais depressa do
/// que se consome (relógios diferentes, rajada): corta-se o mais antigo em vez
/// de deixar o atraso crescer para sempre.
const MAX_BUFFER_FRAMES: usize = 6;
/// Uma fonte calada há mais de 2 s sai do misturador (o descodificador é
/// estado, e uma sala de 50 tem 50 microfones que quase nunca falam).
const IDLE_TICKS: u32 = 100;

struct Source {
    mono: Option<opus_rs::OpusDecoder>,
    stereo: Option<opus_rs::OpusDecoder>,
    buf: VecDeque<i16>,
    primed: bool,
    idle_ticks: u32,
    last_seq: Option<u16>,
}

impl Source {
    fn new(max_buffer: usize) -> Self {
        Self {
            mono: None,
            stereo: None,
            buf: VecDeque::with_capacity(max_buffer),
            primed: false,
            idle_ticks: 0,
            last_seq: None,
        }
    }
}

/// Misturador de UMA chamada: tudo o que a sala diz, menos a própria chamada.
pub struct Mixer {
    /// A própria chamada (a sua publicação nunca entra na mistura).
    own: Uuid,
    sources: HashMap<Uuid, Source>,
    decoded: Vec<f32>,
    /// A soma, sempre a 16 kHz.
    mixed: Vec<i32>,
    /// Só numa perna a 8 kHz: o filtro que desce a soma para metade da taxa, e
    /// o bloco já descido.
    down: Option<(Decimator, Vec<i32>)>,
    /// Pacotes que o descodificador recusou (corrompidos ou de outro codec).
    pub decode_errors: u64,
}

impl Mixer {
    /// Misturador que entrega a 8 kHz: o de uma perna G.711.
    pub fn new(own: Uuid) -> Self {
        Self::build(own, true)
    }

    /// Misturador que entrega a 16 kHz: o de uma perna em Opus (ADR-0017).
    pub fn wideband(own: Uuid) -> Self {
        Self::build(own, false)
    }

    /// **Descodifica-se sempre a 16 kHz**, mesmo para entregar a 8. Pedir 8 kHz
    /// ao descodificador do `opus-rs` desce um pacote SILK de banda larga sem
    /// filtro: tudo o que a sala diz entre 4 e 8 kHz dobra para dentro da banda
    /// do telefone (medido a 2026-10-05: um tom de 6 kHz saía inteiro, a
    /// −27 dB, onde a libopus com filtro dá silêncio). A descida faz-se aqui,
    /// uma vez por perna, sobre a soma.
    fn build(own: Uuid, to_8k: bool) -> Self {
        Self {
            own,
            sources: HashMap::new(),
            // 120 ms, o maior pacote Opus, em estéreo.
            decoded: vec![0.0; MIX_FRAME * 6 * 2],
            mixed: vec![0; MIX_FRAME],
            down: to_8k.then(|| (Decimator::new(), Vec::with_capacity(FRAME_8K))),
            decode_errors: 0,
        }
    }

    /// Um pacote Opus de um participante da sala.
    pub fn push(&mut self, publisher: Uuid, seq: u16, payload: &[u8]) {
        if publisher == self.own || payload.is_empty() {
            return;
        }
        let max_buffer = MIX_FRAME * MAX_BUFFER_FRAMES;
        let src = self
            .sources
            .entry(publisher)
            .or_insert_with(|| Source::new(max_buffer));
        // Duplicado ou atrasado (reordenação): descarta-se. Sem jitter buffer
        // reordenador — um pacote fora de ordem num fluxo de voz a 20 ms já
        // passou o instante em que devia tocar.
        if let Some(last) = src.last_seq {
            let delta = seq.wrapping_sub(last);
            if delta == 0 || delta > 0x8000 {
                return;
            }
        }
        src.last_seq = Some(seq);
        src.idle_ticks = 0;
        // Bit «s» do TOC: 1 = estéreo (RFC 6716 §3.1).
        let stereo = payload[0] & 0x04 != 0;
        let decoder = if stereo {
            &mut src.stereo
        } else {
            &mut src.mono
        };
        if decoder.is_none() {
            *decoder =
                opus_rs::OpusDecoder::new(WIDEBAND_RATE as i32, if stereo { 2 } else { 1 }).ok();
        }
        let Some(dec) = decoder.as_mut() else {
            self.decode_errors += 1;
            return;
        };
        match dec.decode(payload, MIX_FRAME * 6, &mut self.decoded) {
            Ok(n) => {
                if stereo {
                    for i in 0..n {
                        let v = (self.decoded[2 * i] + self.decoded[2 * i + 1]) * 0.5;
                        src.buf.push_back(to_i16(v));
                    }
                } else {
                    for &v in &self.decoded[..n] {
                        src.buf.push_back(to_i16(v));
                    }
                }
                while src.buf.len() > max_buffer {
                    src.buf.pop_front();
                }
            }
            Err(_) => self.decode_errors += 1,
        }
    }

    /// Um tique de 20 ms: devolve um bloco misturado — 160 amostras a 8 kHz
    /// numa perna G.711, 320 a 16 kHz numa perna em Opus.
    pub fn tick(&mut self) -> &[i32] {
        for v in self.mixed.iter_mut() {
            *v = 0;
        }
        let frame = MIX_FRAME;
        self.sources.retain(|_, src| {
            if !src.primed {
                if src.buf.len() >= frame * PREBUFFER_FRAMES {
                    src.primed = true;
                } else {
                    src.idle_ticks += 1;
                    return src.idle_ticks < IDLE_TICKS;
                }
            }
            if src.buf.len() < frame {
                // Esvaziou: volta a encher antes de tocar (evita estalidos de
                // 5 ms em 5 ms quando a rede de quem fala engasga).
                src.primed = false;
                src.idle_ticks += 1;
                return src.idle_ticks < IDLE_TICKS;
            }
            for v in self.mixed.iter_mut() {
                *v += src.buf.pop_front().unwrap_or(0) as i32;
            }
            true
        });
        match self.down.as_mut() {
            Some((filter, block)) => {
                filter.process(&self.mixed, block);
                block
            }
            None => &self.mixed,
        }
    }
}

/// Desce a mistura de 16 para 8 kHz com um passa-baixo antes de deitar fora
/// metade das amostras.
///
/// FIR de fase linear, janela de Kaiser, corte a 4 kHz: plano até aos 3,4 kHz
/// da banda telefónica e ~54 dB abaixo a partir dos 4,6 kHz, que é de onde vem
/// o que dobraria para dentro dela. Atraso de grupo: 23 amostras a 16 kHz
/// (1,4 ms).
struct Decimator {
    taps: [f32; DECIMATOR_TAPS],
    /// As últimas `DECIMATOR_TAPS − 1` amostras do bloco anterior.
    history: [f32; DECIMATOR_TAPS - 1],
    scratch: Vec<f32>,
}

const DECIMATOR_TAPS: usize = 47;

impl Decimator {
    fn new() -> Self {
        const BETA: f64 = 5.0;
        let mid = (DECIMATOR_TAPS - 1) as f64 / 2.0;
        let mut taps = [0f32; DECIMATOR_TAPS];
        let mut sum = 0f64;
        let mut ideal = [0f64; DECIMATOR_TAPS];
        for (i, h) in ideal.iter_mut().enumerate() {
            let n = i as f64 - mid;
            // Passa-baixo ideal com corte a um quarto da taxa (4 kHz a 16 kHz).
            let sinc = if n == 0.0 {
                0.5
            } else {
                (std::f64::consts::PI * n / 2.0).sin() / (std::f64::consts::PI * n)
            };
            let r = n / mid;
            let window = bessel_i0(BETA * (1.0 - r * r).max(0.0).sqrt()) / bessel_i0(BETA);
            *h = sinc * window;
            sum += *h;
        }
        // Ganho unitário em contínua: o nível da mistura não muda.
        for (t, h) in taps.iter_mut().zip(ideal) {
            *t = (h / sum) as f32;
        }
        Self {
            taps,
            history: [0.0; DECIMATOR_TAPS - 1],
            scratch: Vec::with_capacity(MIX_FRAME + DECIMATOR_TAPS),
        }
    }

    /// `input` a 16 kHz (um número par de amostras) → `out` a 8 kHz.
    fn process(&mut self, input: &[i32], out: &mut Vec<i32>) {
        self.scratch.clear();
        self.scratch.extend_from_slice(&self.history);
        self.scratch.extend(input.iter().map(|&v| v as f32));
        out.clear();
        for start in (0..input.len()).step_by(2) {
            let window = &self.scratch[start..start + DECIMATOR_TAPS];
            let acc: f32 = window.iter().zip(&self.taps).map(|(x, t)| x * t).sum();
            out.push(acc.round() as i32);
        }
        let keep = self.scratch.len() - (DECIMATOR_TAPS - 1);
        self.history.copy_from_slice(&self.scratch[keep..]);
    }
}

/// Função de Bessel modificada de ordem zero, pela série — só para a janela
/// de Kaiser, calculada uma vez por perna.
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term) = (1.0f64, 1.0f64);
    let half = x / 2.0;
    for k in 1..32 {
        term *= (half / k as f64) * (half / k as f64);
        sum += term;
        if term < 1e-12 * sum {
            break;
        }
    }
    sum
}

fn to_i16(v: f32) -> i16 {
    (v * 32767.0).clamp(-32768.0, 32767.0) as i16
}

/// Soma → G.711, com limitação suave: somar três pessoas a falar alto
/// ultrapassa o `i16`, e cortar a seco distorce toda a gente.
pub fn encode_mix(law: Law, mixed: &[i32], out: &mut Vec<u8>) {
    out.clear();
    let pcm: Vec<i16> = mixed.iter().map(|&s| soft_limit(s)).collect();
    law.encode(&pcm, out);
}

/// Codificador da mistura para uma perna em Opus (ADR-0017): banda larga,
/// mono, um bloco de 20 ms de cada vez.
pub struct MixEncoder {
    encoder: opus_rs::OpusEncoder,
    pcm: Vec<f32>,
    out: Vec<u8>,
}

impl MixEncoder {
    pub fn new() -> Result<Self, &'static str> {
        let mut encoder =
            opus_rs::OpusEncoder::new(WIDEBAND_RATE as i32, 1, opus_rs::Application::Voip)?;
        // Taxa CONSTANTE, e não por gosto: em taxa variável o `opus-rs` ignora
        // o alvo (medido a 2026-10-05: 84 kbps com 32 pedidos). A 32 kbps a
        // banda de 5–7,5 kHz sai a 0,5 dB do codificador da libopus.
        encoder.bitrate_bps = 32_000;
        encoder.use_cbr = true;
        encoder.complexity = 5;
        let frame = (WIDEBAND_RATE / 50) as usize;
        Ok(Self {
            encoder,
            pcm: vec![0.0; frame],
            out: vec![0u8; 512],
        })
    }

    /// Soma → Opus, com a mesma limitação suave do caminho G.711. `None` se o
    /// bloco não tiver o tamanho de 20 ms ou o codificador falhar.
    pub fn encode(&mut self, mixed: &[i32]) -> Option<&[u8]> {
        if mixed.len() != self.pcm.len() {
            return None;
        }
        for (dst, &s) in self.pcm.iter_mut().zip(mixed) {
            *dst = soft_limit(s) as f32 / 32768.0;
        }
        match self.encoder.encode(&self.pcm, mixed.len(), &mut self.out) {
            Ok(n) => Some(&self.out[..n]),
            Err(e) => {
                tracing::warn!(
                    error = e,
                    "ponte: codificação Opus da mistura falhou — bloco perdido"
                );
                None
            }
        }
    }
}

fn soft_limit(s: i32) -> i16 {
    const KNEE: i32 = 24_000;
    let a = s.abs();
    if a <= KNEE {
        return s as i16;
    }
    // Compressão acima do joelho: aproxima-se de 32767 sem o ultrapassar.
    let over = (a - KNEE) as f32;
    let room = (32767 - KNEE) as f32;
    let compressed = KNEE as f32 + room * (over / (over + room));
    (compressed as i32 * s.signum()) as i16
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn tone_8k(freq: f32, n: usize, amp: f32, phase0: usize) -> Vec<i16> {
        (0..n)
            .map(|i| {
                let t = (i + phase0) as f32 / 8000.0;
                (amp * (2.0 * std::f32::consts::PI * freq * t).sin() * 32767.0) as i16
            })
            .collect()
    }

    /// Magnitude de uma frequência (Goertzel), normalizada: um seno de
    /// amplitude A dá ~A/2.
    pub(crate) fn goertzel(x: &[f32], fs: f32, f: f32) -> f32 {
        let w = 2.0 * std::f32::consts::PI * f / fs;
        let c = 2.0 * w.cos();
        let (mut s1, mut s2) = (0f32, 0f32);
        for &v in x {
            let s0 = v + c * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        ((s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0)).sqrt() / x.len() as f32
    }

    #[test]
    fn nivel_rfc6464() {
        assert_eq!(audio_level_dbov(&[0; 160]), 127);
        let full = tone_8k(1000.0, 160, 1.0, 0);
        assert!(audio_level_dbov(&full) <= 4, "seno a 0 dBFS ≈ -3 dBov");
        let quiet = tone_8k(1000.0, 160, 0.01, 0);
        let l = audio_level_dbov(&quiet);
        assert!((40..=46).contains(&l), "-40 dBFS → ~43 dBov, deu {l}");
    }

    /// Telefone → sala → telefone só com os codecs: o tom de 1 kHz que entra em
    /// G.711 sai do descodificador Opus com a mesma frequência e sem energia
    /// fora dela.
    #[test]
    fn ingress_produz_opus_que_o_misturador_descodifica_com_o_tom() {
        let mut ing = Ingress::new().unwrap();
        let mut mix = Mixer::new(Uuid::new_v4());
        let caller = Uuid::new_v4();
        let mut out = Vec::new();
        let mut ts = 1000u32;
        let mut seq = 0u16;
        let mut expect_ts = None;
        for i in 0..100 {
            let pcm = tone_8k(1000.0, 160, 0.5, i * 160);
            let mut ulaw = Vec::new();
            Law::Mu.encode(&pcm, &mut ulaw);
            for f in ing.push(Law::Mu, &ulaw, ts) {
                if let Some(prev) = expect_ts {
                    assert_eq!(f.timestamp, prev, "relógio de 48 kHz contíguo");
                }
                expect_ts = Some(f.timestamp.wrapping_add(OPUS_TS_PER_FRAME));
                assert!(f.level < 20, "tom a -6 dBFS não é silêncio: {}", f.level);
                mix.push(caller, seq, &f.payload);
                seq = seq.wrapping_add(1);
            }
            ts = ts.wrapping_add(160);
            out.extend(mix.tick().iter().map(|&v| v as f32 / 32768.0));
        }
        let tail = &out[out.len() - 4000..];
        let at = goertzel(tail, 8000.0, 1000.0);
        let off = goertzel(tail, 8000.0, 1700.0);
        assert!(at > 0.2, "1 kHz presente: {at}");
        assert!(off < 0.01, "sem energia fora do tom: {off}");
    }

    #[test]
    fn mix_minus_nao_devolve_a_propria_voz() {
        let own = Uuid::new_v4();
        let mut ing = Ingress::new().unwrap();
        let mut mix = Mixer::new(own);
        let mut ulaw = Vec::new();
        Law::A.encode(&tone_8k(500.0, 160, 0.5, 0), &mut ulaw);
        for (i, f) in (0..10)
            .flat_map(|i| ing.push(Law::A, &ulaw, i * 160))
            .enumerate()
        {
            mix.push(own, i as u16, &f.payload);
        }
        assert_eq!(mix.sources.len(), 0);
        assert!(mix.tick().iter().all(|&v| v == 0));
    }

    #[test]
    fn perda_no_telefone_avanca_o_relogio_da_sala() {
        let mut ing = Ingress::new().unwrap();
        let mut ulaw = Vec::new();
        Law::Mu.encode(&tone_8k(700.0, 160, 0.3, 0), &mut ulaw);
        let a = ing.push(Law::Mu, &ulaw, 0);
        // Pacotes de 160..=640 perdidos.
        let b = ing.push(Law::Mu, &ulaw, 800);
        assert_eq!(a[0].timestamp, 0);
        assert_eq!(b[0].timestamp, 800 * 6);
    }

    #[test]
    fn pacotes_de_10_ms_juntam_se_em_blocos_de_20() {
        let mut ing = Ingress::new().unwrap();
        let mut ulaw = Vec::new();
        Law::Mu.encode(&tone_8k(700.0, 80, 0.3, 0), &mut ulaw);
        assert!(ing.push(Law::Mu, &ulaw, 0).is_empty());
        let f = ing.push(Law::Mu, &ulaw, 80);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].timestamp, 0);
        assert!(ing.push(Law::Mu, &ulaw, 160).is_empty());
        let g = ing.push(Law::Mu, &ulaw, 240);
        assert_eq!(g[0].timestamp, 960);
    }

    #[test]
    fn limitador_nao_estoura_o_i16() {
        let mut out = Vec::new();
        encode_mix(Law::Mu, &[90_000, -90_000, 30_000, 0], &mut out);
        let mut back = Vec::new();
        Law::Mu.decode(&out, &mut back);
        assert!(back[0] > 28_000 && back[1] < -28_000, "{back:?}");
        assert_eq!(soft_limit(24_000), 24_000);
        // Acima do joelho comprime-se: aproxima-se do tecto sem lá chegar (e,
        // sobretudo, sem dar a volta ao i16 — que era o defeito a evitar).
        let extremo = soft_limit(1_000_000);
        assert!(extremo > 24_000 && extremo < i16::MAX, "{extremo}");
    }

    // ------------------------------------------------------------
    //  ADR-0017: a perna em Opus
    // ------------------------------------------------------------

    fn tone(rate: f32, freq: f32, n: usize, amp: f32, phase0: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * (i + phase0) as f32 / rate).sin())
            .collect()
    }

    /// Um «microfone» a 16 kHz: blocos de 20 ms de um tom, em Opus de banda larga.
    fn wideband_packets(freq: f32, blocks: usize) -> Vec<Vec<u8>> {
        let mut enc = opus_rs::OpusEncoder::new(16_000, 1, opus_rs::Application::Voip).unwrap();
        enc.bitrate_bps = 32_000;
        enc.use_cbr = true;
        let mut out = vec![0u8; 512];
        (0..blocks)
            .map(|i| {
                let pcm = tone(16_000.0, freq, 320, 0.5, i * 320);
                let n = enc.encode(&pcm, 320, &mut out).unwrap();
                out[..n].to_vec()
            })
            .collect()
    }

    /// Sala → telefone com a perna em Opus: um tom de 6 kHz — acima de tudo o
    /// que 8 kHz conseguem levar — CHEGA ao telefone. E o controlo negativo: a
    /// mesma sala, pelo caminho G.711, não o entrega. Sem o controlo, o teste
    /// passava mesmo que a «banda larga» fosse só o nome.
    #[test]
    fn banda_larga_leva_6_khz_ao_telefone_e_o_g711_nao() {
        let mic = Uuid::new_v4();
        let packets = wideband_packets(6_000.0, 100);

        // Perna em Opus: mistura a 16 kHz, codificada, e lida como o telefone a lê.
        let mut mix = Mixer::wideband(Uuid::new_v4());
        let mut enc = MixEncoder::new().unwrap();
        let mut phone = opus_rs::OpusDecoder::new(16_000, 1).unwrap();
        let mut pcm = vec![0f32; 320 * 6];
        let mut heard = Vec::new();
        for (i, p) in packets.iter().enumerate() {
            mix.push(mic, i as u16, p);
            let block = mix.tick().to_vec();
            assert_eq!(block.len(), 320, "20 ms a 16 kHz");
            let opus = enc
                .encode(&block)
                .expect("bloco de 20 ms codifica")
                .to_vec();
            assert_eq!(opus[0] >> 3, 9, "SILK de banda larga, 20 ms (config 9)");
            let n = phone.decode(&opus, 320 * 6, &mut pcm).unwrap();
            heard.extend_from_slice(&pcm[..n]);
        }
        let tail = &heard[heard.len() - 8_000..];
        let wide = goertzel(tail, 16_000.0, 6_000.0);
        assert!(
            wide > 0.05,
            "6 kHz chega ao telefone em banda larga: {wide}"
        );

        // Perna em G.711: a mesma sala, a 8 kHz. 6 kHz não cabe.
        let mut mix8 = Mixer::new(Uuid::new_v4());
        let mut g711 = Vec::new();
        let mut narrow = Vec::new();
        for (i, p) in packets.iter().enumerate() {
            mix8.push(mic, i as u16, p);
            encode_mix(Law::A, mix8.tick(), &mut g711);
            let mut back = Vec::new();
            Law::A.decode(&g711, &mut back);
            narrow.extend(back.iter().map(|&v| v as f32 / 32768.0));
        }
        let tail8 = &narrow[narrow.len() - 4_000..];
        // A 8 kHz, 6 kHz dobra para 2 kHz se ninguém filtrar antes de descer —
        // e era o que acontecia: este teste apanhou-o a 0,25, a amplitude toda.
        let folded = goertzel(tail8, 8_000.0, 2_000.0);
        let total: f32 = (tail8.iter().map(|v| v * v).sum::<f32>() / tail8.len() as f32).sqrt();
        assert!(
            folded < 0.005 && total < 0.01,
            "em G.711 o tom não passa nem dobra: dobrado {folded}, rms {total}"
        );
        assert!(
            wide > 10.0 * total,
            "a diferença é a banda: {wide} contra {total}"
        );
    }

    /// O filtro da descida para 8 kHz: plano na banda do telefone, fechado
    /// acima dela, e contínuo de um bloco para o seguinte.
    #[test]
    fn descida_para_8_khz_e_plana_ate_3400_e_fecha_a_partir_de_4600() {
        let gain = |freq: f32| {
            let mut d = Decimator::new();
            let mut out = Vec::new();
            let mut all = Vec::new();
            for b in 0..50 {
                let block: Vec<i32> = tone(16_000.0, freq, 320, 0.5, b * 320)
                    .iter()
                    .map(|v| (v * 32767.0) as i32)
                    .collect();
                d.process(&block, &mut out);
                assert_eq!(out.len(), 160);
                all.extend(out.iter().map(|&v| v as f32 / 32768.0));
            }
            let tail = &all[all.len() - 4_000..];
            (tail.iter().map(|v| v * v).sum::<f32>() / tail.len() as f32).sqrt()
                / (0.5 / 2f32.sqrt())
        };
        for f in [300.0, 1_000.0, 2_000.0, 3_000.0, 3_400.0] {
            let g = gain(f);
            assert!((0.94..1.06).contains(&g), "{f} Hz passa a ±0,5 dB: {g}");
        }
        for f in [4_600.0, 5_000.0, 6_000.0, 7_000.0, 7_900.0] {
            let g = gain(f);
            assert!(g < 0.004, "{f} Hz fica 48 dB ou mais abaixo: {g}");
        }
    }

    /// A mistura a 16 kHz continua a ser mix-minus e continua a ter nível
    /// unitário: um tom de 1 kHz a −6 dBFS sai do codificador com a mesma amplitude.
    #[test]
    fn mistura_em_banda_larga_e_unitaria_e_sem_a_propria_voz() {
        let own = Uuid::new_v4();
        let mic = Uuid::new_v4();
        let mut mix = Mixer::wideband(own);
        let mut enc = MixEncoder::new().unwrap();
        let mut phone = opus_rs::OpusDecoder::new(16_000, 1).unwrap();
        let mut pcm = vec![0f32; 320 * 6];
        let mut heard = Vec::new();
        for (i, p) in wideband_packets(1_000.0, 100).iter().enumerate() {
            mix.push(mic, i as u16, p);
            mix.push(own, i as u16, p); // a própria chamada nunca entra
            let block = mix.tick().to_vec();
            let opus = enc.encode(&block).unwrap().to_vec();
            let n = phone.decode(&opus, 320 * 6, &mut pcm).unwrap();
            heard.extend_from_slice(&pcm[..n]);
        }
        assert_eq!(mix.sources.len(), 1, "só o microfone da sala é fonte");
        let at = goertzel(&heard[heard.len() - 8_000..], 16_000.0, 1_000.0);
        assert!(
            (0.2..0.3).contains(&at),
            "amplitude 0,5 → ~0,25, sem ganho nem perda: {at}"
        );
        assert!(
            enc.encode(&[0; 160]).is_none(),
            "um bloco de 8 kHz não é um bloco desta perna"
        );
    }

    /// Telefone → sala com a perna em Opus: a numeração e o relógio da origem
    /// passam, com os buracos da perda no sítio.
    #[test]
    fn passagem_de_opus_preserva_a_sequencia_e_o_relogio_da_origem() {
        let packets = wideband_packets(1_000.0, 12);
        let mut pass = Passthrough::new();
        let (seq0, ts0) = (65_530u16, 4_000_000_000u32); // dá a volta nos dois
        let mut out = Vec::new();
        for (i, p) in packets.iter().enumerate() {
            if i == 4 || i == 5 {
                continue; // dois pacotes perdidos entre o FreeSWITCH e a ponte
            }
            let seq = seq0.wrapping_add(i as u16);
            let ts = ts0.wrapping_add(i as u32 * OPUS_TS_PER_FRAME);
            out.push((i, pass.push(seq, ts, p).expect("Opus válido passa")));
        }
        for (i, f) in &out {
            assert_eq!(
                f.seq, *i as u16,
                "a sequência conta a partir de zero e mantém o buraco"
            );
            assert_eq!(f.timestamp, *i as u32 * OPUS_TS_PER_FRAME);
            assert!(f.level < 20, "tom a −6 dBFS não é silêncio: {}", f.level);
        }
        assert_eq!(out[4].1.seq, 6, "depois de 3 vem 6: a perda vê-se");
        assert_eq!(pass.rejected, 0);
    }

    #[test]
    fn passagem_de_opus_recusa_o_que_nao_descodifica() {
        let mut pass = Passthrough::new();
        assert!(pass.push(1, 960, &[]).is_none(), "vazio");
        // TOC de CELT em banda cheia com código 3 e contagem de blocos impossível.
        assert!(
            pass.push(2, 1920, &[0xFF, 0xFF, 0xFF, 0xFF]).is_none(),
            "lixo"
        );
        assert_eq!(pass.rejected, 2);
        // E o que vem a seguir entra como primeiro pacote: o lixo não gastou números.
        let ok = wideband_packets(700.0, 1);
        let f = pass.push(3, 2880, &ok[0]).unwrap();
        assert_eq!((f.seq, f.timestamp), (0, 0));
    }

    /// Depois de um silêncio imposto (ForceMute) a saída continua de onde
    /// ficou — e um salto de fluxo (re-INVITE) também não abre um buraco.
    #[test]
    fn passagem_de_opus_volta_a_ancorar_depois_de_silencio_e_de_salto() {
        let p = wideband_packets(700.0, 1).remove(0);
        let mut pass = Passthrough::new();
        assert_eq!(pass.push(100, 96_000, &p).unwrap().seq, 0);
        assert_eq!(pass.push(101, 96_960, &p).unwrap().seq, 1);
        // 500 pacotes silenciados na ponte: não foram publicados.
        pass.discontinuity();
        let f = pass.push(602, 96_000 + 502 * 960, &p).unwrap();
        assert_eq!(
            (f.seq, f.timestamp),
            (2, 1_920),
            "continua, sem 500 pacotes de «perda»"
        );
        // O FreeSWITCH recomeça o RTP noutro ponto.
        let g = pass.push(40_000, 7, &p).unwrap();
        assert_eq!((g.seq, g.timestamp), (3, 2_880));
        // Um atrasado do fluxo novo sai com o seu número e não puxa a saída para trás.
        let late = pass.push(39_999, 7u32.wrapping_sub(960), &p).unwrap();
        assert_eq!(late.seq, 2);
        assert_eq!(pass.push(40_001, 967, &p).unwrap().seq, 4);
    }

    #[test]
    fn fonte_atrasada_e_duplicada_e_descartada_e_buffer_tem_tecto() {
        let own = Uuid::new_v4();
        let other = Uuid::new_v4();
        let mut ing = Ingress::new().unwrap();
        let mut mix = Mixer::new(own);
        let mut ulaw = Vec::new();
        Law::Mu.encode(&tone_8k(700.0, 160, 0.3, 0), &mut ulaw);
        let frames: Vec<_> = (0..20)
            .flat_map(|i| ing.push(Law::Mu, &ulaw, i * 160))
            .collect();
        for (i, f) in frames.iter().enumerate() {
            mix.push(other, i as u16, &f.payload);
            mix.push(other, i as u16, &f.payload); // duplicado
        }
        let src = mix.sources.get(&other).unwrap();
        assert_eq!(
            src.buf.len(),
            MIX_FRAME * MAX_BUFFER_FRAMES,
            "20 blocos sem tique cortam aos 120 ms"
        );
    }
}
