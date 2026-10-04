//! Código de emparelhamento da app Delonix Câmara (ADR-0014 §2.1).
//!
//! `XXXX-XXXX` em Crockford base32 (sem I, L, O, U): 40 bits. Os quatro
//! primeiros caracteres LOCALIZAM o código na base; os quatro últimos são o
//! SEGREDO, guardado só como SHA-256 e comparado em tempo constante.
//!
//! Porquê partir em dois: com o código inteiro como segredo, uma tentativa
//! errada não se consegue atribuir a nenhum código — o limite de tentativas
//! teria de ser global. Com um localizador, a tentativa conta contra UM código,
//! e cinco erros queimam-no: quem acerta nos 20 bits do localizador de um
//! código activo tem 5 hipóteses em 2^20 para o resto.

use delonix_meet_core::{crypto, DomainError};

/// Crockford base32, sem os caracteres ambíguos.
pub const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
pub const HALF: usize = 4;
pub const TTL_SECS: i64 = 600;
pub const MAX_ATTEMPTS: i32 = 5;
/// Validade do token de fonte (uma sessão de estúdio longa).
pub const SOURCE_TOKEN_TTL_SECS: i64 = 12 * 3600;

/// Um código recém-gerado. O `display` só sai na resposta de criação.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCode {
    pub locator: String,
    pub secret: String,
    pub display: String,
}

/// O que se guarda: localizador + hash do segredo.
pub fn secret_hash(locator: &str, secret: &str) -> String {
    // O localizador entra no hash: o mesmo segredo em dois códigos não dá o
    // mesmo hash, e não se trocam hashes entre linhas.
    crypto::sha256_hex(format!("delonix-meet/studio-pairing/{locator}/{secret}"))
}

fn encode(bytes: &[u8; 5]) -> String {
    // 40 bits → 8 símbolos de 5 bits.
    let mut v: u64 = 0;
    for b in bytes {
        v = (v << 8) | u64::from(*b);
    }
    (0..8)
        .rev()
        .map(|i| ALPHABET[((v >> (i * 5)) & 0x1f) as usize] as char)
        .collect()
}

/// Gera um código com o gerador do SO.
pub fn generate() -> NewCode {
    from_bytes(crypto::random_bytes::<5>())
}

/// Determinístico a partir de 40 bits — para os testes.
pub fn from_bytes(bytes: [u8; 5]) -> NewCode {
    let s = encode(&bytes);
    let (locator, secret) = s.split_at(HALF);
    NewCode {
        locator: locator.to_string(),
        secret: secret.to_string(),
        display: format!("{locator}-{secret}"),
    }
}

/// Normaliza o que a pessoa escreveu: maiúsculas, sem hífens nem espaços, e as
/// confusões habituais de Crockford (O→0, I/L→1). Devolve `(localizador, segredo)`.
pub fn parse(input: &str) -> Result<(String, String), DomainError> {
    let mut s = String::with_capacity(8);
    for c in input.chars() {
        if c == '-' || c.is_whitespace() {
            continue;
        }
        let c = match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        };
        if !c.is_ascii() || !ALPHABET.contains(&(c as u8)) {
            return Err(malformed());
        }
        s.push(c);
        if s.len() > 2 * HALF {
            return Err(malformed());
        }
    }
    if s.len() != 2 * HALF {
        return Err(malformed());
    }
    let (l, r) = s.split_at(HALF);
    Ok((l.to_string(), r.to_string()))
}

fn malformed() -> DomainError {
    DomainError::invalid(
        "studio.pairing_malformed",
        "o código tem 8 caracteres, no formato XXXX-XXXX",
    )
    .with_field("code", "XXXX-XXXX")
}

/// A MESMA recusa para tudo o que não é um código válido e activo: não se diz
/// se falhou o localizador, o segredo, a validade ou o uso.
pub fn invalid() -> DomainError {
    DomainError::new(
        delonix_meet_core::ErrorKind::NotFound,
        "studio.pairing_invalid",
        "código inválido, expirado ou já usado — peça um código novo ao operador",
    )
}

/// Estado de um código guardado, a partir dos factos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeState {
    Active,
    Consumed,
    Expired,
    Burned,
}

impl CodeState {
    pub fn of(consumed: bool, attempts: i32, expired: bool, revoked: bool) -> Self {
        if consumed {
            Self::Consumed
        } else if attempts >= MAX_ATTEMPTS || revoked {
            Self::Burned
        } else if expired {
            Self::Expired
        } else {
            Self::Active
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Consumed => "consumed",
            Self::Expired => "expired",
            Self::Burned => "burned",
        }
    }
}

/// A decisão de resgate, sem IO: o segredo bate e o código está activo?
pub fn verify(stored_hash: &str, locator: &str, secret: &str) -> bool {
    crypto::ct_eq(
        stored_hash.as_bytes(),
        secret_hash(locator, secret).as_bytes(),
    )
}

/// Dispositivo declarado pela app. Só informativo: nunca decide acesso.
pub fn validate_device_field(field: &'static str, v: &str) -> Result<String, DomainError> {
    let v = v.trim();
    if v.chars().count() > 64 || v.chars().any(char::is_control) {
        return Err(DomainError::invalid(
            "studio.invalid_device",
            "dados do dispositivo inválidos (máximo 64 caracteres)",
        )
        .with_field(field, "0–64 caracteres"));
    }
    Ok(v.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_codigo_tem_oito_simbolos_do_alfabeto() {
        let c = from_bytes([0xff, 0x00, 0xaa, 0x55, 0x01]);
        assert_eq!(c.display.len(), 9);
        assert_eq!(&c.display[4..5], "-");
        assert!(format!("{}{}", c.locator, c.secret)
            .bytes()
            .all(|b| ALPHABET.contains(&b)));
        // Tudo a zeros e tudo a uns dão os extremos do alfabeto.
        assert_eq!(from_bytes([0; 5]).display, "0000-0000");
        assert_eq!(from_bytes([0xff; 5]).display, "ZZZZ-ZZZZ");
    }

    #[test]
    fn gerados_nao_se_repetem_em_mil() {
        let mut v: Vec<String> = (0..1000).map(|_| generate().display).collect();
        v.sort();
        v.dedup();
        assert_eq!(v.len(), 1000);
    }

    #[test]
    fn a_entrada_aceita_minusculas_hifens_e_confusoes() {
        let c = from_bytes([1, 2, 3, 4, 5]);
        let (l, s) = parse(&c.display.to_lowercase()).unwrap();
        assert_eq!(
            (l.as_str(), s.as_str()),
            (c.locator.as_str(), c.secret.as_str())
        );
        assert_eq!(
            parse("0ol1-i1ab").unwrap(),
            ("0011".to_string(), "11AB".to_string())
        );
        assert_eq!(parse(" 0000 0000 ").unwrap().0, "0000");
    }

    #[test]
    fn forma_errada_e_malformado_e_nao_invalido() {
        for bad in [
            "",
            "ABC",
            "ABCD-EFG",
            "ABCD-EFGHJ",
            "ABCD-EFGU",
            "ÁBCD-EFGH",
        ] {
            assert_eq!(
                parse(bad).unwrap_err().code,
                "studio.pairing_malformed",
                "{bad}"
            );
        }
    }

    #[test]
    fn so_o_segredo_certo_verifica_e_o_localizador_entra_no_hash() {
        let h = secret_hash("AB12", "CD34");
        assert!(verify(&h, "AB12", "CD34"));
        assert!(!verify(&h, "AB12", "CD35"));
        assert!(!verify(&h, "AB13", "CD34"));
        assert_ne!(secret_hash("AB12", "CD34"), secret_hash("AB13", "CD34"));
    }

    #[test]
    fn cinco_tentativas_queimam_e_o_uso_ganha_a_tudo() {
        assert_eq!(CodeState::of(false, 4, false, false), CodeState::Active);
        assert_eq!(CodeState::of(false, 5, false, false), CodeState::Burned);
        assert_eq!(CodeState::of(false, 0, true, false), CodeState::Expired);
        assert_eq!(CodeState::of(true, 5, true, true), CodeState::Consumed);
        assert_eq!(CodeState::of(false, 0, false, true), CodeState::Burned);
    }
}
