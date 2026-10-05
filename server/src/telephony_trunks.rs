//! Operadoras ligadas — troncos SIP por organização (ADR-0009 §2).
//!
//! - `GET    /api/orgs/{org_id}/telephony/trunks`                       lista pela ordem, com estado MEDIDO
//! - `POST   /api/orgs/{org_id}/telephony/trunks`                       `201` + `Location`
//! - `GET    /api/orgs/{org_id}/telephony/trunks/{trunk_id}`            um tronco, com estado
//! - `PATCH  /api/orgs/{org_id}/telephony/trunks/{trunk_id}`            alteração parcial (password write-only)
//! - `DELETE /api/orgs/{org_id}/telephony/trunks/{trunk_id}`            `204`; `409` se o plano o usa
//! - `PUT    /api/orgs/{org_id}/telephony/trunk-order`                  a ordem inteira («arraste para mudar»)
//! - `GET    /api/orgs/{org_id}/telephony/trunks/{trunk_id}/prices`     histórico de preços
//! - `POST   /api/orgs/{org_id}/telephony/trunks/{trunk_id}/prices`     preço novo (nunca retroactivo)
//! - `GET    /api/orgs/{org_id}/telephony/exchange-rates`               histórico das taxas
//! - `POST   /api/orgs/{org_id}/telephony/exchange-rates`               taxa nova (nunca retroactiva)
//!
//! Só administradores da org (capacidade futura: `telephony.manage`). Um id de
//! outra org e um inexistente dão a mesma resposta (`404`).

use std::{collections::HashMap, sync::Arc};

use axum::{
    extract::{Path, Query, State},
    http::{header::LOCATION, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Duration, Utc};
use delonix_meet_core::{
    page::{Page, PageRequest},
    DomainError,
};
use delonix_meet_domain::telephony::{
    cost::{asr, price_at, trunk_health, TrunkState},
    money::{format_e6, parse_amount_e4, parse_rate_e6, Currency, Money},
    ports::{gateway_name, SipSnapshot},
    trunk as rules,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::ApiError,
    telephony_service::{prices_by_trunk, MoneyDto},
    AppState,
};

// ============================================================
//  Tipos
// ============================================================

#[derive(Debug, Clone, sqlx::FromRow)]
struct TrunkRecord {
    id: Uuid,
    name: String,
    short_code: String,
    scope: String,
    host: String,
    port: i32,
    transport: String,
    srtp: String,
    register: bool,
    username: String,
    password_configured: bool,
    prefixes: Vec<String>,
    max_channels: i32,
    position: i32,
    enabled: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

const COLUMNS: &str = "id, name, short_code, scope, host, port, transport, srtp, register, username, \
     (password_sealed <> '') AS password_configured, prefixes, max_channels, position, enabled, created_at, updated_at";

/// Estado de um tronco, MEDIDO — nada aqui é declarado pelo cliente.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct TrunkStatus {
    /// `up` | `degraded` | `down` | `unknown`.
    pub state: String,
    /// Porquê, legível por máquina: `sip_not_configured`, `registration_failed`,
    /// `options_ping_failed`, `low_asr`, `registering`, `no_measurement`,
    /// `not_loaded_on_media_server`, `sip_status_unavailable`, `disabled`.
    pub reasons: Vec<String>,
    /// Registo no media server: `registered` | `trying` | `failed` |
    /// `not_required` | `unknown`; `null` sem medida.
    pub registration: Option<String>,
    /// Canais em uso agora (contador do FreeSWITCH); `null` sem medida.
    pub channels_in_use: Option<u32>,
    pub channels_max: i32,
    /// Answer-seizure ratio em `[0,1]` nas últimas `asr_window_hours`, das
    /// chamadas de saída ingeridas. `null` sem tentativas (`asr_reason`).
    pub asr: Option<f64>,
    pub asr_attempts: i64,
    pub asr_answered: i64,
    pub asr_window_hours: i64,
    pub asr_reason: Option<String>,
    pub measured_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct Trunk {
    pub id: Uuid,
    pub name: String,
    pub short_code: String,
    /// `national` | `international`.
    pub scope: String,
    pub host: String,
    pub port: i32,
    /// `udp` | `tcp` | `tls`.
    pub transport: String,
    /// `mandatory` | `optional` | `off`.
    pub srtp: String,
    pub register: bool,
    pub username: String,
    /// A password nunca sai; só se diz se existe.
    pub password_configured: bool,
    pub prefixes: Vec<String>,
    pub max_channels: i32,
    /// Posição de encaminhamento (0 = primeira).
    pub position: i32,
    /// `primary` | `reserve` | `international` — derivado da ordem.
    pub role: String,
    /// Para `reserve`: 1, 2, …
    pub reserve_rank: Option<usize>,
    pub enabled: bool,
    /// Preço por minuto em vigor agora; `null` sem preço.
    pub current_price_per_min: Option<MoneyDto>,
    /// Nome do gateway no FreeSWITCH.
    pub gateway_name: String,
    pub status: TrunkStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct TrunkPage {
    pub items: Vec<Trunk>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct PriceInput {
    /// Decimal, até 4 casas (`"9.40"`).
    pub amount: String,
    /// `AOA` | `USD`.
    pub currency: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateTrunkReq {
    pub name: String,
    pub short_code: String,
    /// `national` (omissão) | `international`.
    #[serde(default = "national")]
    pub scope: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: i32,
    /// `tls` (omissão) | `tcp` | `udp`.
    #[serde(default = "tls")]
    pub transport: String,
    /// `mandatory` (omissão) | `optional` | `off`. Diferente de `off` exige `tls`.
    #[serde(default = "mandatory")]
    pub srtp: String,
    #[serde(default = "yes")]
    pub register: bool,
    #[serde(default)]
    pub username: String,
    /// Write-only: cifrada em repouso e nunca devolvida.
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub prefixes: Vec<String>,
    pub max_channels: i32,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Preço inicial, em vigor a partir de agora.
    #[serde(default)]
    pub price_per_min: Option<PriceInput>,
}
fn national() -> String {
    "national".into()
}
fn default_port() -> i32 {
    5061
}
fn tls() -> String {
    "tls".into()
}
fn mandatory() -> String {
    "mandatory".into()
}
fn yes() -> bool {
    true
}

#[derive(Deserialize, Default, utoipa::ToSchema)]
pub struct UpdateTrunkReq {
    pub name: Option<String>,
    pub short_code: Option<String>,
    pub scope: Option<String>,
    pub host: Option<String>,
    pub port: Option<i32>,
    pub transport: Option<String>,
    pub srtp: Option<String>,
    pub register: Option<bool>,
    pub username: Option<String>,
    /// `""` apaga a password; ausente não mexe.
    pub password: Option<String>,
    pub prefixes: Option<Vec<String>>,
    pub max_channels: Option<i32>,
    pub enabled: Option<bool>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct TrunkOrderReq {
    /// TODOS os troncos da org, pela ordem nova.
    pub trunk_ids: Vec<Uuid>,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TrunkListQuery {
    /// 1-100, omissão 50.
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
    /// Janela do ASR em horas (1-720, omissão 24).
    pub asr_window_hours: Option<i64>,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct StatusQuery {
    /// Janela do ASR em horas (1-720, omissão 24).
    pub asr_window_hours: Option<i64>,
}

#[derive(Serialize, Deserialize)]
struct Cursor {
    position: i32,
    id: Uuid,
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        list,
        create,
        get_one,
        update,
        delete,
        put_order,
        list_prices,
        create_price,
        list_rates,
        create_rate
    ),
    components(schemas(
        Trunk,
        TrunkStatus,
        TrunkPage,
        CreateTrunkReq,
        UpdateTrunkReq,
        TrunkOrderReq,
        PriceInput,
        MoneyDto,
        Price,
        PricePage,
        CreatePriceReq,
        ExchangeRate,
        ExchangeRatePage,
        CreateRateReq
    ))
)]
pub struct ApiDoc;

// ============================================================
//  Estado medido
// ============================================================

fn window_hours(v: Option<i64>) -> Result<i64, ApiError> {
    match v {
        None => Ok(24),
        Some(h) if (1..=720).contains(&h) => Ok(h),
        Some(_) => Err(DomainError::invalid(
            "telephony.invalid_window",
            "asr_window_hours entre 1 e 720",
        )
        .into()),
    }
}

/// Tentativas e atendidas por tronco, na janela, das chamadas de SAÍDA.
async fn asr_counts(
    state: &AppState,
    org_id: Uuid,
    hours: i64,
) -> Result<HashMap<Uuid, (i64, i64)>, ApiError> {
    let rows: Vec<(Uuid, i64, i64)> = sqlx::query_as(
        "SELECT trunk_id, COUNT(*)::bigint, COUNT(*) FILTER (WHERE outcome = 'answered')::bigint
           FROM telephony_call_records
          WHERE org_id = $1 AND direction = 'outbound' AND trunk_id IS NOT NULL
            AND started_at >= now() - make_interval(hours => $2::int)
          GROUP BY trunk_id",
    )
    .bind(org_id)
    .bind(hours as i32)
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(|(t, a, b)| (t, (a, b))).collect())
}

pub(crate) async fn snapshot_for(
    state: &AppState,
    trunk_ids: &[Uuid],
) -> (Option<SipSnapshot>, Option<&'static str>) {
    let Some(sip) = &state.telephony.sip else {
        return (None, Some("sip_not_configured"));
    };
    let names: Vec<String> = trunk_ids.iter().map(|t| gateway_name(*t)).collect();
    match sip.snapshot(&names).await {
        Ok(s) if s.media.is_some() => (Some(s), None),
        Ok(s) => {
            tracing::warn!(error = ?s.media_error, "estado SIP sem media server");
            (Some(s), Some("sip_status_unavailable"))
        }
        Err(e) => {
            tracing::warn!("estado SIP indisponível: {e}");
            (None, Some("sip_status_unavailable"))
        }
    }
}

async fn assemble(
    state: &AppState,
    org_id: Uuid,
    records: Vec<TrunkRecord>,
    hours: i64,
) -> Result<Vec<Trunk>, ApiError> {
    let ids: Vec<Uuid> = records.iter().map(|r| r.id).collect();
    let (snap, snap_reason) = snapshot_for(state, &ids).await;
    let counts = asr_counts(state, org_id, hours).await?;
    let prices = prices_by_trunk(state, org_id).await?;
    // O papel deriva da ordem de TODOS os troncos nacionais da org, não só
    // da página.
    let national_order: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM telephony_trunks WHERE org_id = $1 AND scope = 'national' ORDER BY position, id",
    )
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    let now = Utc::now();
    Ok(records
        .into_iter()
        .map(|r| {
            let gw = snap
                .as_ref()
                .filter(|_| snap_reason.is_none())
                .and_then(|s| s.gateways.iter().find(|g| g.name == gateway_name(r.id)));
            let (attempts, answered) = counts.get(&r.id).copied().unwrap_or((0, 0));
            let health = trunk_health(
                r.enabled,
                gw.map(|g| g.registration),
                gw.and_then(|g| g.up),
                attempts,
                answered,
            );
            let mut reasons: Vec<String> = Vec::new();
            if let Some(why) = snap_reason {
                if r.enabled {
                    reasons.push(why.to_string());
                }
            }
            for why in health.reasons {
                if !(why == "sip_status_unavailable" && snap_reason.is_some())
                    && !(why == "no_measurement" && snap_reason.is_some())
                {
                    reasons.push(why.to_string());
                }
            }
            let rank = national_order.iter().position(|x| *x == r.id).unwrap_or(0);
            let (role, reserve_rank) = rules::role(&r.scope, rank);
            let current = prices
                .get(&r.id)
                .and_then(|p| price_at(p, now))
                .map(|p| MoneyDto::from(p.price_per_min));
            Trunk {
                gateway_name: gateway_name(r.id),
                status: TrunkStatus {
                    state: match health.state {
                        TrunkState::Up => "up",
                        TrunkState::Degraded => "degraded",
                        TrunkState::Down => "down",
                        TrunkState::Unknown => "unknown",
                    }
                    .into(),
                    reasons,
                    registration: gw.map(|g| {
                        serde_json::to_value(g.registration)
                            .ok()
                            .and_then(|v| v.as_str().map(str::to_string))
                            .unwrap_or_default()
                    }),
                    channels_in_use: gw.and_then(|g| g.channels_in_use),
                    channels_max: r.max_channels,
                    asr: asr(attempts, answered),
                    asr_attempts: attempts,
                    asr_answered: answered,
                    asr_window_hours: hours,
                    asr_reason: (attempts == 0).then(|| "no_calls_in_window".to_string()),
                    measured_at: snap.as_ref().map(|s| s.measured_at).unwrap_or(now),
                },
                id: r.id,
                name: r.name,
                short_code: r.short_code,
                scope: r.scope,
                host: r.host,
                port: r.port,
                transport: r.transport,
                srtp: r.srtp,
                register: r.register,
                username: r.username,
                password_configured: r.password_configured,
                prefixes: r.prefixes,
                max_channels: r.max_channels,
                position: r.position,
                role: role.into(),
                reserve_rank,
                enabled: r.enabled,
                current_price_per_min: current,
                created_at: r.created_at,
                updated_at: r.updated_at,
            }
        })
        .collect())
}

// ============================================================
//  Handlers
// ============================================================

/// Operadoras da org pela ordem de encaminhamento, com estado medido.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/telephony/trunks", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), TrunkListQuery),
    responses(
        (status = 200, body = TrunkPage),
        (status = 400, body = crate::openapi::ErrorBody, description = "page_token ou janela inválidos"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "Membro sem papel de admin."),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Query(q): Query<TrunkListQuery>,
) -> Result<Json<TrunkPage>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let hours = window_hours(q.asr_window_hours)?;
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<Cursor> = page.cursor()?;
    let rows: Vec<TrunkRecord> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM telephony_trunks
          WHERE org_id = $1 AND ($2::int IS NULL OR (position, id) > ($2, $3))
          ORDER BY position, id LIMIT $4"
    ))
    .bind(org_id)
    .bind(cursor.as_ref().map(|c| c.position))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |t| Cursor {
        position: t.position,
        id: t.id,
    });
    let items = assemble(&state, org_id, p.items, hours).await?;
    Ok(Json(TrunkPage {
        items,
        next_page_token: p.next_page_token,
    }))
}

async fn check_host(state: &AppState, host: &str, port: i32) -> Result<(), ApiError> {
    // O FreeSWITCH vai ligar a este host em nome do inquilino: a mesma guarda
    // de um URL escrito pelo cliente (sem destinos internos, salvo
    // OUTBOUND_ALLOW_HOSTS). Um nome que ainda não resolve é aceite.
    let url = if host.contains(':') && !host.starts_with('[') {
        format!("https://[{host}]:{port}/")
    } else {
        format!("https://{host}:{port}/")
    };
    state
        .outbound
        .check_tenant_config_url(&url)
        .await
        .map(|_| ())
        .map_err(|e| {
            DomainError::invalid("telephony.trunk_host_refused", e.to_string())
                .with_field("host", "destino interno ou inválido")
                .into()
        })
}

fn seal_password(state: &AppState, id: &Uuid, plain: &str) -> Result<String, ApiError> {
    if plain.is_empty() {
        return Ok(String::new());
    }
    crate::secrets_at_rest::seal(&state.config, plain, &rules::password_aad(id))
}

fn parse_price(p: &PriceInput) -> Result<Money, ApiError> {
    Ok(Money::new(
        parse_amount_e4(&p.amount)?,
        Currency::parse(&p.currency)?,
    ))
}

fn unique_name(e: sqlx::Error) -> ApiError {
    match e {
        sqlx::Error::Database(d) if d.is_unique_violation() => DomainError::conflict(
            "telephony.trunk_name_taken",
            "já existe uma operadora com esse nome nesta organização",
        )
        .into(),
        other => other.into(),
    }
}

/// Cria uma operadora no FIM da ordem.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/telephony/trunks", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = CreateTrunkReq,
    responses(
        (status = 201, body = Trunk, headers(("Location" = String))),
        (status = 400, body = crate::openapi::ErrorBody, description = "Forma inválida (`telephony.invalid_*`, `telephony.srtp_requires_tls`, `telephony.trunk_host_refused`)."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "`telephony.trunk_name_taken`"),
        (status = 422, body = crate::openapi::ErrorBody, description = "`secrets.encryption_unconfigured`: sem chaves não se guarda a password. `telephony.trunk_limit_reached`: a organização já tem o máximo de troncos."),
    )
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateTrunkReq>,
) -> Result<Response, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let name = rules::validate_name(&req.name)?;
    let short_code = rules::validate_short_code(&req.short_code)?;
    let scope = rules::validate_scope(&req.scope)?;
    let host = rules::validate_host(&req.host)?;
    let port = rules::validate_port(req.port)?;
    let transport = rules::validate_transport(&req.transport)?;
    let srtp = rules::validate_srtp(&req.srtp)?;
    rules::validate_security(transport, srtp)?;
    let prefixes = rules::validate_prefixes(&req.prefixes)?;
    let max_channels = rules::validate_max_channels(req.max_channels)?;
    rules::validate_credentials(&req.username, req.password.as_deref())?;
    let price = req.price_per_min.as_ref().map(parse_price).transpose()?;
    check_host(&state, &host, port).await?;
    let id = Uuid::new_v4();
    let sealed = seal_password(&state, &id, req.password.as_deref().unwrap_or(""))?;

    let mut tx = state.db.begin().await?;
    // Serializa as inserções de posição da org.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('telephony_trunks:' || $1::text))")
        .bind(org_id)
        .execute(&mut *tx)
        .await?;
    // Os troncos de todas as organizações são servidos ao FreeSWITCH num só
    // documento com limite de tamanho: sem tecto, uma organização deixava as
    // outras sem troncos. Dentro do cadeado, para duas criações não passarem.
    let existing: i64 =
        sqlx::query_scalar("SELECT count(*) FROM telephony_trunks WHERE org_id = $1")
            .bind(org_id)
            .fetch_one(&mut *tx)
            .await?;
    if existing >= rules::MAX_TRUNKS_PER_ORG {
        return Err(DomainError::precondition(
            "telephony.trunk_limit_reached",
            format!(
                "uma organização tem no máximo {} troncos",
                rules::MAX_TRUNKS_PER_ORG
            ),
        )
        .into());
    }
    let next: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(position) + 1, 0) FROM telephony_trunks WHERE org_id = $1",
    )
    .bind(org_id)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO telephony_trunks
            (id, org_id, name, short_code, scope, host, port, transport, srtp, register, username,
             password_sealed, prefixes, max_channels, position, enabled, created_by)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)",
    )
    .bind(id)
    .bind(org_id)
    .bind(&name)
    .bind(&short_code)
    .bind(scope)
    .bind(&host)
    .bind(port)
    .bind(transport)
    .bind(srtp)
    .bind(req.register)
    .bind(req.username.trim())
    .bind(&sealed)
    .bind(&prefixes)
    .bind(max_channels)
    .bind(next)
    .bind(req.enabled)
    .bind(auth.user_id)
    .execute(&mut *tx)
    .await
    .map_err(unique_name)?;
    if let Some(p) = price {
        insert_price(&mut tx, org_id, id, p, Utc::now(), auth.user_id).await?;
    }
    tx.commit().await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "telephony.trunk.created",
        &format!("{id} name={name} host={host}:{port} transport={transport} srtp={srtp} max_channels={max_channels}"),
    )
    .await;
    refresh_gateway(&state, id);
    let trunk = fetch_one(&state, org_id, id, 24).await?;
    Ok((
        StatusCode::CREATED,
        [(
            LOCATION,
            format!("/api/orgs/{org_id}/telephony/trunks/{id}"),
        )],
        Json(trunk),
    )
        .into_response())
}

async fn fetch_record(state: &AppState, org_id: Uuid, id: Uuid) -> Result<TrunkRecord, ApiError> {
    sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM telephony_trunks WHERE id = $1 AND org_id = $2"
    ))
    .bind(id)
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)
}

async fn fetch_one(
    state: &AppState,
    org_id: Uuid,
    id: Uuid,
    hours: i64,
) -> Result<Trunk, ApiError> {
    let r = fetch_record(state, org_id, id).await?;
    assemble(state, org_id, vec![r], hours)
        .await?
        .pop()
        .ok_or(ApiError::NotFound)
}

/// Uma linha do histórico de preços de um tronco, como o SQL a devolve.
type LinhaPreco = (Uuid, DateTime<Utc>, i64, String, DateTime<Utc>);
/// Uma linha do histórico de câmbios, como o SQL a devolve.
type LinhaCambio = (Uuid, String, i64, DateTime<Utc>, DateTime<Utc>);

/// Uma operadora, com estado medido.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/telephony/trunks/{trunk_id}", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("trunk_id" = Uuid, Path), StatusQuery),
    responses(
        (status = 200, body = Trunk),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_one(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, trunk_id)): Path<(Uuid, Uuid)>,
    Query(q): Query<StatusQuery>,
) -> Result<Json<Trunk>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let hours = window_hours(q.asr_window_hours)?;
    Ok(Json(fetch_one(&state, org_id, trunk_id, hours).await?))
}

/// Alteração parcial. A password é write-only (`""` apaga-a).
#[utoipa::path(
    patch, path = "/api/orgs/{org_id}/telephony/trunks/{trunk_id}", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("trunk_id" = Uuid, Path)),
    request_body = UpdateTrunkReq,
    responses(
        (status = 200, body = Trunk),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody),
        (status = 422, body = crate::openapi::ErrorBody),
    )
)]
pub async fn update(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, trunk_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateTrunkReq>,
) -> Result<Json<Trunk>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let cur = fetch_record(&state, org_id, trunk_id).await?;
    // Valida TUDO antes de escrever.
    let name = req.name.as_deref().map(rules::validate_name).transpose()?;
    let short_code = req
        .short_code
        .as_deref()
        .map(rules::validate_short_code)
        .transpose()?;
    let scope = req
        .scope
        .as_deref()
        .map(rules::validate_scope)
        .transpose()?;
    let host = req.host.as_deref().map(rules::validate_host).transpose()?;
    let port = req.port.map(rules::validate_port).transpose()?;
    let transport = req
        .transport
        .as_deref()
        .map(rules::validate_transport)
        .transpose()?;
    let srtp = req.srtp.as_deref().map(rules::validate_srtp).transpose()?;
    rules::validate_security(
        transport.unwrap_or(&cur.transport),
        srtp.unwrap_or(&cur.srtp),
    )?;
    let prefixes = req
        .prefixes
        .as_deref()
        .map(rules::validate_prefixes)
        .transpose()?;
    let max_channels = req
        .max_channels
        .map(rules::validate_max_channels)
        .transpose()?;
    rules::validate_credentials(
        req.username.as_deref().unwrap_or(""),
        req.password.as_deref(),
    )?;
    if host.is_some() || port.is_some() {
        check_host(
            &state,
            host.as_deref().unwrap_or(&cur.host),
            port.unwrap_or(cur.port),
        )
        .await?;
    }
    let sealed = req
        .password
        .as_deref()
        .map(|p| seal_password(&state, &trunk_id, p))
        .transpose()?;
    sqlx::query(
        "UPDATE telephony_trunks SET
            name = COALESCE($3, name), short_code = COALESCE($4, short_code),
            scope = COALESCE($5, scope), host = COALESCE($6, host), port = COALESCE($7, port),
            transport = COALESCE($8, transport), srtp = COALESCE($9, srtp),
            register = COALESCE($10, register), username = COALESCE($11, username),
            password_sealed = COALESCE($12, password_sealed), prefixes = COALESCE($13, prefixes),
            max_channels = COALESCE($14, max_channels), enabled = COALESCE($15, enabled),
            updated_at = now()
          WHERE id = $1 AND org_id = $2",
    )
    .bind(trunk_id)
    .bind(org_id)
    .bind(&name)
    .bind(&short_code)
    .bind(scope)
    .bind(&host)
    .bind(port)
    .bind(transport)
    .bind(srtp)
    .bind(req.register)
    .bind(req.username.as_deref().map(str::trim))
    .bind(&sealed)
    .bind(&prefixes)
    .bind(max_channels)
    .bind(req.enabled)
    .execute(&state.db)
    .await
    .map_err(unique_name)?;
    // Diff para a auditoria: nomes dos campos e valores não secretos.
    let mut diff = Vec::new();
    let mut push = |k: &str, old: String, new: Option<String>| {
        if let Some(n) = new {
            if n != old {
                diff.push(format!("{k}: {old} → {n}"));
            }
        }
    };
    push("name", cur.name.clone(), name);
    push("short_code", cur.short_code.clone(), short_code);
    push("scope", cur.scope.clone(), scope.map(str::to_string));
    push("host", cur.host.clone(), host);
    push("port", cur.port.to_string(), port.map(|p| p.to_string()));
    push(
        "transport",
        cur.transport.clone(),
        transport.map(str::to_string),
    );
    push("srtp", cur.srtp.clone(), srtp.map(str::to_string));
    push(
        "register",
        cur.register.to_string(),
        req.register.map(|v| v.to_string()),
    );
    push("username", cur.username.clone(), req.username.clone());
    push(
        "prefixes",
        cur.prefixes.join(","),
        prefixes.map(|p| p.join(",")),
    );
    push(
        "max_channels",
        cur.max_channels.to_string(),
        max_channels.map(|v| v.to_string()),
    );
    push(
        "enabled",
        cur.enabled.to_string(),
        req.enabled.map(|v| v.to_string()),
    );
    if req.password.is_some() {
        diff.push("password: (alterada)".into());
    }
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "telephony.trunk.updated",
        &format!("{trunk_id} {}", diff.join("; ")),
    )
    .await;
    // Só o que MUDOU, e só o que muda a LIGAÇÃO à operadora: mudar o nome ou os
    // prefixos não deita abaixo um registo, e um PATCH que repete o valor que
    // já lá estava também não. A password conta sempre que vem: não se compara.
    const CONNECTION: [&str; 8] = [
        "host: ",
        "port: ",
        "transport: ",
        "srtp: ",
        "register: ",
        "username: ",
        "enabled: ",
        "password: ",
    ];
    if diff
        .iter()
        .any(|d| CONNECTION.iter().any(|f| d.starts_with(f)))
    {
        refresh_gateway(&state, trunk_id);
    }
    Ok(Json(fetch_one(&state, org_id, trunk_id, 24).await?))
}

/// Diz ao FreeSWITCH que este tronco mudou: tira o gateway que ele tem e
/// manda-o reler os troncos (`killgw` + `rescan`, pelo ESL). Sem isto um
/// tronco alterado continuava a registar-se com os dados antigos, e um
/// apagado continuava registado na operadora, até alguém reiniciar o
/// FreeSWITCH (R300).
///
/// Não espera pela resposta nem falha o pedido: a base é a verdade, e o
/// FreeSWITCH volta a ler os troncos sozinho de minuto a minuto. Sem ESL
/// configurado não faz nada — fica só esse ciclo, que não tira gateways.
///
/// Os avisos vão para uma fila com UM trabalhador (`GatewayRefresh`): uma
/// ligação ao ESL de cada vez e um `rescan` por intervalo, por muitos pedidos
/// que cheguem.
fn refresh_gateway(state: &AppState, trunk_id: Uuid) {
    use crate::telephony_service::GatewayRefresh;
    use delonix_meet_domain::telephony::ports::PortError;
    let Some(sip) = state.telephony.sip.clone() else {
        return;
    };
    let queue = state.telephony.gateway_refresh.clone();
    if !queue.enqueue(gateway_name(trunk_id)) {
        return; // já há um trabalhador: leva este no lote seguinte
    }
    tokio::spawn(async move {
        while let Some(batch) = queue.next_batch() {
            match sip.restart_registration(&batch).await {
                Ok(()) | Err(PortError::NotConfigured(_)) => {}
                Err(e) => tracing::warn!(
                    gateways = batch.len(), error = %e,
                    "o FreeSWITCH não foi avisado da alteração de troncos — ficam com os dados antigos até reler ou reiniciar"
                ),
            }
            tokio::time::sleep(GatewayRefresh::MIN_INTERVAL).await;
        }
    });
}

/// Apaga uma operadora. `409 telephony.trunk_in_use` se o plano de marcação a usa.
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/telephony/trunks/{trunk_id}", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("trunk_id" = Uuid, Path)),
    responses(
        (status = 204, description = "Apagada."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "`telephony.trunk_in_use`"),
    )
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, trunk_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let r = sqlx::query("DELETE FROM telephony_trunks WHERE id = $1 AND org_id = $2")
        .bind(trunk_id)
        .bind(org_id)
        .execute(&state.db)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(d) if d.is_foreign_key_violation() => {
                ApiError::from(DomainError::conflict(
                    "telephony.trunk_in_use",
                    "o plano de marcação usa esta operadora — tira-a das regras primeiro",
                ))
            }
            other => other.into(),
        })?;
    if r.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "telephony.trunk.deleted",
        &trunk_id.to_string(),
    )
    .await;
    refresh_gateway(&state, trunk_id);
    Ok(StatusCode::NO_CONTENT)
}

/// A ordem de encaminhamento INTEIRA («arraste para mudar»). Devolve os
/// troncos pela ordem nova.
#[utoipa::path(
    put, path = "/api/orgs/{org_id}/telephony/trunk-order", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = TrunkOrderReq,
    responses(
        (status = 200, body = TrunkPage),
        (status = 400, body = crate::openapi::ErrorBody, description = "`telephony.invalid_trunk_order`: falta, sobra ou repete um tronco."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn put_order(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<TrunkOrderReq>,
) -> Result<Json<TrunkPage>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('telephony_trunks:' || $1::text))")
        .bind(org_id)
        .execute(&mut *tx)
        .await?;
    let existing: Vec<(Uuid,)> =
        sqlx::query_as("SELECT id FROM telephony_trunks WHERE org_id = $1 ORDER BY position, id")
            .bind(org_id)
            .fetch_all(&mut *tx)
            .await?;
    let existing: Vec<Uuid> = existing.into_iter().map(|(i,)| i).collect();
    rules::validate_order(&req.trunk_ids, &existing)?;
    for (pos, id) in req.trunk_ids.iter().enumerate() {
        sqlx::query(
            "UPDATE telephony_trunks SET position = $3, updated_at = now() WHERE id = $1 AND org_id = $2",
        )
        .bind(id)
        .bind(org_id)
        .bind(pos as i32)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    let order: Vec<String> = req.trunk_ids.iter().map(Uuid::to_string).collect();
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "telephony.trunk_order.updated",
        &format!(
            "antes: {} → depois: {}",
            existing
                .iter()
                .map(Uuid::to_string)
                .collect::<Vec<_>>()
                .join(","),
            order.join(",")
        ),
    )
    .await;
    let rows: Vec<TrunkRecord> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM telephony_trunks WHERE org_id = $1 ORDER BY position, id LIMIT 100"
    ))
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(TrunkPage {
        items: assemble(&state, org_id, rows, 24).await?,
        next_page_token: None,
    }))
}

// ============================================================
//  Preços (histórico)
// ============================================================

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct Price {
    pub id: Uuid,
    pub trunk_id: Uuid,
    pub price_per_min: MoneyDto,
    pub valid_from: DateTime<Utc>,
    /// Se é o preço em vigor agora.
    pub in_force: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PricePage {
    pub items: Vec<Price>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreatePriceReq {
    pub price_per_min: PriceInput,
    /// Omissão: agora. Nunca no passado (um preço não reescreve chamadas já
    /// taxadas); tolerância de 60 s para relógios.
    #[serde(default)]
    pub valid_from: Option<DateTime<Utc>>,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct HistoryQuery {
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct HistCursor {
    at: DateTime<Utc>,
    id: Uuid,
}

async fn insert_price(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    trunk_id: Uuid,
    price: Money,
    valid_from: DateTime<Utc>,
    actor: Uuid,
) -> Result<Uuid, ApiError> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO telephony_trunk_prices (id, org_id, trunk_id, currency, price_per_min_e4, valid_from, created_by)
         VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(id)
    .bind(org_id)
    .bind(trunk_id)
    .bind(price.currency.as_str())
    .bind(price.amount_e4)
    .bind(valid_from)
    .bind(actor)
    .execute(&mut **tx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(d) if d.is_unique_violation() => ApiError::from(DomainError::conflict(
            "telephony.price_exists",
            "já há um preço com esse início",
        )),
        other => other.into(),
    })?;
    Ok(id)
}

fn not_backdated(valid_from: Option<DateTime<Utc>>) -> Result<DateTime<Utc>, ApiError> {
    let now = Utc::now();
    match valid_from {
        None => Ok(now),
        Some(v) if v >= now - Duration::seconds(60) => Ok(v),
        Some(_) => Err(DomainError::invalid(
            "telephony.price_backdated",
            "um preço ou taxa não pode começar no passado — as chamadas já taxadas não mudam",
        )
        .with_field("valid_from", "agora ou no futuro")
        .into()),
    }
}

/// Histórico de preços de uma operadora, do mais recente para o mais antigo.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/telephony/trunks/{trunk_id}/prices", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("trunk_id" = Uuid, Path), HistoryQuery),
    responses(
        (status = 200, body = PricePage),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_prices(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, trunk_id)): Path<(Uuid, Uuid)>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<PricePage>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    fetch_record(&state, org_id, trunk_id).await?;
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<HistCursor> = page.cursor()?;
    let rows: Vec<LinhaPreco> = sqlx::query_as(
        "SELECT id, valid_from, price_per_min_e4, currency, created_at FROM telephony_trunk_prices
          WHERE trunk_id = $1 AND org_id = $2
            AND ($3::timestamptz IS NULL OR (valid_from, id) < ($3, $4))
          ORDER BY valid_from DESC, id DESC LIMIT $5",
    )
    .bind(trunk_id)
    .bind(org_id)
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let in_force = crate::telephony_cdr::trunk_prices(&state, trunk_id)
        .await?
        .iter()
        .filter(|p| p.valid_from <= Utc::now())
        .max_by_key(|p| (p.valid_from, p.id))
        .map(|p| p.id);
    let p = Page::from_overfetch(rows, size, |r| HistCursor { at: r.1, id: r.0 });
    let items = p
        .items
        .into_iter()
        .map(|(id, valid_from, e4, cur, created_at)| {
            Ok(Price {
                id,
                trunk_id,
                price_per_min: Money::new(e4, Currency::parse(&cur)?).into(),
                valid_from,
                in_force: in_force == Some(id),
                created_at,
            })
        })
        .collect::<Result<Vec<_>, DomainError>>()?;
    Ok(Json(PricePage {
        items,
        next_page_token: p.next_page_token,
    }))
}

/// Preço novo (histórico: não substitui o anterior, sucede-lhe).
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/telephony/trunks/{trunk_id}/prices", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("trunk_id" = Uuid, Path)),
    request_body = CreatePriceReq,
    responses(
        (status = 201, body = Price, headers(("Location" = String))),
        (status = 400, body = crate::openapi::ErrorBody, description = "`telephony.invalid_amount`, `telephony.invalid_currency`, `telephony.price_backdated`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "`telephony.price_exists`"),
    )
)]
pub async fn create_price(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, trunk_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<CreatePriceReq>,
) -> Result<Response, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    fetch_record(&state, org_id, trunk_id).await?;
    let price = parse_price(&req.price_per_min)?;
    let valid_from = not_backdated(req.valid_from)?;
    let before = crate::telephony_cdr::trunk_prices(&state, trunk_id).await?;
    let old = price_at(&before, valid_from).map(|p| p.price_per_min);
    let mut tx = state.db.begin().await?;
    let id = insert_price(&mut tx, org_id, trunk_id, price, valid_from, auth.user_id).await?;
    tx.commit().await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "telephony.trunk_price.created",
        &format!(
            "{trunk_id} {} → {} {} desde {valid_from}",
            old.map(|m| format!("{} {}", m.amount_string(), m.currency.as_str()))
                .unwrap_or_else(|| "sem preço".into()),
            price.amount_string(),
            price.currency.as_str()
        ),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        [(
            LOCATION,
            format!("/api/orgs/{org_id}/telephony/trunks/{trunk_id}/prices"),
        )],
        Json(Price {
            id,
            trunk_id,
            price_per_min: price.into(),
            valid_from,
            in_force: valid_from <= Utc::now(),
            created_at: Utc::now(),
        }),
    )
        .into_response())
}

// ============================================================
//  Taxas de câmbio (histórico)
// ============================================================

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ExchangeRate {
    pub id: Uuid,
    /// `USD`.
    pub currency: String,
    /// Kz por unidade, decimal com 6 casas.
    pub aoa_per_unit: String,
    pub valid_from: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ExchangeRatePage {
    pub items: Vec<ExchangeRate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateRateReq {
    /// `USD`.
    pub currency: String,
    /// Kz por unidade (`"912.50"`).
    pub aoa_per_unit: String,
    #[serde(default)]
    pub valid_from: Option<DateTime<Utc>>,
}

/// Taxas de câmbio para o consumo em Kz.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/telephony/exchange-rates", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), HistoryQuery),
    responses(
        (status = 200, body = ExchangeRatePage),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_rates(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<ExchangeRatePage>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<HistCursor> = page.cursor()?;
    let rows: Vec<LinhaCambio> = sqlx::query_as(
        "SELECT id, currency, aoa_per_unit_e6, valid_from, created_at FROM telephony_exchange_rates
          WHERE org_id = $1 AND ($2::timestamptz IS NULL OR (valid_from, id) < ($2, $3))
          ORDER BY valid_from DESC, id DESC LIMIT $4",
    )
    .bind(org_id)
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |r| HistCursor { at: r.3, id: r.0 });
    Ok(Json(ExchangeRatePage {
        items: p
            .items
            .into_iter()
            .map(|(id, currency, e6, valid_from, created_at)| ExchangeRate {
                id,
                currency,
                aoa_per_unit: format_e6(e6),
                valid_from,
                created_at,
            })
            .collect(),
        next_page_token: p.next_page_token,
    }))
}

/// Taxa nova (nunca retroactiva).
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/telephony/exchange-rates", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = CreateRateReq,
    responses(
        (status = 201, body = ExchangeRate, headers(("Location" = String))),
        (status = 400, body = crate::openapi::ErrorBody, description = "`telephony.invalid_rate`, `telephony.invalid_currency`, `telephony.price_backdated`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody),
    )
)]
pub async fn create_rate(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateRateReq>,
) -> Result<Response, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let currency = Currency::parse(&req.currency)?;
    if currency == Currency::Aoa {
        return Err(DomainError::invalid(
            "telephony.invalid_currency",
            "a taxa é de uma moeda estrangeira para Kz",
        )
        .into());
    }
    let e6 = parse_rate_e6(&req.aoa_per_unit)?;
    let valid_from = not_backdated(req.valid_from)?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO telephony_exchange_rates (id, org_id, currency, aoa_per_unit_e6, valid_from, created_by)
         VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(id)
    .bind(org_id)
    .bind(currency.as_str())
    .bind(e6)
    .bind(valid_from)
    .bind(auth.user_id)
    .execute(&state.db)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(d) if d.is_unique_violation() => ApiError::from(DomainError::conflict(
            "telephony.rate_exists",
            "já há uma taxa com esse início",
        )),
        other => other.into(),
    })?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "telephony.exchange_rate.created",
        &format!(
            "{id} {} = {} Kz desde {valid_from}",
            currency.as_str(),
            format_e6(e6)
        ),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        [(
            LOCATION,
            format!("/api/orgs/{org_id}/telephony/exchange-rates"),
        )],
        Json(ExchangeRate {
            id,
            currency: currency.as_str().into(),
            aoa_per_unit: format_e6(e6),
            valid_from,
            created_at: Utc::now(),
        }),
    )
        .into_response())
}

/// Todos os troncos da org (até 100) com estado medido — para o resumo do
/// registo SIP.
pub(crate) async fn all_with_status(
    state: &AppState,
    org_id: Uuid,
    hours: i64,
) -> Result<Vec<Trunk>, ApiError> {
    let rows: Vec<TrunkRecord> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM telephony_trunks WHERE org_id = $1 ORDER BY position, id LIMIT 100"
    ))
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    assemble(state, org_id, rows, hours).await
}
