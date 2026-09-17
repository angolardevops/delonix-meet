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

use std::collections::{HashMap, VecDeque};

use uuid::Uuid;

use super::g711::Law;

/// 20 ms a 8 kHz.
pub const FRAME_8K: usize = 160;
/// Incremento de timestamp RTP de 20 ms de Opus (relógio de 48 kHz, RFC 7587).
pub const OPUS_TS_PER_FRAME: u32 = 960;
/// Maior pacote que se aceita do lado Opus: 120 ms a 8 kHz.
const MAX_DECODED_8K: usize = 960;

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
//  Sala → telefone
// ============================================================

/// Amostras acumuladas antes de uma fonte começar a contar para a mistura:
/// 40 ms absorvem o jitter normal de uma rede sem acrescentar atraso audível.
const PREBUFFER: usize = FRAME_8K * 2;
/// Tecto de amostras por fonte: 120 ms. Acima disto a fonte está a chegar mais
/// depressa do que se consome (relógios diferentes, rajada): corta-se o mais
/// antigo em vez de deixar o atraso crescer para sempre.
const MAX_BUFFER: usize = FRAME_8K * 6;
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
    fn new() -> Self {
        Self {
            mono: None,
            stereo: None,
            buf: VecDeque::with_capacity(MAX_BUFFER),
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
    mixed: Vec<i32>,
    /// Pacotes que o descodificador recusou (corrompidos ou de outro codec).
    pub decode_errors: u64,
}

impl Mixer {
    pub fn new(own: Uuid) -> Self {
        Self {
            own,
            sources: HashMap::new(),
            decoded: vec![0.0; MAX_DECODED_8K * 2],
            mixed: vec![0; FRAME_8K],
            decode_errors: 0,
        }
    }

    /// Número de fontes vivas (para métricas e testes).
    pub fn sources(&self) -> usize {
        self.sources.len()
    }

    /// Um pacote Opus de um participante da sala.
    pub fn push(&mut self, publisher: Uuid, seq: u16, payload: &[u8]) {
        if publisher == self.own || payload.is_empty() {
            return;
        }
        let src = self.sources.entry(publisher).or_insert_with(Source::new);
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
            *decoder = opus_rs::OpusDecoder::new(8000, if stereo { 2 } else { 1 }).ok();
        }
        let Some(dec) = decoder.as_mut() else {
            self.decode_errors += 1;
            return;
        };
        match dec.decode(payload, MAX_DECODED_8K, &mut self.decoded) {
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
                while src.buf.len() > MAX_BUFFER {
                    src.buf.pop_front();
                }
            }
            Err(_) => self.decode_errors += 1,
        }
    }

    /// Um tique de 20 ms: devolve 160 amostras misturadas.
    pub fn tick(&mut self) -> &[i32] {
        for v in self.mixed.iter_mut() {
            *v = 0;
        }
        self.sources.retain(|_, src| {
            if !src.primed {
                if src.buf.len() >= PREBUFFER {
                    src.primed = true;
                } else {
                    src.idle_ticks += 1;
                    return src.idle_ticks < IDLE_TICKS;
                }
            }
            if src.buf.len() < FRAME_8K {
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
        &self.mixed
    }
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
        assert_eq!(mix.sources(), 0);
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
        assert!(soft_limit(1_000_000) <= 32_767);
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
            MAX_BUFFER,
            "20 blocos sem tique cortam aos 120 ms"
        );
    }
}
