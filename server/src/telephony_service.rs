//! Telefonia — serviço partilhado (ADR-0009). É a ÚNICA implementação das
//! regras que mais de uma superfície usa: o ecrã de telefonia (frente C) e os
//! canais na sala (frente D) chamam estas funções; nenhuma volta a ler o plano
//! de marcação ou a falar com o FreeSWITCH por conta própria.
//!
//! Contrato para a frente D: `notas-ui-template/contrato-telefonia.md`.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use delonix_meet_core::DomainError;
use delonix_meet_domain::telephony::{
    cost::{price_at, PricePoint},
    dial_plan::{resolve, DialRule, Pattern, Resolution, ResolutionOutcome, RuleAction, TrunkRef},
    money::{format_e4, Currency, Money},
    number::{mask, parse_dialed, DialedNumber},
    ports::{
        gateway_name, AfterAnswer, CallEvent, CallEventSink, CallOriginator, DialLeg,
        OriginateRequest, PortError, SipControl, SmsGateways, SmsQueued, SmsSendRequest,
    },
};
use serde::Serialize;
use uuid::Uuid;

use crate::{error::ApiError, AppState};

/// Os adaptadores montados a partir da configuração. `None` = infraestrutura
/// não configurada nesta instalação.
#[derive(Clone, Default)]
pub struct Adapters {
    pub sip: Option<Arc<dyn SipControl>>,
    pub originator: Option<Arc<dyn CallOriginator>>,
    /// Os gateways que o FreeSWITCH tem de reler, e se já há quem os leve.
    pub gateway_refresh: Arc<GatewayRefresh>,
}

/// A fila dos avisos «este tronco mudou» (R297). Um só trabalhador de cada
/// vez: sem ela cada pedido de um administrador abria a sua ligação ao ESL e
/// mandava o seu `rescan` — que relê o XML inteiro, aloca um gateway novo e
/// escreve uma linha por gateway de TODAS as organizações. Com ela, mil
/// alterações seguidas dão um `rescan` por intervalo, com os gateways juntos.
#[derive(Default)]
pub struct GatewayRefresh {
    /// (gateways à espera, há um trabalhador vivo)
    inner: std::sync::Mutex<(std::collections::HashSet<String>, bool)>,
}

impl GatewayRefresh {
    /// O intervalo mínimo entre dois `rescan` pedidos por alterações de troncos.
    pub const MIN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

    fn lock(&self) -> std::sync::MutexGuard<'_, (std::collections::HashSet<String>, bool)> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Põe um gateway na fila. `true` = não havia trabalhador: quem chama
    /// tem de lançar um.
    pub fn enqueue(&self, gateway: String) -> bool {
        let mut g = self.lock();
        g.0.insert(gateway);
        !std::mem::replace(&mut g.1, true)
    }

    /// O lote seguinte; `None` = fila vazia, e o trabalhador dá-se por
    /// terminado no mesmo instante (quem enfileirar a seguir lança outro).
    pub fn next_batch(&self) -> Option<Vec<String>> {
        let mut g = self.lock();
        if g.0.is_empty() {
            g.1 = false;
            return None;
        }
        Some(g.0.drain().collect())
    }
}

impl Adapters {
    pub fn from_config(
        config: &crate::config::Config,
        outbound: &crate::net_guard::Outbound,
    ) -> Self {
        use crate::telephony_esl::{
            EslConfig, FreeswitchOriginator, FreeswitchSipControl, KamailioRpc,
        };
        let esl = config.telephony_esl_addr.as_ref().map(|addr| EslConfig {
            addr: addr.clone(),
            password: config.telephony_esl_password.clone(),
            sofia_profile: config.telephony_sofia_profile.clone(),
        });
        let kamailio = config
            .telephony_kamailio_rpc_url
            .as_ref()
            .map(|url| KamailioRpc {
                url: url.clone(),
                outbound: outbound.clone(),
            });
        let sip: Option<Arc<dyn SipControl>> = if esl.is_some() || kamailio.is_some() {
            Some(Arc::new(FreeswitchSipControl {
                esl: esl.clone(),
                kamailio,
            }))
        } else {
            None
        };
        let originator: Option<Arc<dyn CallOriginator>> =
            esl.map(|esl| Arc::new(FreeswitchOriginator { esl }) as Arc<dyn CallOriginator>);
        Self {
            sip,
            originator,
            gateway_refresh: Arc::default(),
        }
    }
}

// ============================================================
//  Tipos de fronteira
// ============================================================

/// Dinheiro na API: texto decimal + moeda (nunca um número JSON).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct MoneyDto {
    /// Decimal com 4 casas, p.ex. `"9.4000"`.
    pub amount: String,
    /// `AOA` | `USD`.
    pub currency: String,
}

impl From<Money> for MoneyDto {
    fn from(m: Money) -> Self {
        Self {
            amount: format_e4(m.amount_e4),
            currency: m.currency.as_str().to_string(),
        }
    }
}

pub(crate) fn port_error(e: PortError) -> ApiError {
    match e {
        PortError::NotConfigured(m) => DomainError::precondition(
            "telephony.not_configured",
            format!("a infraestrutura de voz não está configurada nesta instalação ({m})"),
        )
        .into(),
        PortError::Unavailable(m) => DomainError::new(
            delonix_meet_core::ErrorKind::Unavailable,
            "telephony.media_server_unavailable",
            format!("o servidor de media não respondeu: {m}"),
        )
        .into(),
        PortError::Rejected(m) => {
            DomainError::precondition("telephony.media_server_rejected", m).into()
        }
        PortError::Protocol(m) => DomainError::new(
            delonix_meet_core::ErrorKind::Unavailable,
            "telephony.media_server_protocol",
            m,
        )
        .into(),
    }
}

// ============================================================
//  Leitura do que a org configurou
// ============================================================

#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct TrunkRow {
    pub id: Uuid,
    pub name: String,
    pub short_code: String,
    pub max_channels: i32,
    pub enabled: bool,
}

/// Troncos da org pela ordem de encaminhamento.
pub(crate) async fn load_trunks(state: &AppState, org_id: Uuid) -> Result<Vec<TrunkRow>, ApiError> {
    Ok(sqlx::query_as(
        "SELECT id, name, short_code, max_channels, enabled
           FROM telephony_trunks WHERE org_id = $1 ORDER BY position, id",
    )
    .bind(org_id)
    .fetch_all(&state.db)
    .await?)
}

#[derive(sqlx::FromRow)]
struct RuleRow {
    pattern: String,
    description: String,
    action: String,
    trunk_id: Option<Uuid>,
    fallback_trunk_id: Option<Uuid>,
    record: bool,
    emergency: bool,
}

/// O plano gravado. Foi validado ao escrever; aqui só se volta a construir.
/// Uma linha que já não se lê (padrão antigo) é saltada com erro no log — a
/// resolução continua para as restantes, e a emergência não depende dela.
pub(crate) async fn load_rules(state: &AppState, org_id: Uuid) -> Result<Vec<DialRule>, ApiError> {
    let rows: Vec<RuleRow> = sqlx::query_as(
        "SELECT pattern, description, action, trunk_id, fallback_trunk_id, record, emergency
           FROM telephony_dial_rules WHERE org_id = $1 ORDER BY position",
    )
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        match (Pattern::parse(&r.pattern), RuleAction::parse(&r.action)) {
            (Ok(pattern), Ok(action)) => out.push(DialRule {
                pattern,
                description: r.description,
                action,
                trunk_id: r.trunk_id,
                fallback_trunk_id: r.fallback_trunk_id,
                record: r.record && !r.emergency,
                emergency: r.emergency,
            }),
            _ => {
                tracing::error!(org = %org_id, pattern = %r.pattern, "regra do plano ilegível — saltada")
            }
        }
    }
    Ok(out)
}

pub(crate) fn trunk_refs(trunks: &[TrunkRow]) -> Vec<TrunkRef> {
    trunks
        .iter()
        .map(|t| TrunkRef {
            id: t.id,
            enabled: t.enabled,
        })
        .collect()
}

pub(crate) fn parse_number(state: &AppState, number: &str) -> Result<DialedNumber, ApiError> {
    Ok(parse_dialed(
        number,
        &state.config.telephony_country_code,
        state.config.telephony_national_len,
    )?)
}

pub(crate) fn mask_number(state: &AppState, number: &str) -> String {
    mask(number, &state.config.telephony_country_code)
}

/// Preços de todos os troncos da org, por tronco.
pub(crate) async fn prices_by_trunk(
    state: &AppState,
    org_id: Uuid,
) -> Result<std::collections::HashMap<Uuid, Vec<PricePoint>>, ApiError> {
    let rows: Vec<(Uuid, Uuid, DateTime<Utc>, i64, String)> = sqlx::query_as(
        "SELECT trunk_id, id, valid_from, price_per_min_e4, currency
           FROM telephony_trunk_prices WHERE org_id = $1",
    )
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    let mut map: std::collections::HashMap<Uuid, Vec<PricePoint>> = Default::default();
    for (trunk, id, valid_from, e4, cur) in rows {
        map.entry(trunk).or_default().push(PricePoint {
            id,
            valid_from,
            price_per_min: Money::new(e4, Currency::parse(&cur)?),
        });
    }
    Ok(map)
}

// ============================================================
//  Resolução de um número (sem ligar)
// ============================================================

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct TrunkSummary {
    pub id: Uuid,
    pub name: String,
    pub short_code: String,
}

pub(crate) struct ResolvedNumber {
    pub dialed: DialedNumber,
    pub resolution: Resolution,
    /// Os troncos de `resolution.legs`, pela mesma ordem.
    pub trunks: Vec<TrunkSummary>,
    /// Preço em vigor AGORA no primeiro tronco. `None` sem tronco ou sem preço.
    pub price_per_min: Option<Money>,
}

pub(crate) async fn resolve_number(
    state: &AppState,
    org_id: Uuid,
    number: &str,
) -> Result<ResolvedNumber, ApiError> {
    let dialed = parse_number(state, number)?;
    let trunks = load_trunks(state, org_id).await?;
    let rules = load_rules(state, org_id).await?;
    let resolution = resolve(
        &rules,
        &dialed.digits,
        &state.config.telephony_emergency_numbers,
        &trunk_refs(&trunks),
    );
    let summaries: Vec<TrunkSummary> = resolution
        .legs
        .iter()
        .filter_map(|id| trunks.iter().find(|t| t.id == *id))
        .map(|t| TrunkSummary {
            id: t.id,
            name: t.name.clone(),
            short_code: t.short_code.clone(),
        })
        .collect();
    let price_per_min = match resolution.legs.first() {
        Some(first) => {
            let prices = crate::telephony_cdr::trunk_prices(state, *first).await?;
            price_at(&prices, Utc::now()).map(|p| p.price_per_min)
        }
        None => None,
    };
    Ok(ResolvedNumber {
        dialed,
        resolution,
        trunks: summaries,
        price_per_min,
    })
}

/// Preço por minuto em vigor agora para ligar a `number`.
#[allow(dead_code)] // consumidor: frente D (custo antes de convidar)
pub(crate) async fn estimate_price_per_min(
    state: &AppState,
    org_id: Uuid,
    number: &str,
) -> Result<Option<Money>, ApiError> {
    Ok(resolve_number(state, org_id, number).await?.price_per_min)
}

// ============================================================
//  Ligar para fora
// ============================================================

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // RoomInvite: consumidor frente D
pub enum CallPurpose {
    QuickTest,
    RoomInvite { room_code: String },
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct OutboundCall {
    pub id: Uuid,
    /// `quick_test` | `room_invite`.
    pub purpose: String,
    pub room_code: Option<String>,
    /// Mascarado (`+244 923 ***108`).
    #[sqlx(rename = "to_number")]
    pub to_masked: String,
    /// `dialing` → `ringing` → `answered` (fica, com `finished_at` no fim) |
    /// `no_answer` | `busy` | `failed`.
    pub status: String,
    /// Troncos tentados, por ordem.
    pub trunk_ids: Vec<Uuid>,
    pub rule_position: Option<i32>,
    pub record: bool,
    pub emergency: bool,
    /// Do pedido ao atendimento, medido no ESL. `null` se não atendeu.
    pub answer_latency_ms: Option<i64>,
    /// Causa Q.850 do FreeSWITCH.
    pub hangup_cause: Option<String>,
    /// Porque falhou antes de chegar à operadora (media server em baixo…).
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub answered_at: Option<DateTime<Utc>>,
    /// Segundos falados, quando a chamada acabou atendida.
    pub billsec: Option<i32>,
    /// Fim da chamada (ou da tentativa). `null` enquanto decorre.
    pub finished_at: Option<DateTime<Utc>>,
}

pub(crate) const OUTBOUND_COLUMNS: &str = "id, purpose, room_code, to_number, status, trunk_ids, rule_position, record, emergency, answer_latency_ms, hangup_cause, error, created_at, answered_at, billsec, finished_at";

pub(crate) fn masked(state: &AppState, mut c: OutboundCall) -> OutboundCall {
    c.to_masked = mask_number(state, &c.to_masked);
    c
}

fn refuse(code: &'static str, msg: impl Into<String>) -> ApiError {
    DomainError::precondition(code, msg).into()
}

/// Tempo máximo a tocar.
const ANSWER_TIMEOUT_SECS: u32 = 30;

/// Liga para fora pelo plano de marcação. Devolve logo a linha `dialing`; a
/// chamada corre numa tarefa e o resultado fica na mesma linha (e, no fim, no
/// CDR que o FreeSWITCH envia).
pub(crate) async fn place_call(
    state: &Arc<AppState>,
    org_id: Uuid,
    actor_id: Uuid,
    number: &str,
    after_answer: AfterAnswer,
    purpose: CallPurpose,
    listener: Option<Arc<dyn CallEventSink>>,
) -> Result<OutboundCall, ApiError> {
    let originator = state.telephony.originator.clone().ok_or_else(|| {
        refuse(
            "telephony.not_configured",
            "sem servidor de media (TELEPHONY_ESL_ADDR) — nenhuma chamada sai desta instalação",
        )
    })?;
    let r = resolve_number(state, org_id, number).await?;
    if r.resolution.emergency {
        return Err(match purpose {
            CallPurpose::QuickTest => refuse(
                "telephony.test_call_emergency_refused",
                "um teste nunca liga para um número de emergência",
            ),
            CallPurpose::RoomInvite { .. } => refuse(
                "telephony.emergency_not_invitable",
                "um número de emergência não se convida para uma reunião",
            ),
        });
    }
    match r.resolution.outcome {
        ResolutionOutcome::Route => {}
        ResolutionOutcome::Blocked => {
            return Err(refuse(
                "telephony.number_blocked",
                "o plano de marcação bloqueia este número",
            ))
        }
        ResolutionOutcome::NoMatch => {
            return Err(refuse(
                "telephony.no_matching_rule",
                "nenhuma regra do plano de marcação casa este número",
            ))
        }
        ResolutionOutcome::Internal => {
            return Err(refuse(
                "telephony.destination_not_external",
                "este número é interno (sala por PIN ou ramal) — não sai por uma operadora",
            ))
        }
        ResolutionOutcome::NoAvailableTrunk => {
            return Err(refuse(
                "telephony.no_available_trunk",
                "nenhuma das operadoras desta regra está activa",
            ))
        }
    }
    // Limite por org ANTES de gastar canais: uma chamada custa dinheiro.
    if let Err(wait) = state.telephony_call_limiter.acquire(&org_id.to_string()) {
        return Err(DomainError::new(
            delonix_meet_core::ErrorKind::ResourceExhausted,
            "telephony.call_rate_limited",
            format!(
                "demasiadas chamadas desta organização; tenta daqui a {} s",
                wait.as_secs().max(1)
            ),
        )
        .into());
    }

    // Canais: tira os troncos cheios, medidos agora no media server. Sem
    // medida (media server não responde) não se inventa: segue, e o
    // FreeSWITCH recusa se estiver cheio.
    let trunks = load_trunks(state, org_id).await?;
    let mut legs_ids = r.resolution.legs.clone();
    if let Some(sip) = &state.telephony.sip {
        let names: Vec<String> = legs_ids.iter().map(|t| gateway_name(*t)).collect();
        if let Ok(snap) = sip.snapshot(&names).await {
            legs_ids.retain(|id| {
                let max = trunks
                    .iter()
                    .find(|t| t.id == *id)
                    .map(|t| t.max_channels)
                    .unwrap_or(0);
                let used = snap
                    .gateways
                    .iter()
                    .find(|g| g.name == gateway_name(*id))
                    .and_then(|g| g.channels_in_use);
                used.is_none_or(|u| (u as i64) < max as i64)
            });
            if legs_ids.is_empty() {
                return Err(refuse(
                    "telephony.channels_exhausted",
                    "todas as operadoras desta regra estão sem canais livres",
                ));
            }
        }
    }
    // O número como a operadora o quer: E.164 sem `+` quando é público.
    let wire_number = r
        .dialed
        .e164
        .as_deref()
        .map(|e| e.trim_start_matches('+').to_string())
        .unwrap_or_else(|| r.dialed.digits.clone());
    let legs: Vec<DialLeg> = legs_ids
        .iter()
        .map(|t| DialLeg {
            trunk_id: *t,
            gateway_name: gateway_name(*t),
            number: wire_number.clone(),
        })
        .collect();

    let call_id = Uuid::new_v4();
    let (purpose_str, room_code) = match &purpose {
        CallPurpose::QuickTest => ("quick_test", None),
        CallPurpose::RoomInvite { room_code } => ("room_invite", Some(room_code.clone())),
    };
    let row: OutboundCall = sqlx::query_as(&format!(
        "INSERT INTO telephony_outbound_calls
            (id, org_id, created_by, purpose, room_code, to_number, status, trunk_ids, rule_position, record, emergency)
         VALUES ($1,$2,$3,$4,$5,$6,'dialing',$7,$8,$9,false)
         RETURNING {OUTBOUND_COLUMNS}"
    ))
    .bind(call_id)
    .bind(org_id)
    .bind(actor_id)
    .bind(purpose_str)
    .bind(&room_code)
    .bind(r.dialed.e164.as_deref().unwrap_or(&r.dialed.digits))
    .bind(&legs_ids)
    .bind(r.resolution.rule_position.map(|p| p as i32))
    .bind(r.resolution.record)
    .fetch_one(&state.db)
    .await?;
    // O alvo é o id: o número é dado pessoal e a auditoria é imutável.
    crate::audit::log(
        &state.db,
        Some(org_id),
        actor_id,
        match purpose {
            CallPurpose::QuickTest => "telephony.test_call.placed",
            CallPurpose::RoomInvite { .. } => "telephony.call.placed",
        },
        &call_id.to_string(),
    )
    .await;

    let req = OriginateRequest {
        call_id,
        org_id,
        legs,
        caller_id: None,
        record: r.resolution.record,
        emergency: false,
        rule_position: r.resolution.rule_position,
        answer_timeout_secs: ANSWER_TIMEOUT_SECS,
        after_answer,
    };
    let st = state.clone();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<CallEvent>();
    let sink: Arc<dyn CallEventSink> = Arc::new(RowSink {
        tx,
        extra: listener,
    });
    // Os eventos gravam-se pela ordem, numa só tarefa.
    let db = state.db.clone();
    tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            if let Err(e) = apply_event(&db, call_id, &ev).await {
                tracing::error!(call = %call_id, "não consegui gravar o evento da chamada: {e}");
            }
            if matches!(ev, CallEvent::Ended { .. }) {
                break;
            }
        }
    });
    // Se o processo parar a meio, a linha fica `dialing`/`ringing`: o
    // `finish_stale` fecha-a.
    tokio::spawn(async move {
        if let Err(e) = originator.originate(&req, sink).await {
            let _ = sqlx::query(
                "UPDATE telephony_outbound_calls
                    SET status = 'failed', error = $2, finished_at = now()
                  WHERE id = $1 AND status IN ('dialing','ringing')",
            )
            .bind(req.call_id)
            .bind(e.to_string())
            .execute(&st.db)
            .await;
        }
    });
    Ok(masked(state, row))
}

/// Encaminha os eventos para a fila da linha e para quem mais os quiser.
struct RowSink {
    tx: tokio::sync::mpsc::UnboundedSender<CallEvent>,
    extra: Option<Arc<dyn CallEventSink>>,
}

impl CallEventSink for RowSink {
    fn on_event(&self, call_id: Uuid, event: CallEvent) {
        if let Some(x) = &self.extra {
            x.on_event(call_id, event.clone());
        }
        let _ = self.tx.send(event);
    }
}

/// Estado da linha a partir de um evento. Só avança (nunca volta atrás).
async fn apply_event(db: &sqlx::PgPool, id: Uuid, ev: &CallEvent) -> Result<(), sqlx::Error> {
    match ev {
        CallEvent::Dialing { .. } | CallEvent::AttemptFailed { .. } => Ok(()),
        CallEvent::Ringing { .. } => sqlx::query(
            "UPDATE telephony_outbound_calls SET status = 'ringing' WHERE id = $1 AND status = 'dialing'",
        )
        .bind(id)
        .execute(db)
        .await
        .map(|_| ()),
        CallEvent::Answered { latency_ms, .. } => sqlx::query(
            "UPDATE telephony_outbound_calls
                SET status = 'answered', answer_latency_ms = $2, answered_at = now()
              WHERE id = $1 AND status IN ('dialing','ringing')",
        )
        .bind(id)
        .bind(*latency_ms as i64)
        .execute(db)
        .await
        .map(|_| ()),
        CallEvent::Ended { answered, cause, billsec } => {
            let status = if *answered {
                "answered"
            } else {
                match cause.as_str() {
                    "USER_BUSY" | "CALL_REJECTED" => "busy",
                    "NO_ANSWER" | "NO_USER_RESPONSE" | "ALLOTTED_TIMEOUT" | "ORIGINATOR_CANCEL" => "no_answer",
                    _ => "failed",
                }
            };
            sqlx::query(
                "UPDATE telephony_outbound_calls
                    SET status = $2, hangup_cause = $3, billsec = $4, finished_at = now()
                  WHERE id = $1 AND finished_at IS NULL",
            )
            .bind(id)
            .bind(status)
            .bind(cause)
            .bind(billsec.map(|b| b as i32))
            .execute(db)
            .await
            .map(|_| ())
        }
    }
}

/// Linhas `dialing` com mais de 5 minutos: o processo que as seguia morreu.
pub(crate) async fn finish_stale(state: &AppState, org_id: Uuid) -> Result<(), ApiError> {
    sqlx::query(
        "UPDATE telephony_outbound_calls
            SET status = 'failed', error = 'o servidor reiniciou antes do resultado', finished_at = now()
          WHERE org_id = $1 AND status IN ('dialing','ringing') AND created_at < now() - interval '5 minutes'",
    )
    .bind(org_id)
    .execute(&state.db)
    .await?;
    Ok(())
}

// ============================================================
//  Custo por sala (frente D)
// ============================================================

/// Custo acumulado das chamadas de uma sala desde `since`, por moeda.
#[allow(dead_code)] // consumidor: frente D (canais na sala)
pub(crate) async fn room_call_costs(
    state: &AppState,
    org_id: Uuid,
    room_code: &str,
    since: DateTime<Utc>,
) -> Result<Vec<Money>, ApiError> {
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT SUM(cost_e4)::bigint, cost_currency FROM telephony_call_records
          WHERE org_id = $1 AND room_code = $2 AND started_at >= $3 AND cost_e4 IS NOT NULL
          GROUP BY cost_currency",
    )
    .bind(org_id)
    .bind(room_code)
    .bind(since)
    .fetch_all(&state.db)
    .await?;
    let mut out = Vec::new();
    for (e4, cur) in rows {
        out.push(Money::new(e4, Currency::parse(&cur)?));
    }
    Ok(delonix_meet_domain::telephony::cost::sum_by_currency(out))
}

// ============================================================
//  SMS (ADR-0005) pela porta
// ============================================================

#[allow(dead_code)] // adaptador da porta SmsGateways; consumidor: frente D
pub struct QueueSmsGateways {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl SmsGateways for QueueSmsGateways {
    async fn send(&self, req: &SmsSendRequest) -> Result<SmsQueued, PortError> {
        let m = crate::sms::enqueue(
            &self.state,
            req.org_id,
            req.actor_id,
            &req.to,
            &req.body,
            &req.route,
            req.idempotency_key.as_deref(),
        )
        .await
        .map_err(|e| PortError::Rejected(e.to_string()))?;
        Ok(m)
    }
}

/// SMS pela fila do ADR-0005, com a mesma regra do `POST /sms/messages`.
#[allow(dead_code)] // consumidor: frente D (SMS com PIN)
pub(crate) async fn send_sms(state: &AppState, req: SmsSendRequest) -> Result<SmsQueued, ApiError> {
    crate::sms::enqueue(
        state,
        req.org_id,
        req.actor_id,
        &req.to,
        &req.body,
        &req.route,
        req.idempotency_key.as_deref(),
    )
    .await
}
