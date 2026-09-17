//! «Como entro nas sessões» — preferências pessoais que valem para todas as
//! salas.
//!
//! Quase todas são aplicadas pelo CLIENTE (é ele que tem o microfone, a câmara
//! e o desfoque): o servidor guarda-as e devolve-as no `join` da sala. Uma é
//! imposta pelo servidor: [`JoinPreferences::warn_before_recording`] — quem a
//! tem ligada e é anfitrião tem de confirmar explicitamente o início da
//! gravação no servidor ([`recording_start_decision`]).
//!
//! As omissões são as do comportamento de hoje (nada desligado, nada
//! desfocado, sem aviso): uma conta que nunca abriu a página entra como sempre
//! entrou.

use delonix_meet_core::DomainError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinPreferences {
    pub join_muted: bool,
    pub join_camera_off: bool,
    pub blur_background: bool,
    pub noise_suppression: bool,
    pub captions_always_on: bool,
    /// Idioma das legendas quando `captions_always_on` (BCP 47 curto). `None`
    /// = o idioma da sessão.
    pub captions_language: Option<String>,
    pub warn_before_recording: bool,
}

impl Default for JoinPreferences {
    fn default() -> Self {
        Self {
            join_muted: false,
            join_camera_off: false,
            blur_background: false,
            noise_suppression: false,
            captions_always_on: false,
            captions_language: None,
            warn_before_recording: false,
        }
    }
}

/// Idiomas de legenda aceites (os do motor de transcrição e tradução).
pub const CAPTION_LANGUAGES: &[&str] = &["pt", "en", "fr", "zh", "es", "ln", "kg", "umb"];

pub fn validate_captions_language(raw: Option<&str>) -> Result<Option<String>, DomainError> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(l) => CAPTION_LANGUAGES
            .iter()
            .find(|c| c.eq_ignore_ascii_case(l))
            .map(|c| Some(c.to_string()))
            .ok_or_else(|| {
                DomainError::invalid(
                    "join_preferences.invalid_captions_language",
                    format!(
                        "idioma de legendas não suportado: {}",
                        CAPTION_LANGUAGES.join(", ")
                    ),
                )
                .with_field("captions_language", CAPTION_LANGUAGES.join(" | "))
            }),
    }
}

/// O que o servidor faz com um pedido de INÍCIO de gravação.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingStart {
    /// Começa.
    Start,
    /// Não começa: o anfitrião tem «avisar antes de gravar» e não confirmou.
    /// O servidor responde a ESSE anfitrião a pedir confirmação.
    ConfirmationRequired,
}

/// `warn`: a preferência de quem pede; `confirmed`: o pedido traz a
/// confirmação explícita. Sem a preferência, o pedido antigo (sem campo)
/// continua a começar a gravação — compatível com os clientes actuais.
pub fn recording_start_decision(warn: bool, confirmed: bool) -> RecordingStart {
    if warn && !confirmed {
        RecordingStart::ConfirmationRequired
    } else {
        RecordingStart::Start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_todays_behaviour() {
        let d = JoinPreferences::default();
        assert!(!d.join_muted && !d.join_camera_off && !d.warn_before_recording);
        assert_eq!(d.captions_language, None);
    }

    #[test]
    fn captions_language_catalogue() {
        assert_eq!(validate_captions_language(None).unwrap(), None);
        assert_eq!(validate_captions_language(Some("  ")).unwrap(), None);
        assert_eq!(
            validate_captions_language(Some("PT")).unwrap(),
            Some("pt".into())
        );
        assert_eq!(
            validate_captions_language(Some("klingon"))
                .unwrap_err()
                .code,
            "join_preferences.invalid_captions_language"
        );
    }

    #[test]
    fn warn_before_recording_needs_explicit_confirmation() {
        use RecordingStart::*;
        assert_eq!(recording_start_decision(false, false), Start);
        assert_eq!(recording_start_decision(false, true), Start);
        assert_eq!(recording_start_decision(true, false), ConfirmationRequired);
        assert_eq!(recording_start_decision(true, true), Start);
    }
}
