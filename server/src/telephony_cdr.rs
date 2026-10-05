//! Registos de chamada do FreeSWITCH: `mod_json_cdr` → `POST
//! /internal/v1/telephony/call-records` (ADR-0009 §6).
//!
//! **Porquê `mod_json_cdr` e não ler o ESL.** O CDR por HTTP chega no fim de
//! CADA chamada, com todas as variáveis do canal, e o módulo guarda-o em disco
//! e volta a tentar se o servidor não responder `2xx` (`retries`,
//! `log-dir`). Um subscritor ESL perde os eventos enquanto estiver desligado —
//! e um CDR perdido é uma chamada que não se cobra nem se audita.
//!
//! **Autenticação:** o segredo interno (`VOICE_INTERNAL_SECRET`) por HTTP Basic
//! (`cred` do `mod_json_cdr`) ou `X-Voice-Secret`, só no listener interno quando
//! `INTERNAL_BIND_ADDR` está definido.
//!
//! **Idempotência:** `(source, source_call_id)` é único. O reenvio do mesmo
//! CDR (o `mod_json_cdr` reenvia em falha, e um `2xx` perdido na rede também)
//! devolve `200` com o registo que já existe, sem duplicar custo.

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, TimeZone, Utc};
use delonix_meet_core::DomainError;
use delonix_meet_domain::telephony::{
    cost::{call_cost, price_at, PricePoint},
    money::{Currency, Money},
    ports::{trunk_id_from_gateway, CallDetail, CallOutcome, CdrSource, Direction, PortError},
};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::{error::ApiError, AppState};

/// Máximo de um CDR JSON (um canal com muitas variáveis e callflow longo).
pub const MAX_CDR_BYTES: usize = 512 * 1024;

pub struct FreeswitchJsonCdr;

/// O `mod_json_cdr` codifica os valores em URL (`encode-values`, por omissão).
fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(v);
                i += 3;
                continue;
            }
        }
        // `+` fica `+`: o FreeSWITCH codifica-o como `%2B`, e um `+` cru é
        // o do número E.164, não um espaço.
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn var(vars: &Value, key: &str) -> Option<String> {
    vars.get(key)
        .and_then(|v| match v {
            Value::String(s) => Some(pct_decode(s)),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .filter(|s| !s.is_empty())
}

fn epoch(vars: &Value, key: &str) -> Option<DateTime<Utc>> {
    var(vars, key)
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|v| *v > 0)
        .and_then(|v| Utc.timestamp_opt(v, 0).single())
}

fn num(vars: &Value, key: &str) -> Option<f64> {
    var(vars, key).and_then(|v| v.parse::<f64>().ok())
}

fn protocol(msg: impl Into<String>) -> PortError {
    PortError::Protocol(msg.into())
}

/// Causa Q.850 → resultado do produto, quando nenhuma variável nossa o disse.
/// Atendida = houve `answer_epoch`: uma chamada atendida e desligada em menos
/// de um segundo tem `billsec` 0 (medido no FreeSWITCH 1.11.3).
pub fn outcome_from_cause(cause: Option<&str>, answered: bool) -> CallOutcome {
    if answered {
        return CallOutcome::Answered;
    }
    match cause.unwrap_or("") {
        "NO_ANSWER" | "NO_USER_RESPONSE" | "ORIGINATOR_CANCEL" | "ALLOTTED_TIMEOUT" => {
            CallOutcome::NoAnswer
        }
        "USER_BUSY" | "CALL_REJECTED" => CallOutcome::Busy,
        "NORMAL_CLEARING" => CallOutcome::NoAnswer,
        _ => CallOutcome::Failed,
    }
}

impl CdrSource for FreeswitchJsonCdr {
    fn parse(&self, payload: &[u8]) -> Result<CallDetail, PortError> {
        let root: Value =
            serde_json::from_slice(payload).map_err(|e| protocol(format!("JSON inválido: {e}")))?;
        let vars = root
            .get("variables")
            .ok_or_else(|| protocol("CDR sem «variables»"))?;
        let source_call_id = var(vars, "uuid").ok_or_else(|| protocol("CDR sem «uuid»"))?;
        if source_call_id.len() > 128 {
            return Err(protocol("uuid demasiado longo"));
        }
        let direction = match var(vars, "delonix_direction")
            .or_else(|| var(vars, "direction"))
            .as_deref()
        {
            // No FreeSWITCH, `inbound` é o canal que ENTROU no FS. Uma chamada
            // de saída nossa nasce de um `originate`, que é `outbound`.
            Some("outbound") => Direction::Outbound,
            _ => Direction::Inbound,
        };
        let started_at =
            epoch(vars, "start_epoch").ok_or_else(|| protocol("CDR sem start_epoch"))?;
        let ended_at = epoch(vars, "end_epoch").unwrap_or(started_at);
        let answered_at = epoch(vars, "answer_epoch");
        let duration_secs = var(vars, "duration")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or_else(|| (ended_at - started_at).num_seconds())
            .max(0);
        let billsec = var(vars, "billsec")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0)
            .max(0);
        let hangup_cause = var(vars, "hangup_cause");
        let outcome = var(vars, "delonix_outcome")
            .and_then(|o| CallOutcome::parse(&o))
            .unwrap_or_else(|| {
                outcome_from_cause(
                    hangup_cause.as_deref(),
                    answered_at.is_some() || billsec > 0,
                )
            });
        let trunk_id = var(vars, "delonix_trunk_id")
            .and_then(|v| Uuid::parse_str(&v).ok())
            .or_else(|| var(vars, "sip_gateway_name").and_then(|g| trunk_id_from_gateway(&g)));
        let emergency = var(vars, "delonix_emergency").as_deref() == Some("true");
        // Emergência nunca gravada, mesmo que a variável diga o contrário.
        let recorded = !emergency && var(vars, "delonix_record").as_deref() == Some("true");
        let packets = num(vars, "rtp_audio_in_packet_count");
        let skipped = num(vars, "rtp_audio_in_skip_packet_count");
        let loss_pct = match (packets, skipped) {
            (Some(p), Some(s)) if p + s > 0.0 => Some((s / (p + s)) * 100.0),
            _ => None,
        };
        // O FreeSWITCH publica a VARIÂNCIA do intervalo entre pacotes (ms²);
        // a raiz da máxima é o que se mostra como jitter. Não é o jitter do
        // RFC 3550 — ver ADR-0009 §6.
        let jitter_ms = num(vars, "rtp_audio_in_jitter_max_variance")
            .filter(|v| *v >= 0.0)
            .map(f64::sqrt);
        let clip = |s: Option<String>, n: usize| s.map(|v| v.chars().take(n).collect::<String>());
        Ok(CallDetail {
            source_call_id,
            org_id: var(vars, "delonix_org_id").and_then(|v| Uuid::parse_str(&v).ok()),
            call_id: var(vars, "delonix_call_id").and_then(|v| Uuid::parse_str(&v).ok()),
            direction,
            from_number: clip(
                var(vars, "caller_id_number")
                    .or_else(|| var(vars, "effective_caller_id_number"))
                    .or_else(|| var(vars, "sip_from_user")),
                32,
            )
            .unwrap_or_default(),
            to_number: clip(
                var(vars, "delonix_dialed")
                    .or_else(|| var(vars, "destination_number"))
                    .or_else(|| var(vars, "sip_to_user")),
                32,
            )
            .unwrap_or_default(),
            trunk_id,
            room_code: clip(var(vars, "delonix_room_code"), 64),
            outcome,
            hangup_cause: clip(hangup_cause, 64),
            started_at,
            answered_at,
            ended_at,
            duration_secs,
            billsec,
            recorded,
            emergency,
            rule_position: var(vars, "delonix_rule_position").and_then(|v| v.parse().ok()),
            jitter_ms,
            loss_pct,
            mos: num(vars, "rtp_audio_in_mos"),
            destination_label: clip(var(vars, "delonix_destination_label"), 120),
        })
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct IngestResult {
    pub id: Uuid,
    /// `true` quando este CDR já tinha sido ingerido (reenvio).
    pub duplicate: bool,
}

/// Guarda um CDR. Resolve a org (variável nossa, ou a sala de voz do dial-in),
/// confirma que o tronco é dessa org, e congela o custo ao preço em vigor.
pub(crate) async fn ingest(state: &AppState, d: CallDetail) -> Result<IngestResult, ApiError> {
    let org_id = d.org_id.ok_or_else(|| {
        ApiError::from(DomainError::precondition(
            "telephony.cdr_org_unresolved",
            "CDR sem delonix_org_id — a chamada não passou pelo plano de marcação da plataforma",
        ))
    })?;
    let org_exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM organizations WHERE id = $1")
        .bind(org_id)
        .fetch_optional(&state.db)
        .await?;
    if org_exists.is_none() {
        return Err(DomainError::precondition(
            "telephony.cdr_org_unresolved",
            "delonix_org_id não corresponde a nenhuma organização",
        )
        .into());
    }
    // Um tronco de OUTRA org não se atribui a esta (nem ao custo dela).
    let trunk_id = match d.trunk_id {
        Some(t) => {
            sqlx::query_scalar::<_, Uuid>(
                "SELECT id FROM telephony_trunks WHERE id = $1 AND org_id = $2",
            )
            .bind(t)
            .bind(org_id)
            .fetch_optional(&state.db)
            .await?
        }
        None => None,
    };
    if d.trunk_id.is_some() && trunk_id.is_none() {
        tracing::warn!(org = %org_id, "CDR com tronco que não é da organização — ignorado o tronco");
    }

    let (cost, price_id, cost_reason) = match (d.direction, trunk_id) {
        (Direction::Inbound, _) => (None, None, Some("inbound_not_billed")),
        (Direction::Outbound, None) => (None, None, Some("no_trunk")),
        (Direction::Outbound, Some(t)) => {
            let points = trunk_prices(state, t).await?;
            match price_at(&points, d.started_at) {
                Some(p) => (
                    Some(call_cost(d.billsec, p.price_per_min)),
                    Some(p.id),
                    None,
                ),
                None => (None, None, Some("no_price_in_force")),
            }
        }
    };

    let id = Uuid::new_v4();
    let inserted: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO telephony_call_records
            (id, org_id, source, source_call_id, outbound_call_id, direction, from_number, to_number,
             trunk_id, room_code, destination_label, outcome, hangup_cause, started_at, answered_at,
             ended_at, duration_secs, billsec, recorded, emergency, rule_position, jitter_ms,
             loss_pct, mos, cost_e4, cost_currency, price_id, cost_reason)
         VALUES ($1,$2,'freeswitch',$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,
                 $20,$21,$22,$23,$24,$25,$26,$27)
         ON CONFLICT (source, source_call_id) DO NOTHING
         RETURNING id",
    )
    .bind(id)
    .bind(org_id)
    .bind(&d.source_call_id)
    .bind(d.call_id)
    .bind(match d.direction {
        Direction::Inbound => "inbound",
        Direction::Outbound => "outbound",
    })
    .bind(&d.from_number)
    .bind(&d.to_number)
    .bind(trunk_id)
    .bind(&d.room_code)
    .bind(&d.destination_label)
    .bind(d.outcome.as_str())
    .bind(&d.hangup_cause)
    .bind(d.started_at)
    .bind(d.answered_at)
    .bind(d.ended_at)
    .bind(d.duration_secs.min(i32::MAX as i64) as i32)
    .bind(d.billsec.min(i32::MAX as i64) as i32)
    .bind(d.recorded && !d.emergency)
    .bind(d.emergency)
    .bind(d.rule_position)
    .bind(d.jitter_ms)
    .bind(d.loss_pct)
    .bind(d.mos)
    .bind(cost.map(|c| c.amount_e4))
    .bind(cost.map(|c| c.currency.as_str()))
    .bind(price_id)
    .bind(cost_reason)
    .fetch_optional(&state.db)
    .await?;
    match inserted {
        Some(id) => Ok(IngestResult {
            id,
            duplicate: false,
        }),
        None => {
            let existing: Uuid = sqlx::query_scalar(
                "SELECT id FROM telephony_call_records WHERE source = 'freeswitch' AND source_call_id = $1",
            )
            .bind(&d.source_call_id)
            .fetch_one(&state.db)
            .await?;
            Ok(IngestResult {
                id: existing,
                duplicate: true,
            })
        }
    }
}

/// Histórico de preços de um tronco.
pub(crate) async fn trunk_prices(
    state: &AppState,
    trunk_id: Uuid,
) -> Result<Vec<PricePoint>, ApiError> {
    let rows: Vec<(Uuid, DateTime<Utc>, i64, String)> = sqlx::query_as(
        "SELECT id, valid_from, price_per_min_e4, currency FROM telephony_trunk_prices WHERE trunk_id = $1",
    )
    .bind(trunk_id)
    .fetch_all(&state.db)
    .await?;
    rows.into_iter()
        .map(|(id, valid_from, e4, cur)| {
            Ok(PricePoint {
                id,
                valid_from,
                price_per_min: Money::new(e4, Currency::parse(&cur)?),
            })
        })
        .collect::<Result<_, DomainError>>()
        .map_err(Into::into)
}

/// `POST /internal/v1/telephony/call-records` — o `mod_json_cdr` do FreeSWITCH.
/// `201` novo, `200` reenvio, `422` sem organização (o FreeSWITCH guarda-o no
/// `log-dir` e volta a tentar; um operador vê-o no log de erros).
pub async fn ingest_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    crate::voice::check_media_secret(&state, &headers)?;
    if body.len() > MAX_CDR_BYTES {
        return Err(DomainError::invalid("telephony.cdr_too_large", "CDR demasiado grande").into());
    }
    // O mod_json_cdr pode enviar `cdr=<json>` em form-urlencoded ou JSON cru.
    let raw: Vec<u8> = match body.strip_prefix(b"cdr=") {
        Some(rest) => pct_decode(&String::from_utf8_lossy(rest)).into_bytes(),
        None => body.to_vec(),
    };
    // A perna A de uma chamada pelo plano (o PBX que marcou): o custo e o ASR
    // estão nas pernas B. Aceita-se (o FreeSWITCH não volta a tentar) e ignora-se.
    if serde_json::from_slice::<Value>(&raw)
        .ok()
        .and_then(|v| var(&v["variables"], "delonix_cdr_skip"))
        .as_deref()
        == Some("true")
    {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    let mut detail = FreeswitchJsonCdr.parse(&raw).map_err(|e| {
        ApiError::from(DomainError::invalid("telephony.cdr_invalid", e.to_string()))
    })?;
    // Dial-in pelo IVR herdado: a org vem da sala de voz.
    if detail.org_id.is_none() {
        let voice_room = serde_json::from_slice::<Value>(&raw)
            .ok()
            .and_then(|v| var(&v["variables"], "delonix_voice_room_id"))
            .and_then(|v| Uuid::parse_str(&v).ok());
        if let Some(vr) = voice_room {
            detail.org_id = sqlx::query_scalar("SELECT org_id FROM voice_room WHERE id = $1")
                .bind(vr)
                .fetch_optional(&state.db)
                .await?;
        }
    }
    // Uma chamada que não passou por um tronco nem pelo plano de marcação —
    // ramal para ramal, o IVR do dial-in, a perna para a ponte da sala — não
    // é da telefonia: não tem organização a quem a atribuir, nem custo. A
    // configuração distribuída já só regista a perna de um tronco (R291), mas
    // sobram pernas que nunca chegaram a um plano de marcação, e uma
    // configuração mais antiga entrega todas; recusar estas fazia o módulo
    // tentar outra vez e guardar cada uma em disco, para sempre. Aceita-se e
    // ignora-se, como a perna A. Um CDR COM
    // tronco e sem organização continua a ser recusado: é uma chamada de
    // operadora que não se consegue atribuir, e tem de ficar à vista. «Sem
    // tronco» lê-se no nome do gateway EM BRUTO: um gateway que não seja nosso
    // (`dlx-<id>`) não dá `trunk_id`, e é uma saída para a rede pública na mesma.
    let by_gateway = serde_json::from_slice::<Value>(&raw)
        .ok()
        .and_then(|v| var(&v["variables"], "sip_gateway_name"))
        .is_some();
    if detail.org_id.is_none() && detail.trunk_id.is_none() && !by_gateway {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    let res = ingest(&state, detail).await?;
    let status = if res.duplicate {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((status, Json(res)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> serde_json::Value {
        serde_json::json!({
            "core-uuid": "x",
            "variables": {
                "uuid": "a1b2c3",
                "direction": "outbound",
                "start_epoch": "1789000000",
                "answer_epoch": "1789000003",
                "end_epoch": "1789000128",
                "duration": "128",
                "billsec": "125",
                "hangup_cause": "NORMAL_CLEARING",
                "caller_id_number": "%2B244222000000",
                "destination_number": "244923447108",
                "sip_gateway_name": "dlx-00000000-0000-0000-0000-000000000001",
                "delonix_org_id": "00000000-0000-0000-0000-00000000000a",
                "delonix_record": "true",
                "delonix_emergency": "false",
                "delonix_rule_position": "0",
                "rtp_audio_in_packet_count": "990",
                "rtp_audio_in_skip_packet_count": "10",
                "rtp_audio_in_jitter_max_variance": "36",
                "rtp_audio_in_mos": "4.4"
            }
        })
    }

    #[test]
    fn parses_mod_json_cdr() {
        let d = FreeswitchJsonCdr
            .parse(sample().to_string().as_bytes())
            .unwrap();
        assert_eq!(d.source_call_id, "a1b2c3");
        assert_eq!(d.direction, Direction::Outbound);
        assert_eq!(d.from_number, "+244222000000");
        assert_eq!(d.billsec, 125);
        assert_eq!(d.outcome, CallOutcome::Answered);
        assert_eq!(
            d.trunk_id,
            Some(Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap())
        );
        assert!(d.recorded);
        assert_eq!(d.loss_pct, Some(1.0));
        assert_eq!(d.jitter_ms, Some(6.0));
        assert_eq!(d.rule_position, Some(0));
    }

    #[test]
    fn emergency_cdr_is_never_recorded() {
        let mut v = sample();
        v["variables"]["delonix_emergency"] = "true".into();
        let d = FreeswitchJsonCdr.parse(v.to_string().as_bytes()).unwrap();
        assert!(d.emergency && !d.recorded);
    }

    #[test]
    fn outcomes_and_garbage() {
        assert_eq!(
            outcome_from_cause(Some("USER_BUSY"), false),
            CallOutcome::Busy
        );
        assert_eq!(
            outcome_from_cause(Some("NO_ANSWER"), false),
            CallOutcome::NoAnswer
        );
        assert_eq!(
            outcome_from_cause(Some("NORMAL_TEMPORARY_FAILURE"), false),
            CallOutcome::Failed
        );
        let mut v = sample();
        v["variables"]["delonix_outcome"] = "wrong_pin".into();
        v["variables"]["billsec"] = "0".into();
        v["variables"]["answer_epoch"] = "0".into();
        let d = FreeswitchJsonCdr.parse(v.to_string().as_bytes()).unwrap();
        assert_eq!(d.outcome, CallOutcome::WrongPin);
        assert!(FreeswitchJsonCdr.parse(b"{}").is_err());
        assert!(FreeswitchJsonCdr.parse(b"nope").is_err());
        assert_eq!(pct_decode("%2B244%20x"), "+244 x");
        assert_eq!(pct_decode("100%"), "100%");
    }
}
