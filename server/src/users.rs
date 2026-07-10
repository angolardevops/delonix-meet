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
    #[serde(default)]
    pub locale: String,
}

pub async fn fetch_public(db: &PgPool, user_id: Uuid) -> Result<UserPublic, ApiError> {
    Ok(sqlx::query_as::<_, UserPublic>(
        "SELECT id, email, username, created_at, COALESCE(locale, 'pt') AS locale FROM users WHERE id = $1",
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
pub struct UpdateMeReq {
    pub username: Option<String>,
    pub password: Option<String>,
    pub locale: Option<String>,
}

/// Atualiza os próprios dados: username e/ou password (cada campo é opcional).
pub async fn update_me(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<UpdateMeReq>,
) -> Result<Json<UserPublic>, ApiError> {
    if let Some(raw) = req.username.as_deref() {
        let username = raw.trim();
        if username.is_empty() || username.len() > 40 {
            return Err(ApiError::BadRequest(
                "username deve ter 1-40 caracteres".into(),
            ));
        }
        sqlx::query("UPDATE users SET username = $1 WHERE id = $2")
            .bind(username)
            .bind(auth.user_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(password) = req.password.as_deref() {
        if password.len() < 8 {
            return Err(ApiError::BadRequest(
                "a password deve ter pelo menos 8 caracteres".into(),
            ));
        }
        let hash = crate::auth::hash_password(password)?;
        sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
            .bind(hash)
            .bind(auth.user_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(locale) = req.locale.as_deref() {
        let locale = locale.trim();
        if matches!(locale, "pt" | "en" | "fr") {
            sqlx::query("UPDATE users SET locale = $1 WHERE id = $2")
                .bind(locale)
                .bind(auth.user_id)
                .execute(&state.db)
                .await?;
        }
    }
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
    // Isolamento multi-tenant: só encontra utilizadores que partilham uma
    // organização com quem pesquisa (não vaza o diretório de outras empresas).
    let users = sqlx::query_as::<_, UserPublic>(
        "SELECT u.id, u.email, u.username, u.created_at FROM users u
         WHERE u.id <> $1 AND (lower(u.username) LIKE $2 OR lower(u.email) LIKE $2)
           AND EXISTS (SELECT 1 FROM org_members a JOIN org_members b ON a.org_id = b.org_id
                       WHERE a.user_id = $1 AND b.user_id = u.id)
         ORDER BY u.username LIMIT 10",
    )
    .bind(auth.user_id)
    .bind(&pattern)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(users))
}
