# Contrato interno da telefonia (frente C → frente D)

> **Resgatado a 2026-09-30 de uma pasta temporária, onde esteve treze dias.** É o
> contrato de interface que a frente C escreveu para a frente D, datado de
> **2026-09-17** e provado contra um FreeSWITCH 1.11.3 real. Entra no repositório
> porque a frente C **ainda não está portada** (9 772 linhas em
> `origin/delonix-meet-backend/v3-telecom` e `…/v3-canais`), e este documento é o
> mapa para a portar — ver a skill `delonix-meet-telefonia`.
>
> **O que mudou desde que foi escrito, e o texto abaixo não sabe:**
>
> | Onde | O que dizia | O que é verdade a 2026-09-30 |
> |---|---|---|
> | §4.4 | «A media não passa. A ponte FreeSWITCH↔SFU é EXTERNAL» | **A ponte existe** desde o #130: quem entra por telefone é participante da sala. Ver [ADR-0010](adr/0010-ponte-telefone-sala.md), R221/R222 |
> | §2, `AfterAnswer::RoomBridge` | descrevia o que era preciso construir | está construído — o `voice::room_bridge_for` devolve `sip_uri`/`channel_vars` e o `dialin_ivr.lua` faz o `bridge` |
> | §3 | «`#[allow(dead_code)]` à espera da D» | continua verdade **na branch**; na `main` estes símbolos não existem |
>
> A observação do §2 — «esta imagem não tem `mod_rtp`, por isso a ponte tem de ser um
> **UA SIP mínimo**» — é a medição que sustenta o ADR-0010. Estava certa, e é a razão
> pela qual a Abordagem B foi abandonada.

---


Autor: frente C (`delonix-meet-backend/v3-telecom`, worktree `.worktrees/delonix-meet/v3-telecom`).
Autoridade: ADR-0009 «Telefonia: troncos, encaminhamento e custo» (nessa branch).
Estado (2026-09-17, branch rebaseada sobre `origin/main` `03fecb9`): tudo abaixo está
commitado e **provado contra o FreeSWITCH 1.11.3 real** (`delonix-dev/freeswitch:1.11.3`) —
ver o §6. Commits: `git log origin/main..delonix-meet-backend/v3-telecom`.

A frente D (canais na sala) NÃO fala com o FreeSWITCH nem com o gateway de SMS
directamente: consome as portas abaixo e as funções de serviço do §3. Se precisares de
algo que não está aqui, pede — não abras um segundo caminho para a mesma
infraestrutura.

## 1. Onde está o código

| O quê | Caminho |
|---|---|
| Regras puras (sem IO) | `server/crates/delonix-meet-domain/src/telephony/` |
| Normalizar/mascarar números | `telephony::number::{parse_dialed, mask}` |
| Plano de marcação | `telephony::dial_plan::{Pattern, DialRule, validate_plan, resolve, Resolution, ResolutionOutcome, TrunkRef}` |
| Dinheiro | `telephony::money::{Money, Currency, parse_amount_e4, format_e4, to_aoa}` (décimas-milésimas, nunca `f64`) |
| Custo e saúde | `telephony::cost::{price_at, call_cost, billed_minutes, asr, trunk_health}` |
| Portas | `telephony::ports::{SipControl, CallOriginator, CdrSource, SmsGateways, PortError, …}` |
| Adaptadores + serviço (servidor) | `server/src/telephony_*.rs` |

## 2. As portas (traits)

```rust
#[async_trait]
pub trait SipControl: Send + Sync {
    async fn snapshot(&self, gateway_names: &[String]) -> Result<SipSnapshot, PortError>;
    async fn restart_registration(&self, gateway_names: &[String]) -> Result<(), PortError>;
}

#[async_trait]
pub trait CallOriginator: Send + Sync {
    /// Liga e espera pelo atendimento ou pela recusa (até answer_timeout_secs).
    /// Os eventos continuam a chegar a `events` depois de devolver, até ao Ended.
    async fn originate(&self, req: &OriginateRequest, events: Arc<dyn CallEventSink>)
        -> Result<OriginateOutcome, PortError>;
    /// Desliga TODAS as pernas da chamada (hupall pela variável delonix_call_id).
    async fn hangup(&self, call_id: Uuid) -> Result<(), PortError>;
}

/// (b) pedido da D: progresso da chamada.
pub trait CallEventSink: Send + Sync {
    fn on_event(&self, call_id: Uuid, event: CallEvent);   // não bloquear
}
pub struct IgnoreEvents;   // para quem só quer o resultado

pub enum CallEvent {
    Dialing { trunk_id: Option<Uuid> },                         // canal criado para um tronco
    Ringing { trunk_id: Option<Uuid>, early_media: bool },      // 180 / 183
    AttemptFailed { trunk_id: Option<Uuid>, cause: String },    // um tronco recusou; segue o próximo
    Answered { trunk_id: Option<Uuid>, latency_ms: u64 },       // no máximo um
    Ended { answered: bool, cause: String, billsec: Option<i64> }, // EXACTAMENTE um, sempre o último
}

pub trait CdrSource: Send + Sync {
    fn parse(&self, payload: &[u8]) -> Result<CallDetail, PortError>;
}

#[async_trait]
pub trait SmsGateways: Send + Sync {
    async fn send(&self, req: &SmsSendRequest) -> Result<SmsQueued, PortError>;
}

pub enum PortError { NotConfigured(String), Unavailable(String), Rejected(String), Protocol(String) }
```

Tipos principais:

- `OriginateRequest { call_id, org_id, legs: Vec<DialLeg>, caller_id, record, emergency, rule_position, answer_timeout_secs, after_answer: AfterAnswer }`
- `AfterAnswer`:
  - `TestTone { secs }` — teste rápido;
  - `Conference { room_code }` — conferência LOCAL do FreeSWITCH (quem liga não ouve a reunião);
  - **(a) pedido da D:** `RoomBridge { room_code, bridge_host: IpAddr, bridge_port: u16, codec: Option<String> }`
    — depois de atender, o FreeSWITCH faz `bridge` para `sofia/<perfil>/room-<room_code>@bridge_host:bridge_port`.
    **Não é RTP cru:** esta imagem não tem `mod_rtp`, por isso a ponte tem de ser um **UA SIP mínimo**
    que atende o INVITE `room-<sala>` e dá o seu endereço RTP na resposta SDP (codec pedido em
    `codec`, p.ex. `PCMA`). Se a ponte só puder falar RTP cru, é preciso recompilar o FreeSWITCH com
    `mod_rtp` e acrescentar outra variante — pede.
- `DialLeg { trunk_id, gateway_name /* dlx-<trunk_id> */, number }`
- `OriginateOutcome { answered, answer_latency_ms, hangup_cause }`
- `CallDetail { … }`, `CallOutcome = answered | no_answer | busy | failed | wrong_pin | waiting_room | forwarded`
- `SmsSendRequest { org_id, actor_id, to, body, route /* auto|usb|operator */, idempotency_key }` → `SmsQueued { message_id, status, route }`
- `gateway_name(trunk_id)` / `trunk_id_from_gateway(name)` — o ÚNICO sítio que dá nome aos gateways.

## 3. Funções de serviço que a frente D chama (em `server/src/telephony_service.rs`)

Todas recebem o `org_id` já autorizado por quem chama (a D decide quem na sala pode
convidar por telefone; a C não repete essa regra).

```rust
/// Número → regra, troncos, gravação, emergência. Não liga.
pub(crate) async fn resolve_number(state: &AppState, org_id: Uuid, number: &str)
    -> Result<ResolvedNumber, ApiError>;
// ResolvedNumber { dialed: DialedNumber, resolution: Resolution,
//                  trunks: Vec<TrunkSummary>, price_per_min: Option<Money> }

/// Liga para fora pelo plano de marcação (rejeita block/no_match/no_available_trunk/interno),
/// aplica o limite de canais medido, grava a tentativa e devolve já (202) — o resultado
/// chega à linha `telephony_outbound_calls` e ao CDR.
pub(crate) async fn place_call(state: &Arc<AppState>, org_id: Uuid, actor_id: Uuid,
    number: &str, after_answer: AfterAnswer, purpose: CallPurpose,
    listener: Option<Arc<dyn CallEventSink>>)            // (b): recebe os mesmos eventos que a linha
    -> Result<OutboundCall, ApiError>;
// CallPurpose::QuickTest | CallPurpose::RoomInvite { room_code }
// OutboundCall { id, purpose, room_code, to_masked,
//                status: dialing → ringing → answered (fica) | no_answer | busy | failed,
//                trunk_ids, rule_position, record, emergency, answer_latency_ms, hangup_cause, error,
//                created_at, answered_at, billsec, finished_at /* null enquanto decorre */ }
// Estado da chamada: linha `telephony_outbound_calls` (a tarefa de fundo actualiza-a).

/// Preço por minuto em vigor AGORA no primeiro tronco da resolução. `None` sem preço.
pub(crate) async fn estimate_price_per_min(state: &AppState, org_id: Uuid, number: &str)
    -> Result<Option<Money>, ApiError>;

/// Custo acumulado das chamadas de uma sala (para «Custo desta sessão»),
/// somado por moeda a partir dos CDRs ingeridos, com o preço em vigor em cada chamada.
pub(crate) async fn room_call_costs(state: &AppState, org_id: Uuid, room_code: &str,
    since: DateTime<Utc>) -> Result<Vec<Money>, ApiError>;

/// (c) SMS pela fila do ADR-0005 (mesma regra que POST /sms/messages). DECIDIDO: a extracção é
/// da frente C e já está feita — `sms::enqueue_message` saiu de `send_message` (as duas rotas e
/// esta função chamam a mesma regra), e a porta `SmsGateways` é implementada por
/// `telephony_service::QueueSmsGateways`. A D chama `send_sms`; não volta a extrair nada.
pub(crate) async fn send_sms(state: &AppState, req: SmsSendRequest) -> Result<SmsQueued, ApiError>;
```

`estimate_price_per_min`, `room_call_costs`, `send_sms`, `CallPurpose::RoomInvite` e
`QueueSmsGateways` estão marcados `#[allow(dead_code)]` à espera da D: ao usá-los, tira o `allow`.
`RoomInvite` já tem a recusa de emergência implementada; o `purpose` grava `room_invite` e o
`room_code`. Os CDRs só levam `room_code` se o FreeSWITCH puser `delonix_room_code` (o
`originate` com `AfterAnswer::Conference` já o põe).

## 4. Regras que a D herda e não pode contornar

1. **Emergência** (`112,113,115` por omissão, `TELEPHONY_EMERGENCY_NUMBERS`): nunca gravada,
   nunca bloqueada. `place_call` com `CallPurpose::RoomInvite` para um número de emergência
   é **recusado** (`422 telephony.emergency_not_invitable`) — não se chama os bombeiros
   para dentro de uma reunião; o invariante é sobre quem MARCA, não sobre quem convida.
2. **Custo**: um convite por telefone custa dinheiro. `place_call` passa pelo limite por org
   (`telephony_call_limiter`) e escreve auditoria `telephony.call.placed`.
3. **Não simular**: sem `TELEPHONY_ESL_ADDR`, `place_call` devolve
   `422 telephony.not_configured`. O falso (`FakeOriginator`) só existe em `#[cfg(test)]`/`tests/`.
4. **A media não passa.** Uma chamada `AfterAnswer::Conference` entra na conferência do
   FreeSWITCH, NÃO no SFU. A ponte FreeSWITCH↔SFU é EXTERNAL (ver `sip-realidade.md` §3).
   A UI da D tem de o dizer («a pessoa ouve os outros ao telefone, não a reunião») até a
   ponte existir.
5. `org_id` nunca vem do corpo; números nunca em auditoria (só ids).

## 5. Commits

`git log --oneline origin/main..delonix-meet-backend/v3-telecom` (os traits estão no primeiro;
eventos e `RoomBridge` em «eventos da chamada e RoomBridge na porta CallOriginator»).

## 6. Provado contra o FreeSWITCH real (1.11.3, 2026-09-17)

- `server/tests/telephony_freeswitch.rs` (4/4): failover com eventos por esta ordem
  `Dialing(A) → AttemptFailed(A, NORMAL_TEMPORARY_FAILURE) → Dialing(B) → Ringing(B) → Answered(B, ~345 ms)
  → Ended(answered, NORMAL_CLEARING, billsec 3)`; ocupado `Dialing → AttemptFailed(USER_BUSY) → Ended(false)`;
  `RoomBridge` com a perna `sofia/external/room-sala-…@127.0.0.1:5190` a existir e `hangup` pela porta a
  fechar com `Ended`; `SipControl` a ler gateways reais.
- `web/e2e/telefonia-freeswitch.mjs` (20/20): gateways por `xml_curl`, `xmlstatus`, originate com failover,
  CDR com custo e reenvio idempotente, plano servido por `xml_curl` numa chamada, `limit_execute`.

Duas coisas medidas que a D tem de saber: o `origination_uuid` só vale para a PRIMEIRA tentativa
(identifica-se a chamada por `delonix_call_id`); e a perna para a ponte fica em `RINGING` até o UA
da ponte atender — o `Answered` da porta é o da pessoa ao telefone, não o da ponte.
