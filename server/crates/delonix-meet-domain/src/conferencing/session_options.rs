//! Opções de sessão de uma reunião agendada (R184): o formato, a sala de
//! espera, a gravação automática e a qualidade pedida ao gravador.
//!
//! São da REUNIÃO e passam à SALA quando ela nasce (no `start` da BFF, ou logo
//! na criação pela v1, que já devolve o link). A BFF e a v1 validam com estas
//! funções — uma regra, dois adaptadores (ADR-0004 §5, regra 8).
//!
//! O contrato de dados é o da UI (`MeetingSessionOptions`): `format` ∈
//! `meeting|training|broadcast|hybrid`, `record_quality` ∈
//! `2160p|1080p|720p|audio`; por omissão `meeting`, sem sala de espera, sem
//! gravação automática, `1080p`.

use delonix_meet_core::DomainError;

pub const FORMATS: [&str; 4] = ["meeting", "training", "broadcast", "hybrid"];
pub const RECORD_QUALITIES: [&str; 4] = ["2160p", "1080p", "720p", "audio"];

pub const DEFAULT_FORMAT: &str = "meeting";
pub const DEFAULT_RECORD_QUALITY: &str = "1080p";

/// `format` tem de ser um dos quatro. Código estável `meeting.invalid_format`.
pub fn validate_format(format: &str) -> Result<(), DomainError> {
    if FORMATS.contains(&format) {
        return Ok(());
    }
    Err(DomainError::invalid(
        "meeting.invalid_format",
        "format tem de ser meeting, training, broadcast ou hybrid",
    )
    .with_field("format", "meeting | training | broadcast | hybrid"))
}

/// `record_quality` tem de ser uma das quatro. Código estável
/// `meeting.invalid_record_quality`.
pub fn validate_record_quality(quality: &str) -> Result<(), DomainError> {
    if RECORD_QUALITIES.contains(&quality) {
        return Ok(());
    }
    Err(DomainError::invalid(
        "meeting.invalid_record_quality",
        "record_quality tem de ser 2160p, 1080p, 720p ou audio",
    )
    .with_field("record_quality", "2160p | 1080p | 720p | audio"))
}

/// Formato da reunião → formato da sala (`rooms.format`). A sala chama
/// `normal` ao que a reunião chama `meeting`; só `training` abre salas de grupo.
pub fn room_format_for(meeting_format: &str) -> &'static str {
    match meeting_format {
        "training" => "training",
        "broadcast" => "broadcast",
        "hybrid" => "hybrid",
        _ => "normal",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert!(validate_format(DEFAULT_FORMAT).is_ok());
        assert!(validate_record_quality(DEFAULT_RECORD_QUALITY).is_ok());
    }

    #[test]
    fn every_listed_value_is_accepted() {
        for f in FORMATS {
            assert!(validate_format(f).is_ok(), "{f}");
        }
        for q in RECORD_QUALITIES {
            assert!(validate_record_quality(q).is_ok(), "{q}");
        }
    }

    #[test]
    fn unknown_values_are_refused_with_stable_codes() {
        let e = validate_format("webinar").unwrap_err();
        assert_eq!(e.code, "meeting.invalid_format");
        assert_eq!(e.details[0].field, "format");
        // Sem normalização: o contrato é minúsculas.
        assert!(validate_format("Meeting").is_err());
        let e = validate_record_quality("4k").unwrap_err();
        assert_eq!(e.code, "meeting.invalid_record_quality");
        assert_eq!(e.details[0].field, "record_quality");
    }

    #[test]
    fn room_format_mapping() {
        assert_eq!(room_format_for("meeting"), "normal");
        assert_eq!(room_format_for("training"), "training");
        assert_eq!(room_format_for("broadcast"), "broadcast");
        assert_eq!(room_format_for("hybrid"), "hybrid");
        assert_eq!(room_format_for("desconhecido"), "normal");
    }
}
