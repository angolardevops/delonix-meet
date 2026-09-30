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
//! - a **normalização** e o **mascaramento** do número (o número é dado
//!   pessoal: quem não é anfitrião nunca o vê inteiro);
//! - a política de **marcação para fora** (emergência bloqueada, só E.164);
//! - a máquina de estados de uma **chamada de saída**;
//! - o **custo** de uma sessão, em unidades mínimas de moeda e sem conversões
//!   inventadas.

use delonix_meet_core::DomainError;
use serde::{Deserialize, Serialize};

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
//  Números
// ============================================================

/// Número normalizado em E.164 (`+` e 8 a 15 dígitos).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct E164(String);

impl E164 {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Dígitos sem o `+`.
    pub fn digits(&self) -> &str {
        &self.0[1..]
    }

    pub fn is_angolan(&self) -> bool {
        self.digits().starts_with("244")
    }
}

/// Normaliza o que a pessoa escreveu. Aceita `+244 923 000 000`,
/// `00244923000000`, `923000000` (nacional de 9 dígitos, assume Angola) e
/// números internacionais com `+`/`00`. Recusa códigos curtos: é por aqui que
/// os números de emergência nunca chegam a um tronco (ver [`check_dial_policy`]).
pub fn normalize_e164(input: &str) -> Result<E164, DomainError> {
    let compact: String = input
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '.' | '(' | ')' | '\u{a0}'))
        .collect();
    if let Some(code) = emergency_code(&compact) {
        return Err(emergency_error(code));
    }
    let digits = if let Some(rest) = compact.strip_prefix('+') {
        rest.to_string()
    } else if let Some(rest) = compact.strip_prefix("00") {
        rest.to_string()
    } else if compact.len() == 9 && compact.starts_with('9') {
        format!("244{compact}")
    } else {
        compact.clone()
    };
    let ok = (8..=15).contains(&digits.len())
        && digits.bytes().all(|b| b.is_ascii_digit())
        && !digits.starts_with('0');
    if !ok {
        return Err(DomainError::invalid(
            "channels.invalid_number",
            "número inválido: use o formato internacional, p.ex. +244 923 000 000",
        )
        .with_field("number", "E.164: + e 8 a 15 dígitos"));
    }
    Ok(E164(format!("+{digits}")))
}

/// Números de emergência e de serviço que NUNCA se marcam a partir de uma
/// sala: Angola (111 bombeiros/protecção civil, 112, 113 polícia, 115, 116),
/// e os internacionais mais comuns (911, 999, 000).
const EMERGENCY_CODES: [&str; 8] = ["111", "112", "113", "115", "116", "911", "999", "000"];

fn emergency_code(compact: &str) -> Option<&'static str> {
    let d = compact.trim_start_matches('+');
    let d = if d.len() > 3 {
        d.strip_prefix("00").unwrap_or(d)
    } else {
        d
    };
    let d = d.strip_prefix("244").unwrap_or(d);
    EMERGENCY_CODES.iter().copied().find(|c| d == *c)
}

fn emergency_error(code: &str) -> DomainError {
    DomainError::precondition(
        "channels.emergency_not_allowed",
        format!(
            "o {code} é um número de emergência: ligue-o directamente de um telefone, \
             nunca a partir de uma reunião"
        ),
    )
}

/// Política de marcação para fora a partir de uma sala.
///
/// **Emergência bloqueada** (decisão do ADR-0010 §5): uma chamada de emergência
/// feita por uma ponte de conferência chega ao centro de atendimento sem a
/// localização de quem precisa de ajuda, com o identificador do tronco da
/// empresa, e mete o operador de emergência dentro de uma reunião. Quem está
/// numa emergência liga do seu telefone. O plano de marcação da telefonia
/// (frente C, ADR-0009) continua a ser quem decide o resto (tarifas
/// especiais, destinos proibidos por operadora).
pub fn check_dial_policy(number: &E164) -> Result<(), DomainError> {
    if let Some(code) = emergency_code(number.as_str()) {
        return Err(emergency_error(code));
    }
    Ok(())
}

/// Códigos de país com 1 ou 2 dígitos (ITU-T E.164). Os restantes têm 3.
const CC_1: [&str; 2] = ["1", "7"];
const CC_2: [&str; 43] = [
    "20", "27", "30", "31", "32", "33", "34", "36", "39", "40", "41", "43", "44", "45", "46", "47",
    "48", "49", "51", "52", "53", "54", "55", "56", "57", "58", "60", "61", "62", "63", "64", "65",
    "66", "81", "82", "84", "86", "90", "91", "92", "93", "94", "95",
];

fn country_code_len(digits: &str) -> usize {
    if CC_1.iter().any(|c| digits.starts_with(c)) {
        1
    } else if CC_2.iter().any(|c| digits.starts_with(c)) {
        2
    } else {
        3
    }
}

/// Número mascarado para quem não é anfitrião: `+244 951 ***447`,
/// `+27 11 ***0192`. Fica o indicativo, os primeiros dígitos (que dizem a
/// rede, e é isso que o crachá mostra) e o fim (para reconhecer o número sem
/// o poder marcar).
pub fn mask_number(number: &E164) -> String {
    let digits = number.digits();
    let cc = country_code_len(digits);
    let national = &digits[cc..];
    let (head_len, tail_len) = if cc == 3 { (3, 3) } else { (2, 4) };
    if national.len() <= head_len + tail_len {
        let keep = national.len().saturating_sub(2);
        return format!("+{} ***{}", &digits[..cc], &national[keep..]);
    }
    format!(
        "+{} {} ***{}",
        &digits[..cc],
        &national[..head_len],
        &national[national.len() - tail_len..]
    )
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
        matches!(
            (self, to),
            (Queued, Dialing | Sent | Failed | Cancelled)
                | (Dialing, Ringing | InCall | Declined | NoAnswer | Failed | Cancelled)
                | (Ringing, InCall | Declined | NoAnswer | Failed | Cancelled)
                | (InCall, Ended | Failed)
        )
    }
}

/// Estado final a partir da causa de desligar (Q.850, como o FreeSWITCH a
/// reporta em `Hangup-Cause`), para uma chamada que NUNCA foi atendida. Uma
/// chamada atendida termina sempre em `Ended`, seja qual for a causa.
pub fn status_from_hangup_cause(cause: &str, answered: bool) -> DialOutStatus {
    if answered {
        return DialOutStatus::Ended;
    }
    match cause {
        "USER_BUSY" | "CALL_REJECTED" | "SUBSCRIBER_ABSENT" => DialOutStatus::Declined,
        "NO_ANSWER" | "NO_USER_RESPONSE" | "ALLOTTED_TIMEOUT" | "RECOVERY_ON_TIMER_EXPIRE" => {
            DialOutStatus::NoAnswer
        }
        "ORIGINATOR_CANCEL" => DialOutStatus::Cancelled,
        _ => DialOutStatus::Failed,
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
//  Custo
// ============================================================

/// Minutos facturáveis: cada minuto começado conta inteiro. É a mesma regra do
/// dial-in (`voice::estimate_cost`) — as operadoras angolanas cobram ao minuto.
pub fn billable_minutes(duration_secs: i64) -> i64 {
    (duration_secs.max(0) + 59) / 60
}

/// Custo em unidades mínimas da moeda (cêntimos de kwanza, cêntimos de dólar).
/// Inteiro de propósito: somar custos em `f64` perde cêntimos numa sessão
/// longa, e um relatório de custo que não bate com a factura não serve.
pub fn call_cost_minor(duration_secs: i64, rate_minor_per_min: i64) -> i64 {
    billable_minutes(duration_secs) * rate_minor_per_min.max(0)
}

/// Um item do custo da sessão.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostItem {
    pub kind: DialOutKind,
    pub currency: String,
    pub amount_minor: i64,
}

/// Total de uma moeda.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CurrencyTotal {
    pub currency: String,
    pub amount_minor: i64,
}

/// Resumo do custo de uma sessão.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionCost {
    /// Um total POR MOEDA. Não se converte: a taxa de câmbio não é nossa e um
    /// total «em Kz» que mistura dólares a uma taxa inventada é pior do que
    /// dois totais honestos.
    pub totals: Vec<CurrencyTotal>,
    /// Chamadas de voz (rede telefónica e WhatsApp).
    pub calls: u32,
    /// Convites por WhatsApp.
    pub whatsapp: u32,
    /// SMS enviados.
    pub sms: u32,
}

pub fn session_cost(items: &[CostItem]) -> SessionCost {
    let mut totals: Vec<CurrencyTotal> = Vec::new();
    let (mut calls, mut whatsapp, mut sms) = (0, 0, 0);
    for it in items {
        match it.kind {
            DialOutKind::Voice => calls += 1,
            DialOutKind::WhatsappVoice | DialOutKind::WhatsappInvite => whatsapp += 1,
            DialOutKind::SmsPin => sms += 1,
        }
        if it.currency.is_empty() {
            continue;
        }
        match totals.iter_mut().find(|t| t.currency == it.currency) {
            Some(t) => t.amount_minor += it.amount_minor,
            None => totals.push(CurrencyTotal {
                currency: it.currency.clone(),
                amount_minor: it.amount_minor,
            }),
        }
    }
    totals.sort_by(|a, b| a.currency.cmp(&b.currency));
    SessionCost {
        totals,
        calls,
        whatsapp,
        sms,
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

    #[test]
    fn normaliza_formatos_angolanos_e_internacionais() {
        assert_eq!(
            normalize_e164("923 000 000").unwrap().as_str(),
            "+244923000000"
        );
        assert_eq!(
            normalize_e164("+244 951-000-447").unwrap().as_str(),
            "+244951000447"
        );
        assert_eq!(
            normalize_e164("0027110000192").unwrap().as_str(),
            "+27110000192"
        );
        assert_eq!(
            normalize_e164("invalid.number").unwrap_err().code,
            "channels.invalid_number"
        );
        assert_eq!(
            normalize_e164("+12").unwrap_err().code,
            "channels.invalid_number"
        );
    }

    #[test]
    fn emergencia_e_recusada_em_qualquer_forma() {
        for n in ["112", "+244 113", "00244115", "911", "999", " 1 1 1 "] {
            let err = normalize_e164(n).unwrap_err();
            assert_eq!(err.code, "channels.emergency_not_allowed", "{n}");
        }
    }

    #[test]
    fn mascara_mostra_rede_e_fim_sem_o_numero_inteiro() {
        let n = normalize_e164("+244951000447").unwrap();
        assert_eq!(mask_number(&n), "+244 951 ***447");
        let n = normalize_e164("+27110000192").unwrap();
        assert_eq!(mask_number(&n), "+27 11 ***0192");
        let n = normalize_e164("+14155550100").unwrap();
        assert_eq!(mask_number(&n), "+1 41 ***0100");
        let m = mask_number(&normalize_e164("+244923123108").unwrap());
        assert!(!m.contains("923123108"), "{m}");
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
    fn causa_de_desligar() {
        use DialOutStatus::*;
        assert_eq!(status_from_hangup_cause("USER_BUSY", false), Declined);
        assert_eq!(status_from_hangup_cause("NO_ANSWER", false), NoAnswer);
        assert_eq!(
            status_from_hangup_cause("ORIGINATOR_CANCEL", false),
            Cancelled
        );
        assert_eq!(
            status_from_hangup_cause("UNALLOCATED_NUMBER", false),
            Failed
        );
        assert_eq!(status_from_hangup_cause("NORMAL_CLEARING", true), Ended);
        assert_eq!(status_from_hangup_cause("USER_BUSY", true), Ended);
    }

    #[test]
    fn custo_ao_minuto_comecado_e_por_moeda() {
        // 18 min a 9,40 Kz/min = 169,20 Kz (o exemplo do ecrã: «169 Kz»).
        assert_eq!(call_cost_minor(18 * 60, 940), 16_920);
        assert_eq!(call_cost_minor(61, 940), 1_880);
        assert_eq!(call_cost_minor(0, 940), 0);
        let cost = session_cost(&[
            CostItem {
                kind: DialOutKind::Voice,
                currency: "AOA".into(),
                amount_minor: 16_920,
            },
            CostItem {
                kind: DialOutKind::Voice,
                currency: "AOA".into(),
                amount_minor: 8_460,
            },
            CostItem {
                kind: DialOutKind::Voice,
                currency: "USD".into(),
                amount_minor: 48,
            },
            CostItem {
                kind: DialOutKind::WhatsappInvite,
                currency: String::new(),
                amount_minor: 0,
            },
            CostItem {
                kind: DialOutKind::SmsPin,
                currency: "AOA".into(),
                amount_minor: 1_200,
            },
        ]);
        assert_eq!(cost.calls, 3);
        assert_eq!(cost.whatsapp, 1);
        assert_eq!(cost.sms, 1);
        assert_eq!(
            cost.totals,
            vec![
                CurrencyTotal {
                    currency: "AOA".into(),
                    amount_minor: 26_580
                },
                CurrencyTotal {
                    currency: "USD".into(),
                    amount_minor: 48
                },
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
