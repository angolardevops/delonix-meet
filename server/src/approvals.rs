//! Pedidos de aprovação (ADR-0008 §6).
//!
//! «Requer aprovação» NÃO executa: `org::require_capability` cria (ou reutiliza)
//! um pedido ligado a `(capacidade, acção, hash canónico do alvo)` e recusa com
//! `403 authz.approval_required`. Quem tem `allow` nessa capacidade — e não é
//! quem pediu — aprova ou recusa. A aprovação é uma licença de USO ÚNICO,
//! válida 24 h: a repetição do mesmo pedido pela mesma pessoa consome-a
//! atomicamente e executa. Mudar o papel, o departamento ou o estado de quem
//! pediu invalida o que estava pendente ou aprovado.
//!
//! - `GET  /api/orgs/{org_id}/approval-requests`                         lista
//! - `GET  /api/orgs/{org_id}/approval-requests/{request_id}`            um
//! - `POST /api/orgs/{org_id}/approval-requests/{request_id}/approve`    método
//! - `POST /api/orgs/{org_id}/approval-requests/{request_id}/reject`     método

use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::{
    page::{Page, PageRequest},
    DomainError,
};
use delonix_meet_domain::identity::authorization::{
    self as authz, Capability, Decision, ResourceScope,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, org::Action, AppState};

/// Validade de um pedido pendente e de uma aprovação por consumir.
const TTL_HOURS: i64 = 24;

fn target_hash(action: &Action<'_>) -> String {
    delonix_meet_core::crypto::sha256_hex(authz::canonical_json(&action.target))
}

/// Consome uma aprovação válida para esta pessoa, capacidade, acção e alvo.
/// Atómico: de duas execuções concorrentes, só uma recebe `true`.
pub(crate) async fn consume(
    state: &AppState,
    org_id: Uuid,
    requester: Uuid,
    cap: Capability,
    action: &Action<'_>,
) -> Result<bool, ApiError> {
    let id: Option<Uuid> = sqlx::query_scalar(
        "UPDATE approval_requests SET status = 'consumed', consumed_at = now()
          WHERE id = (SELECT id FROM approval_requests
                       WHERE org_id = $1 AND requester_id = $2 AND capability = $3
                         AND action = $4 AND target_hash = $5
                         AND status = 'approved' AND expires_at > now()
                       LIMIT 1)
            AND status = 'approved' AND expires_at > now()
      RETURNING id",
    )
    .bind(org_id)
    .bind(requester)
    .bind(cap.as_str())
    .bind(action.name)
    .bind(target_hash(action))
    .fetch_optional(&state.db)
    .await?;
    if let Some(id) = id {
        crate::audit::log(
            &state.db,
            Some(org_id),
            requester,
            "approval.consumed",
            &serde_json::json!({"approval_request_id": id, "capability": cap.as_str(), "action": action.name}).to_string(),
        )
        .await;
    }
    Ok(id.is_some())
}

/// Cria o pedido, ou devolve o pendente igual.
pub(crate) async fn open_request(
    state: &AppState,
    org_id: Uuid,
    requester: Uuid,
    cap: Capability,
    action: &Action<'_>,
) -> Result<Uuid, ApiError> {
    let hash = target_hash(action);
    let inserted: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO approval_requests
            (org_id, requester_id, capability, action, target, target_hash, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, now() + make_interval(hours => $7))
         ON CONFLICT (org_id, requester_id, capability, action, target_hash)
            WHERE status IN ('pending', 'approved')
         DO NOTHING
         RETURNING id",
    )
    .bind(org_id)
    .bind(requester)
    .bind(cap.as_str())
    .bind(action.name)
    .bind(&action.target)
    .bind(&hash)
    .bind(TTL_HOURS as i32)
    .fetch_optional(&state.db)
    .await?;
    if let Some(id) = inserted {
        crate::audit::log(
            &state.db,
            Some(org_id),
            requester,
            "approval.requested",
            &serde_json::json!({"approval_request_id": id, "capability": cap.as_str(),
                                "action": action.name, "target": action.target})
            .to_string(),
        )
        .await;
        return Ok(id);
    }
    let id: Uuid = sqlx::query_scalar(
        "SELECT id FROM approval_requests
          WHERE org_id = $1 AND requester_id = $2 AND capability = $3 AND action = $4
            AND target_hash = $5 AND status IN ('pending', 'approved')",
    )
    .bind(org_id)
    .bind(requester)
    .bind(cap.as_str())
    .bind(action.name)
    .bind(&hash)
    .fetch_one(&state.db)
    .await?;
    Ok(id)
}

/// O papel, o departamento ou o estado de `user_id` mudou: o que estava
/// pendente ou aprovado (e não consumido) deixa de valer.
pub(crate) async fn invalidate_for(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<u64, ApiError> {
    let n = sqlx::query(
        "UPDATE approval_requests SET status = 'invalidated', decided_at = COALESCE(decided_at, now())
          WHERE org_id = $1 AND requester_id = $2 AND status IN ('pending', 'approved')",
    )
    .bind(org_id)
    .bind(user_id)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    Ok(n)
}

/// Sweeper: pendentes e aprovados fora de prazo passam a `expired`.
pub(crate) async fn expire(state: &AppState) -> Result<u64, ApiError> {
    let rows: Vec<(Uuid, Uuid, Uuid)> = sqlx::query_as(
        "UPDATE approval_requests SET status = 'expired'
          WHERE status IN ('pending', 'approved') AND expires_at <= now()
      RETURNING id, org_id, requester_id",
    )
    .fetch_all(&state.db)
    .await?;
    for (id, org, requester) in &rows {
        crate::audit::log(
            &state.db,
            Some(*org),
            *requester,
            "approval.expired",
            &serde_json::json!({"approval_request_id": id}).to_string(),
        )
        .await;
    }
    Ok(rows.len() as u64)
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct ApprovalRequest {
    pub id: Uuid,
    pub org_id: Uuid,
    pub requester_id: Uuid,
    pub requester_name: String,
    pub capability: String,
    pub action: String,
    #[schema(value_type = Object)]
    pub target: serde_json::Value,
    /// `pending` | `approved` | `rejected` | `consumed` | `expired` | `invalidated`.
    pub status: String,
    pub reason: String,
    pub decided_by: Option<Uuid>,
    pub decided_at: Option<DateTime<Utc>>,
    pub consumed_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

const COLUMNS: &str =
    "a.id, a.org_id, a.requester_id, u.username AS requester_name, a.capability, \
     a.action, a.target, a.status, a.reason, a.decided_by, a.decided_at, a.consumed_at, \
     a.expires_at, a.created_at";

#[derive(Serialize, utoipa::ToSchema)]
pub struct ApprovalRequestPage {
    pub items: Vec<ApprovalRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListQuery {
    /// Filtra pelo estado (`pending`, …).
    pub status: Option<String>,
    /// 1-100, omissão 50.
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
}

#[derive(Deserialize, Default, utoipa::ToSchema)]
pub struct DecisionReq {
    /// Obrigatória ao recusar; opcional ao aprovar.
    #[serde(default)]
    pub reason: String,
}

#[derive(Serialize, Deserialize)]
struct Cursor {
    at: DateTime<Utc>,
    id: Uuid,
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(list, get_one, approve, reject),
    components(schemas(ApprovalRequest, ApprovalRequestPage, DecisionReq))
)]
pub struct ApiDoc;

/// Quem pode VER a fila: quem pode decidir alguma capacidade (tem `admin.manage_roles`).
/// Quem pediu vê os seus.
async fn can_see_queue(state: &AppState, org_id: Uuid, user: Uuid) -> Result<bool, ApiError> {
    match crate::org::decide(
        state,
        org_id,
        user,
        Capability::AdminManageRoles,
        ResourceScope::Organization,
    )
    .await?
    {
        None => Err(ApiError::NotFound),
        Some((_, d)) => Ok(d == Decision::Allow),
    }
}

/// Pedidos de aprovação da organização (quem gere papéis vê todos; os outros, os seus).
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/approval-requests", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ListQuery),
    responses(
        (status = 200, body = ApprovalRequestPage),
        (status = 400, body = crate::openapi::ErrorBody, description = "page_token ou status inválido"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "não é membro activo"),
    )
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Query(q): Query<ListQuery>,
) -> Result<Json<ApprovalRequestPage>, ApiError> {
    let all = can_see_queue(&state, org_id, auth.user_id).await?;
    if let Some(s) = q.status.as_deref() {
        if !matches!(
            s,
            "pending" | "approved" | "rejected" | "consumed" | "expired" | "invalidated"
        ) {
            return Err(
                DomainError::invalid("approval.invalid_status", "estado desconhecido")
                    .with_field(
                        "status",
                        "pending|approved|rejected|consumed|expired|invalidated",
                    )
                    .into(),
            );
        }
    }
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<Cursor> = page.cursor()?;
    let rows: Vec<ApprovalRequest> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM approval_requests a JOIN users u ON u.id = a.requester_id
          WHERE a.org_id = $1
            AND ($2 OR a.requester_id = $3)
            AND ($4::text IS NULL OR a.status = $4)
            AND ($5::timestamptz IS NULL OR (a.created_at, a.id) < ($5, $6))
          ORDER BY a.created_at DESC, a.id DESC
          LIMIT $7"
    ))
    .bind(org_id)
    .bind(all)
    .bind(auth.user_id)
    .bind(q.status)
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |r| Cursor {
        at: r.created_at,
        id: r.id,
    });
    Ok(Json(ApprovalRequestPage {
        items: p.items,
        next_page_token: p.next_page_token,
    }))
}

async fn load(
    state: &AppState,
    org_id: Uuid,
    id: Uuid,
    viewer: Uuid,
) -> Result<ApprovalRequest, ApiError> {
    let all = can_see_queue(state, org_id, viewer).await?;
    let row: Option<ApprovalRequest> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM approval_requests a JOIN users u ON u.id = a.requester_id
          WHERE a.org_id = $1 AND a.id = $2"
    ))
    .bind(org_id)
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    match row {
        Some(r) if all || r.requester_id == viewer => Ok(r),
        _ => Err(DomainError::not_found("approval.not_found").into()),
    }
}

/// Um pedido.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/approval-requests/{request_id}", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("request_id" = Uuid, Path)),
    responses(
        (status = 200, body = ApprovalRequest),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_one(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ApprovalRequest>, ApiError> {
    Ok(Json(load(&state, org_id, id, auth.user_id).await?))
}

async fn decide_request(
    state: &AppState,
    auth: &AuthUser,
    org_id: Uuid,
    id: Uuid,
    approve: bool,
    reason: String,
) -> Result<ApprovalRequest, ApiError> {
    let req = load(state, org_id, id, auth.user_id).await?;
    if req.requester_id == auth.user_id {
        return Err(DomainError::forbidden("approval.self_approval")
            .with_message("quem pede não decide o próprio pedido")
            .into());
    }
    let cap = Capability::parse(&req.capability)?;
    // Quem decide tem `allow` NESSA capacidade (não basta gerir papéis).
    match crate::org::decide(
        state,
        org_id,
        auth.user_id,
        cap,
        ResourceScope::Organization,
    )
    .await?
    {
        Some((_, Decision::Allow)) => {}
        Some(_) => {
            return Err(DomainError::forbidden("authz.missing_capability")
                .with_message(format!("só quem tem {} decide este pedido", cap.as_str()))
                .with_field("capability", cap.as_str())
                .into())
        }
        None => return Err(ApiError::NotFound),
    }
    if !approve && reason.trim().is_empty() {
        return Err(
            DomainError::invalid("approval.reason_required", "recusar exige uma razão")
                .with_field("reason", "texto")
                .into(),
        );
    }
    let updated = sqlx::query(
        "UPDATE approval_requests
            SET status = $3, decided_by = $4, decided_at = now(), reason = $5,
                expires_at = CASE WHEN $3 = 'approved' THEN now() + make_interval(hours => $6) ELSE expires_at END
          WHERE org_id = $1 AND id = $2 AND status = 'pending' AND expires_at > now()",
    )
    .bind(org_id)
    .bind(id)
    .bind(if approve { "approved" } else { "rejected" })
    .bind(auth.user_id)
    .bind(reason.trim())
    .bind(TTL_HOURS as i32)
    .execute(&state.db)
    .await?
    .rows_affected();
    if updated == 0 {
        return Err(DomainError::conflict(
            "approval.not_pending",
            format!("o pedido já não está pendente ({})", req.status),
        )
        .into());
    }
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        if approve {
            "approval.approved"
        } else {
            "approval.rejected"
        },
        &serde_json::json!({"approval_request_id": id, "capability": req.capability,
                            "requester_id": req.requester_id, "reason": reason.trim()})
        .to_string(),
    )
    .await;
    load(state, org_id, id, auth.user_id).await
}

/// Aprova (quem tem `allow` na capacidade, e não é quem pediu). Vale 24 h, uma vez.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/approval-requests/{request_id}/approve", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("request_id" = Uuid, Path)),
    request_body = DecisionReq,
    responses(
        (status = 200, body = ApprovalRequest),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`approval.self_approval` ou `authz.missing_capability`"),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "`approval.not_pending`"),
    )
)]
pub async fn approve(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
    body: Option<Json<DecisionReq>>,
) -> Result<Json<ApprovalRequest>, ApiError> {
    let reason = body.map(|b| b.0.reason).unwrap_or_default();
    Ok(Json(
        decide_request(&state, &auth, org_id, id, true, reason).await?,
    ))
}

/// Recusa, com razão.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/approval-requests/{request_id}/reject", tag = "authorization",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("request_id" = Uuid, Path)),
    request_body = DecisionReq,
    responses(
        (status = 200, body = ApprovalRequest),
        (status = 400, body = crate::openapi::ErrorBody, description = "`approval.reason_required`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody),
    )
)]
pub async fn reject(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
    Json(req): Json<DecisionReq>,
) -> Result<Json<ApprovalRequest>, ApiError> {
    Ok(Json(
        decide_request(&state, &auth, org_id, id, false, req.reason).await?,
    ))
}
