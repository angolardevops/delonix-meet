//! Os aparelhos de um ramal móvel (ADR-0023): o que se aceita ao registar um, e o que um *wake* pode
//! mandar. Sem IO: o registo e o envio estão em `server/src/voice_devices.rs`.

/// O sistema do aparelho.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Android,
    Ios,
}

impl Platform {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "android" => Some(Self::Android),
            "ios" => Some(Self::Ios),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Android => "android",
            Self::Ios => "ios",
        }
    }
}

/// Quem entrega o push.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    /// Firebase Cloud Messaging (mensagem só de dados, prioridade alta).
    Fcm,
    /// APNs, push VoIP (`apns-push-type: voip`). Só iOS: o iOS obriga a reportar ao CallKit.
    ApnsVoip,
    /// Só laboratório e testes: o servidor entrega o pedido a um URL do operador (`PUSH_LAB_URL`).
    Lab,
}

impl Provider {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "fcm" => Some(Self::Fcm),
            "apns_voip" => Some(Self::ApnsVoip),
            "lab" => Some(Self::Lab),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fcm => "fcm",
            Self::ApnsVoip => "apns_voip",
            Self::Lab => "lab",
        }
    }

    /// Um push VoIP da Apple só chega a um iPhone, e o FCM só se usa num Android (o iPhone usa APNs
    /// VoIP: um push normal não o acorda a tempo nem o deixa reportar ao CallKit). `lab` serve os dois.
    pub fn serves(self, platform: Platform) -> bool {
        matches!(
            (self, platform),
            (Self::Fcm, Platform::Android) | (Self::ApnsVoip, Platform::Ios) | (Self::Lab, _)
        )
    }
}

/// O token de push de um aparelho: texto visível, sem espaços, de tamanho razoável. O servidor nunca
/// o interpreta (é do fornecedor), só o guarda cifrado.
pub const MAX_TOKEN_LEN: usize = 4096;

pub fn is_valid_token(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_TOKEN_LEN && s.chars().all(|c| c.is_ascii_graphic())
}

/// Identificador da versão da app (só para diagnóstico): curto e sem controlo.
pub const MAX_APP_VERSION_LEN: usize = 64;

pub fn is_valid_app_version(s: &str) -> bool {
    s.len() <= MAX_APP_VERSION_LEN && s.chars().all(|c| c.is_ascii_graphic() || c == ' ')
}

/// Quantos *wakes* um ramal aguenta por minuto. Acima disto o servidor deixa de acordar e a chamada
/// falha, como antes: um chamador não pode usar isto para inundar o telemóvel de alguém.
pub const MAX_WAKES_PER_MINUTE: i64 = 12;

/// Quanto tempo se guardam os pedidos de *wake* (só servem a idempotência e o limite).
pub const WAKE_RETENTION_HOURS: i64 = 24;

/// Quantos aparelhos activos um ramal pode ter (um telemóvel, um tablet, um segundo telemóvel…).
pub const MAX_DEVICES_PER_EXTENSION: i64 = 8;

/// O que o Lua manda como `caller_extension`: o número curto de quem liga (ou o utilizador SIP do
/// chamador, no caso de ramais da empresa). Só texto sem controlo e curto: vai num push.
pub fn clean_caller(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+'))
        .take(40)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_and_provider_parse_only_known_names() {
        assert_eq!(Platform::parse("android"), Some(Platform::Android));
        assert_eq!(Platform::parse("ios"), Some(Platform::Ios));
        assert_eq!(Platform::parse("Android"), None);
        assert_eq!(Platform::parse("windows"), None);
        assert_eq!(Provider::parse("fcm"), Some(Provider::Fcm));
        assert_eq!(Provider::parse("apns_voip"), Some(Provider::ApnsVoip));
        assert_eq!(Provider::parse("apns"), None);
        assert_eq!(Provider::parse(""), None);
    }

    #[test]
    fn a_provider_only_serves_the_platform_it_can_wake() {
        assert!(Provider::Fcm.serves(Platform::Android));
        assert!(!Provider::Fcm.serves(Platform::Ios));
        assert!(Provider::ApnsVoip.serves(Platform::Ios));
        assert!(!Provider::ApnsVoip.serves(Platform::Android));
        assert!(Provider::Lab.serves(Platform::Android) && Provider::Lab.serves(Platform::Ios));
    }

    #[test]
    fn token_shape() {
        assert!(is_valid_token("fcm:APA91bH-x_Y.z"));
        assert!(!is_valid_token(""));
        assert!(!is_valid_token("com espaço"));
        assert!(!is_valid_token("nova\nlinha"));
        assert!(!is_valid_token("acentuação"));
        assert!(is_valid_token(&"a".repeat(MAX_TOKEN_LEN)));
        assert!(!is_valid_token(&"a".repeat(MAX_TOKEN_LEN + 1)));
    }

    #[test]
    fn app_version_shape() {
        assert!(is_valid_app_version(""));
        assert!(is_valid_app_version("0.1.0+3"));
        assert!(!is_valid_app_version("tab\t"));
        assert!(!is_valid_app_version(&"1".repeat(MAX_APP_VERSION_LEN + 1)));
    }

    #[test]
    fn caller_is_cleaned_before_it_goes_into_a_push() {
        assert_eq!(clean_caller("1004"), "1004");
        assert_eq!(clean_caller("ramal_ab-12.x"), "ramal_ab-12.x");
        assert_eq!(clean_caller("a\"b'c\\d<e>f g\n"), "abcdefg");
        assert_eq!(clean_caller(&"9".repeat(100)).len(), 40);
    }
}
