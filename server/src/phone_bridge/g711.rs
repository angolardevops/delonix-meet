//! G.711 (ITU-T) — lei μ (PCMU, tipo de payload 0) e lei A (PCMA, tipo 8).
//!
//! É o codec de TODA a rede telefónica: o que chega de uma operadora (Unitel,
//! Africell, Movicel) ou de uma sala SIP pelo FreeSWITCH vem quase sempre
//! assim, a 8 kHz, um byte por amostra, 160 bytes por pacote de 20 ms.
//!
//! A implementação é a de referência da Sun (`g711.c`, domínio público).
//! Medido contra o `ffmpeg` 6 (ADR-0010, «Medições»): a DESCODIFICAÇÃO dá os
//! mesmos 256 valores; a CODIFICAÇÃO difere em 512 (μ) e 964 (A) dos 65 536
//! valores de 16 bits — só nas fronteiras entre degraus, onde o `ffmpeg`
//! arredonda para o degrau mais próximo e a Sun trunca. Os dois fluxos são
//! G.711 válidos. Os valores fixos dos testes abaixo vêm dessa medição.

const SEG_SHIFT: u8 = 4;
const QUANT_MASK: u8 = 0x0F;
const SEG_MASK: u8 = 0x70;
const SIGN_BIT: u8 = 0x80;

const SEG_UEND: [i32; 8] = [0x3F, 0x7F, 0xFF, 0x1FF, 0x3FF, 0x7FF, 0xFFF, 0x1FFF];
const SEG_AEND: [i32; 8] = [0x1F, 0x3F, 0x7F, 0xFF, 0x1FF, 0x3FF, 0x7FF, 0xFFF];

/// Lei de compressão de um fluxo G.711.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Law {
    /// PCMU — América do Norte, Japão, e a maioria dos troncos SIP por omissão.
    Mu,
    /// PCMA — Europa e África (a norma nas operadoras angolanas).
    A,
}

impl Law {
    /// Tipo de payload RTP estático (RFC 3551).
    pub fn payload_type(self) -> u8 {
        match self {
            Law::Mu => 0,
            Law::A => 8,
        }
    }

    /// Lei a partir do tipo de payload RTP; `None` para tudo o que não é G.711.
    pub fn from_payload_type(pt: u8) -> Option<Law> {
        match pt {
            0 => Some(Law::Mu),
            8 => Some(Law::A),
            _ => None,
        }
    }

    pub fn encode(self, pcm: &[i16], out: &mut Vec<u8>) {
        match self {
            Law::Mu => out.extend(pcm.iter().map(|&s| linear_to_ulaw(s))),
            Law::A => out.extend(pcm.iter().map(|&s| linear_to_alaw(s))),
        }
    }

    pub fn decode(self, bytes: &[u8], out: &mut Vec<i16>) {
        match self {
            Law::Mu => out.extend(bytes.iter().map(|&b| ulaw_to_linear(b))),
            Law::A => out.extend(bytes.iter().map(|&b| alaw_to_linear(b))),
        }
    }
}

fn segment(value: i32, table: &[i32; 8]) -> usize {
    table.iter().position(|&end| value <= end).unwrap_or(8)
}

pub fn linear_to_ulaw(sample: i16) -> u8 {
    const BIAS: i32 = 0x84;
    const CLIP: i32 = 8159;
    let mut pcm = (sample as i32) >> 2;
    let mask: u8 = if pcm < 0 {
        pcm = -pcm;
        0x7F
    } else {
        0xFF
    };
    if pcm > CLIP {
        pcm = CLIP;
    }
    pcm += BIAS >> 2;
    let seg = segment(pcm, &SEG_UEND);
    if seg >= 8 {
        return 0x7F ^ mask;
    }
    let uval = ((seg as i32) << 4) | ((pcm >> (seg + 1)) & 0xF);
    (uval as u8) ^ mask
}

pub fn ulaw_to_linear(byte: u8) -> i16 {
    const BIAS: i32 = 0x84;
    let u = !byte;
    let mut t = (((u & QUANT_MASK) as i32) << 3) + BIAS;
    t <<= (u & SEG_MASK) >> SEG_SHIFT;
    if u & SIGN_BIT != 0 {
        (BIAS - t) as i16
    } else {
        (t - BIAS) as i16
    }
}

pub fn linear_to_alaw(sample: i16) -> u8 {
    let mut pcm = (sample as i32) >> 3;
    let mask: u8 = if pcm >= 0 {
        0xD5
    } else {
        pcm = -pcm - 1;
        0x55
    };
    let seg = segment(pcm, &SEG_AEND);
    if seg >= 8 {
        return 0x7F ^ mask;
    }
    let mut aval = (seg as i32) << 4;
    if seg < 2 {
        aval |= (pcm >> 1) & QUANT_MASK as i32;
    } else {
        aval |= (pcm >> seg) & QUANT_MASK as i32;
    }
    (aval as u8) ^ mask
}

pub fn alaw_to_linear(byte: u8) -> i16 {
    let a = byte ^ 0x55;
    let mut t = ((a & QUANT_MASK) as i32) << 4;
    let seg = (a & SEG_MASK) >> SEG_SHIFT;
    match seg {
        0 => t += 8,
        1 => t += 0x108,
        _ => {
            t += 0x108;
            t <<= seg - 1;
        }
    }
    if a & SIGN_BIT != 0 {
        t as i16
    } else {
        -t as i16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silencio_e_extremos_da_lei_mu() {
        // Valores de referência da tabela G.711: 0 → 0xFF, e o byte 0xFF
        // descodifica em 0 (o silêncio da rede telefónica).
        assert_eq!(linear_to_ulaw(0), 0xFF);
        assert_eq!(ulaw_to_linear(0xFF), 0);
        assert_eq!(linear_to_ulaw(i16::MAX), 0x80);
        assert_eq!(linear_to_ulaw(i16::MIN), 0x00);
        assert_eq!(ulaw_to_linear(0x80), 32124);
        assert_eq!(ulaw_to_linear(0x00), -32124);
    }

    #[test]
    fn silencio_e_extremos_da_lei_a() {
        assert_eq!(linear_to_alaw(0), 0xD5);
        assert_eq!(alaw_to_linear(0xD5), 8);
        assert_eq!(linear_to_alaw(i16::MAX), 0xAA);
        assert_eq!(linear_to_alaw(i16::MIN), 0x2A);
        assert_eq!(alaw_to_linear(0xAA), 32256);
        assert_eq!(alaw_to_linear(0x2A), -32256);
    }

    /// Codificar o que se descodificou devolve o MESMO byte, para os 256
    /// valores: é a propriedade que torna a ponte transparente quando o áudio
    /// não é tocado (um tom que entra e sai sem mistura não se degrada).
    #[test]
    fn ida_e_volta_e_identidade_nos_256_codigos() {
        for b in 0..=255u8 {
            // 0x7F e 0xFF são o −0 e o +0 da lei μ: os dois descodificam em 0,
            // e o 0 codifica-se como 0xFF.
            let mu = linear_to_ulaw(ulaw_to_linear(b));
            assert!(mu == b || (b == 0x7F && mu == 0xFF), "μ {b:#x} → {mu:#x}");
            let a = linear_to_alaw(alaw_to_linear(b));
            assert_eq!(a, b, "A {b:#x}");
        }
    }

    #[test]
    fn erro_de_quantizacao_limitado() {
        for s in (-32000i32..32000).step_by(97) {
            let s = s as i16;
            let mu = ulaw_to_linear(linear_to_ulaw(s)) as i32;
            let a = alaw_to_linear(linear_to_alaw(s)) as i32;
            // Pior degrau do último segmento: 1024 (μ) e 1024 (A).
            assert!((mu - s as i32).abs() <= 1024, "μ {s} → {mu}");
            assert!((a - s as i32).abs() <= 1024, "A {s} → {a}");
        }
    }

    #[test]
    fn tipo_de_payload() {
        assert_eq!(Law::from_payload_type(0), Some(Law::Mu));
        assert_eq!(Law::from_payload_type(8), Some(Law::A));
        assert_eq!(Law::from_payload_type(111), None);
        assert_eq!(Law::A.payload_type(), 8);
    }
}
