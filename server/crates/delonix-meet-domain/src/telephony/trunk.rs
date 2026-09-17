//! Regras de FORMA de um tronco SIP (uma operadora). O adaptador HTTP/Postgres
//! só as chama.

use delonix_meet_core::DomainError;
use uuid::Uuid;

pub const MAX_NAME: usize = 80;
pub const MAX_PREFIXES: usize = 20;
pub const MAX_CHANNELS: i32 = 10_000;

fn bad(code: &'static str, field: &str, msg: impl Into<String>) -> DomainError {
    DomainError::invalid(code, msg).with_field(field, code)
}

pub fn validate_name(name: &str) -> Result<String, DomainError> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > MAX_NAME {
        return Err(bad(
            "telephony.invalid_trunk_name",
            "name",
            format!("nome obrigatório, até {MAX_NAME} caracteres"),
        ));
    }
    Ok(n.to_string())
}

/// Sigla do cartão: 2 a 4 letras/dígitos, guardada em maiúsculas.
pub fn validate_short_code(code: &str) -> Result<String, DomainError> {
    let c = code.trim().to_ascii_uppercase();
    if !(2..=4).contains(&c.len()) || !c.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(bad(
            "telephony.invalid_short_code",
            "short_code",
            "sigla de 2 a 4 letras ou dígitos (UNI, AFR)",
        ));
    }
    Ok(c)
}

pub fn validate_scope(scope: &str) -> Result<&'static str, DomainError> {
    match scope {
        "national" => Ok("national"),
        "international" => Ok("international"),
        _ => Err(bad(
            "telephony.invalid_scope",
            "scope",
            "âmbito: national | international",
        )),
    }
}

/// Nome de host ou IP literal, sem esquema, porta nem caminho.
pub fn validate_host(host: &str) -> Result<String, DomainError> {
    let h = host.trim().to_ascii_lowercase();
    let ok = !h.is_empty()
        && h.len() <= 253
        && h.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'[' | b']'))
        && !h.starts_with('-')
        && !h.contains("..");
    if !ok {
        return Err(bad(
            "telephony.invalid_trunk_host",
            "host",
            "host inválido — só o nome ou o IP (sip.unitel.ao), sem esquema nem porta",
        ));
    }
    Ok(h)
}

pub fn validate_port(port: i32) -> Result<i32, DomainError> {
    if (1..=65535).contains(&port) {
        Ok(port)
    } else {
        Err(bad(
            "telephony.invalid_port",
            "port",
            "porta entre 1 e 65535",
        ))
    }
}

pub fn validate_transport(t: &str) -> Result<&'static str, DomainError> {
    match t {
        "udp" => Ok("udp"),
        "tcp" => Ok("tcp"),
        "tls" => Ok("tls"),
        _ => Err(bad(
            "telephony.invalid_transport",
            "transport",
            "transporte: udp | tcp | tls",
        )),
    }
}

pub fn validate_srtp(s: &str) -> Result<&'static str, DomainError> {
    match s {
        "mandatory" => Ok("mandatory"),
        "optional" => Ok("optional"),
        "off" => Ok("off"),
        _ => Err(bad(
            "telephony.invalid_srtp",
            "srtp",
            "SRTP: mandatory | optional | off",
        )),
    }
}

/// Com SDES, as chaves do SRTP viajam no SDP: sem TLS na sinalização, quem
/// escuta a rede lê-as. SRTP sem TLS é segurança fingida — recusa-se.
pub fn validate_security(transport: &str, srtp: &str) -> Result<(), DomainError> {
    if srtp != "off" && transport != "tls" {
        return Err(bad(
            "telephony.srtp_requires_tls",
            "srtp",
            "SRTP exige transporte TLS (as chaves SDES vão no SDP da sinalização)",
        ));
    }
    Ok(())
}

/// Prefixos como o cliente os lê: dígitos, opcionalmente `+` no início.
pub fn validate_prefixes(prefixes: &[String]) -> Result<Vec<String>, DomainError> {
    if prefixes.len() > MAX_PREFIXES {
        return Err(bad(
            "telephony.invalid_prefixes",
            "prefixes",
            format!("no máximo {MAX_PREFIXES} prefixos"),
        ));
    }
    let mut out = Vec::with_capacity(prefixes.len());
    for p in prefixes {
        let p = p.trim();
        let digits = p.strip_prefix('+').unwrap_or(p);
        if p.is_empty()
            || p.len() > 8
            || !digits.bytes().all(|b| b.is_ascii_digit())
            || (digits.is_empty() && p != "+")
        {
            return Err(bad(
                "telephony.invalid_prefixes",
                "prefixes",
                format!("prefixo «{p}» inválido — dígitos, com «+» opcional no início"),
            ));
        }
        if !out.iter().any(|x: &String| x == p) {
            out.push(p.to_string());
        }
    }
    Ok(out)
}

pub fn validate_max_channels(n: i32) -> Result<i32, DomainError> {
    if (1..=MAX_CHANNELS).contains(&n) {
        Ok(n)
    } else {
        Err(bad(
            "telephony.invalid_max_channels",
            "max_channels",
            format!("canais máximos entre 1 e {MAX_CHANNELS}"),
        ))
    }
}

pub fn validate_credentials(username: &str, password: Option<&str>) -> Result<(), DomainError> {
    if username.len() > 128 || username.chars().any(char::is_control) {
        return Err(bad(
            "telephony.invalid_username",
            "username",
            "utilizador até 128 caracteres, sem controlo",
        ));
    }
    if let Some(p) = password {
        if p.len() > 256 || p.chars().any(char::is_control) {
            return Err(bad(
                "telephony.invalid_password",
                "password",
                "password até 256 caracteres, sem controlo",
            ));
        }
    }
    Ok(())
}

/// Contexto da cifra da password do tronco (linha a linha).
pub fn password_aad(trunk_id: &Uuid) -> String {
    format!("telephony_trunks.password:{trunk_id}")
}

/// A ordem nova tem de ser uma permutação EXACTA dos troncos da org.
pub fn validate_order(requested: &[Uuid], existing: &[Uuid]) -> Result<(), DomainError> {
    let mut a = requested.to_vec();
    let mut b = existing.to_vec();
    a.sort();
    b.sort();
    let dup = a.windows(2).any(|w| w[0] == w[1]);
    if dup || a != b {
        return Err(bad(
            "telephony.invalid_trunk_order",
            "trunk_ids",
            "a ordem tem de listar TODOS os troncos da organização, cada um uma vez",
        ));
    }
    Ok(())
}

/// Rótulo derivado da posição: `primary`, `reserve` (com o número) ou
/// `international`.
pub fn role(scope: &str, national_rank: usize) -> (&'static str, Option<usize>) {
    match (scope, national_rank) {
        ("international", _) => ("international", None),
        (_, 0) => ("primary", None),
        (_, n) => ("reserve", Some(n)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes() {
        assert_eq!(validate_short_code("uni").unwrap(), "UNI");
        assert!(validate_short_code("U").is_err());
        assert!(validate_short_code("UN-I").is_err());
        assert_eq!(validate_host("SIP.Unitel.ao").unwrap(), "sip.unitel.ao");
        for bad in ["", "sip://x", "a b", "x/y", "-x", "a..b"] {
            assert!(validate_host(bad).is_err(), "{bad}");
        }
        assert!(validate_port(0).is_err());
        assert!(validate_security("tls", "mandatory").is_ok());
        assert_eq!(
            validate_security("udp", "mandatory").unwrap_err().code,
            "telephony.srtp_requires_tls"
        );
        assert!(validate_security("udp", "off").is_ok());
        assert_eq!(
            validate_prefixes(&["+244 9".into()]).unwrap_err().code,
            "telephony.invalid_prefixes"
        );
        assert_eq!(
            validate_prefixes(&["9".into(), "95".into(), "9".into(), "+".into(), "00".into()])
                .unwrap(),
            vec!["9", "95", "+", "00"]
        );
    }

    #[test]
    fn order_is_exact_permutation() {
        let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        assert!(validate_order(&[c, a, b], &[a, b, c]).is_ok());
        assert!(validate_order(&[a, b], &[a, b, c]).is_err());
        assert!(validate_order(&[a, a, b], &[a, b, c]).is_err());
        assert!(validate_order(&[a, b, Uuid::new_v4()], &[a, b, c]).is_err());
    }

    #[test]
    fn roles() {
        assert_eq!(role("national", 0), ("primary", None));
        assert_eq!(role("national", 2), ("reserve", Some(2)));
        assert_eq!(role("international", 0), ("international", None));
    }
}
