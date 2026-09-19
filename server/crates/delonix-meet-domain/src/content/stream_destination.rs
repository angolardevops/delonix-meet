//! Destinos de emissão em directo guardados por organização (G1).
//!
//! As regras de FORMA de um destino — que plataformas, que estados, que URL —
//! vivem aqui, sem IO. O adaptador Postgres e o HTTP só as chamam.
//!
//! O URL valida-se por forma e não por alcançabilidade: o tipo `internal`
//! existe PARA apontar a um servidor RTMP dentro da rede da organização, que é
//! exactamente o que a guarda anti-SSRF dos webhooks bloquearia. A decisão de
//! para onde o `ffmpeg` pode empurrar media é do módulo de directo (ADR-0003).

use delonix_meet_core::DomainError;

pub const MAX_LABEL: usize = 80;
pub const MAX_URL: usize = 2048;
pub const MAX_KEY: usize = 4096;
/// Caracteres da chave mostrados para o admin a reconhecer — nunca para a
/// reconstituir.
pub const KEY_PREFIX_LEN: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Youtube,
    Facebook,
    Linkedin,
    Rtmp,
    Internal,
}

impl Kind {
    pub const ALL: [&'static str; 5] = ["youtube", "facebook", "linkedin", "rtmp", "internal"];

    pub fn parse(s: &str) -> Result<Self, DomainError> {
        Ok(match s {
            "youtube" => Self::Youtube,
            "facebook" => Self::Facebook,
            "linkedin" => Self::Linkedin,
            "rtmp" => Self::Rtmp,
            "internal" => Self::Internal,
            other => {
                return Err(DomainError::invalid(
                    "stream_destination.invalid_kind",
                    format!(
                        "tipo de destino inválido «{other}» — válidos: {}",
                        Self::ALL.join(", ")
                    ),
                )
                .with_field("kind", Self::ALL.join(" | ")))
            }
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Youtube => "youtube",
            Self::Facebook => "facebook",
            Self::Linkedin => "linkedin",
            Self::Rtmp => "rtmp",
            Self::Internal => "internal",
        }
    }
}

/// Estado declarado do destino. O servidor não sonda a plataforma: o cliente
/// marca `expired` quando a plataforma recusa a chave.
pub fn validate_state(s: &str) -> Result<(), DomainError> {
    if matches!(s, "ready" | "expired" | "error") {
        Ok(())
    } else {
        Err(DomainError::invalid(
            "stream_destination.invalid_state",
            format!("estado inválido «{s}» — válidos: ready, expired, error"),
        )
        .with_field("state", "ready | expired | error"))
    }
}

pub fn validate_label(label: &str) -> Result<String, DomainError> {
    let l = label.trim();
    if l.is_empty() || l.chars().count() > MAX_LABEL {
        return Err(DomainError::invalid(
            "stream_destination.invalid_label",
            format!("o rótulo tem de ter 1-{MAX_LABEL} caracteres"),
        )
        .with_field("label", format!("1-{MAX_LABEL} caracteres")));
    }
    Ok(l.to_string())
}

pub fn validate_key(key: &str) -> Result<(), DomainError> {
    if key.chars().count() > MAX_KEY || key.chars().any(char::is_control) {
        return Err(DomainError::invalid(
            "stream_destination.invalid_key",
            "chave inválida (demasiado longa ou com caracteres de controlo)",
        )
        .with_field(
            "stream_key",
            format!("até {MAX_KEY} caracteres, sem controlo"),
        ));
    }
    Ok(())
}

/// Forma do URL: esquema RTMP/RTMPS/HTTP(S), com host, sem credenciais embutidas
/// (uma credencial no URL escapava à cifra da chave e aparecia em logs).
pub fn validate_url(raw: &str) -> Result<String, DomainError> {
    let invalid = |m: &str| {
        DomainError::invalid("stream_destination.invalid_url", m.to_string())
            .with_field("url", "rtmp(s):// ou http(s)://, com host, sem credenciais")
    };
    let t = raw.trim();
    if t.is_empty() || t.chars().count() > MAX_URL {
        return Err(invalid("URL vazio ou demasiado longo"));
    }
    let u = url::Url::parse(t).map_err(|_| invalid("URL inválido"))?;
    if !matches!(u.scheme(), "rtmp" | "rtmps" | "http" | "https") {
        return Err(invalid(
            "esquema inválido — use rtmp://, rtmps://, http:// ou https://",
        ));
    }
    if u.host_str().is_none() {
        return Err(invalid("URL sem host"));
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err(invalid(
            "o URL não pode conter credenciais — a chave vai no campo próprio",
        ));
    }
    Ok(t.to_string())
}

pub fn key_prefix(key: &str) -> String {
    key.chars().take(KEY_PREFIX_LEN).collect()
}

/// O contexto da cifra da chave: liga o texto cifrado À LINHA. Copiar o valor
/// para outro destino (mesmo da mesma org) não o abre.
pub fn key_aad(destination_id: &uuid::Uuid) -> String {
    format!("stream_destinations.stream_key:{destination_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_roundtrip_and_refuse_unknown() {
        for k in Kind::ALL {
            assert_eq!(Kind::parse(k).unwrap().as_str(), k);
        }
        assert_eq!(
            Kind::parse("twitch").unwrap_err().code,
            "stream_destination.invalid_kind"
        );
    }

    #[test]
    fn url_shape() {
        assert!(validate_url("rtmp://a.rtmp.youtube.com/live2").is_ok());
        assert!(validate_url("rtmps://live-api-s.facebook.com:443/rtmp/").is_ok());
        assert!(
            validate_url("rtmp://10.0.0.5/live").is_ok(),
            "internal é permitido"
        );
        assert!(validate_url("ftp://x/y").is_err());
        assert!(validate_url("rtmp://user:pass@host/live").is_err());
        assert!(validate_url("   ").is_err());
    }

    #[test]
    fn label_key_state_and_prefix() {
        assert_eq!(validate_label("  YouTube  ").unwrap(), "YouTube");
        assert!(validate_label(&"x".repeat(81)).is_err());
        assert!(validate_key("abcd-efgh").is_ok());
        assert!(validate_key("a\nb").is_err());
        assert!(validate_state("expired").is_ok());
        assert!(validate_state("zombie").is_err());
        assert_eq!(key_prefix("live_1234567"), "live");
        let id = uuid::Uuid::nil();
        assert_eq!(
            key_aad(&id),
            "stream_destinations.stream_key:00000000-0000-0000-0000-000000000000"
        );
    }
}
