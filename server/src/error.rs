use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    BadRequest(String),
    #[error("unauthorized")]
    Unauthorized,
    /// Autenticado, mas sem o papel que a operação exige. Distinto de
    /// `Unauthorized` (401), que o cliente web lê como «a sessão não serve» e
    /// tenta renovar: uma falta de PERMISSÃO não se resolve renovando a sessão.
    #[error("forbidden")]
    Forbidden,
    #[error("{0}")]
    Conflict(String),
    #[error("not found")]
    NotFound,
    #[error("too many requests")]
    TooManyRequests,
    /// Este nó não pode servir AGORA, mas outro pode — é o caso do drain. É
    /// diferente de `Unauthorized` (nunca pode) e de `Internal` (avariou): diz
    /// ao cliente para voltar a tentar, e o balanceador manda-o para outro pod.
    #[error("{0}")]
    ServiceUnavailable(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl ApiError {
    pub fn internal<E: std::fmt::Display>(e: E) -> Self {
        Self::Internal(e.to_string())
    }

    /// Uma violação de unicidade vira `409` com a mensagem dada; qualquer
    /// outro erro da base segue o caminho de sempre. É o helper que a skill
    /// `delonix-meet-backend` pede em vez de mais um `match` à mão.
    pub fn from_unique(e: sqlx::Error, conflict: &str) -> Self {
        match &e {
            sqlx::Error::Database(db) if db.is_unique_violation() => {
                Self::Conflict(conflict.to_string())
            }
            _ => e.into(),
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        match &e {
            sqlx::Error::RowNotFound => ApiError::NotFound,
            _ => ApiError::internal(e),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m.clone()),
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized".into()),
            ApiError::Forbidden => (StatusCode::FORBIDDEN, "forbidden".into()),
            ApiError::Conflict(m) => (StatusCode::CONFLICT, m.clone()),
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not found".into()),
            ApiError::TooManyRequests => {
                (StatusCode::TOO_MANY_REQUESTS, "too many requests".into())
            }
            ApiError::ServiceUnavailable(m) => (StatusCode::SERVICE_UNAVAILABLE, m.clone()),
            ApiError::Internal(e) => {
                tracing::error!(error = %e, "internal error");
                // Never leak internals to the client.
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
            }
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}
