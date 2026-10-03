//! Preferências de notificação por TIPO e por CANAL.
//!
//! Os tipos são os do centro de notificações (G8, [`super::Kind`]) — não há
//! preferência para um evento que nenhum produtor emite. Os canais:
//!
//! - `in_app`: a caixa de entrada e o aviso imediato pelo `/rtc`. É o único
//!   canal com entrega implementada; desligá-lo para um tipo faz o produtor
//!   não criar a notificação.
//! - `email`: guardado, mas **sem entrega** — não há adaptador de correio de
//!   saída no servidor. A API di-lo em `channels[].delivery = "not_configured"`.
//! - `sms`: guardado; a entrega depende do gateway de SMS (ADR-0005) e do
//!   telefone do perfil, e ainda não há produtor que envie notificações por
//!   SMS — por isso também `not_configured`.
//!
//! Guardar uma preferência para um canal sem entrega é honesto (a pessoa diz
//! o que quer para quando existir) desde que a API não finja que entrega.

use super::Kind;
use delonix_meet_core::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    InApp,
    Email,
    Sms,
}

impl Channel {
    pub const ALL: [Channel; 3] = [Channel::Email, Channel::InApp, Channel::Sms];

    pub fn as_str(self) -> &'static str {
        match self {
            Channel::InApp => "in_app",
            Channel::Email => "email",
            Channel::Sms => "sms",
        }
    }

    pub fn parse(s: &str) -> Option<Channel> {
        Channel::ALL.into_iter().find(|c| c.as_str() == s)
    }
}

/// Estado de entrega de um canal nesta instalação.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    Available,
    NotConfigured,
}

impl Delivery {
    pub fn as_str(self) -> &'static str {
        match self {
            Delivery::Available => "available",
            Delivery::NotConfigured => "not_configured",
        }
    }
}

/// Hoje só o `in_app` entrega (ver o topo do módulo).
pub fn delivery(channel: Channel) -> Delivery {
    match channel {
        Channel::InApp => Delivery::Available,
        Channel::Email | Channel::Sms => Delivery::NotConfigured,
    }
}

/// Omissão de um par (tipo, canal) que a pessoa nunca mexeu: `in_app` ligado
/// para tudo (é o comportamento de hoje), correio ligado para convites,
/// cancelamentos e «a começar», SMS desligado (custa dinheiro).
pub fn default_enabled(kind: Kind, channel: Channel) -> bool {
    match channel {
        Channel::InApp => true,
        Channel::Email => matches!(
            kind,
            Kind::MeetingInvited
                | Kind::MeetingCancelled
                | Kind::MeetingStarting
                | Kind::RecordingReady
                | Kind::TranscriptionReady
        ),
        Channel::Sms => false,
    }
}

pub fn parse_pair(kind: &str, channel: &str) -> Result<(Kind, Channel), DomainError> {
    let k = Kind::parse(kind).ok_or_else(|| {
        DomainError::invalid(
            "notification_preferences.unknown_kind",
            format!("tipo de notificação desconhecido: {kind}"),
        )
        .with_field("kind", Kind::ALL.map(|k| k.as_str()).join(" | "))
    })?;
    let c = Channel::parse(channel).ok_or_else(|| {
        DomainError::invalid(
            "notification_preferences.unknown_channel",
            format!("canal desconhecido: {channel}"),
        )
        .with_field("channel", "email | in_app | sms")
    })?;
    Ok((k, c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_app_is_the_only_delivering_channel_and_on_by_default() {
        for k in Kind::ALL {
            assert!(default_enabled(k, Channel::InApp));
            assert!(!default_enabled(k, Channel::Sms));
        }
        assert_eq!(delivery(Channel::InApp), Delivery::Available);
        assert_eq!(delivery(Channel::Email).as_str(), "not_configured");
        assert_eq!(delivery(Channel::Sms), Delivery::NotConfigured);
    }

    #[test]
    fn pairs_are_parsed_against_closed_catalogues() {
        assert_eq!(
            parse_pair("recording.ready", "email").unwrap(),
            (Kind::RecordingReady, Channel::Email)
        );
        assert_eq!(
            parse_pair("meeting.created", "email").unwrap_err().code,
            "notification_preferences.unknown_kind"
        );
        assert_eq!(
            parse_pair("call.missed", "push").unwrap_err().code,
            "notification_preferences.unknown_channel"
        );
    }
}
