//! Plano de marcação da organização (ADR-0009 §3).
//!
//! - `GET  /api/orgs/{org_id}/telephony/dial-plan`        o plano (regras ordenadas)
//! - `PUT  /api/orgs/{org_id}/telephony/dial-plan`        substitui o plano INTEIRO
//! - `POST /api/orgs/{org_id}/telephony/dial-plan/test`   número → regra, operadora, reserva, gravado, custo
//!
//! O pedido original falava em `dial-plan:test`; a convenção do repo
//! (`docs/reference/api-routes.md`) é o método personalizado como segmento
//! (`/rotate-key`, `/start`), e foi essa a seguida.
//!
//! As regras e o invariante de emergência vivem em
//! `delonix_meet_domain::telephony::dial_plan`; aqui só se lê, grava e mostra.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_domain::telephony::dial_plan::{validate_plan, DialRuleInput, ResolutionOutcome};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::ApiError,
    telephony_service::{load_trunks, resolve_number, MoneyDto, TrunkSummary},
    AppState,
};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct DialRuleDto {
    /// `9XXXXXXXX`, `00X.`, `1XX`, `112,113,115`. X=0-9, Z=1-9, N=2-9,
    /// `[1-5]`, `.` (1+ dígitos, no fim), `!` (0+ dígitos, no fim).
    pub pattern: String,
    /// `Móvel nacional`, `Sala por PIN — entra na sessão`.
    pub description: String,
    /// `external` | `room_pin` | `extension` | `block`.
    pub action: String,
    /// Operadora (só `external`).
    #[serde(default)]
    pub trunk_id: Option<Uuid>,
    /// Reserva (só `external`, diferente da operadora).
    #[serde(default)]
    pub fallback_trunk_id: Option<Uuid>,
    /// Gravar. Sempre `false` numa regra de emergência (recusado se `true`).
    #[serde(default)]
    pub record: bool,
    /// Regra de emergência: nunca gravada, nunca bloqueada.
    #[serde(default)]
    pub emergency: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DialPlan {
    /// Por ordem: a primeira que casar vale.
    pub rules: Vec<DialRuleDto>,
    /// Números de emergência desta instalação (sempre resolvidos primeiro).
    pub emergency_numbers: Vec<String>,
    /// Incrementa a cada gravação; `0` = nunca gravado.
    pub version: i64,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct PutDialPlanReq {
    pub rules: Vec<DialRuleDto>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct TestNumberReq {
    pub number: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct MatchedRule {
    /// 0-based.
    pub position: usize,
    pub pattern: String,
    pub description: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct TestNumberResp {
    /// Os dígitos contra os quais o plano casou.
    pub dialed: String,
    /// `+244…` quando é um número público; `null` para curtos/internos.
    pub e164: Option<String>,
    /// `route` | `internal` | `blocked` | `no_match` | `no_available_trunk`.
    pub outcome: String,
    pub matched_rule: Option<MatchedRule>,
    /// `external` | `room_pin` | `extension` | `block`.
    pub action: Option<String>,
    /// Operadora principal (a primeira activa).
    pub trunk: Option<TrunkSummary>,
    /// Reservas, pela ordem de tentativa.
    pub fallbacks: Vec<TrunkSummary>,
    pub recorded: bool,
    pub emergency: bool,
    /// Numa emergência: a regra que teria casado primeiro e foi ultrapassada.
    pub overridden_rule_position: Option<usize>,
    /// Preço por minuto em vigor AGORA na operadora principal. `null` sem preço.
    pub estimated_price_per_min: Option<MoneyDto>,
    /// Porque o preço é `null`: `not_external` | `no_price_in_force`.
    pub price_reason: Option<String>,
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(get_plan, put_plan, test_number),
    components(schemas(
        DialPlan,
        DialRuleDto,
        PutDialPlanReq,
        TestNumberReq,
        TestNumberResp,
        MatchedRule,
        TrunkSummary
    ))
)]
pub struct ApiDoc;

async fn read_plan(state: &AppState, org_id: Uuid) -> Result<DialPlan, ApiError> {
    let meta: Option<(i64, DateTime<Utc>)> =
        sqlx::query_as("SELECT version, updated_at FROM telephony_dial_plans WHERE org_id = $1")
            .bind(org_id)
            .fetch_optional(&state.db)
            .await?;
    let rules: Vec<DialRuleDto> = sqlx::query_as(
        "SELECT pattern, description, action, trunk_id, fallback_trunk_id, record, emergency
           FROM telephony_dial_rules WHERE org_id = $1 ORDER BY position",
    )
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(DialPlan {
        rules,
        emergency_numbers: state.config.telephony_emergency_numbers.clone(),
        version: meta.map(|m| m.0).unwrap_or(0),
        updated_at: meta.map(|m| m.1),
    })
}

/// O plano de marcação.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/telephony/dial-plan", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses(
        (status = 200, body = DialPlan),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_plan(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<DialPlan>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    Ok(Json(read_plan(&state, org_id).await?))
}

/// Substitui o plano inteiro (a ordem é o significado). Valida tudo antes de
/// escrever; a gravação é atómica.
#[utoipa::path(
    put, path = "/api/orgs/{org_id}/telephony/dial-plan", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = PutDialPlanReq,
    responses(
        (status = 200, body = DialPlan),
        (status = 400, body = crate::openapi::ErrorBody, description = "`telephony.invalid_pattern`, `telephony.emergency_never_recorded`, `telephony.emergency_cannot_be_blocked`, `telephony.unknown_trunk`, `telephony.rule_requires_trunk`, … (o campo `details[].field` diz a regra)"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn put_plan(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<PutDialPlanReq>,
) -> Result<Json<DialPlan>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let trunks: Vec<Uuid> = load_trunks(&state, org_id)
        .await?
        .into_iter()
        .map(|t| t.id)
        .collect();
    let input: Vec<DialRuleInput> = req
        .rules
        .iter()
        .map(|r| DialRuleInput {
            pattern: r.pattern.clone(),
            description: r.description.clone(),
            action: r.action.clone(),
            trunk_id: r.trunk_id,
            fallback_trunk_id: r.fallback_trunk_id,
            record: r.record,
            emergency: r.emergency,
        })
        .collect();
    let valid = validate_plan(&input, &trunks, &state.config.telephony_emergency_numbers)?;
    let before = read_plan(&state, org_id).await?;

    let mut tx = state.db.begin().await?;
    sqlx::query(
        "INSERT INTO telephony_dial_plans (org_id, version, updated_by, updated_at)
         VALUES ($1, 1, $2, now())
         ON CONFLICT (org_id) DO UPDATE
            SET version = telephony_dial_plans.version + 1, updated_by = $2, updated_at = now()",
    )
    .bind(org_id)
    .bind(auth.user_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM telephony_dial_rules WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?;
    for (pos, r) in valid.iter().enumerate() {
        sqlx::query(
            "INSERT INTO telephony_dial_rules
                (org_id, position, pattern, description, action, trunk_id, fallback_trunk_id, record, emergency)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        )
        .bind(org_id)
        .bind(pos as i32)
        .bind(r.pattern.as_str())
        .bind(&r.description)
        .bind(r.action.as_str())
        .bind(r.trunk_id)
        .bind(r.fallback_trunk_id)
        .bind(r.record)
        .bind(r.emergency)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    let summary = |rules: &[DialRuleDto]| {
        rules
            .iter()
            .map(|r| {
                format!(
                    "{}→{}{}{}",
                    r.pattern,
                    r.action,
                    if r.record { "+rec" } else { "" },
                    if r.emergency { "+emerg" } else { "" }
                )
            })
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let after = read_plan(&state, org_id).await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "telephony.dial_plan.updated",
        &format!(
            "v{}→v{} antes: [{}] depois: [{}]",
            before.version,
            after.version,
            summary(&before.rules),
            summary(&after.rules)
        ),
    )
    .await;
    Ok(Json(after))
}

/// Método personalizado: que regra casa este número, por que operadora sai,
/// se é gravado e quanto custa o minuto. Não liga.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/telephony/dial-plan/test", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = TestNumberReq,
    responses(
        (status = 200, body = TestNumberResp),
        (status = 400, body = crate::openapi::ErrorBody, description = "`telephony.invalid_number`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn test_number(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<TestNumberReq>,
) -> Result<Json<TestNumberResp>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let r = resolve_number(&state, org_id, &req.number).await?;
    let rules = crate::telephony_service::load_rules(&state, org_id).await?;
    let external = r.resolution.outcome == ResolutionOutcome::Route;
    let mut trunks = r.trunks.into_iter();
    let trunk = trunks.next();
    Ok(Json(TestNumberResp {
        dialed: r.dialed.digits,
        e164: r.dialed.e164,
        outcome: serde_json::to_value(r.resolution.outcome)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default(),
        matched_rule: r.resolution.rule_position.and_then(|p| {
            rules.get(p).map(|rule| MatchedRule {
                position: p,
                pattern: rule.pattern.as_str().to_string(),
                description: rule.description.clone(),
            })
        }),
        action: r.resolution.action.map(|a| a.as_str().to_string()),
        trunk,
        fallbacks: trunks.collect(),
        recorded: r.resolution.record,
        emergency: r.resolution.emergency,
        overridden_rule_position: r.resolution.overridden_rule_position,
        price_reason: if !external {
            Some("not_external".into())
        } else if r.price_per_min.is_none() {
            Some("no_price_in_force".into())
        } else {
            None
        },
        estimated_price_per_min: r.price_per_min.map(Into::into),
    }))
}
