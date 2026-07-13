//! Registos de auditoria — trilha de eventos de segurança/administração.
//!
//! Escrita: `audit::log(...)` best-effort nos pontos-chave (login, membros,
//! webhooks, api keys, partilhas de gravação, definições da org). Nunca falha
//! a operação principal — erros de escrita são só registados no tracing.
//! Leitura: `GET /api/orgs/{org_id}/audit` (admins da org). Eventos sem org
//! (ex.: login) aparecem quando o ator é membro da org consultada.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, org::require_admin_pub, AppState};

/// Escreve um evento de auditoria (best-effort — nunca propaga erro).
pub async fn log(db: &PgPool, org_id: Option<Uuid>, actor_id: Uuid, action: &str, target: &str) {
    let res = sqlx::query(
        "INSERT INTO audit_logs (org_id, actor_id, action, target) VALUES ($1, $2, $3, $4)",
    )
    .bind(org_id)
    .bind(actor_id)
    .bind(action)
    .bind(target)
    .execute(db)
    .await;
    if let Err(e) = res {
        tracing::warn!(error = %e, action, "audit log write failed");
    }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AuditEntry {
    pub id: i64,
    pub actor: String,
    pub action: String,
    pub target: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct AuditQuery {
    #[serde(default)]
    pub limit: Option<i64>,
}

/// Últimos eventos de auditoria da org (só admins).
pub async fn list(
    State(state): State<Arc<AppState>>,
    Path(org_id): Path<Uuid>,
    Query(q): Query<AuditQuery>,
    auth: AuthUser,
) -> Result<Json<Vec<AuditEntry>>, ApiError> {
    require_admin_pub(&state, org_id, auth.user_id).await?;
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let rows: Vec<AuditEntry> = sqlx::query_as(
        "SELECT a.id, u.username AS actor, a.action, a.target, a.created_at
         FROM audit_logs a
         JOIN users u ON u.id = a.actor_id
         WHERE a.org_id = $1
            OR (a.org_id IS NULL AND EXISTS (
                  SELECT 1 FROM org_members om WHERE om.org_id = $1 AND om.user_id = a.actor_id))
         ORDER BY a.created_at DESC
         LIMIT $2",
    )
    .bind(org_id)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}
