//! Canais de TV pela Internet (RFC-0001, Fase 1).
//!
//! Um canal é uma identidade persistente da organização — nome, endereço,
//! fuso, visibilidade e política de gravação — que existe entre emissões. As
//! regras de FORMA vivem aqui, sem IO; o adaptador Postgres e o HTTP só as
//! chamam. O fuso, que só a base sabe validar por inteiro (a lista IANA), é
//! verificado no adaptador.
//!
//! Chama-se `TvChannel` e não `Channel` porque `conferencing::channels::Channel`
//! já é o canal de LIGAÇÃO de um participante (app, SIP, telefone…).

use delonix_meet_core::DomainError;

pub const MAX_NAME: usize = 120;
pub const MAX_DESCRIPTION: usize = 1000;
pub const MIN_SLUG: usize = 3;
pub const MAX_SLUG: usize = 63;
pub const MAX_TIMEZONE: usize = 64;
pub const MAX_RETENTION_DAYS: i32 = 3650;
/// Luanda: o fuso do mercado de origem. É só a omissão; o canal guarda o seu.
pub const DEFAULT_TIMEZONE: &str = "Africa/Luanda";

/// Quem pode ver o canal. `restricted` exige autorização por espectador (RF-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Private,
    Restricted,
}

impl Visibility {
    pub const ALL: [&'static str; 3] = ["public", "private", "restricted"];

    pub fn parse(s: &str) -> Result<Self, DomainError> {
        Ok(match s {
            "public" => Self::Public,
            "private" => Self::Private,
            "restricted" => Self::Restricted,
            other => {
                return Err(DomainError::invalid(
                    "tv.channel.invalid_visibility",
                    format!(
                        "visibilidade inválida «{other}» — válidas: {}",
                        Self::ALL.join(", ")
                    ),
                )
                .with_field("visibility", Self::ALL.join(" | ")))
            }
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
            Self::Restricted => "restricted",
        }
    }
}

pub fn validate_name(raw: &str) -> Result<String, DomainError> {
    let name = raw.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME || name.chars().any(char::is_control) {
        return Err(DomainError::invalid(
            "tv.channel.invalid_name",
            format!("o nome tem de ter entre 1 e {MAX_NAME} caracteres, sem controlo"),
        )
        .with_field("name", format!("1 a {MAX_NAME} caracteres")));
    }
    Ok(name.to_string())
}

pub fn validate_description(raw: &str) -> Result<String, DomainError> {
    let d = raw.trim();
    if d.chars().count() > MAX_DESCRIPTION {
        return Err(DomainError::invalid(
            "tv.channel.invalid_description",
            format!("a descrição não pode passar de {MAX_DESCRIPTION} caracteres"),
        )
        .with_field("description", format!("até {MAX_DESCRIPTION} caracteres")));
    }
    Ok(d.to_string())
}

/// `a-z`, `0-9` e `-`; sem hífen nas pontas nem hífenes seguidos. Vai num URL
/// público, por isso é estrito: não se normaliza em silêncio, recusa-se.
pub fn validate_slug(raw: &str) -> Result<String, DomainError> {
    let s = raw.trim();
    let ok = (MIN_SLUG..=MAX_SLUG).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--");
    if !ok {
        return Err(DomainError::invalid(
            "tv.channel.invalid_slug",
            format!(
                "o endereço tem de ter {MIN_SLUG} a {MAX_SLUG} caracteres, só a-z, 0-9 e hífen \
                 (sem hífen nas pontas nem repetido)"
            ),
        )
        .with_field("slug", "a-z, 0-9 e hífen"));
    }
    Ok(s.to_string())
}

/// Forma do nome de um fuso IANA (`Area/Local`, ou `UTC`). Que ele EXISTA
/// confirma-o o adaptador contra `pg_timezone_names`.
pub fn validate_timezone_shape(raw: &str) -> Result<String, DomainError> {
    let tz = raw.trim();
    let ok = !tz.is_empty()
        && tz.len() <= MAX_TIMEZONE
        && tz
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'+' | b'-'))
        && !tz.starts_with('/')
        && !tz.ends_with('/')
        && !tz.contains("//");
    if !ok {
        return Err(DomainError::invalid(
            "tv.channel.invalid_timezone",
            "fuso inválido — use um nome IANA, por exemplo Africa/Luanda",
        )
        .with_field("timezone", "nome IANA"));
    }
    Ok(tz.to_string())
}

pub fn validate_retention_days(days: i32) -> Result<i32, DomainError> {
    if !(1..=MAX_RETENTION_DAYS).contains(&days) {
        return Err(DomainError::invalid(
            "tv.channel.invalid_retention",
            format!("a retenção tem de ser de 1 a {MAX_RETENTION_DAYS} dias"),
        )
        .with_field(
            "recording_retention_days",
            format!("1 a {MAX_RETENTION_DAYS}"),
        ));
    }
    Ok(days)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_e_estrito() {
        for ok in ["tv-angola", "canal1", "a1b", "abc"] {
            assert_eq!(validate_slug(ok).unwrap(), ok);
        }
        for mau in [
            "",
            "ab",
            "-abc",
            "abc-",
            "a--b",
            "TV-Angola",
            "tv angola",
            "tv_angola",
            "tv/angola",
            "canal.ao",
            "çanal",
        ] {
            let e = validate_slug(mau).unwrap_err();
            assert_eq!(e.code, "tv.channel.invalid_slug", "{mau:?}");
        }
        assert!(validate_slug(&"a".repeat(MAX_SLUG + 1)).is_err());
        assert!(validate_slug(&"a".repeat(MAX_SLUG)).is_ok());
    }

    #[test]
    fn nome_e_descricao_tem_limites() {
        assert_eq!(validate_name("  Canal Um ").unwrap(), "Canal Um");
        assert!(validate_name("   ").is_err());
        assert!(validate_name("a\nb").is_err());
        assert!(validate_name(&"x".repeat(MAX_NAME + 1)).is_err());
        assert!(validate_description(&"x".repeat(MAX_DESCRIPTION)).is_ok());
        assert!(validate_description(&"x".repeat(MAX_DESCRIPTION + 1)).is_err());
    }

    #[test]
    fn visibilidade_recusa_o_desconhecido() {
        for v in Visibility::ALL {
            assert_eq!(Visibility::parse(v).unwrap().as_str(), v);
        }
        let e = Visibility::parse("publico").unwrap_err();
        assert_eq!(e.code, "tv.channel.invalid_visibility");
    }

    #[test]
    fn fuso_tem_forma_iana() {
        for ok in [
            "Africa/Luanda",
            "UTC",
            "America/Argentina/Buenos_Aires",
            "Etc/GMT+1",
        ] {
            assert!(validate_timezone_shape(ok).is_ok(), "{ok}");
        }
        for mau in [
            "",
            "/UTC",
            "Africa/",
            "Africa//Luanda",
            "Africa/Lu anda",
            "../etc/passwd;",
        ] {
            assert!(validate_timezone_shape(mau).is_err(), "{mau:?}");
        }
    }

    #[test]
    fn retencao_dentro_do_intervalo() {
        assert!(validate_retention_days(0).is_err());
        assert!(validate_retention_days(1).is_ok());
        assert!(validate_retention_days(MAX_RETENTION_DAYS).is_ok());
        assert!(validate_retention_days(MAX_RETENTION_DAYS + 1).is_err());
    }
}
