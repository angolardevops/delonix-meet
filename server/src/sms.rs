//! Gateway de SMS (ADR-0005) — fila, encaminhamento e as duas superfícies.
//!
//! - **Consola (BFF)**: `/api/orgs/{org_id}/sms/*`, sessão, só administrador da
//!   org. Enviar SMS custa dinheiro — é superfície de fraude, como o dial-in.
//! - **Agente USB**: `/api/integrations/sms-agent/v1/*`, token `dlxg_` do gateway. O agente corre
//!   na máquina onde o telefone está ligado e liga para fora; o servidor nunca
//!   vê USB (imagem distroless, pod K8s).
//! - **Operadores**: um worker reclama as mensagens `operator` e envia-as por
//!   SMPP (`sms_smpp.rs`) com as credenciais do ambiente.
//!
//! Entrega no máximo uma vez: uma mensagem reclamada que não é confirmada passa
//! a `failed` e nunca volta à fila (ver o ADR para o porquê).

use axum::{
    extract::{FromRequestParts, Path, Query, State},
    http::{header, request::Parts, HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::ApiError,
    org::require_admin_pub,
    sms_codec::{self, CodecError},
    sms_smpp::SmppLink,
    AppState,
};

/// Um dispositivo ou gateway sem relatório há mais do que isto está desligado.
const ONLINE_WINDOW_SECS: i64 = 30;
/// Intervalo de relatório pedido ao agente.
const AGENT_POLL_SECS: u64 = 5;
/// Uma mensagem USB não reclamada ou não confirmada ao fim disto falha.
const STALE_MINUTES: i64 = 10;
const CLAIM_BATCH: i64 = 5;
const MAX_DEVICES_PER_REPORT: usize = 64;

// ============================================================
//  Operadores e plano de numeração
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Unitel,
    Movicel,
    Africell,
}

/// Prefixos móveis angolanos por operador. **A CONFIRMAR com a publicação do
/// regulador** antes de qualquer demonstração a um operador (ADR-0005). É a
/// única tabela: a consola lê-a daqui.
const NUMBERING_PLAN: [(Operator, &str, &[&str]); 3] = [
    (Operator::Unitel, "Unitel", &["92", "93", "94"]),
    (Operator::Movicel, "Movicel", &["91", "99"]),
    (Operator::Africell, "Africell", &["95"]),
];

impl Operator {
    pub fn as_str(self) -> &'static str {
        match self {
            Operator::Unitel => "unitel",
            Operator::Movicel => "movicel",
            Operator::Africell => "africell",
        }
    }

    fn label(self) -> &'static str {
        NUMBERING_PLAN
            .iter()
            .find(|(o, _, _)| *o == self)
            .map(|(_, l, _)| *l)
            .unwrap_or("")
    }

    fn parse(s: &str) -> Option<Operator> {
        NUMBERING_PLAN
            .iter()
            .map(|(o, _, _)| *o)
            .find(|o| o.as_str() == s)
    }
}

/// Normaliza um número móvel angolano para E.164. Aceita `923 000 000`,
/// `+244 923-000-000`, `00244923000000`. Outros países ficam de fora nesta fase.
pub fn normalize_msisdn(input: &str) -> Result<String, ApiError> {
    let compact: String = input
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '.' | '(' | ')'))
        .collect();
    let digits = if let Some(rest) = compact.strip_prefix('+') {
        rest.to_string()
    } else if let Some(rest) = compact.strip_prefix("00") {
        rest.to_string()
    } else if compact.len() == 9 {
        format!("244{compact}")
    } else {
        compact
    };
    let national = digits.strip_prefix("244").unwrap_or("");
    if national.len() == 9
        && national.starts_with('9')
        && national.bytes().all(|b| b.is_ascii_digit())
    {
        Ok(format!("+{digits}"))
    } else {
        Err(ApiError::Unprocessable(
            "nesta fase só números móveis angolanos: +244 9XX XXX XXX".into(),
        ))
    }
}

/// Operador de um número já normalizado.
pub fn operator_for(e164: &str) -> Option<Operator> {
    let national = e164.strip_prefix("+244")?;
    NUMBERING_PLAN
        .iter()
        .find(|(_, _, prefixes)| prefixes.iter().any(|p| national.starts_with(p)))
        .map(|(o, _, _)| *o)
}

/// Ligação SMPP configurada para o operador, se houver e for válida.
fn operator_link(state: &AppState, op: Operator) -> Option<SmppLink> {
    let raw = match op {
        Operator::Unitel => state.config.sms_unitel_smpp.as_deref(),
        Operator::Movicel => state.config.sms_movicel_smpp.as_deref(),
        Operator::Africell => state.config.sms_africell_smpp.as_deref(),
    }?;
    SmppLink::parse(raw).ok()
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct OperatorInfo {
    operator: &'static str,
    label: &'static str,
    prefixes: Vec<&'static str>,
    configured: bool,
}

fn operators_info(state: &AppState) -> Vec<OperatorInfo> {
    NUMBERING_PLAN
        .iter()
        .map(|(op, label, prefixes)| OperatorInfo {
            operator: op.as_str(),
            label,
            prefixes: prefixes.to_vec(),
            configured: operator_link(state, *op).is_some(),
        })
        .collect()
}

// ============================================================
//  Consola — gateways
// ============================================================

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct GatewayInfo {
    id: Uuid,
    name: String,
    prefix: String,
    created_at: DateTime<Utc>,
    last_seen_at: Option<DateTime<Utc>>,
    online: bool,
}

/// Documentação OpenAPI do gateway de SMS (ADR-0005). As rotas `/api/integrations/sms-agent/v1/*`
/// autenticam com o token de gateway `dlxg_` (esquema `api_key`, com esse prefixo).
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        list_gateways,
        create_gateway,
        revoke_gateway,
        list_devices,
        get_route,
        put_route,
        list_messages,
        get_message,
        send_message,
        agent_put_devices,
        agent_claim,
        agent_result,
        get_sms_policy,
        put_sms_policy,
        set_member_phone,
        get_sms_preferences,
        put_sms_preferences
    ),
    components(schemas(
        OperatorInfo,
        GatewayInfo,
        CreateGatewayReq,
        CreatedGateway,
        DeviceInfo,
        RouteInfo,
        PutRouteReq,
        Message,
        MessagePage,
        SendReq,
        DeviceReport,
        DevicesReq,
        DevicesResp,
        ClaimedMessage,
        ClaimResp,
        ResultReq,
        SmsSendPolicy,
        SmsPolicyResp,
        PutSmsPolicyReq,
        MemberPhone,
        PhoneChange,
        OrgPhone,
        SmsPreferences,
        PutSmsPreferencesReq
    ))
)]
pub struct ApiDoc;

#[utoipa::path(
    get, path = "/api/orgs/{org_id}/sms/gateways", tag = "sms",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses(
        (status = 200, body = Vec<GatewayInfo>),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_gateways(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<GatewayInfo>>, ApiError> {
    require_admin_pub(&state, org_id, auth.user_id).await?;
    let rows = sqlx::query_as::<_, GatewayInfo>(
        "SELECT id, name, token_prefix AS prefix, created_at, last_seen_at,
                COALESCE(last_seen_at > now() - make_interval(secs => $2), false) AS online
         FROM sms_gateway WHERE org_id = $1 AND revoked_at IS NULL
         ORDER BY created_at",
    )
    .bind(org_id)
    .bind(ONLINE_WINDOW_SECS as f64)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateGatewayReq {
    #[serde(default)]
    name: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CreatedGateway {
    id: Uuid,
    name: String,
    prefix: String,
    /// Só devolvido AGORA; guarda-se o SHA-256.
    token: String,
}

#[utoipa::path(
    post, path = "/api/orgs/{org_id}/sms/gateways", tag = "sms",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = CreateGatewayReq,
    responses(
        (status = 201, body = CreatedGateway, description = "O token `dlxg_` sai UMA vez."),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn create_gateway(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateGatewayReq>,
) -> Result<impl IntoResponse, ApiError> {
    require_admin_pub(&state, org_id, auth.user_id).await?;
    let name: String = req.name.trim().chars().take(60).collect();
    let name = if name.is_empty() {
        "Gateway USB".to_string()
    } else {
        name
    };
    let token = crate::crypto::random_token("dlxg_");
    let prefix: String = token.chars().take(13).collect();
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO sms_gateway (org_id, name, token_prefix, token_hash, created_by)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(org_id)
    .bind(&name)
    .bind(&prefix)
    .bind(crate::crypto::sha256_hex(&token))
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "sms.gateway.created",
        &id.to_string(),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        [(
            header::LOCATION,
            format!("/api/orgs/{org_id}/sms/gateways/{id}"),
        )],
        Json(CreatedGateway {
            id,
            name,
            prefix,
            token,
        }),
    ))
}

#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/sms/gateways/{gateway_id}", tag = "sms",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("gateway_id" = Uuid, Path)),
    responses(
        (status = 204, description = "Revogado."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn revoke_gateway(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, gateway_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    require_admin_pub(&state, org_id, auth.user_id).await?;
    let mut tx = state.db.begin().await?;
    let done = sqlx::query(
        "UPDATE sms_gateway SET revoked_at = now()
         WHERE id = $1 AND org_id = $2 AND revoked_at IS NULL",
    )
    .bind(gateway_id)
    .bind(org_id)
    .execute(&mut *tx)
    .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    // Os dispositivos saem com ele: a rota da org deixa de apontar para um
    // telefone que já ninguém pode alcançar (ON DELETE SET NULL).
    sqlx::query("DELETE FROM sms_device WHERE gateway_id = $1")
        .bind(gateway_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "sms.gateway.revoked",
        &gateway_id.to_string(),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ============================================================
//  Consola — dispositivos e rota
// ============================================================

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct DeviceInfo {
    id: Uuid,
    gateway_id: Uuid,
    gateway_name: String,
    device_key: String,
    vendor_id: String,
    product_id: String,
    manufacturer: Option<String>,
    product: Option<String>,
    serial: Option<String>,
    kind: String,
    transport: String,
    port: Option<String>,
    capable: bool,
    reason: Option<String>,
    operator_name: Option<String>,
    signal_percent: Option<i32>,
    last_seen_at: DateTime<Utc>,
    online: bool,
    selected: bool,
}

#[utoipa::path(
    get, path = "/api/orgs/{org_id}/sms/devices", tag = "sms",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses(
        (status = 200, body = Vec<DeviceInfo>),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_devices(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<DeviceInfo>>, ApiError> {
    require_admin_pub(&state, org_id, auth.user_id).await?;
    let rows = sqlx::query_as::<_, DeviceInfo>(
        "SELECT d.id, d.gateway_id, g.name AS gateway_name, d.device_key, d.vendor_id,
                d.product_id, d.manufacturer, d.product, d.serial, d.kind, d.transport,
                d.port, d.capable, d.reason, d.operator_name, d.signal_percent,
                d.last_seen_at,
                d.last_seen_at > now() - make_interval(secs => $2) AS online,
                COALESCE(r.device_id = d.id, false) AS selected
         FROM sms_device d
         JOIN sms_gateway g ON g.id = d.gateway_id AND g.revoked_at IS NULL
         LEFT JOIN sms_org_route r ON r.org_id = d.org_id
         WHERE d.org_id = $1
         ORDER BY d.last_seen_at DESC",
    )
    .bind(org_id)
    .bind(ONLINE_WINDOW_SECS as f64)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RouteInfo {
    device_id: Option<Uuid>,
    operators: Vec<OperatorInfo>,
}

async fn route_info(state: &AppState, org_id: Uuid) -> Result<RouteInfo, ApiError> {
    let device_id: Option<Uuid> =
        sqlx::query_scalar("SELECT device_id FROM sms_org_route WHERE org_id = $1")
            .bind(org_id)
            .fetch_optional(&state.db)
            .await?
            .flatten();
    Ok(RouteInfo {
        device_id,
        operators: operators_info(state),
    })
}

#[utoipa::path(
    get, path = "/api/orgs/{org_id}/sms/route", tag = "sms",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses(
        (status = 200, body = RouteInfo),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_route(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<RouteInfo>, ApiError> {
    require_admin_pub(&state, org_id, auth.user_id).await?;
    Ok(Json(route_info(&state, org_id).await?))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct PutRouteReq {
    device_id: Option<Uuid>,
}

#[utoipa::path(
    put, path = "/api/orgs/{org_id}/sms/route", tag = "sms",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = PutRouteReq,
    responses(
        (status = 200, body = RouteInfo),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 422, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn put_route(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<PutRouteReq>,
) -> Result<Json<RouteInfo>, ApiError> {
    require_admin_pub(&state, org_id, auth.user_id).await?;
    if let Some(device_id) = req.device_id {
        // Só um dispositivo DESTA org, de um gateway não revogado. De outra org
        // é 404: não se confirma que existe.
        let capable: Option<(bool, Option<String>)> = sqlx::query_as(
            "SELECT d.capable, d.reason FROM sms_device d
             JOIN sms_gateway g ON g.id = d.gateway_id AND g.revoked_at IS NULL
             WHERE d.id = $1 AND d.org_id = $2",
        )
        .bind(device_id)
        .bind(org_id)
        .fetch_optional(&state.db)
        .await?;
        match capable {
            None => return Err(ApiError::NotFound),
            Some((false, reason)) => {
                return Err(ApiError::Unprocessable(reason.unwrap_or_else(|| {
                    "este dispositivo não consegue enviar SMS".into()
                })))
            }
            Some((true, _)) => {}
        }
    }
    sqlx::query(
        "INSERT INTO sms_org_route (org_id, device_id, updated_by, updated_at)
         VALUES ($1, $2, $3, now())
         ON CONFLICT (org_id) DO UPDATE
         SET device_id = EXCLUDED.device_id, updated_by = EXCLUDED.updated_by, updated_at = now()",
    )
    .bind(org_id)
    .bind(req.device_id)
    .bind(auth.user_id)
    .execute(&state.db)
    .await?;
    let target = req
        .device_id
        .map(|d| d.to_string())
        .unwrap_or_else(|| "nenhum".into());
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "sms.route.updated",
        &target,
    )
    .await;
    Ok(Json(route_info(&state, org_id).await?))
}

// ============================================================
//  Consola — mensagens
// ============================================================

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Message {
    id: Uuid,
    #[sqlx(rename = "to_e164")]
    to: String,
    body: String,
    encoding: String,
    segments: i32,
    route: String,
    operator: Option<String>,
    device_id: Option<Uuid>,
    status: String,
    error: Option<String>,
    provider_ref: Option<String>,
    created_at: DateTime<Utc>,
    sent_at: Option<DateTime<Utc>>,
}

const MESSAGE_COLUMNS: &str = "id, to_e164, body, encoding, segments, route, operator, device_id,
     status, error, provider_ref, created_at, sent_at";

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageQuery {
    page_size: Option<i64>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct MessagePage {
    items: Vec<Message>,
    /// Sempre `null` por agora: só se servem as mais recentes. O campo existe
    /// para o contrato não mudar quando o cursor chegar.
    next_page_token: Option<String>,
}

#[utoipa::path(
    get, path = "/api/orgs/{org_id}/sms/messages", tag = "sms",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), PageQuery),
    responses(
        (status = 200, body = MessagePage),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_messages(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Query(q): Query<PageQuery>,
) -> Result<Json<MessagePage>, ApiError> {
    require_admin_pub(&state, org_id, auth.user_id).await?;
    let page_size = q.page_size.unwrap_or(50);
    if !(1..=100).contains(&page_size) {
        return Err(ApiError::BadRequest(
            "page_size tem de estar entre 1 e 100".into(),
        ));
    }
    let items = sqlx::query_as::<_, Message>(&format!(
        "SELECT {MESSAGE_COLUMNS} FROM sms_message WHERE org_id = $1
         ORDER BY created_at DESC LIMIT $2"
    ))
    .bind(org_id)
    .bind(page_size)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(MessagePage {
        items,
        next_page_token: None,
    }))
}

#[utoipa::path(
    get, path = "/api/orgs/{org_id}/sms/messages/{message_id}", tag = "sms",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("message_id" = Uuid, Path)),
    responses(
        (status = 200, body = Message),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_message(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, message_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Message>, ApiError> {
    require_admin_pub(&state, org_id, auth.user_id).await?;
    let m = sqlx::query_as::<_, Message>(&format!(
        "SELECT {MESSAGE_COLUMNS} FROM sms_message WHERE id = $1 AND org_id = $2"
    ))
    .bind(message_id)
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(Json(m))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct SendReq {
    to: String,
    body: String,
    #[serde(default = "auto_route")]
    route: String,
}
fn auto_route() -> String {
    "auto".into()
}

/// A rota escolhida para uma mensagem, decidida no pedido e gravada.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Operator(Operator),
    Usb(Uuid),
}

/// Regra de encaminhamento (ADR-0005), pura para se testar sem base de dados.
pub fn decide(
    requested: &str,
    operator: Option<Operator>,
    operator_configured: bool,
    usb_device: Option<Uuid>,
) -> Result<Decision, String> {
    let (want_operator, want_usb) = match requested {
        "auto" => (true, true),
        "operator" => (true, false),
        "usb" => (false, true),
        _ => return Err("route tem de ser auto, usb ou operator".into()),
    };
    if want_operator && operator_configured {
        if let Some(op) = operator {
            return Ok(Decision::Operator(op));
        }
    }
    if want_usb {
        if let Some(d) = usb_device {
            return Ok(Decision::Usb(d));
        }
    }
    let op_reason = match operator {
        Some(op) => format!("a ligação à {} não está contratada", op.label()),
        None => "o número não corresponde a nenhum operador conhecido".into(),
    };
    let usb_reason = "não há dispositivo USB seleccionado, capaz e ligado";
    Err(match requested {
        "operator" => format!("sem rota: {op_reason}"),
        "usb" => format!("sem rota: {usb_reason}"),
        _ => format!("sem rota: {op_reason} e {usb_reason}"),
    })
}

#[utoipa::path(
    post, path = "/api/orgs/{org_id}/sms/messages", tag = "sms",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("Idempotency-Key" = Option<String>, Header, description = "Repetir com a mesma chave devolve a mesma mensagem.")),
    request_body = SendReq,
    responses(
        (status = 202, body = Message, description = "Aceite na fila."),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 422, body = crate::openapi::ErrorBody, description = "Sem rota ou dispositivo."),
        (status = 429, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn send_message(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    headers: HeaderMap,
    Json(req): Json<SendReq>,
) -> Result<impl IntoResponse, ApiError> {
    require_admin_pub(&state, org_id, auth.user_id).await?;
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|k| !k.is_empty());
    if idempotency_key.is_some_and(|k| k.len() > 128) {
        return Err(ApiError::BadRequest(
            "Idempotency-Key com mais de 128 caracteres".into(),
        ));
    }
    // Uma repetição com a mesma chave devolve a mesma mensagem e não gasta quota.
    if let Some(key) = idempotency_key {
        if let Some(m) = find_by_idempotency_key(&state, org_id, key).await? {
            return Ok((StatusCode::ACCEPTED, Json(m)));
        }
    }
    if !state.sms_send_limiter.check(&org_id.to_string()) {
        return Err(ApiError::TooManyRequests);
    }

    let to = normalize_msisdn(&req.to)?;
    let encoded = sms_codec::encode(&req.body).map_err(|e| match e {
        CodecError::InvalidNumber => ApiError::BadRequest(e.to_string()),
        _ => ApiError::Unprocessable(e.to_string()),
    })?;
    let operator = operator_for(&to);
    let configured = operator.is_some_and(|op| operator_link(&state, op).is_some());
    let usb_device: Option<Uuid> = sqlx::query_scalar(
        "SELECT d.id FROM sms_org_route r
         JOIN sms_device d ON d.id = r.device_id AND d.capable
         JOIN sms_gateway g ON g.id = d.gateway_id AND g.revoked_at IS NULL
         WHERE r.org_id = $1 AND d.last_seen_at > now() - make_interval(secs => $2)",
    )
    .bind(org_id)
    .bind(ONLINE_WINDOW_SECS as f64)
    .fetch_optional(&state.db)
    .await?;
    let decision =
        decide(req.route.trim(), operator, configured, usb_device).map_err(|reason| {
            if matches!(req.route.trim(), "auto" | "usb" | "operator") {
                ApiError::Unprocessable(reason)
            } else {
                ApiError::BadRequest(reason)
            }
        })?;
    let (route, device_id) = match decision {
        Decision::Operator(_) => ("operator", None),
        Decision::Usb(d) => ("usb", Some(d)),
    };

    let inserted: Option<Message> = sqlx::query_as::<_, Message>(&format!(
        "INSERT INTO sms_message
             (org_id, created_by, to_e164, body, encoding, segments, route, operator, device_id, idempotency_key)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         ON CONFLICT (org_id, idempotency_key) WHERE idempotency_key IS NOT NULL DO NOTHING
         RETURNING {MESSAGE_COLUMNS}"
    ))
    .bind(org_id)
    .bind(auth.user_id)
    .bind(&to)
    .bind(&req.body)
    .bind(encoded.encoding.as_str())
    .bind(encoded.parts.len() as i32)
    .bind(route)
    .bind(operator.map(Operator::as_str))
    .bind(device_id)
    .bind(idempotency_key)
    .fetch_optional(&state.db)
    .await?;
    let message = match inserted {
        Some(m) => m,
        // Corrida entre dois pedidos com a mesma chave: ganhou o outro.
        None => find_by_idempotency_key(&state, org_id, idempotency_key.unwrap_or_default())
            .await?
            .ok_or_else(|| ApiError::internal("mensagem idempotente desaparecida"))?,
    };
    // O alvo é o id: o número de telefone é dado pessoal e a auditoria é imutável.
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "sms.queued",
        &message.id.to_string(),
    )
    .await;
    Ok((StatusCode::ACCEPTED, Json(message)))
}

async fn find_by_idempotency_key(
    state: &AppState,
    org_id: Uuid,
    key: &str,
) -> Result<Option<Message>, ApiError> {
    Ok(sqlx::query_as::<_, Message>(&format!(
        "SELECT {MESSAGE_COLUMNS} FROM sms_message WHERE org_id = $1 AND idempotency_key = $2"
    ))
    .bind(org_id)
    .bind(key)
    .fetch_optional(&state.db)
    .await?)
}

// ============================================================
//  Agente USB — autenticação e superfície
// ============================================================

/// Pedido autenticado por token de gateway (`Authorization: Bearer dlxg_…`).
pub struct SmsGatewayAuth {
    pub gateway_id: Uuid,
    pub org_id: Uuid,
}

impl FromRequestParts<Arc<AppState>> for SmsGatewayAuth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, ApiError> {
        let token = crate::auth::bearer_token(&parts.headers)
            .filter(|t| t.starts_with("dlxg_"))
            .ok_or(ApiError::Unauthorized)?;
        let row: Option<(Uuid, Uuid)> = sqlx::query_as(
            "UPDATE sms_gateway SET last_seen_at = now()
             WHERE token_hash = $1 AND revoked_at IS NULL
             RETURNING id, org_id",
        )
        .bind(crate::crypto::sha256_hex(token))
        .fetch_optional(&state.db)
        .await?;
        let (gateway_id, org_id) = row.ok_or(ApiError::Unauthorized)?;
        Ok(SmsGatewayAuth { gateway_id, org_id })
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct DeviceReport {
    device_key: String,
    vendor_id: String,
    product_id: String,
    manufacturer: Option<String>,
    product: Option<String>,
    serial: Option<String>,
    kind: String,
    transport: String,
    port: Option<String>,
    capable: bool,
    reason: Option<String>,
    operator_name: Option<String>,
    signal_percent: Option<i32>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct DevicesReq {
    devices: Vec<DeviceReport>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DevicesResp {
    poll_interval_secs: u64,
}

fn clip(s: Option<String>) -> Option<String> {
    s.map(|v| v.chars().take(200).collect::<String>())
        .filter(|v| !v.is_empty())
}

/// Substitui o inventário deste gateway. Um dispositivo que deixa de aparecer
/// NÃO é apagado — fica desligado (`online: false`) e mantém a selecção, para
/// que tirar e voltar a ligar o cabo não obrigue a escolher outra vez.
#[utoipa::path(
    put, path = "/api/integrations/sms-agent/v1/devices", tag = "sms",
    security(("api_key" = [])),
    request_body = DevicesReq,
    responses(
        (status = 200, body = DevicesResp),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, description = "Token `dlxg_` inválido ou revogado.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn agent_put_devices(
    State(state): State<Arc<AppState>>,
    gw: SmsGatewayAuth,
    Json(req): Json<DevicesReq>,
) -> Result<Json<DevicesResp>, ApiError> {
    if req.devices.len() > MAX_DEVICES_PER_REPORT {
        return Err(ApiError::BadRequest(format!(
            "no máximo {MAX_DEVICES_PER_REPORT} dispositivos por relatório"
        )));
    }
    let mut tx = state.db.begin().await?;
    for d in req.devices {
        const KINDS: [&str; 5] = [
            "modem",
            "android_adb",
            "android_mtp",
            "mass_storage_modem",
            "unknown",
        ];
        const TRANSPORTS: [&str; 3] = ["at_serial", "modemmanager", "none"];
        if d.device_key.is_empty() || d.device_key.len() > 200 {
            return Err(ApiError::BadRequest("device_key inválido".into()));
        }
        if !KINDS.contains(&d.kind.as_str()) || !TRANSPORTS.contains(&d.transport.as_str()) {
            return Err(ApiError::BadRequest(
                "kind ou transport desconhecido".into(),
            ));
        }
        // Um dispositivo sem transporte não é capaz, diga o agente o que disser.
        let capable = d.capable && d.transport != "none";
        sqlx::query(
            "INSERT INTO sms_device
                 (gateway_id, org_id, device_key, vendor_id, product_id, manufacturer, product,
                  serial, kind, transport, port, capable, reason, operator_name, signal_percent, last_seen_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, now())
             ON CONFLICT (gateway_id, device_key) DO UPDATE SET
                 vendor_id = EXCLUDED.vendor_id, product_id = EXCLUDED.product_id,
                 manufacturer = EXCLUDED.manufacturer, product = EXCLUDED.product,
                 serial = EXCLUDED.serial, kind = EXCLUDED.kind, transport = EXCLUDED.transport,
                 port = EXCLUDED.port, capable = EXCLUDED.capable, reason = EXCLUDED.reason,
                 operator_name = EXCLUDED.operator_name, signal_percent = EXCLUDED.signal_percent,
                 last_seen_at = now()",
        )
        .bind(gw.gateway_id)
        .bind(gw.org_id)
        .bind(&d.device_key)
        .bind(d.vendor_id.chars().take(8).collect::<String>())
        .bind(d.product_id.chars().take(8).collect::<String>())
        .bind(clip(d.manufacturer))
        .bind(clip(d.product))
        .bind(clip(d.serial))
        .bind(&d.kind)
        .bind(&d.transport)
        .bind(clip(d.port))
        .bind(capable)
        .bind(clip(d.reason))
        .bind(clip(d.operator_name))
        .bind(d.signal_percent.map(|s| s.clamp(0, 100)))
        .execute(&mut *tx)
        .await?;
    }
    // Os que não se vêem há uma semana saem do inventário.
    sqlx::query(
        "DELETE FROM sms_device WHERE gateway_id = $1 AND last_seen_at < now() - interval '7 days'",
    )
    .bind(gw.gateway_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(DevicesResp {
        poll_interval_secs: AGENT_POLL_SECS,
    }))
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ClaimedMessage {
    id: Uuid,
    device_key: String,
    to: String,
    body: String,
    pdus: Vec<sms_codec::AtPdu>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ClaimResp {
    messages: Vec<ClaimedMessage>,
}

/// Referência de concatenação: estável por mensagem, para as partes baterem.
fn concat_reference(id: Uuid) -> u8 {
    id.as_bytes()[15]
}

#[utoipa::path(
    post, path = "/api/integrations/sms-agent/v1/claim", tag = "sms",
    security(("api_key" = [])),
    responses(
        (status = 200, body = ClaimResp),
        (status = 401, description = "Token `dlxg_` inválido ou revogado.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn agent_claim(
    State(state): State<Arc<AppState>>,
    gw: SmsGatewayAuth,
) -> Result<Json<ClaimResp>, ApiError> {
    // Só mensagens da org do token, dirigidas a um dispositivo DESTE gateway.
    let rows: Vec<(Uuid, String, String, String)> = sqlx::query_as(
        "UPDATE sms_message m SET status = 'claimed', claimed_at = now(), claimed_by = $1
         FROM sms_device d
         WHERE d.id = m.device_id
           AND m.id IN (
               SELECT m2.id FROM sms_message m2
               JOIN sms_device d2 ON d2.id = m2.device_id
               WHERE m2.org_id = $2 AND m2.route = 'usb' AND m2.status = 'queued'
                 AND d2.gateway_id = $1
               ORDER BY m2.created_at
               LIMIT $3
               FOR UPDATE OF m2 SKIP LOCKED)
         RETURNING m.id, d.device_key, m.to_e164, m.body",
    )
    .bind(gw.gateway_id)
    .bind(gw.org_id)
    .bind(CLAIM_BATCH)
    .fetch_all(&state.db)
    .await?;
    let mut messages = Vec::with_capacity(rows.len());
    for (id, device_key, to, body) in rows {
        // Foi codificado sem erro na criação; um erro aqui é defeito nosso.
        let enc = sms_codec::encode(&body).map_err(ApiError::internal)?;
        let pdus =
            sms_codec::at_pdus(&to, &enc, concat_reference(id)).map_err(ApiError::internal)?;
        messages.push(ClaimedMessage {
            id,
            device_key,
            to,
            body,
            pdus,
        });
    }
    Ok(Json(ClaimResp { messages }))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ResultReq {
    ok: bool,
    error: Option<String>,
    provider_ref: Option<String>,
}

#[utoipa::path(
    post, path = "/api/integrations/sms-agent/v1/messages/{message_id}/result", tag = "sms",
    security(("api_key" = [])),
    params(("message_id" = Uuid, Path)),
    request_body = ResultReq,
    responses(
        (status = 204, description = "Registado."),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 401, description = "Token `dlxg_` inválido ou revogado.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn agent_result(
    State(state): State<Arc<AppState>>,
    gw: SmsGatewayAuth,
    Path(message_id): Path<Uuid>,
    Json(req): Json<ResultReq>,
) -> Result<StatusCode, ApiError> {
    let done = sqlx::query(
        "UPDATE sms_message
         SET status = CASE WHEN $4 THEN 'sent' ELSE 'failed' END,
             sent_at = CASE WHEN $4 THEN now() END,
             error = $5, provider_ref = $6
         WHERE id = $1 AND org_id = $2 AND claimed_by = $3 AND status = 'claimed'",
    )
    .bind(message_id)
    .bind(gw.org_id)
    .bind(gw.gateway_id)
    .bind(req.ok)
    .bind(clip(req.error.filter(|_| !req.ok)))
    .bind(clip(req.provider_ref))
    .execute(&state.db)
    .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ============================================================
//  Worker: operadores por SMPP e varrimento das mensagens paradas
// ============================================================

/// Arranca o worker. Pára sozinho quando o pod começa a drenar: não se reclama
/// trabalho novo num processo que vai morrer.
pub fn spawn_worker(state: Arc<AppState>) {
    for (op, _, _) in NUMBERING_PLAN {
        let raw = match op {
            Operator::Unitel => state.config.sms_unitel_smpp.as_deref(),
            Operator::Movicel => state.config.sms_movicel_smpp.as_deref(),
            Operator::Africell => state.config.sms_africell_smpp.as_deref(),
        };
        match raw.map(SmppLink::parse) {
            None => tracing::info!(operator = op.as_str(), "SMS: operador por contratar"),
            Some(Ok(link)) => {
                tracing::info!(operator = op.as_str(), host = %link.host, "SMS: operador configurado")
            }
            Some(Err(e)) => {
                tracing::error!(operator = op.as_str(), error = %e, "SMS: configuração SMPP inválida — operador desligado")
            }
        }
    }
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(2));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut ticks: u64 = 0;
        loop {
            ticker.tick().await;
            if state.draining.load(std::sync::atomic::Ordering::Relaxed) {
                tracing::info!("SMS: worker parado (drain)");
                return;
            }
            if let Err(e) = dispatch_operator_batch(&state).await {
                tracing::warn!(error = %e, "SMS: lote de operador falhou");
            }
            ticks += 1;
            if ticks.is_multiple_of(30) {
                if let Err(e) = fail_stale(&state).await {
                    tracing::warn!(error = %e, "SMS: varrimento de paradas falhou");
                }
            }
        }
    });
}

async fn dispatch_operator_batch(state: &AppState) -> Result<(), ApiError> {
    let rows: Vec<(Uuid, String, String, Option<String>)> = sqlx::query_as(
        "UPDATE sms_message SET status = 'claimed', claimed_at = now()
         WHERE id IN (
             SELECT id FROM sms_message
             WHERE route = 'operator' AND status = 'queued'
             ORDER BY created_at LIMIT 10
             FOR UPDATE SKIP LOCKED)
         RETURNING id, to_e164, body, operator",
    )
    .fetch_all(&state.db)
    .await?;
    for (id, to, body, operator) in rows {
        let outcome = match operator.as_deref().and_then(Operator::parse) {
            None => Err("mensagem de operador sem operador".to_string()),
            Some(op) => match operator_link(state, op) {
                // A configuração saiu entre o pedido e o envio (reinício sem a variável).
                None => Err(format!(
                    "a ligação à {} deixou de estar configurada",
                    op.label()
                )),
                Some(link) => match sms_codec::encode(&body) {
                    Err(e) => Err(e.to_string()),
                    Ok(enc) => crate::sms_smpp::send(&link, &to, &enc, concat_reference(id))
                        .await
                        .map_err(|e| e.to_string()),
                },
            },
        };
        let (ok, error, provider_ref) = match outcome {
            Ok(r) => (true, None, Some(r)),
            Err(e) => (false, Some(e), None),
        };
        sqlx::query(
            "UPDATE sms_message
             SET status = CASE WHEN $2 THEN 'sent' ELSE 'failed' END,
                 sent_at = CASE WHEN $2 THEN now() END, error = $3, provider_ref = $4
             WHERE id = $1 AND status = 'claimed'",
        )
        .bind(id)
        .bind(ok)
        .bind(clip(error))
        .bind(clip(provider_ref))
        .execute(&state.db)
        .await?;
    }
    Ok(())
}

/// No máximo uma vez: o que ficou parado FALHA, não volta à fila.
async fn fail_stale(state: &AppState) -> Result<(), ApiError> {
    sqlx::query(
        "UPDATE sms_message SET status = 'failed',
             error = CASE status
                 WHEN 'queued' THEN 'nenhum gateway reclamou a mensagem a tempo'
                 ELSE 'o envio não foi confirmado; não se reenvia para não duplicar' END
         WHERE status IN ('queued', 'claimed')
           AND COALESCE(claimed_at, created_at) < now() - make_interval(mins => $1)",
    )
    .bind(STALE_MINUTES as i32)
    .execute(&state.db)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
//  Política de envio da org, telefone por pertença, preferências da conta
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, utoipa::ToSchema, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SmsSendPolicy {
    Admins,
    Members,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SmsPolicyResp {
    send_policy: SmsSendPolicy,
}

/// Quem pode enviar SMS pela organização (só admin lê e muda — enviar custa
/// dinheiro, a mesma razão de `get_route`/`put_route`).
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/sms/policy", tag = "sms",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses(
        (status = 200, body = SmsPolicyResp),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_sms_policy(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<SmsPolicyResp>, ApiError> {
    require_admin_pub(&state, org_id, auth.user_id).await?;
    let policy: (String,) =
        sqlx::query_as("SELECT sms_send_policy FROM organizations WHERE id = $1")
            .bind(org_id)
            .fetch_one(&state.db)
            .await?;
    Ok(Json(SmsPolicyResp {
        send_policy: match policy.0.as_str() {
            "members" => SmsSendPolicy::Members,
            _ => SmsSendPolicy::Admins,
        },
    }))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct PutSmsPolicyReq {
    send_policy: SmsSendPolicy,
}

#[utoipa::path(
    put, path = "/api/orgs/{org_id}/sms/policy", tag = "sms",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = PutSmsPolicyReq,
    responses(
        (status = 200, body = SmsPolicyResp),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn put_sms_policy(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<PutSmsPolicyReq>,
) -> Result<Json<SmsPolicyResp>, ApiError> {
    require_admin_pub(&state, org_id, auth.user_id).await?;
    let stored = match req.send_policy {
        SmsSendPolicy::Admins => "admins",
        SmsSendPolicy::Members => "members",
    };
    sqlx::query("UPDATE organizations SET sms_send_policy = $1 WHERE id = $2")
        .bind(stored)
        .bind(org_id)
        .execute(&state.db)
        .await?;
    Ok(Json(SmsPolicyResp {
        send_policy: req.send_policy,
    }))
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct MemberPhone {
    user_id: Uuid,
    phone: Option<String>,
    /// `manual` (só isto por agora — `odoo` fica para a sincronização do
    /// directório, que não existe ainda) ou `null` sem telefone.
    phone_source: Option<String>,
}

/// `{"phone": "..."} | {"phone": null} | {"follow_directory": true}`. Sem
/// sincronização do directório ainda, `follow_directory` só limpa a
/// substituição manual — não há valor nenhum para "seguir".
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(untagged)]
pub enum PhoneChange {
    FollowDirectory { follow_directory: bool },
    Set { phone: Option<String> },
}

/// Telefone da pertença (não da conta — a mesma pessoa pode ter números
/// diferentes em organizações diferentes). O próprio muda o seu; um admin
/// muda o de outro membro. `phone: null` apaga a substituição manual.
#[utoipa::path(
    put, path = "/api/orgs/{org_id}/members/{user_id}/phone", tag = "sms",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("user_id" = Uuid, Path)),
    request_body = PhoneChange,
    responses(
        (status = 200, body = MemberPhone),
        (status = 400, description = "`follow_directory: false` — não pede nenhuma alteração.", body = crate::openapi::ErrorBody),
        (status = 422, description = "Número fora do formato aceite (`normalize_msisdn`: só móveis angolanos nesta fase).", body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, description = "Não é o próprio nem admin da organização.", body = crate::openapi::ErrorBody),
        (status = 404, description = "O membro alvo não existe ou não é activo nesta organização.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn set_member_phone(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, user_id)): Path<(Uuid, Uuid)>,
    Json(change): Json<PhoneChange>,
) -> Result<Json<MemberPhone>, ApiError> {
    if auth.user_id != user_id {
        require_admin_pub(&state, org_id, auth.user_id).await?;
    }
    crate::org::require_member_pub(&state, org_id, user_id).await?;

    let (phone, source): (Option<String>, Option<&'static str>) = match change {
        // `false` não pede nada — não há "não seguir o directório" para
        // fazer, e silenciá-lo escondia o campo por ler (catraca do clippy).
        PhoneChange::FollowDirectory {
            follow_directory: false,
        } => {
            return Err(ApiError::BadRequest(
                "follow_directory tem de ser true; false não pede nenhuma alteração".into(),
            ))
        }
        PhoneChange::FollowDirectory {
            follow_directory: true,
        } => (None, None),
        PhoneChange::Set { phone: None } => (None, None),
        PhoneChange::Set { phone: Some(raw) } => (Some(normalize_msisdn(&raw)?), Some("manual")),
    };
    crate::org::set_member_phone(&state, org_id, user_id, phone.as_deref(), source).await?;
    Ok(Json(MemberPhone {
        user_id,
        phone,
        phone_source: source.map(str::to_string),
    }))
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct OrgPhone {
    org_id: Uuid,
    org_name: String,
    phone: Option<String>,
    phone_source: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SmsPreferences {
    /// SMS de contacto directo (chamadas/mensagens de um colega).
    contact_opt_out: bool,
    /// SMS de reunião (convite, lembrete).
    meeting_opt_out: bool,
    /// Uma entrada por organização ACTIVA de quem pede.
    phones: Vec<OrgPhone>,
}

async fn load_sms_preferences(state: &AppState, user_id: Uuid) -> Result<SmsPreferences, ApiError> {
    let (contact_opt_out, meeting_opt_out): (bool, bool) =
        sqlx::query_as("SELECT sms_contact_opt_out, sms_meeting_opt_out FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&state.db)
            .await?;
    let rows = crate::org::member_phones(state, user_id).await?;
    Ok(SmsPreferences {
        contact_opt_out,
        meeting_opt_out,
        phones: rows
            .into_iter()
            .map(|(org_id, org_name, phone, phone_source)| OrgPhone {
                org_id,
                org_name,
                phone,
                phone_source,
            })
            .collect(),
    })
}

#[utoipa::path(
    get, path = "/api/users/me/sms-preferences", tag = "sms",
    security(("session" = [])),
    responses(
        (status = 200, body = SmsPreferences),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_sms_preferences(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<SmsPreferences>, ApiError> {
    Ok(Json(load_sms_preferences(&state, auth.user_id).await?))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct PutSmsPreferencesReq {
    /// Omisso ⇒ mantém o valor actual.
    #[serde(default)]
    contact_opt_out: Option<bool>,
    #[serde(default)]
    meeting_opt_out: Option<bool>,
}

#[utoipa::path(
    put, path = "/api/users/me/sms-preferences", tag = "sms",
    security(("session" = [])),
    request_body = PutSmsPreferencesReq,
    responses(
        (status = 200, body = SmsPreferences),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn put_sms_preferences(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<PutSmsPreferencesReq>,
) -> Result<Json<SmsPreferences>, ApiError> {
    sqlx::query(
        "UPDATE users SET sms_contact_opt_out = COALESCE($1, sms_contact_opt_out),
             sms_meeting_opt_out = COALESCE($2, sms_meeting_opt_out)
         WHERE id = $3",
    )
    .bind(req.contact_opt_out)
    .bind(req.meeting_opt_out)
    .bind(auth.user_id)
    .execute(&state.db)
    .await?;
    Ok(Json(load_sms_preferences(&state, auth.user_id).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_angolan_mobile_numbers() {
        for input in [
            "923 000 000",
            "+244 923-000-000",
            "00244923000000",
            "244923000000",
            "(+244) 923.000.000",
        ] {
            assert_eq!(normalize_msisdn(input).unwrap(), "+244923000000", "{input}");
        }
        for bad in [
            "+351912345678",
            "222 000 000",
            "92300000",
            "+24492300000x",
            "",
        ] {
            assert!(normalize_msisdn(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn numbering_plan_maps_prefixes_to_operators() {
        assert_eq!(operator_for("+244923000000"), Some(Operator::Unitel));
        assert_eq!(operator_for("+244941000000"), Some(Operator::Unitel));
        assert_eq!(operator_for("+244912000000"), Some(Operator::Movicel));
        assert_eq!(operator_for("+244991000000"), Some(Operator::Movicel));
        assert_eq!(operator_for("+244951000000"), Some(Operator::Africell));
        assert_eq!(operator_for("+244971000000"), None);
        assert_eq!(operator_for("+351912000000"), None);
    }

    #[test]
    fn every_operator_round_trips_through_its_name() {
        for (op, _, _) in NUMBERING_PLAN {
            assert_eq!(Operator::parse(op.as_str()), Some(op));
        }
    }

    #[test]
    fn auto_prefers_a_configured_operator_then_usb() {
        let dev = Uuid::new_v4();
        let unitel = Some(Operator::Unitel);
        assert_eq!(
            decide("auto", unitel, true, Some(dev)),
            Ok(Decision::Operator(Operator::Unitel))
        );
        assert_eq!(
            decide("auto", unitel, false, Some(dev)),
            Ok(Decision::Usb(dev))
        );
        assert_eq!(
            decide("auto", None, true, Some(dev)),
            Ok(Decision::Usb(dev))
        );
    }

    #[test]
    fn explicit_route_is_never_silently_swapped() {
        let dev = Uuid::new_v4();
        let unitel = Some(Operator::Unitel);
        assert_eq!(
            decide("usb", unitel, true, Some(dev)),
            Ok(Decision::Usb(dev))
        );
        let err = decide("operator", unitel, false, Some(dev)).unwrap_err();
        assert!(
            err.contains("Unitel") && err.contains("não está contratada"),
            "{err}"
        );
        let err = decide("usb", unitel, true, None).unwrap_err();
        assert!(err.contains("USB"), "{err}");
    }

    #[test]
    fn no_route_explains_both_halves() {
        let err = decide("auto", Some(Operator::Africell), false, None).unwrap_err();
        assert!(err.contains("Africell") && err.contains("USB"), "{err}");
        assert!(decide("carrier-pigeon", None, false, None).is_err());
    }
}
