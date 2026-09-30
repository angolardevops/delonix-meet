//! Portas da telefonia (ADR-0009 §4).
//!
//! Tudo o que depende de infraestrutura externa — o SBC Kamailio, o FreeSWITCH,
//! uma operadora — entra por uma destas portas. Cada uma tem um adaptador REAL
//! (no servidor) e um FALSO que só existe nos testes. Em produção, sem
//! configuração, o adaptador devolve [`PortError::NotConfigured`] e a API
//! responde com o estado honesto (`not_configured`) — nunca com números
//! inventados.
//!
//! | Porta | Adaptador real | Quem a usa |
//! |---|---|---|
//! | [`SipControl`] | FreeSWITCH ESL (`sofia xmlstatus`, `limit_usage`) + Kamailio JSON-RPC | estado do registo, troncos, «Reiniciar registo» |
//! | [`CallOriginator`] | FreeSWITCH ESL `bgapi originate` | «Ligar agora»; frente D: chamar alguém para a sala |
//! | [`CdrSource`] | `mod_json_cdr` → `POST /internal/v1/telephony/call-records` | registo de chamadas e custo |
//! | [`SmsGateways`] | fila do ADR-0005 (`sms::enqueue`) | «Enviar SMS»; frente D: SMS com PIN |

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use super::cost::RegistrationState;

/// Falha de uma porta, já classificada para a API.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PortError {
    /// A instalação não tem esta infraestrutura configurada (ex.: sem
    /// `TELEPHONY_ESL_ADDR`). A API diz `not_configured`.
    #[error("não configurado: {0}")]
    NotConfigured(String),
    /// Configurada, mas não respondeu (ligação recusada, tempo esgotado).
    #[error("indisponível: {0}")]
    Unavailable(String),
    /// Respondeu e recusou (ex.: `-ERR` do ESL, autenticação falhada).
    #[error("recusado: {0}")]
    Rejected(String),
    /// Respondeu algo que não se entende.
    #[error("protocolo: {0}")]
    Protocol(String),
}

// ============================================================
//  SipControl
// ============================================================

/// Um tronco no media server. `name` é o nome do gateway no FreeSWITCH
/// (`dlx-<trunk_id>`), nunca o nome que o cliente deu.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GatewayStatus {
    pub name: String,
    pub registration: RegistrationState,
    /// Resultado do último SIP OPTIONS (`ping`), se o gateway o faz.
    pub up: Option<bool>,
    pub ping_ms: Option<f64>,
    /// Canais em uso AGORA (contador `limit` do FreeSWITCH), se medido.
    pub channels_in_use: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MediaServerStatus {
    /// `FreeSWITCH`.
    pub software: String,
    pub version: Option<String>,
    pub uptime_secs: Option<u64>,
    pub sessions_active: Option<u32>,
    /// Codecs oferecidos na saída, pela ordem de preferência.
    pub codecs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SbcStatus {
    /// `Kamailio`.
    pub software: String,
    pub version: Option<String>,
    pub uptime_secs: Option<u64>,
}

/// Uma fotografia do plano de sinalização, tirada agora.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SipSnapshot {
    pub measured_at: DateTime<Utc>,
    /// `None`: o media server não respondeu (o erro vem em `media_error`).
    pub media: Option<MediaServerStatus>,
    pub media_error: Option<String>,
    /// `None`: sem SBC configurado, ou não respondeu (`sbc_error`).
    pub sbc: Option<SbcStatus>,
    pub sbc_error: Option<String>,
    /// Só os gateways pedidos; um pedido que o media server não conhece vem
    /// com `registration: unknown`.
    pub gateways: Vec<GatewayStatus>,
}

#[async_trait]
pub trait SipControl: Send + Sync {
    /// Estado do media server, do SBC e dos gateways `gateway_names`.
    async fn snapshot(&self, gateway_names: &[String]) -> Result<SipSnapshot, PortError>;
    /// Força um novo REGISTER dos gateways (FreeSWITCH `sofia profile … killgw`
    /// + `rescan`). Não espera pelo resultado: o estado lê-se no `snapshot`.
    async fn restart_registration(&self, gateway_names: &[String]) -> Result<(), PortError>;
}

// ============================================================
//  CallOriginator
// ============================================================

/// Uma tentativa por tronco, pela ordem (failover).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DialLeg {
    pub trunk_id: Uuid,
    /// `dlx-<trunk_id>`.
    pub gateway_name: String,
    /// O número como a operadora o quer (E.164 sem `+`, ou nacional).
    pub number: String,
}

/// O que acontece depois de a pessoa atender.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AfterAnswer {
    /// Teste rápido: um tom durante `secs` segundos e desliga.
    TestTone { secs: u32 },
    /// Entra na conferência LOCAL do FreeSWITCH com o nome da sala. Quem liga
    /// ouve os outros ao telefone, NÃO a reunião.
    Conference { room_code: String },
    /// Frente D: liga a chamada atendida à PONTE da sala (o lado SFU), e não à
    /// conferência local. O FreeSWITCH desta instalação não tem `mod_rtp`: não
    /// consegue mandar RTP cru para um endereço. A ponte é por isso um UA SIP
    /// mínimo em `bridge_host:bridge_port`; o FreeSWITCH envia-lhe um INVITE
    /// para `room-<room_code>` e o endereço RTP da ponte vem na resposta SDP.
    /// Se a ponte só souber falar RTP cru, é preciso recompilar com `mod_rtp`
    /// (ADR-0009 §EXTERNAL).
    RoomBridge {
        room_code: String,
        bridge_host: std::net::IpAddr,
        bridge_port: u16,
        /// Codec que a ponte aceita (`PCMA`, `OPUS`…); vazio = o do perfil.
        codec: Option<String>,
    },
}

/// Progresso de uma chamada originada. Cada chamada recebe, por ordem:
/// zero ou mais tentativas (`Dialing`, talvez `Ringing`, `AttemptFailed`), no
/// máximo um `Answered`, e EXACTAMENTE um `Ended` no fim.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CallEvent {
    /// O media server criou o canal para este tronco.
    Dialing { trunk_id: Option<Uuid> },
    /// A tocar (`180`) ou com media antecipada (`183`).
    Ringing {
        trunk_id: Option<Uuid>,
        early_media: bool,
    },
    /// Um tronco recusou (a seguir vem o próximo, ou `Ended`).
    AttemptFailed {
        trunk_id: Option<Uuid>,
        cause: String,
    },
    Answered {
        trunk_id: Option<Uuid>,
        latency_ms: u64,
    },
    /// Fim da chamada. `billsec` só quando foi atendida.
    Ended {
        answered: bool,
        cause: String,
        billsec: Option<i64>,
    },
}

/// Quem quer acompanhar a chamada. Chamado na ordem dos eventos; não deve
/// bloquear (despacha para uma fila se precisar de IO).
pub trait CallEventSink: Send + Sync {
    fn on_event(&self, call_id: Uuid, event: CallEvent);
}

/// Para quem só quer o resultado.
pub struct IgnoreEvents;

impl CallEventSink for IgnoreEvents {
    fn on_event(&self, _: Uuid, _: CallEvent) {}
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OriginateRequest {
    /// Id nosso da chamada; vai como `origination_uuid` e volta no CDR.
    pub call_id: Uuid,
    pub org_id: Uuid,
    pub legs: Vec<DialLeg>,
    /// Número apresentado; `None` = o do tronco.
    pub caller_id: Option<String>,
    pub record: bool,
    pub emergency: bool,
    /// Posição da regra do plano que decidiu (vai para o CDR).
    pub rule_position: Option<usize>,
    pub answer_timeout_secs: u32,
    pub after_answer: AfterAnswer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OriginateOutcome {
    pub answered: bool,
    /// Do pedido ao atendimento (inclui o toque).
    pub answer_latency_ms: Option<u64>,
    /// Causa Q.850 do FreeSWITCH quando não atendeu (`NO_ANSWER`, `USER_BUSY`…).
    pub hangup_cause: Option<String>,
}

#[async_trait]
pub trait CallOriginator: Send + Sync {
    /// Liga e espera pelo atendimento (ou pela recusa), até
    /// `answer_timeout_secs`. Os eventos (`events`) continuam a chegar depois
    /// de devolver, até ao `Ended`.
    async fn originate(
        &self,
        req: &OriginateRequest,
        events: std::sync::Arc<dyn CallEventSink>,
    ) -> Result<OriginateOutcome, PortError>;
    /// Desliga a chamada `call_id`, se ainda existir.
    async fn hangup(&self, call_id: Uuid) -> Result<(), PortError>;
}

// ============================================================
//  CdrSource
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Inbound,
    Outbound,
}

/// Como acabou a chamada, na linguagem do produto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallOutcome {
    Answered,
    NoAnswer,
    Busy,
    Failed,
    /// Dial-in: PIN errado até ao limite.
    WrongPin,
    /// Dial-in: ficou na sala de espera e não foi admitida.
    WaitingRoom,
    /// Reencaminhada para outro destino.
    Forwarded,
}

impl CallOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Answered => "answered",
            Self::NoAnswer => "no_answer",
            Self::Busy => "busy",
            Self::Failed => "failed",
            Self::WrongPin => "wrong_pin",
            Self::WaitingRoom => "waiting_room",
            Self::Forwarded => "forwarded",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "answered" => Self::Answered,
            "no_answer" => Self::NoAnswer,
            "busy" => Self::Busy,
            "failed" => Self::Failed,
            "wrong_pin" => Self::WrongPin,
            "waiting_room" => Self::WaitingRoom,
            "forwarded" => Self::Forwarded,
            _ => return None,
        })
    }
}

/// Um registo de chamada normalizado, independente de quem o produziu.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CallDetail {
    /// Id da chamada no media server (FreeSWITCH `uuid`) — a chave de
    /// idempotência da ingestão.
    pub source_call_id: String,
    /// `delonix_org_id` da chamada. `None` → a ingestão recusa (`422`).
    pub org_id: Option<Uuid>,
    /// `delonix_call_id` quando fomos nós a originar.
    pub call_id: Option<Uuid>,
    pub direction: Direction,
    pub from_number: String,
    pub to_number: String,
    /// `dlx-<trunk_id>` → `trunk_id`.
    pub trunk_id: Option<Uuid>,
    pub room_code: Option<String>,
    pub outcome: CallOutcome,
    pub hangup_cause: Option<String>,
    pub started_at: DateTime<Utc>,
    pub answered_at: Option<DateTime<Utc>>,
    pub ended_at: DateTime<Utc>,
    pub duration_secs: i64,
    pub billsec: i64,
    pub recorded: bool,
    pub emergency: bool,
    pub rule_position: Option<i32>,
    pub jitter_ms: Option<f64>,
    pub loss_pct: Option<f64>,
    pub mos: Option<f64>,
    /// Texto livre do destino (`convite por voz`, descrição da regra).
    pub destination_label: Option<String>,
}

/// Transforma o que o media server envia num [`CallDetail`]. Síncrono: a
/// entrega (HTTP) é do adaptador de entrada; esta porta só interpreta.
pub trait CdrSource: Send + Sync {
    fn parse(&self, payload: &[u8]) -> Result<CallDetail, PortError>;
}

// ============================================================
//  SmsGateways (ADR-0005)
// ============================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmsSendRequest {
    pub org_id: Uuid,
    /// Quem pede (auditoria). A autorização é de quem chama a porta.
    pub actor_id: Uuid,
    pub to: String,
    pub body: String,
    /// `auto` | `usb` | `operator`.
    pub route: String,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SmsQueued {
    pub message_id: Uuid,
    /// `queued` (sempre, na aceitação — o envio é assíncrono).
    pub status: String,
    /// `usb` | `operator`.
    pub route: String,
}

#[async_trait]
pub trait SmsGateways: Send + Sync {
    /// Põe na fila do ADR-0005 (limite por org, encaminhamento, auditoria).
    /// Recusas do encaminhamento chegam como [`PortError::Rejected`].
    async fn send(&self, req: &SmsSendRequest) -> Result<SmsQueued, PortError>;
}

/// Nome do gateway do FreeSWITCH para um tronco. Um só sítio, porque o
/// gerador da configuração, o `SipControl` e o parser de CDR têm de concordar.
pub fn gateway_name(trunk_id: Uuid) -> String {
    format!("dlx-{trunk_id}")
}

/// O inverso de [`gateway_name`].
pub fn trunk_id_from_gateway(name: &str) -> Option<Uuid> {
    name.strip_prefix("dlx-")
        .and_then(|s| Uuid::parse_str(s).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_name_round_trips() {
        let id = Uuid::new_v4();
        assert_eq!(trunk_id_from_gateway(&gateway_name(id)), Some(id));
        assert_eq!(trunk_id_from_gateway("unitel"), None);
        assert_eq!(trunk_id_from_gateway("dlx-nope"), None);
    }

    #[test]
    fn outcome_round_trips() {
        for o in [
            CallOutcome::Answered,
            CallOutcome::NoAnswer,
            CallOutcome::Busy,
            CallOutcome::Failed,
            CallOutcome::WrongPin,
            CallOutcome::WaitingRoom,
            CallOutcome::Forwarded,
        ] {
            assert_eq!(CallOutcome::parse(o.as_str()), Some(o));
        }
    }
}
