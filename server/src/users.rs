use axum::{
    extract::{Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct UserPublic {
    pub id: Uuid,
    pub email: String,
    pub username: String,
    pub created_at: DateTime<Utc>,
}

pub async fn fetch_public(db: &PgPool, user_id: Uuid) -> Result<UserPublic, ApiError> {
    Ok(sqlx::query_as::<_, UserPublic>(
        "SELECT id, email, username, created_at FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(db)
    .await?)
}

pub async fn me(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<UserPublic>, ApiError> {
    Ok(Json(fetch_public(&state.db, auth.user_id).await?))
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
}

/// Pesquisa utilizadores por email/username (para convidar/partilhar).
/// Devolve no máximo 10; exclui o próprio.
pub async fn search(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Vec<UserPublic>>, ApiError> {
    let term = q.q.trim();
    if term.len() < 2 {
        return Ok(Json(vec![]));
    }
    let pattern = format!("%{}%", term.to_lowercase());
    let users = sqlx::query_as::<_, UserPublic>(
        "SELECT id, email, username, created_at FROM users
         WHERE id <> $1 AND (lower(username) LIKE $2 OR lower(email) LIKE $2)
         ORDER BY username LIMIT 10",
    )
    .bind(auth.user_id)
    .bind(&pattern)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(users))
}
