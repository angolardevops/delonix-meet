//! Chamadas: teste rápido, registo de chamadas externas e consumo do mês
//! (ADR-0009 §6–§7).
//!
//! - `POST /api/orgs/{org_id}/telephony/test-calls`                  «Ligar agora» (`202`, resultado real)
//! - `GET  /api/orgs/{org_id}/telephony/test-calls`                  últimos testes (o mais recente primeiro)
//! - `GET  /api/orgs/{org_id}/telephony/test-calls/{test_call_id}`   um teste (para acompanhar)
//! - `GET  /api/orgs/{org_id}/telephony/call-records`                CDRs paginados e filtrados
//! - `GET  /api/orgs/{org_id}/telephony/usage`                       consumo de um mês, por operadora
//!
//! «Enviar SMS» do teste rápido usa a rota que já existe
//! (`POST /api/orgs/{org_id}/sms/messages`): a mesma regra, o mesmo limite e a
//! mesma auditoria — não há uma segunda porta para o mesmo SMS.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::{
    page::{Page, PageRequest},
    DomainError,
};
use delonix_meet_domain::telephony::{
    cost::billed_minutes,
    money::{to_aoa, Currency, Money},
    ports::{AfterAnswer, CallOutcome},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::ApiError,
    telephony_service::{
        finish_stale, mask_number, masked, place_call, CallPurpose, MoneyDto, OutboundCall,
        OUTBOUND_COLUMNS,
    },
    AppState,
};

#[derive(Deserialize, utoipa::ToSchema)]
pub struct TestCallReq {
    pub number: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct OutboundCallPage {
    pub items: Vec<OutboundCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageQuery {
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Cursor {
    at: DateTime<Utc>,
    id: Uuid,
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        create_test_call,
        list_test_calls,
        get_test_call,
        list_call_records,
        usage
    ),
    components(schemas(
        TestCallReq,
        OutboundCall,
        OutboundCallPage,
        CallRecord,
        CallRecordPage,
        Usage,
        UsageByTrunk
    ))
)]
pub struct ApiDoc;

/// «Ligar agora»: liga pelo plano de marcação e mede o atendimento. Devolve
/// logo a tentativa (`dialing`); o resultado — atendida e em quantos ms, ou a
/// causa — lê-se em `GET …/test-calls/{id}`. Nunca liga a números de
/// emergência. Limitado por organização.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/telephony/test-calls", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = TestCallReq,
    responses(
        (status = 202, body = OutboundCall, description = "A tocar; acompanhar pelo id."),
        (status = 400, body = crate::openapi::ErrorBody, description = "`telephony.invalid_number`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 422, body = crate::openapi::ErrorBody, description = "`telephony.not_configured`, `telephony.test_call_emergency_refused`, `telephony.number_blocked`, `telephony.no_matching_rule`, `telephony.destination_not_external`, `telephony.no_available_trunk`, `telephony.channels_exhausted`"),
        (status = 429, body = crate::openapi::ErrorBody, description = "`telephony.call_rate_limited`"),
    )
)]
pub async fn create_test_call(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<TestCallReq>,
) -> Result<(StatusCode, Json<OutboundCall>), ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let call = place_call(
        &state,
        org_id,
        auth.user_id,
        &req.number,
        AfterAnswer::TestTone { secs: 3 },
        CallPurpose::QuickTest,
        None,
    )
    .await?;
    Ok((StatusCode::ACCEPTED, Json(call)))
}

/// Últimos testes rápidos, do mais recente para o mais antigo.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/telephony/test-calls", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), PageQuery),
    responses(
        (status = 200, body = OutboundCallPage),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_test_calls(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Query(q): Query<PageQuery>,
) -> Result<Json<OutboundCallPage>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    finish_stale(&state, org_id).await?;
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<Cursor> = page.cursor()?;
    let rows: Vec<OutboundCall> = sqlx::query_as(&format!(
        "SELECT {OUTBOUND_COLUMNS} FROM telephony_outbound_calls
          WHERE org_id = $1 AND purpose = 'quick_test'
            AND ($2::timestamptz IS NULL OR (created_at, id) < ($2, $3))
          ORDER BY created_at DESC, id DESC LIMIT $4"
    ))
    .bind(org_id)
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |c| Cursor {
        at: c.created_at,
        id: c.id,
    });
    Ok(Json(OutboundCallPage {
        items: p.items.into_iter().map(|c| masked(&state, c)).collect(),
        next_page_token: p.next_page_token,
    }))
}

/// Um teste rápido.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/telephony/test-calls/{test_call_id}", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("test_call_id" = Uuid, Path)),
    responses(
        (status = 200, body = OutboundCall),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_test_call(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<OutboundCall>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    finish_stale(&state, org_id).await?;
    let c: OutboundCall = sqlx::query_as(&format!(
        "SELECT {OUTBOUND_COLUMNS} FROM telephony_outbound_calls
          WHERE id = $1 AND org_id = $2 AND purpose = 'quick_test'"
    ))
    .bind(id)
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(Json(masked(&state, c)))
}

// ============================================================
//  Registo de chamadas (CDR)
// ============================================================

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct CallRecord {
    pub id: Uuid,
    /// `inbound` | `outbound`.
    pub direction: String,
    /// Mascarado (`+244 923 ***108`).
    pub from_number: String,
    /// Mascarado.
    pub to_number: String,
    pub trunk_id: Option<Uuid>,
    pub trunk_name: Option<String>,
    pub room_code: Option<String>,
    pub destination_label: Option<String>,
    /// `answered` | `no_answer` | `busy` | `failed` | `wrong_pin` | `waiting_room` | `forwarded`.
    pub outcome: String,
    pub hangup_cause: Option<String>,
    pub started_at: DateTime<Utc>,
    pub answered_at: Option<DateTime<Utc>>,
    pub ended_at: DateTime<Utc>,
    pub duration_secs: i32,
    pub billsec: i32,
    pub recorded: bool,
    pub emergency: bool,
    #[sqlx(skip)]
    pub cost: Option<MoneyDto>,
    /// Porque o custo é `null`: `inbound_not_billed` | `no_trunk` | `no_price_in_force`.
    pub cost_reason: Option<String>,
    #[serde(skip)]
    cost_e4: Option<i64>,
    #[serde(skip)]
    cost_currency: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CallRecordPage {
    pub items: Vec<CallRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CallRecordQuery {
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
    /// `inbound` | `outbound`.
    pub direction: Option<String>,
    pub trunk_id: Option<Uuid>,
    /// Um resultado (ver `CallRecord.outcome`).
    pub outcome: Option<String>,
    /// Início (inclusive), RFC 3339.
    pub since: Option<DateTime<Utc>>,
    /// Fim (exclusivo), RFC 3339.
    pub until: Option<DateTime<Utc>>,
    /// Sala.
    pub room_code: Option<String>,
}

#[derive(Serialize, Deserialize, PartialEq)]
struct CdrCursor {
    at: DateTime<Utc>,
    id: Uuid,
    /// Impressão digital dos filtros: um cursor não serve para outra pesquisa.
    f: String,
}

/// Chamadas externas da org, da mais recente para a mais antiga. Filtros
/// exactos (`direction`, `trunk_id`, `outcome`, `since`/`until`,
/// `room_code`); a pesquisa livre do motor da ADR-0007 entra quando ele estiver
/// nesta linha (ver ADR-0009 §6).
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/telephony/call-records", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), CallRecordQuery),
    responses(
        (status = 200, body = CallRecordPage),
        (status = 400, body = crate::openapi::ErrorBody, description = "`page.invalid_token`, `search.page_token_mismatch`, `telephony.invalid_filter`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_call_records(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Query(q): Query<CallRecordQuery>,
) -> Result<Json<CallRecordPage>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let bad = |m: &str| -> ApiError {
        DomainError::invalid("telephony.invalid_filter", m.to_string()).into()
    };
    if let Some(d) = &q.direction {
        if !matches!(d.as_str(), "inbound" | "outbound") {
            return Err(bad("direction: inbound | outbound"));
        }
    }
    if let Some(o) = &q.outcome {
        if CallOutcome::parse(o).is_none() {
            return Err(bad("outcome desconhecido"));
        }
    }
    let fingerprint = format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        q.direction, q.trunk_id, q.outcome, q.since, q.until, q.room_code
    );
    let fingerprint = delonix_meet_core::crypto::sha256_hex(fingerprint);
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token.clone(),
    };
    let size = page.size();
    let cursor: Option<CdrCursor> = page.cursor()?;
    if cursor.as_ref().is_some_and(|c| c.f != fingerprint) {
        return Err(DomainError::invalid(
            "search.page_token_mismatch",
            "o page_token é de outra pesquisa",
        )
        .into());
    }
    let rows: Vec<CallRecord> = sqlx::query_as(
        "SELECT c.id, c.direction, c.from_number, c.to_number, c.trunk_id, t.name AS trunk_name,
                c.room_code, c.destination_label, c.outcome, c.hangup_cause, c.started_at,
                c.answered_at, c.ended_at, c.duration_secs, c.billsec, c.recorded, c.emergency,
                c.cost_reason, c.cost_e4, c.cost_currency
           FROM telephony_call_records c
           LEFT JOIN telephony_trunks t ON t.id = c.trunk_id AND t.org_id = c.org_id
          WHERE c.org_id = $1
            AND ($2::text IS NULL OR c.direction = $2)
            AND ($3::uuid IS NULL OR c.trunk_id = $3)
            AND ($4::text IS NULL OR c.outcome = $4)
            AND ($5::timestamptz IS NULL OR c.started_at >= $5)
            AND ($6::timestamptz IS NULL OR c.started_at < $6)
            AND ($7::text IS NULL OR c.room_code = $7)
            AND ($8::timestamptz IS NULL OR (c.started_at, c.id) < ($8, $9))
          ORDER BY c.started_at DESC, c.id DESC
          LIMIT $10",
    )
    .bind(org_id)
    .bind(&q.direction)
    .bind(q.trunk_id)
    .bind(&q.outcome)
    .bind(q.since)
    .bind(q.until)
    .bind(&q.room_code)
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |r| CdrCursor {
        at: r.started_at,
        id: r.id,
        f: fingerprint.clone(),
    });
    let items = p
        .items
        .into_iter()
        .map(|mut r| {
            r.from_number = mask_number(&state, &r.from_number);
            r.to_number = mask_number(&state, &r.to_number);
            r.cost = match (r.cost_e4, r.cost_currency.as_deref()) {
                (Some(e4), Some(cur)) => Currency::parse(cur)
                    .ok()
                    .map(|c| MoneyDto::from(Money::new(e4, c))),
                _ => None,
            };
            r
        })
        .collect();
    Ok(Json(CallRecordPage {
        items,
        next_page_token: p.next_page_token,
    }))
}

// ============================================================
//  Consumo do mês
// ============================================================

#[derive(Serialize, utoipa::ToSchema)]
pub struct UsageByTrunk {
    pub trunk_id: Option<Uuid>,
    /// `null` para chamadas sem tronco (ex.: sem preço/tronco).
    pub trunk_name: Option<String>,
    pub calls: i64,
    pub minutes: i64,
    /// Na moeda do tronco, por moeda.
    pub cost: Vec<MoneyDto>,
    /// Convertido para Kz com a taxa em vigor em cada chamada; `null` se falta
    /// uma taxa.
    pub cost_aoa: Option<MoneyDto>,
    /// Percentagem do total em Kz (0-100); `null` sem total em Kz.
    pub share_pct: Option<f64>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct Usage {
    /// `AAAA-MM`.
    pub month: String,
    /// Fuso usado para os limites do mês.
    pub timezone: String,
    pub calls: i64,
    /// Minutos taxados (por começo de minuto) das chamadas atendidas de saída.
    pub minutes: i64,
    pub totals: Vec<MoneyDto>,
    /// Total em Kz; `null` quando falta uma taxa de câmbio (`total_aoa_reason`).
    pub total_aoa: Option<MoneyDto>,
    /// `missing_exchange_rate`.
    pub total_aoa_reason: Option<String>,
    /// Chamadas de saída atendidas sem custo (sem preço em vigor), que NÃO
    /// entram nos totais.
    pub unpriced_calls: i64,
    pub by_trunk: Vec<UsageByTrunk>,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct UsageQuery {
    /// `AAAA-MM`; omissão: o mês corrente no fuso.
    pub month: Option<String>,
}

const USAGE_TZ: &str = "Africa/Luanda";

/// Consumo de um mês a partir dos CDRs de saída, ao preço em vigor em cada
/// chamada (congelado na ingestão).
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/telephony/usage", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), UsageQuery),
    responses(
        (status = 200, body = Usage),
        (status = 400, body = crate::openapi::ErrorBody, description = "`telephony.invalid_month`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn usage(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Query(q): Query<UsageQuery>,
) -> Result<Json<Usage>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let month = match q.month.as_deref() {
        None => {
            sqlx::query_scalar::<_, String>("SELECT to_char(now() AT TIME ZONE $1, 'YYYY-MM')")
                .bind(USAGE_TZ)
                .fetch_one(&state.db)
                .await?
        }
        Some(m) => {
            let ok = m.len() == 7
                && m.as_bytes()[4] == b'-'
                && m[..4].bytes().all(|b| b.is_ascii_digit())
                && m[5..].parse::<u32>().is_ok_and(|mm| (1..=12).contains(&mm));
            if !ok {
                return Err(
                    DomainError::invalid("telephony.invalid_month", "month: AAAA-MM").into(),
                );
            }
            m.to_string()
        }
    };
    #[derive(sqlx::FromRow)]
    struct Line {
        trunk_id: Option<Uuid>,
        started_at: DateTime<Utc>,
        billsec: i32,
        cost_e4: Option<i64>,
        cost_currency: Option<String>,
    }
    let lines: Vec<Line> = sqlx::query_as(
        "SELECT trunk_id, started_at, billsec, cost_e4, cost_currency
           FROM telephony_call_records
          WHERE org_id = $1 AND direction = 'outbound' AND outcome = 'answered'
            AND started_at >= (($2 || '-01')::date::timestamp AT TIME ZONE $3)
            AND started_at <  ((($2 || '-01')::date + interval '1 month')::timestamp AT TIME ZONE $3)",
    )
    .bind(org_id)
    .bind(&month)
    .bind(USAGE_TZ)
    .fetch_all(&state.db)
    .await?;
    let rates: Vec<(DateTime<Utc>, i64)> = sqlx::query_as(
        "SELECT valid_from, aoa_per_unit_e6 FROM telephony_exchange_rates
          WHERE org_id = $1 AND currency = 'USD' ORDER BY valid_from",
    )
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    let rate_at = |at: DateTime<Utc>| rates.iter().rev().find(|(v, _)| *v <= at).map(|(_, r)| *r);
    let names: std::collections::HashMap<Uuid, String> = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT id, name FROM telephony_trunks WHERE org_id = $1",
    )
    .bind(org_id)
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .collect();

    struct Acc {
        calls: i64,
        minutes: i64,
        cost: Vec<Money>,
        aoa: Option<i64>,
    }
    let mut per: Vec<(Option<Uuid>, Acc)> = Vec::new();
    let (mut calls, mut minutes, mut unpriced) = (0i64, 0i64, 0i64);
    let mut total_aoa: Option<i64> = Some(0);
    for l in &lines {
        calls += 1;
        let mins = billed_minutes(l.billsec as i64);
        minutes += mins;
        let idx = match per.iter().position(|(t, _)| *t == l.trunk_id) {
            Some(i) => i,
            None => {
                per.push((
                    l.trunk_id,
                    Acc {
                        calls: 0,
                        minutes: 0,
                        cost: Vec::new(),
                        aoa: Some(0),
                    },
                ));
                per.len() - 1
            }
        };
        let acc = &mut per[idx].1;
        acc.calls += 1;
        acc.minutes += mins;
        let money = match (l.cost_e4, l.cost_currency.as_deref().map(Currency::parse)) {
            (Some(e4), Some(Ok(c))) => Money::new(e4, c),
            _ => {
                unpriced += 1;
                continue;
            }
        };
        acc.cost.push(money);
        match to_aoa(money, rate_at(l.started_at)) {
            Some(k) => {
                acc.aoa = acc.aoa.map(|a| a + k.amount_e4);
                total_aoa = total_aoa.map(|a| a + k.amount_e4);
            }
            None => {
                acc.aoa = None;
                total_aoa = None;
            }
        }
    }
    let all_costs: Vec<Money> = per.iter().flat_map(|(_, a)| a.cost.clone()).collect();
    let totals = delonix_meet_domain::telephony::cost::sum_by_currency(all_costs);
    let by_trunk = per
        .into_iter()
        .map(|(t, a)| UsageByTrunk {
            trunk_id: t,
            trunk_name: t.and_then(|id| names.get(&id).cloned()),
            calls: a.calls,
            minutes: a.minutes,
            cost: delonix_meet_domain::telephony::cost::sum_by_currency(a.cost)
                .into_iter()
                .map(Into::into)
                .collect(),
            cost_aoa: a.aoa.map(|v| Money::new(v, Currency::Aoa).into()),
            share_pct: match (a.aoa, total_aoa) {
                (Some(v), Some(tot)) if tot > 0 => {
                    Some(((v as f64 / tot as f64) * 1000.0).round() / 10.0)
                }
                _ => None,
            },
        })
        .collect();
    Ok(Json(Usage {
        month,
        timezone: USAGE_TZ.into(),
        calls,
        minutes,
        totals: totals.into_iter().map(Into::into).collect(),
        total_aoa: total_aoa.map(|v| Money::new(v, Currency::Aoa).into()),
        total_aoa_reason: total_aoa.is_none().then(|| "missing_exchange_rate".into()),
        unpriced_calls: unpriced,
        by_trunk,
    }))
}
