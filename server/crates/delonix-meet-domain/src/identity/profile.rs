//! «A minha conta» — os campos do perfil que a própria pessoa edita, e os que
//! não pode editar porque vêm do Odoo.
//!
//! Regras (sem IO):
//!
//! - **Idioma da interface** ([`canonical_locale`]): um catálogo fechado. Os
//!   códigos novos do template (`pt-AO`, `en`, `fr-FR`, `zh-CN`) e os antigos
//!   que o web já grava (`pt`, `en`, `fr`) são todos aceites e guardados tal
//!   como chegam, em forma canónica — nenhuma conta existente muda de idioma
//!   por actualizar o servidor. Um código fora do catálogo é recusado, nunca
//!   ignorado.
//! - **Fuso horário** ([`validate_timezone`]): um nome IANA da base `tz`
//!   (`Africa/Luanda`), validado contra a base compilada — não contra uma
//!   lista escrita à mão, que envelhece.
//! - **Nome a mostrar e cargo**: texto de uma linha, sem caracteres de
//!   controlo nem de direcção (um `\u{202e}` desfigurava a lista de
//!   participantes), com tecto.
//! - **Campos geridos pelo Odoo** ([`ManagedField`], [`check_not_managed`]):
//!   numa conta gerida por um Odoo, o nome legal, o correio e o departamento
//!   são do ERP. Tentar alterá-los é `409 profile.field_managed_by_odoo` — o
//!   pedido está bem formado, mas o estado da conta não o admite.
//! - **Fotografia** ([`sniff_avatar`]): PNG, JPEG ou WebP, reconhecidos pelos
//!   primeiros bytes (o `Content-Type` do cliente não conta), até
//!   [`AVATAR_MAX_BYTES`].

use delonix_meet_core::DomainError;

pub const DISPLAY_NAME_MAX: usize = 80;
pub const JOB_TITLE_MAX: usize = 100;
/// Tecto da fotografia de perfil. Um retrato de 512×512 em JPEG fica muito
/// abaixo; o tecto trava quem carrega uma foto de 20 MB da câmara.
pub const AVATAR_MAX_BYTES: usize = 1024 * 1024;

/// Minutos entre sincronizações do directório do Odoo (o `SYNC_MAX_AGE_SECS`
/// de `odoo_sso`, em minutos). É o «alterações do Odoo chegam em N min» do
/// ecrã: é o pior caso, não uma promessa de tempo real.
pub const ODOO_SYNC_INTERVAL_MINUTES: u32 = 60;

/// Idiomas da interface. O primeiro elemento é o código canónico.
pub const LOCALES: &[&str] = &["pt-AO", "en", "fr-FR", "zh-CN", "pt", "fr"];

/// Forma canónica de um idioma aceite (`pt-ao` → `pt-AO`), ou erro.
pub fn canonical_locale(raw: &str) -> Result<&'static str, DomainError> {
    let wanted = raw.trim();
    LOCALES
        .iter()
        .find(|l| l.eq_ignore_ascii_case(wanted))
        .copied()
        .ok_or_else(|| {
            DomainError::invalid(
                "profile.invalid_locale",
                format!("idioma não suportado: use um de {}", LOCALES.join(", ")),
            )
            .with_field("locale", LOCALES.join(" | "))
        })
}

/// Um nome IANA (`Africa/Luanda`). Devolve a forma canónica da base `tz`.
pub fn validate_timezone(raw: &str) -> Result<String, DomainError> {
    let name = raw.trim();
    name.parse::<chrono_tz::Tz>()
        .map(|tz| tz.name().to_string())
        .map_err(|_| {
            DomainError::invalid(
                "profile.invalid_timezone",
                "fuso horário desconhecido: use um nome IANA, p.ex. Africa/Luanda",
            )
            .with_field("timezone", "nome IANA")
        })
}

/// Deslocamento actual do fuso em relação a UTC, em minutos (para mostrar
/// «WAT (UTC+1)» sem o cliente ter a base `tz`).
pub fn utc_offset_minutes(tz_name: &str, at: chrono::DateTime<chrono::Utc>) -> Option<i32> {
    use chrono::Offset;
    let tz: chrono_tz::Tz = tz_name.parse().ok()?;
    Some(at.with_timezone(&tz).offset().fix().local_minus_utc() / 60)
}

fn one_line(
    raw: &str,
    max: usize,
    code: &'static str,
    field: &'static str,
    allow_empty: bool,
) -> Result<String, DomainError> {
    let v = raw.trim();
    let bad_char = v
        .chars()
        .any(|c| c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'));
    let len = v.chars().count();
    if bad_char || len > max || (!allow_empty && len == 0) {
        let min = if allow_empty { 0 } else { 1 };
        return Err(DomainError::invalid(
            code,
            format!("{field}: {min}-{max} caracteres numa só linha"),
        )
        .with_field(
            field,
            format!("{min}-{max} caracteres, sem caracteres de controlo"),
        ));
    }
    Ok(v.to_string())
}

/// Nome que aparece na sala e nas legendas: 1–80 caracteres.
pub fn validate_display_name(raw: &str) -> Result<String, DomainError> {
    one_line(
        raw,
        DISPLAY_NAME_MAX,
        "profile.invalid_display_name",
        "display_name",
        false,
    )
}

/// Cargo: 0–100 caracteres (vazio apaga).
pub fn validate_job_title(raw: &str) -> Result<String, DomainError> {
    one_line(
        raw,
        JOB_TITLE_MAX,
        "profile.invalid_job_title",
        "job_title",
        true,
    )
}

/// Campos cuja autoridade é o Odoo numa conta gerida.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedField {
    LegalName,
    Email,
    Department,
}

impl ManagedField {
    pub const ALL: [ManagedField; 3] = [
        ManagedField::LegalName,
        ManagedField::Email,
        ManagedField::Department,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ManagedField::LegalName => "legal_name",
            ManagedField::Email => "email",
            ManagedField::Department => "department",
        }
    }
}

pub const FIELD_MANAGED_BY_ODOO: &str = "profile.field_managed_by_odoo";
pub const FIELD_READ_ONLY: &str = "profile.field_read_only";

/// Recusa a escrita de um campo gerido. `odoo_managed`: a conta tem autoridade
/// Odoo. Numa conta local estes campos também não se mudam por esta rota (o
/// correio é identidade de login e muda-se por outro fluxo; o nome legal e o
/// departamento vêm da administração), mas o código é outro — o cliente não
/// deve mandar a pessoa «editar no Odoo» quando não há Odoo.
pub fn check_not_managed(
    odoo_managed: bool,
    attempted: &[ManagedField],
) -> Result<(), DomainError> {
    let Some(first) = attempted.first() else {
        return Ok(());
    };
    let mut err = if odoo_managed {
        DomainError::conflict(
            FIELD_MANAGED_BY_ODOO,
            format!(
                "{} vem do Odoo: altere-o lá e chega aqui na sincronização seguinte (até {} min)",
                first.as_str(),
                ODOO_SYNC_INTERVAL_MINUTES
            ),
        )
    } else {
        DomainError::conflict(
            FIELD_READ_ONLY,
            format!("{} não se altera nesta rota", first.as_str()),
        )
    };
    for f in attempted {
        err = err.with_field(f.as_str(), "só leitura");
    }
    Err(err)
}

/// Tipo de imagem reconhecido pelos primeiros bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvatarType {
    Png,
    Jpeg,
    Webp,
}

impl AvatarType {
    pub fn mime(self) -> &'static str {
        match self {
            AvatarType::Png => "image/png",
            AvatarType::Jpeg => "image/jpeg",
            AvatarType::Webp => "image/webp",
        }
    }
}

/// Valida a fotografia: tamanho e assinatura. Um SVG (script embebido) ou um
/// HTML com `Content-Type: image/png` não passam.
pub fn sniff_avatar(bytes: &[u8]) -> Result<AvatarType, DomainError> {
    if bytes.is_empty() {
        return Err(DomainError::invalid(
            "profile.avatar_empty",
            "a fotografia está vazia",
        ));
    }
    if bytes.len() > AVATAR_MAX_BYTES {
        return Err(DomainError::new(
            delonix_meet_core::ErrorKind::FailedPrecondition,
            "profile.avatar_too_large",
            format!(
                "a fotografia tem de ter no máximo {} KiB",
                AVATAR_MAX_BYTES / 1024
            ),
        ));
    }
    let t = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        AvatarType::Png
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        AvatarType::Jpeg
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        AvatarType::Webp
    } else {
        return Err(DomainError::new(
            delonix_meet_core::ErrorKind::FailedPrecondition,
            "profile.avatar_unsupported_type",
            "a fotografia tem de ser PNG, JPEG ou WebP",
        ));
    };
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_new_and_legacy_are_accepted_in_canonical_form() {
        assert_eq!(canonical_locale("pt-AO").unwrap(), "pt-AO");
        assert_eq!(canonical_locale(" pt-ao ").unwrap(), "pt-AO");
        assert_eq!(canonical_locale("zh-cn").unwrap(), "zh-CN");
        assert_eq!(canonical_locale("fr-FR").unwrap(), "fr-FR");
        // os antigos continuam a valer (o web grava `pt`)
        for l in ["pt", "en", "fr"] {
            assert_eq!(canonical_locale(l).unwrap(), l);
        }
        for bad in ["", "de", "pt-BR", "zh", "en-US", "pt_AO"] {
            let e = canonical_locale(bad).unwrap_err();
            assert_eq!(e.code, "profile.invalid_locale", "{bad}");
        }
    }

    #[test]
    fn timezones_are_iana_names() {
        assert_eq!(validate_timezone("Africa/Luanda").unwrap(), "Africa/Luanda");
        assert_eq!(
            validate_timezone(" Europe/Lisbon ").unwrap(),
            "Europe/Lisbon"
        );
        assert_eq!(validate_timezone("UTC").unwrap(), "UTC");
        for bad in [
            "",
            "WAT",
            "UTC+1",
            "Africa/Luandaa",
            "../etc/passwd",
            "África/Luanda",
        ] {
            assert_eq!(
                validate_timezone(bad).unwrap_err().code,
                "profile.invalid_timezone",
                "{bad}"
            );
        }
        let at = chrono::DateTime::parse_from_rfc3339("2026-09-17T09:41:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(utc_offset_minutes("Africa/Luanda", at), Some(60));
        assert_eq!(utc_offset_minutes("Europe/Lisbon", at), Some(60)); // verão
        assert_eq!(utc_offset_minutes("nope", at), None);
    }

    #[test]
    fn display_name_and_job_title_shape() {
        assert_eq!(validate_display_name("  Ana Mbala ").unwrap(), "Ana Mbala");
        assert!(validate_display_name("").is_err());
        assert!(validate_display_name("   ").is_err());
        assert!(validate_display_name(&"a".repeat(81)).is_err());
        assert!(
            validate_display_name(&"é".repeat(80)).is_ok(),
            "conta caracteres"
        );
        assert!(validate_display_name("Ana\nMbala").is_err());
        assert!(validate_display_name("Ana\u{202e}alabM").is_err());
        assert_eq!(validate_job_title("").unwrap(), "");
        assert_eq!(
            validate_job_title("Directora de Formação").unwrap(),
            "Directora de Formação"
        );
        assert_eq!(
            validate_job_title(&"x".repeat(101)).unwrap_err().code,
            "profile.invalid_job_title"
        );
    }

    #[test]
    fn managed_fields_are_refused_with_a_stable_code() {
        assert!(check_not_managed(true, &[]).is_ok());
        let e =
            check_not_managed(true, &[ManagedField::Email, ManagedField::Department]).unwrap_err();
        assert_eq!(e.code, FIELD_MANAGED_BY_ODOO);
        assert_eq!(e.kind, delonix_meet_core::ErrorKind::Conflict);
        assert_eq!(e.details.len(), 2);
        assert!(e.message.contains("60 min"), "{}", e.message);
        let e = check_not_managed(false, &[ManagedField::LegalName]).unwrap_err();
        assert_eq!(e.code, FIELD_READ_ONLY);
    }

    #[test]
    fn avatar_is_sniffed_not_trusted() {
        let png = b"\x89PNG\r\n\x1a\n0000";
        assert_eq!(sniff_avatar(png).unwrap(), AvatarType::Png);
        assert_eq!(
            sniff_avatar(&[0xFF, 0xD8, 0xFF, 0xE0]).unwrap().mime(),
            "image/jpeg"
        );
        assert_eq!(
            sniff_avatar(b"RIFF\0\0\0\0WEBPVP8 ").unwrap(),
            AvatarType::Webp
        );
        assert_eq!(
            sniff_avatar(b"<svg onload=alert(1)>").unwrap_err().code,
            "profile.avatar_unsupported_type"
        );
        assert_eq!(sniff_avatar(b"").unwrap_err().code, "profile.avatar_empty");
        let mut big = png.to_vec();
        big.resize(AVATAR_MAX_BYTES + 1, 0);
        assert_eq!(
            sniff_avatar(&big).unwrap_err().code,
            "profile.avatar_too_large"
        );
    }
}
