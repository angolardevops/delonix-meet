//! Contexto **notification**: a caixa de entrada pessoal (G8).
//!
//! Uma notificação é a MEMÓRIA de um evento que já aconteceu noutro contexto
//! (convite, reunião a começar, chamada perdida, gravação pronta, transcrição
//! entregue). As regras vivem aqui, sem IO:
//!
//! - **que tipos existem** ([`Kind`]) — um tipo novo entra aqui e na `CHECK`
//!   da migração, nunca só num dos dois;
//! - **o que cada evento produz** ([`Draft`]): título e corpo em português,
//!   e um link RELATIVO da app (nunca um URL externo: a notificação é
//!   mostrada como link clicável, e um URL absoluto seria phishing servido
//!   por nós);
//! - **coalescência**: cada evento tem uma chave (`dedupe_key`) e um mesmo
//!   destinatário não recebe duas notificações com a mesma chave — o cron do
//!   auto-ring corre a cada minuto e pode ver a mesma reunião duas vezes;
//! - **limites de tamanho**: títulos de reunião e nomes de ficheiro vêm do
//!   utilizador e são cortados, não recusados (o evento de origem já foi
//!   aceite; a notificação não o pode fazer falhar);
//! - **retenção** ([`retention_cutoffs`]).

use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

pub const MAX_TITLE: usize = 140;
pub const MAX_BODY: usize = 500;
pub const MAX_LINK: usize = 512;

/// Lidas: apagadas ao fim de 90 dias.
pub const READ_RETENTION_DAYS: i64 = 90;
/// Todas (lidas ou não): apagadas ao fim de 180 dias.
pub const ALL_RETENTION_DAYS: i64 = 180;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    MeetingInvited,
    MeetingStarting,
    MeetingCancelled,
    CallMissed,
    RecordingReady,
    TranscriptionReady,
}

impl Kind {
    pub const ALL: [Kind; 6] = [
        Kind::MeetingInvited,
        Kind::MeetingStarting,
        Kind::MeetingCancelled,
        Kind::CallMissed,
        Kind::RecordingReady,
        Kind::TranscriptionReady,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::MeetingInvited => "meeting.invited",
            Kind::MeetingStarting => "meeting.starting",
            Kind::MeetingCancelled => "meeting.cancelled",
            Kind::CallMissed => "call.missed",
            Kind::RecordingReady => "recording.ready",
            Kind::TranscriptionReady => "transcription.ready",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// O que um evento produz, antes de ter destinatário e id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub kind: Kind,
    pub title: String,
    pub body: String,
    pub link: String,
    /// Chave de coalescência: por destinatário, no máximo uma notificação
    /// com esta chave.
    pub dedupe_key: String,
}

impl Draft {
    fn new(kind: Kind, entity: &str, title: String, body: String, link: String) -> Self {
        Draft {
            kind,
            title: clip(&title, MAX_TITLE),
            body: clip(&body, MAX_BODY),
            link: if is_safe_link(&link) {
                link
            } else {
                // Nunca um link inseguro: cai para a página inicial da app.
                "/".to_string()
            },
            dedupe_key: format!("{}:{entity}", kind.as_str()),
        }
    }
}

/// Corta a `max` caracteres (não bytes), com reticências, e tira caracteres de
/// controlo (um título com `\n` ou `\u{202e}` desfigurava a lista).
pub fn clip(s: &str, max: usize) -> String {
    let clean: String = s
        .trim()
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .filter(|c| !matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
        .collect();
    if clean.chars().count() <= max {
        return clean;
    }
    let mut out: String = clean.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Um link de notificação é um caminho RELATIVO da app: começa por `/`, não
/// por `//` (que o browser lê como outro host), sem `\`, sem esquema, sem
/// controlo, e com tamanho limitado.
pub fn is_safe_link(link: &str) -> bool {
    link.starts_with('/')
        && !link.starts_with("//")
        && !link.contains('\\')
        && !link.contains("://")
        && link.len() <= MAX_LINK
        && !link.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// Os códigos de sala são `[a-z-]` (ver `rooms::insert_room`); outra coisa
/// não entra num link.
fn room_path(code: &str) -> String {
    if !code.is_empty() && code.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
        format!("/#/r/{code}")
    } else {
        "/#/calendar".to_string()
    }
}

fn when(at: DateTime<Utc>) -> String {
    at.format("%d/%m/%Y %H:%M UTC").to_string()
}

pub fn meeting_invited(
    meeting_id: Uuid,
    meeting_title: &str,
    host_name: &str,
    starts_at: DateTime<Utc>,
) -> Draft {
    Draft::new(
        Kind::MeetingInvited,
        &meeting_id.to_string(),
        format!("Convite: {meeting_title}"),
        format!(
            "{host_name} convidou-o para «{meeting_title}», {}.",
            when(starts_at)
        ),
        "/#/calendar".to_string(),
    )
}

pub fn meeting_starting(meeting_id: Uuid, meeting_title: &str, room_code: &str) -> Draft {
    Draft::new(
        Kind::MeetingStarting,
        &meeting_id.to_string(),
        format!("A começar: {meeting_title}"),
        format!("A reunião «{meeting_title}» está a começar."),
        room_path(room_code),
    )
}

pub fn meeting_cancelled(
    meeting_id: Uuid,
    meeting_title: &str,
    host_name: &str,
    starts_at: DateTime<Utc>,
) -> Draft {
    Draft::new(
        Kind::MeetingCancelled,
        &meeting_id.to_string(),
        format!("Cancelada: {meeting_title}"),
        format!(
            "{host_name} cancelou «{meeting_title}», marcada para {}.",
            when(starts_at)
        ),
        "/#/calendar".to_string(),
    )
}

/// Uma chamada que tocou várias vezes na mesma sala é UMA chamada perdida.
pub fn call_missed(room_code: &str, caller_name: &str, voice: bool) -> Draft {
    let what = if voice {
        "chamada de voz"
    } else {
        "videochamada"
    };
    Draft::new(
        Kind::CallMissed,
        room_code,
        format!("Chamada perdida de {caller_name}"),
        format!("Não atendeu uma {what} de {caller_name}."),
        room_path(room_code),
    )
}

pub fn recording_ready(recording_id: Uuid, filename: &str) -> Draft {
    Draft::new(
        Kind::RecordingReady,
        &recording_id.to_string(),
        "Gravação pronta".to_string(),
        format!("A gravação «{filename}» já está na biblioteca."),
        "/#/recordings".to_string(),
    )
}

pub fn transcription_ready(recording_id: Uuid, filename: &str) -> Draft {
    Draft::new(
        Kind::TranscriptionReady,
        &recording_id.to_string(),
        "Transcrição pronta".to_string(),
        format!("A transcrição de «{filename}» está disponível."),
        "/#/recordings".to_string(),
    )
}

/// `(lidas_antes_de, todas_antes_de)`: apagam-se as lidas criadas antes do
/// primeiro instante e todas as criadas antes do segundo.
pub fn retention_cutoffs(now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    (
        now - Duration::days(READ_RETENTION_DAYS),
        now - Duration::days(ALL_RETENTION_DAYS),
    )
}

/// A mesma regra, para uma linha — é o que a consulta do varredor implementa.
pub fn is_expired(
    created_at: DateTime<Utc>,
    read_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> bool {
    let (read_cut, all_cut) = retention_cutoffs(now);
    created_at < all_cut || (read_at.is_some() && created_at < read_cut)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_roundtrip() {
        for k in Kind::ALL {
            assert_eq!(Kind::parse(k.as_str()), Some(k));
        }
        assert_eq!(Kind::parse("meeting.created"), None);
    }

    #[test]
    fn same_event_same_key_different_events_different_keys() {
        let m = Uuid::new_v4();
        let at = Utc::now();
        assert_eq!(
            meeting_starting(m, "A", "abc-def").dedupe_key,
            meeting_starting(m, "Outro título", "abc-def").dedupe_key
        );
        assert_ne!(
            meeting_invited(m, "A", "h", at).dedupe_key,
            meeting_cancelled(m, "A", "h", at).dedupe_key,
            "cancelar depois de convidar não é coalescido"
        );
        assert_ne!(
            meeting_starting(m, "A", "x").dedupe_key,
            meeting_starting(Uuid::new_v4(), "A", "x").dedupe_key
        );
        assert_eq!(
            call_missed("abc-def", "Ana", true).dedupe_key,
            "call.missed:abc-def"
        );
    }

    #[test]
    fn texts_are_portuguese_and_bounded() {
        let long = "x".repeat(1000);
        let d = meeting_invited(Uuid::nil(), &long, "Ana", Utc::now());
        assert!(d.title.chars().count() <= MAX_TITLE);
        assert!(d.body.chars().count() <= MAX_BODY);
        assert!(d.title.ends_with('…'));
        assert_eq!(d.link, "/#/calendar");
        let c = call_missed("abc-def", "Ana", false);
        assert_eq!(c.title, "Chamada perdida de Ana");
        assert!(c.body.contains("videochamada"));
        assert_eq!(c.link, "/#/r/abc-def");
        assert_eq!(clip("  a\nb\u{202e}c ", 10), "a bc");
    }

    #[test]
    fn links_are_relative_app_paths_only() {
        assert!(is_safe_link("/#/recordings"));
        assert!(!is_safe_link("https://evil.example/"));
        assert!(!is_safe_link("//evil.example/"));
        assert!(!is_safe_link("/\\evil.example"));
        assert!(!is_safe_link("javascript:alert(1)"));
        assert!(!is_safe_link("/a b"));
        // Um código de sala estranho não chega ao link.
        assert_eq!(
            meeting_starting(Uuid::nil(), "t", "x//evil.example").link,
            "/#/calendar"
        );
    }

    #[test]
    fn retention_rule() {
        let now = Utc::now();
        let d = |n: i64| now - Duration::days(n);
        assert!(!is_expired(d(89), Some(now), now));
        assert!(is_expired(d(91), Some(now), now));
        assert!(!is_expired(d(91), None, now), "não lida fica até 180");
        assert!(is_expired(d(181), None, now));
        let (read_cut, all_cut) = retention_cutoffs(now);
        assert!(all_cut < read_cut);
    }
}
