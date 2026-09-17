//! O número marcado: normalização e máscara.
//!
//! O plano de marcação casa contra a forma NACIONAL do número (como uma pessoa
//! o marca num telefone da empresa): `+244 923 447 108` e `00244923447108` são
//! `923447108`; um número de outro país (`+27 11 …`) passa a `0027…`; os curtos
//! (`112`, `1XX`, `84209`) ficam como estão. O indicativo do país é um
//! parâmetro (dado da instalação), não uma constante escondida.

use delonix_meet_core::DomainError;
use serde::Serialize;

pub const MIN_DIGITS: usize = 2;
pub const MAX_DIGITS: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DialedNumber {
    /// Só dígitos, na forma contra a qual o plano casa.
    pub digits: String,
    /// `+<país><número>` quando o número é público; `None` para curtos/internos.
    pub e164: Option<String>,
}

/// Normaliza o que a pessoa escreveu. `country_code` sem `+` (`"244"`);
/// `national_len` é o comprimento de um número nacional completo (9 em Angola).
pub fn parse_dialed(
    input: &str,
    country_code: &str,
    national_len: usize,
) -> Result<DialedNumber, DomainError> {
    let invalid = || {
        DomainError::invalid(
            "telephony.invalid_number",
            "número inválido — só dígitos, espaços, hífens e um «+» inicial",
        )
        .with_field("number", "ex.: +244 923 447 108, 112, 00271112345678")
    };
    let compact: String = input
        .trim()
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '.' | '(' | ')'))
        .collect();
    let (plus, rest) = match compact.strip_prefix('+') {
        Some(r) => (true, r),
        None => (false, compact.as_str()),
    };
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    // Forma internacional: `+CC…` ou `00CC…`.
    let international = if plus {
        Some(rest)
    } else {
        rest.strip_prefix("00")
    };
    let (digits, e164) = match international {
        Some(intl) => match intl.strip_prefix(country_code) {
            Some(national) if national.len() == national_len => {
                (national.to_string(), Some(format!("+{intl}")))
            }
            _ => (format!("00{intl}"), Some(format!("+{intl}"))),
        },
        None if rest.len() == national_len => {
            (rest.to_string(), Some(format!("+{country_code}{rest}")))
        }
        None => (rest.to_string(), None),
    };
    if digits.len() < MIN_DIGITS || digits.len() > MAX_DIGITS {
        return Err(invalid());
    }
    Ok(DialedNumber { digits, e164 })
}

/// Máscara para listas: `+244 923 ***108`. Com o indicativo da instalação
/// (`home_cc`) mostra-o, os três primeiros dígitos nacionais e os três últimos;
/// outro país mostra os quatro primeiros dígitos e os três últimos
/// (`+2711 ***192`) — sem tabela de indicativos, não se adivinha onde acaba o
/// do país. Curtos (≤ 6 dígitos) ficam como estão: são serviços (`112`) ou
/// ramais, não pessoas.
pub fn mask(number: &str, home_cc: &str) -> String {
    let n: String = number
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '+')
        .collect();
    let digits: String = n.chars().filter(char::is_ascii_digit).collect();
    if digits.len() <= 6 {
        return n;
    }
    let tail = &digits[digits.len() - 3..];
    // O FreeSWITCH manda E.164 sem `+` (`244923447108`): um número que começa
    // pelo indicativo e tem pelo menos o comprimento internacional mínimo é
    // lido como internacional.
    let n = if !n.starts_with('+') && n.starts_with(home_cc) && n.len() >= home_cc.len() + 9 {
        format!("+{n}")
    } else {
        n
    };
    match n.strip_prefix('+') {
        Some(intl) => match intl.strip_prefix(home_cc) {
            Some(national) if national.len() > 6 => {
                format!("+{home_cc} {} ***{tail}", &national[..3])
            }
            _ => format!("+{} ***{tail}", &digits[..4]),
        },
        None => format!("{} ***{tail}", &digits[..3]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> DialedNumber {
        parse_dialed(s, "244", 9).unwrap()
    }

    #[test]
    fn national_forms_collapse() {
        for s in [
            "+244 923 447 108",
            "00244923447108",
            "923447108",
            "923-447-108",
        ] {
            let d = p(s);
            assert_eq!(d.digits, "923447108", "{s}");
            assert_eq!(d.e164.as_deref(), Some("+244923447108"), "{s}");
        }
    }

    #[test]
    fn foreign_and_short() {
        let d = p("+27 11 555 0192");
        assert_eq!(d.digits, "0027115550192");
        assert_eq!(d.e164.as_deref(), Some("+27115550192"));
        let d = p("112");
        assert_eq!(d.digits, "112");
        assert_eq!(d.e164, None);
        assert_eq!(p("84209").digits, "84209");
    }

    #[test]
    fn garbage_is_refused() {
        for bad in [
            "",
            "abc",
            "+",
            "9",
            "++244",
            "1234567890123456789012",
            "12#",
        ] {
            let e = parse_dialed(bad, "244", 9).unwrap_err();
            assert_eq!(e.code, "telephony.invalid_number", "{bad}");
        }
    }

    #[test]
    fn masking() {
        assert_eq!(mask("+244923447108", "244"), "+244 923 ***108");
        assert_eq!(mask("+27115550192", "244"), "+2711 ***192");
        assert_eq!(mask("112", "244"), "112");
        assert_eq!(mask("923447108", "244"), "923 ***108");
        assert_eq!(mask("244923447108", "244"), "+244 923 ***108");
    }
}
