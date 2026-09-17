//! Codificação de SMS — o ÚNICO sítio que a decide (ADR-0005).
//!
//! Três coisas, todas puras e sem I/O:
//! 1. **Alfabeto:** GSM 03.38 (7 bits, tabela base + extensão) quando o texto
//!    cabe nele, senão UCS-2. `ç`, `ã`, `õ` NÃO estão na tabela base — um texto
//!    em português com eles vai em UCS-2 e cada parte leva 67 caracteres, não
//!    153. É a surpresa de custo mais comum, e por isso o contador da consola
//!    usa a mesma regra.
//! 2. **Segmentação:** 160/153 septetos ou 70/67 unidades UTF-16, sem partir um
//!    escape GSM nem um par substituto.
//! 3. **PDU SMS-SUBMIT (3GPP TS 23.040)** para o modem por AT (`AT+CMGS` em modo
//!    PDU). O agente USB recebe os PDUs feitos e não reimplementa nada disto.
//!
//! O PDU SMPP (operadores) vive em `sms_smpp.rs` e usa as partes daqui.

/// Máximo de partes aceite numa mensagem. Seis partes GSM são 918 caracteres;
/// acima disto é quase sempre um erro de quem chama, e cada parte custa.
pub const MAX_SEGMENTS: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Gsm7,
    Ucs2,
}

impl Encoding {
    pub fn as_str(self) -> &'static str {
        match self {
            Encoding::Gsm7 => "gsm7",
            Encoding::Ucs2 => "ucs2",
        }
    }
}

/// Uma mensagem já dividida. Em GSM, cada parte são septetos NÃO empacotados
/// (um por octeto, escapes incluídos); em UCS-2, octetos UTF-16BE.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoded {
    pub encoding: Encoding,
    pub parts: Vec<Vec<u8>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CodecError {
    Empty,
    TooLong { segments: usize },
    InvalidNumber,
}

impl std::fmt::Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodecError::Empty => write!(f, "a mensagem está vazia"),
            CodecError::TooLong { segments } => write!(
                f,
                "a mensagem daria {segments} partes; o máximo são {MAX_SEGMENTS}"
            ),
            CodecError::InvalidNumber => write!(f, "número de destino inválido"),
        }
    }
}

/// Tabela base GSM 03.38, pelo índice do septeto. `\u{1b}` (0x1B) é o escape.
const GSM_BASIC: [char; 128] = [
    '@', '£', '$', '¥', 'è', 'é', 'ù', 'ì', 'ò', 'Ç', '\n', 'Ø', 'ø', '\r', 'Å', 'å', //
    'Δ', '_', 'Φ', 'Γ', 'Λ', 'Ω', 'Π', 'Ψ', 'Σ', 'Θ', 'Ξ', '\u{1b}', 'Æ', 'æ', 'ß', 'É', //
    ' ', '!', '"', '#', '¤', '%', '&', '\'', '(', ')', '*', '+', ',', '-', '.', '/', //
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', ':', ';', '<', '=', '>', '?', //
    '¡', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', //
    'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'Ä', 'Ö', 'Ñ', 'Ü', '§', //
    '¿', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', //
    'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', 'ä', 'ö', 'ñ', 'ü', 'à', //
];

/// Tabela de extensão (precedida de 0x1B): cada um destes custa DOIS septetos.
const GSM_EXTENSION: [(char, u8); 10] = [
    ('\u{0c}', 0x0A),
    ('^', 0x14),
    ('{', 0x28),
    ('}', 0x29),
    ('\\', 0x2F),
    ('[', 0x3C),
    ('~', 0x3D),
    (']', 0x3E),
    ('|', 0x40),
    ('€', 0x65),
];

const ESC: u8 = 0x1B;

/// Septetos de um carácter, ou `None` se não couber em GSM 03.38.
fn gsm_septets(c: char) -> Option<Vec<u8>> {
    if c != '\u{1b}' {
        if let Some(i) = GSM_BASIC.iter().position(|&g| g == c) {
            return Some(vec![i as u8]);
        }
    }
    GSM_EXTENSION
        .iter()
        .find(|(g, _)| *g == c)
        .map(|(_, code)| vec![ESC, *code])
}

/// Codifica e divide uma mensagem. Recusa vazia e acima de `MAX_SEGMENTS`.
pub fn encode(body: &str) -> Result<Encoded, CodecError> {
    if body.is_empty() {
        return Err(CodecError::Empty);
    }
    let gsm: Option<Vec<Vec<u8>>> = body.chars().map(gsm_septets).collect();
    let encoded = match gsm {
        Some(chars) => Encoded {
            encoding: Encoding::Gsm7,
            parts: split_units(&chars, 160, 153),
        },
        None => {
            // Cada carácter são 1 ou 2 unidades UTF-16 (par substituto), que
            // ficam juntas na mesma parte.
            let units: Vec<Vec<u8>> = body
                .chars()
                .map(|c| {
                    let mut buf = [0u16; 2];
                    c.encode_utf16(&mut buf)
                        .iter()
                        .flat_map(|u| u.to_be_bytes())
                        .collect()
                })
                .collect();
            // Os limites são em unidades UTF-16 = octetos / 2.
            let parts = split_units(&units, 140, 134);
            Encoded {
                encoding: Encoding::Ucs2,
                parts,
            }
        }
    };
    if encoded.parts.len() > MAX_SEGMENTS {
        return Err(CodecError::TooLong {
            segments: encoded.parts.len(),
        });
    }
    Ok(encoded)
}

/// Divide unidades indivisíveis (um carácter = um bloco de octetos) pelo limite
/// de uma parte única ou, se não couber, pelo limite por parte concatenada.
fn split_units(units: &[Vec<u8>], single: usize, per_part: usize) -> Vec<Vec<u8>> {
    let total: usize = units.iter().map(Vec::len).sum();
    if total <= single {
        return vec![units.concat()];
    }
    let mut parts = Vec::new();
    let mut current: Vec<u8> = Vec::new();
    for u in units {
        if current.len() + u.len() > per_part {
            parts.push(std::mem::take(&mut current));
        }
        current.extend_from_slice(u);
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

/// Elemento de concatenação (IE 0x00, referência de 8 bits), ou vazio. SEM o
/// octeto de comprimento (UDHL): quem monta o PDU acrescenta-o uma vez.
pub fn concat_udh(reference: u8, total: usize, seq: usize) -> Vec<u8> {
    if total <= 1 {
        return Vec::new();
    }
    vec![0x00, 0x03, reference, total as u8, seq as u8]
}

/// Destino em E.164 (`+244923000000`) → dígitos, validados.
pub fn e164_digits(to: &str) -> Result<String, CodecError> {
    let digits = to.strip_prefix('+').ok_or(CodecError::InvalidNumber)?;
    if !(8..=15).contains(&digits.len()) || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(CodecError::InvalidNumber);
    }
    Ok(digits.to_string())
}

/// Um PDU pronto para `AT+CMGS=<tpdu_len>` seguido de `<hex>` e Ctrl-Z.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct AtPdu {
    pub hex: String,
    /// Octetos do TPDU, SEM o octeto do SMSC — é o número que o `AT+CMGS` pede.
    pub tpdu_len: usize,
}

/// PDUs SMS-SUBMIT, um por parte. O SMSC é o do SIM (octeto `00`), e não se
/// pede período de validade (o operador aplica o seu).
pub fn at_pdus(to_e164: &str, enc: &Encoded, reference: u8) -> Result<Vec<AtPdu>, CodecError> {
    let digits = e164_digits(to_e164)?;
    let total = enc.parts.len();
    let mut out = Vec::with_capacity(total);
    for (i, part) in enc.parts.iter().enumerate() {
        let udh = concat_udh(reference, total, i + 1);
        let mut tpdu = vec![if udh.is_empty() { 0x01 } else { 0x41 }, 0x00];
        tpdu.push(digits.len() as u8);
        tpdu.push(0x91); // internacional, ISDN
        tpdu.extend(semi_octets(&digits));
        tpdu.push(0x00); // TP-PID
        match enc.encoding {
            Encoding::Gsm7 => {
                tpdu.push(0x00);
                let (udl, ud) = pack_gsm7(&udh, part);
                tpdu.push(udl as u8);
                tpdu.extend(ud);
            }
            Encoding::Ucs2 => {
                tpdu.push(0x08);
                let udl = if udh.is_empty() { 0 } else { udh.len() + 1 } + part.len();
                tpdu.push(udl as u8);
                if !udh.is_empty() {
                    tpdu.push(udh.len() as u8);
                    tpdu.extend(&udh);
                }
                tpdu.extend(part);
            }
        }
        let tpdu_len = tpdu.len();
        let mut hex = String::from("00");
        hex.push_str(&hex::encode_upper(&tpdu));
        out.push(AtPdu { hex, tpdu_len });
    }
    Ok(out)
}

/// Dígitos em semi-octetos trocados, com `F` a preencher o ímpar.
fn semi_octets(digits: &str) -> Vec<u8> {
    let d: Vec<u8> = digits.bytes().map(|b| b - b'0').collect();
    d.chunks(2)
        .map(|c| {
            let hi = if c.len() == 2 { c[1] } else { 0x0F };
            (hi << 4) | c[0]
        })
        .collect()
}

/// Empacota septetos (LSB primeiro) depois de um UDH opcional. Devolve o TP-UDL
/// em SEPTETOS (UDH incluído, com os bits de enchimento) e os octetos.
fn pack_gsm7(udh: &[u8], septets: &[u8]) -> (usize, Vec<u8>) {
    let header: Vec<u8> = if udh.is_empty() {
        Vec::new()
    } else {
        let mut h = vec![udh.len() as u8];
        h.extend_from_slice(udh);
        h
    };
    let header_septets = (header.len() * 8).div_ceil(7);
    let total_septets = header_septets + septets.len();
    let mut out = vec![0u8; (total_septets * 7).div_ceil(8)];
    out[..header.len()].copy_from_slice(&header);
    for (i, &s) in septets.iter().enumerate() {
        let bit = (header_septets + i) * 7;
        let (byte, shift) = (bit / 8, bit % 8);
        out[byte] |= s << shift;
        if shift > 1 {
            out[byte + 1] |= s >> (8 - shift);
        }
    }
    (total_septets, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_ascii_is_gsm7_single_part_up_to_160() {
        let e = encode(&"a".repeat(160)).unwrap();
        assert_eq!(e.encoding, Encoding::Gsm7);
        assert_eq!(e.parts.len(), 1);
        let e = encode(&"a".repeat(161)).unwrap();
        assert_eq!(e.parts.len(), 2);
        assert_eq!(e.parts[0].len(), 153);
        assert_eq!(e.parts[1].len(), 8);
    }

    #[test]
    fn euro_costs_two_septets_and_escape_is_never_split() {
        let e = encode("€").unwrap();
        assert_eq!(e.parts, vec![vec![0x1B, 0x65]]);
        // 152 «a» + «€»: o escape não pode ficar na parte 1 e o código na 2.
        let body = format!("{}€{}", "a".repeat(152), "b".repeat(10));
        let e = encode(&body).unwrap();
        assert_eq!(e.parts[0].len(), 152);
        assert_eq!(&e.parts[1][..2], &[0x1B, 0x65]);
    }

    #[test]
    fn portuguese_accents_outside_basic_table_force_ucs2() {
        // «Ç» maiúsculo está na tabela base; «ç», «ã» e «õ» não estão.
        assert_eq!(encode("Ç").unwrap().encoding, Encoding::Gsm7);
        for s in ["ç", "ã", "õ", "ê"] {
            assert_eq!(encode(s).unwrap().encoding, Encoding::Ucs2, "{s}");
        }
        let e = encode(&"ã".repeat(71)).unwrap();
        assert_eq!(e.parts.len(), 2);
        assert_eq!(e.parts[0].len(), 134); // 67 unidades
    }

    #[test]
    fn emoji_surrogate_pair_is_never_split() {
        let body = format!("{}😀", "ã".repeat(66));
        let e = encode(&body).unwrap();
        assert_eq!(e.parts.len(), 1, "66 + 2 unidades = 68 cabe em 70");
        let body = format!("{}😀x", "ã".repeat(66));
        let e = encode(&body).unwrap();
        assert_eq!(e.parts.len(), 1, "69 unidades cabe em 70");
        let body = format!("{}😀{}", "ã".repeat(66), "x".repeat(10));
        let e = encode(&body).unwrap();
        assert_eq!(e.parts.len(), 2);
        assert_eq!(e.parts[0].len(), 132, "o par não cabe inteiro na parte 1");
    }

    #[test]
    fn rejects_empty_and_too_many_segments() {
        assert_eq!(encode(""), Err(CodecError::Empty));
        assert!(matches!(
            encode(&"a".repeat(153 * 6 + 1)),
            Err(CodecError::TooLong { segments: 7 })
        ));
    }

    #[test]
    fn gsm7_packing_matches_the_reference_vector() {
        // Vector clássico da especificação: «hellohello».
        let e = encode("hellohello").unwrap();
        let (udl, ud) = pack_gsm7(&[], &e.parts[0]);
        assert_eq!(udl, 10);
        assert_eq!(hex::encode_upper(ud), "E8329BFD4697D9EC37");
    }

    #[test]
    fn at_pdu_for_single_part_matches_hand_built_submit() {
        let e = encode("hellohello").unwrap();
        let p = at_pdus("+46708251358", &e, 0).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].hex, "0001000B916407281553F800000AE8329BFD4697D9EC37");
        assert_eq!(p[0].tpdu_len, p[0].hex.len() / 2 - 1);
    }

    #[test]
    fn multipart_gsm7_has_udhi_header_and_fill_bit() {
        let e = encode(&"a".repeat(200)).unwrap();
        let p = at_pdus("+244923000000", &e, 0x2A).unwrap();
        assert_eq!(p.len(), 2);
        let bytes = hex::decode(&p[0].hex).unwrap();
        assert_eq!(bytes[1], 0x41, "UDHI ligado");
        // SMSC(1) FO(1) MR(1) len(1) toa(1) número(6) PID(1) DCS(1) → UDL
        let udl_at = 1 + 1 + 1 + 1 + 1 + 6 + 1 + 1;
        assert_eq!(
            bytes[udl_at],
            (7 + 153) as u8,
            "6 octetos de UDH ocupam 7 septetos"
        );
        assert_eq!(
            &bytes[udl_at + 1..udl_at + 7],
            &[0x05, 0x00, 0x03, 0x2A, 2, 1]
        );
    }

    #[test]
    fn ucs2_pdu_uses_dcs_08_and_octet_length() {
        let e = encode("ã").unwrap();
        let p = at_pdus("+244923000000", &e, 0).unwrap();
        // PID 00, DCS 08, UDL 02 (octetos), «ã» = U+00E3.
        assert!(p[0].hex.ends_with("00080200E3"), "{}", p[0].hex);
    }

    #[test]
    fn number_must_be_e164() {
        assert!(e164_digits("923000000").is_err());
        assert!(e164_digits("+24492300000a").is_err());
        assert_eq!(e164_digits("+244923000000").unwrap(), "244923000000");
        assert_eq!(
            semi_octets("244923000000"),
            vec![0x42, 0x94, 0x32, 0x00, 0x00, 0x00]
        );
    }
}
