//! Canais na sala (ADR-0010): **como cada pessoa está ligada**.
//!
//! Uma sala do Delonix Meet deixa de ser só de quem tem a app. Entra-se também
//! por uma sala SIP (sistema de videoconferência de sala, câmara IP), por uma
//! chamada telefónica (Unitel, Africell, Movicel, internacional) e pelo
//! WhatsApp. O que está aqui são as regras que não dependem de rede nem de
//! base:
//!
//! - o **canal** e a **origem** de cada participante, e o que cada canal NÃO
//!   recebe (quem está fora da app ouve a mistura de áudio e é ouvido — não vê
//!   o ecrã partilhado, o quadro nem as legendas);
//! - quem se pode **convidar** por número (só E.164; emergência recusada). A
//!   normalização, a máscara, o dinheiro e o custo são do contexto
//!   `telephony` (ADR-0009) e chamam-se, não se copiam;
//! - a máquina de estados de uma **chamada de saída**;
//! - o **resumo do custo** de uma sessão, por moeda e sem conversões
//!   inventadas.

use delonix_meet_core::DomainError;
use serde::{Deserialize, Serialize};

use crate::telephony::cost::sum_by_currency;
use crate::telephony::money::Money;
use crate::telephony::ports::CallOutcome;

// ============================================================
//  Canal e origem
// ============================================================

/// Por onde a pessoa está ligada à sala.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// Browser ou app Delonix (WebRTC completo).
    #[default]
    App,
    /// Sistema de sala por SIP (sala de reuniões, codec de hardware).
    SipRoom,
    /// Câmara IP (RTSP/SIP) — só publica, não é uma pessoa.
    IpCamera,
    /// Chamada de voz pelo WhatsApp.
    Whatsapp,
    /// Chamada telefónica (rede móvel ou fixa).
    Phone,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::App => "app",
            Channel::SipRoom => "sip_room",
            Channel::IpCamera => "ip_camera",
            Channel::Whatsapp => "whatsapp",
            Channel::Phone => "phone",
        }
    }

    pub fn parse(s: &str) -> Option<Channel> {
        [
            Channel::App,
            Channel::SipRoom,
            Channel::IpCamera,
            Channel::Whatsapp,
            Channel::Phone,
        ]
        .into_iter()
        .find(|c| c.as_str() == s)
    }

    /// Está «fora da app»: conta no «M fora da app» do cabeçalho da sala.
    pub fn outside_app(self) -> bool {
        !matches!(self, Channel::App)
    }

    /// O que este canal recebe da sala. É a mesma tabela que o aviso «O que
    /// muda fora da app» mostra — a UI lê-a daqui, não a reescreve.
    pub fn receives(self) -> Receives {
        match self {
            Channel::App => Receives {
                audio_mix: false,
                video: true,
                screen_share: true,
                whiteboard: true,
                captions: true,
                chat: true,
            },
            // A sala SIP recebe vídeo (composição do SFU, quando existir) mas
            // não o quadro nem as legendas. Enquanto a composição não existe,
            // só áudio — é o que a ponte faz hoje (ADR-0010 §3).
            Channel::SipRoom | Channel::Phone | Channel::Whatsapp => Receives {
                audio_mix: true,
                video: false,
                screen_share: false,
                whiteboard: false,
                captions: false,
                chat: false,
            },
            Channel::IpCamera => Receives {
                audio_mix: false,
                video: false,
                screen_share: false,
                whiteboard: false,
                captions: false,
                chat: false,
            },
        }
    }

    /// O vídeo desta pessoa NÃO chega à sala (crachá «Vídeo indisponível»).
    pub fn video_unavailable(self) -> bool {
        matches!(self, Channel::Phone | Channel::Whatsapp)
    }
}

/// O que um canal recebe da sala.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Receives {
    /// Recebe a MISTURA de áudio (um só fluxo), não um fluxo por pessoa.
    pub audio_mix: bool,
    pub video: bool,
    pub screen_share: bool,
    pub whiteboard: bool,
    pub captions: bool,
    pub chat: bool,
}

/// Rede por onde a chamada telefónica sai/entra.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Carrier {
    Unitel,
    Africell,
    Movicel,
    /// Número angolano que não é móvel (fixo, serviços).
    National,
    International,
}

impl Carrier {
    pub fn as_str(self) -> &'static str {
        match self {
            Carrier::Unitel => "unitel",
            Carrier::Africell => "africell",
            Carrier::Movicel => "movicel",
            Carrier::National => "national",
            Carrier::International => "international",
        }
    }

    pub fn parse(s: &str) -> Option<Carrier> {
        [
            Carrier::Unitel,
            Carrier::Africell,
            Carrier::Movicel,
            Carrier::National,
            Carrier::International,
        ]
        .into_iter()
        .find(|c| c.as_str() == s)
    }
}

/// Canal pedido para acrescentar alguém à reunião por número.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DialOutKind {
    /// Chamada de voz pela rede telefónica (tem custo por minuto).
    Voice,
    /// SMS com um PIN de entrada por telefone (custo por mensagem).
    SmsPin,
    /// Convite pelo WhatsApp com o link da sala (sem custo por minuto).
    WhatsappInvite,
    /// Chamada de voz pelo WhatsApp (só se a conta Business a tiver).
    WhatsappVoice,
}

impl DialOutKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DialOutKind::Voice => "voice",
            DialOutKind::SmsPin => "sms_pin",
            DialOutKind::WhatsappInvite => "whatsapp_invite",
            DialOutKind::WhatsappVoice => "whatsapp_voice",
        }
    }

    pub fn parse(s: &str) -> Option<DialOutKind> {
        [
            DialOutKind::Voice,
            DialOutKind::SmsPin,
            DialOutKind::WhatsappInvite,
            DialOutKind::WhatsappVoice,
        ]
        .into_iter()
        .find(|c| c.as_str() == s)
    }

    /// Canal com que a pessoa aparece na sala se atender.
    pub fn channel(self) -> Channel {
        match self {
            DialOutKind::Voice | DialOutKind::SmsPin => Channel::Phone,
            DialOutKind::WhatsappInvite | DialOutKind::WhatsappVoice => Channel::Whatsapp,
        }
    }

    /// Há uma chamada de media a seguir (estado em tempo real até desligar).
    /// Os convites (SMS, WhatsApp) terminam no envio.
    pub fn is_call(self) -> bool {
        matches!(self, DialOutKind::Voice | DialOutKind::WhatsappVoice)
    }
}

// ============================================================
//  Números — a normalização e a máscara são da telefonia
// ============================================================

/// Indicativo da instalação e comprimento do número nacional. Um só sítio até a
/// telefonia (ADR-0009) os tornar configuração.
pub const HOME_COUNTRY_CODE: &str = "244";
pub const HOME_NATIONAL_LEN: usize = 9;

/// Um número que se pode convidar para uma sala: normalizado pela telefonia
/// (`telephony::number::parse_dialed`) e COM forma E.164. Códigos curtos
/// (`112`, `84209`) não têm E.164 e não se convidam — nem por voz, nem por SMS,
/// nem por WhatsApp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invitee {
    /// Dígitos como o plano de marcação os lê (nacional ou `00…`).
    pub digits: String,
    /// `+244923000000`.
    pub e164: String,
}

pub fn parse_invitee(input: &str, emergency_numbers: &[String]) -> Result<Invitee, DomainError> {
    let dialed =
        crate::telephony::number::parse_dialed(input, HOME_COUNTRY_CODE, HOME_NATIONAL_LEN)?;
    // Emergência primeiro: o `112` não tem E.164 e cairia no erro genérico,
    // e quem o escreveu merece a razão verdadeira.
    if emergency_numbers.iter().any(|n| *n == dialed.digits) {
        return Err(DomainError::precondition(
            "telephony.emergency_not_invitable",
            "um número de emergência não se chama para dentro de uma reunião: \
             quem precisa de ajuda liga-o directamente",
        )
        .with_field("number", "número de emergência"));
    }
    match dialed.e164 {
        Some(e164) => Ok(Invitee {
            digits: dialed.digits,
            e164,
        }),
        None => Err(DomainError::invalid(
            "channels.number_not_invitable",
            "só se convida um número completo, com indicativo (p.ex. +244 923 000 000)",
        )
        .with_field("number", "E.164")),
    }
}

/// Máscara para quem não é anfitrião: `+244 951 ***447`. É a da telefonia.
pub fn mask(e164: &str) -> String {
    crate::telephony::number::mask(e164, HOME_COUNTRY_CODE)
}

// ============================================================
//  Chamada de saída — máquina de estados
// ============================================================

/// Estado de um pedido de «adicionar à reunião por número».
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DialOutStatus {
    /// Aceite, à espera de ser posto na rede.
    Queued,
    /// Posto na rede, ainda sem toque.
    Dialing,
    /// O telefone do destino está a tocar.
    Ringing,
    /// Atendeu: a pessoa está na sala.
    InCall,
    /// Convite (SMS/WhatsApp) entregue ao fornecedor. Estado final.
    Sent,
    /// Desligou depois de estar em chamada. Estado final.
    Ended,
    /// O destino recusou (ocupado ou rejeitou). Estado final.
    Declined,
    /// Tocou e ninguém atendeu. Estado final.
    NoAnswer,
    /// Falhou (rede, número inexistente, fornecedor). Estado final.
    Failed,
    /// O anfitrião cancelou antes de atender. Estado final.
    Cancelled,
}

impl DialOutStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            DialOutStatus::Queued => "queued",
            DialOutStatus::Dialing => "dialing",
            DialOutStatus::Ringing => "ringing",
            DialOutStatus::InCall => "in_call",
            DialOutStatus::Sent => "sent",
            DialOutStatus::Ended => "ended",
            DialOutStatus::Declined => "declined",
            DialOutStatus::NoAnswer => "no_answer",
            DialOutStatus::Failed => "failed",
            DialOutStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<DialOutStatus> {
        use DialOutStatus::*;
        [
            Queued, Dialing, Ringing, InCall, Sent, Ended, Declined, NoAnswer, Failed, Cancelled,
        ]
        .into_iter()
        .find(|c| c.as_str() == s)
    }

    pub fn is_final(self) -> bool {
        use DialOutStatus::*;
        matches!(
            self,
            Sent | Ended | Declined | NoAnswer | Failed | Cancelled
        )
    }

    /// Ainda se pode cancelar (ninguém atendeu).
    pub fn cancellable(self) -> bool {
        use DialOutStatus::*;
        matches!(self, Queued | Dialing | Ringing)
    }

    /// Transição permitida. Os eventos da rede chegam fora de ordem e
    /// repetidos (um `PROGRESS` depois do `ANSWER`, dois `HANGUP`): uma
    /// transição que não está aqui é IGNORADA, nunca aplicada — é isso que
    /// impede uma chamada terminada de «voltar a tocar» no ecrã.
    pub fn can_go(self, to: DialOutStatus) -> bool {
        use DialOutStatus::*;
        if self.is_final() {
            return false;
        }
        match (self, to) {
            (Queued, Dialing | Sent | Failed | Cancelled) => true,
            (Dialing, Ringing | InCall | Declined | NoAnswer | Failed | Cancelled) => true,
            (Ringing, InCall | Declined | NoAnswer | Failed | Cancelled) => true,
            (InCall, Ended | Failed) => true,
            _ => false,
        }
    }
}

/// Estado final de uma chamada de saída a partir do resultado que a telefonia
/// reporta (`OriginateOutcome`/`CallDetail`). Uma chamada atendida termina
/// sempre em `Ended`.
pub fn status_from_outcome(outcome: CallOutcome) -> DialOutStatus {
    match outcome {
        CallOutcome::Answered | CallOutcome::Forwarded => DialOutStatus::Ended,
        CallOutcome::Busy => DialOutStatus::Declined,
        CallOutcome::NoAnswer => DialOutStatus::NoAnswer,
        CallOutcome::Failed | CallOutcome::WrongPin | CallOutcome::WaitingRoom => {
            DialOutStatus::Failed
        }
    }
}

/// Causa Q.850 (como o FreeSWITCH a dá em `hangup_cause`) → resultado, para
/// quando a porta devolve `answered: false` só com a causa.
pub fn outcome_from_hangup_cause(cause: Option<&str>) -> CallOutcome {
    match cause.unwrap_or("") {
        "USER_BUSY" | "CALL_REJECTED" | "SUBSCRIBER_ABSENT" => CallOutcome::Busy,
        "NO_ANSWER"
        | "NO_USER_RESPONSE"
        | "ALLOTTED_TIMEOUT"
        | "RECOVERY_ON_TIMER_EXPIRE"
        | "ORIGINATOR_CANCEL" => CallOutcome::NoAnswer,
        _ => CallOutcome::Failed,
    }
}

// ============================================================
//  PIN de entrada por telefone (SMS)
// ============================================================

/// Dígitos do PIN de uso único. Seis, como o PIN da sala de voz: o IVR do
/// FreeSWITCH (`dialin_ivr.lua`) lê seis dígitos, e os dois caminhos passam
/// pelo mesmo travão de tentativas por número de acesso.
pub const ONE_TIME_PIN_DIGITS: usize = 6;
/// Validade por omissão do PIN enviado por SMS.
pub const ONE_TIME_PIN_TTL_SECS: i64 = 15 * 60;
/// Validade máxima que o anfitrião pode pedir.
pub const ONE_TIME_PIN_MAX_TTL_SECS: i64 = 24 * 60 * 60;

/// Validade pedida, dentro dos limites.
pub fn pin_ttl(requested_secs: Option<i64>) -> Result<i64, DomainError> {
    match requested_secs {
        None => Ok(ONE_TIME_PIN_TTL_SECS),
        Some(s) if (60..=ONE_TIME_PIN_MAX_TTL_SECS).contains(&s) => Ok(s),
        Some(_) => Err(DomainError::invalid(
            "channels.invalid_pin_ttl",
            format!(
                "a validade do PIN tem de estar entre 60 e {ONE_TIME_PIN_MAX_TTL_SECS} segundos"
            ),
        )
        .with_field("pin_ttl_secs", "60–86400")),
    }
}

/// Texto do SMS com o PIN. Não leva o nome da reunião: um SMS pode ser lido
/// no ecrã bloqueado por quem estiver ao lado, e o título de uma reunião
/// («Despedimentos Q4») é informação da empresa.
pub fn sms_pin_text(dial_in_number: &str, pin: &str, ttl_secs: i64) -> String {
    let minutes = (ttl_secs + 59) / 60;
    format!(
        "Delonix Meet: foi convidado para uma reuniao. Ligue {dial_in_number} e marque o PIN {pin}. \
         Valido {minutes} min, uma so vez. A reuniao pode ser gravada."
    )
}

// ============================================================
//  Custo desta sessão
// ============================================================

/// Um item do custo da sessão. O valor vem da telefonia
/// (`telephony::cost::call_cost` sobre o preço em vigor); aqui só se conta e
/// soma por moeda.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostItem {
    pub kind: DialOutKind,
    /// `None`: sem custo (convite WhatsApp) ou sem preço conhecido.
    pub cost: Option<Money>,
}

/// Resumo do custo de uma sessão.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCost {
    /// Um total POR MOEDA (`sum_by_currency`). Não se converte: a taxa de
    /// câmbio não é nossa, e um total «em Kz» que mistura dólares a uma taxa
    /// inventada é pior do que dois totais honestos.
    pub totals: Vec<Money>,
    /// Chamadas de voz pela rede telefónica.
    pub calls: u32,
    /// Convites e chamadas pelo WhatsApp.
    pub whatsapp: u32,
    /// SMS com PIN.
    pub sms: u32,
    /// Itens com custo desconhecido (sem preço configurado): o total é um
    /// MÍNIMO, e a UI tem de o dizer.
    pub unpriced: u32,
}

pub fn session_cost(items: &[CostItem]) -> SessionCost {
    let (mut calls, mut whatsapp, mut sms, mut unpriced) = (0, 0, 0, 0);
    for it in items {
        match it.kind {
            DialOutKind::Voice => calls += 1,
            DialOutKind::WhatsappVoice | DialOutKind::WhatsappInvite => whatsapp += 1,
            DialOutKind::SmsPin => sms += 1,
        }
        if it.cost.is_none() && !matches!(it.kind, DialOutKind::WhatsappInvite) {
            unpriced += 1;
        }
    }
    SessionCost {
        totals: sum_by_currency(items.iter().filter_map(|i| i.cost)),
        calls,
        whatsapp,
        sms,
        unpriced,
    }
}

// ============================================================
//  Qualidade da ligação («Ligação fraca»)
// ============================================================

/// Jitter acima do qual a ligação é fraca (ms). G.114/G.107: acima de ~30 ms
/// o buffer de jitter começa a acrescentar atraso audível.
pub const WEAK_JITTER_MS: f64 = 30.0;
/// Perda acima da qual a ligação é fraca (fracção). A 3 % o G.711 sem PLC já
/// se ouve a cortar.
pub const WEAK_LOSS: f64 = 0.03;

/// A ligação desta pessoa está fraca? Com histerese, para o crachá não piscar:
/// entra acima dos limites, só sai abaixo de metade deles.
pub fn weak_link(currently_weak: bool, jitter_ms: f64, loss: f64) -> bool {
    if currently_weak {
        jitter_ms > WEAK_JITTER_MS / 2.0 || loss > WEAK_LOSS / 2.0
    } else {
        jitter_ms > WEAK_JITTER_MS || loss > WEAK_LOSS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emergencia() -> Vec<String> {
        crate::telephony::dial_plan::parse_emergency_numbers("112,113,115")
    }

    #[test]
    fn convidado_tem_de_ter_e164() {
        let i = parse_invitee("923 000 000", &emergencia()).unwrap();
        assert_eq!(i.e164, "+244923000000");
        let i = parse_invitee("+27 11 555 0192", &emergencia()).unwrap();
        assert_eq!(i.e164, "+27115550192");
        assert_eq!(
            parse_invitee("84209", &emergencia()).unwrap_err().code,
            "channels.number_not_invitable"
        );
        assert_eq!(
            parse_invitee("abc", &emergencia()).unwrap_err().code,
            "telephony.invalid_number"
        );
    }

    #[test]
    fn emergencia_nao_se_convida() {
        for n in ["112", "113", " 115 "] {
            assert_eq!(
                parse_invitee(n, &emergencia()).unwrap_err().code,
                "telephony.emergency_not_invitable",
                "{n}"
            );
        }
    }

    #[test]
    fn mascara_e_a_da_telefonia() {
        assert_eq!(mask("+244951000447"), "+244 951 ***447");
        assert!(!mask("+244923123108").contains("123108"));
    }

    #[test]
    fn estados_finais_nao_voltam_atras() {
        use DialOutStatus::*;
        assert!(Dialing.can_go(Ringing));
        assert!(Ringing.can_go(InCall));
        assert!(InCall.can_go(Ended));
        assert!(!Ended.can_go(Ringing), "um PROGRESS atrasado não reabre");
        assert!(!InCall.can_go(Ringing));
        assert!(!Cancelled.can_go(InCall));
        assert!(
            !InCall.can_go(Cancelled),
            "atendida não se cancela, desliga-se"
        );
        assert!(Queued.can_go(Sent));
        assert!(!Sent.can_go(Failed));
    }

    #[test]
    fn resultado_da_chamada() {
        use DialOutStatus::*;
        assert_eq!(status_from_outcome(CallOutcome::Busy), Declined);
        assert_eq!(status_from_outcome(CallOutcome::NoAnswer), NoAnswer);
        assert_eq!(status_from_outcome(CallOutcome::Failed), Failed);
        assert_eq!(status_from_outcome(CallOutcome::Answered), Ended);
        assert_eq!(
            outcome_from_hangup_cause(Some("USER_BUSY")),
            CallOutcome::Busy
        );
        assert_eq!(
            outcome_from_hangup_cause(Some("NO_ANSWER")),
            CallOutcome::NoAnswer
        );
        assert_eq!(
            outcome_from_hangup_cause(Some("UNALLOCATED_NUMBER")),
            CallOutcome::Failed
        );
        assert_eq!(outcome_from_hangup_cause(None), CallOutcome::Failed);
    }

    #[test]
    fn custo_da_sessao_por_moeda_e_contagens() {
        use crate::telephony::cost::call_cost;
        use crate::telephony::money::Currency;
        // 18 min a 9,40 Kz/min = 169,20 Kz (o exemplo do ecrã: «169 Kz»).
        let unitel = call_cost(18 * 60, Money::new(94_000, Currency::Aoa));
        assert_eq!(unitel.amount_e4, 1_692_000);
        let cost = session_cost(&[
            CostItem {
                kind: DialOutKind::Voice,
                cost: Some(unitel),
            },
            CostItem {
                kind: DialOutKind::Voice,
                cost: Some(Money::new(846_000, Currency::Aoa)),
            },
            CostItem {
                kind: DialOutKind::Voice,
                cost: Some(Money::new(4_000, Currency::Usd)),
            },
            CostItem {
                kind: DialOutKind::WhatsappInvite,
                cost: None,
            },
            CostItem {
                kind: DialOutKind::SmsPin,
                cost: None,
            },
        ]);
        assert_eq!(
            (cost.calls, cost.whatsapp, cost.sms, cost.unpriced),
            (3, 1, 1, 1)
        );
        assert_eq!(
            cost.totals,
            vec![
                Money::new(2_538_000, Currency::Aoa),
                Money::new(4_000, Currency::Usd)
            ]
        );
    }

    #[test]
    fn fora_da_app_nao_recebe_ecra_quadro_nem_legendas() {
        for c in [Channel::Phone, Channel::Whatsapp, Channel::SipRoom] {
            let r = c.receives();
            assert!(
                r.audio_mix && !r.screen_share && !r.whiteboard && !r.captions,
                "{c:?}"
            );
            assert!(c.outside_app());
        }
        assert!(!Channel::App.outside_app());
        assert!(Channel::Phone.video_unavailable());
        assert!(!Channel::SipRoom.video_unavailable());
    }

    #[test]
    fn ligacao_fraca_com_histerese() {
        assert!(!weak_link(false, 20.0, 0.01));
        assert!(weak_link(false, 45.0, 0.0));
        assert!(weak_link(false, 5.0, 0.05));
        // Já fraca: 20 ms ainda é fraca (acima de metade), 10 ms não.
        assert!(weak_link(true, 20.0, 0.0));
        assert!(!weak_link(true, 10.0, 0.01));
    }

    #[test]
    fn ttl_do_pin() {
        assert_eq!(pin_ttl(None).unwrap(), 900);
        assert_eq!(pin_ttl(Some(3600)).unwrap(), 3600);
        assert_eq!(
            pin_ttl(Some(10)).unwrap_err().code,
            "channels.invalid_pin_ttl"
        );
    }

    #[test]
    fn sms_nao_leva_o_titulo_da_reuniao() {
        let t = sms_pin_text("+244222000100", "123456", 900);
        assert!(t.contains("+244222000100") && t.contains("123456") && t.contains("15 min"));
        assert!(t.is_ascii(), "GSM-7 sem acentos: um segmento, não três");
    }
}
