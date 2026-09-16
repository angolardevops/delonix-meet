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
    /// Bem formado, mas não pode ser cumprido tal como pedido (ex.: um SMS sem
    /// rota). Distinto de `BadRequest`: repetir o mesmo pedido não o corrige,
    /// mudar o estado (seleccionar um dispositivo, contratar o operador) sim.
    #[error("{0}")]
    Unprocessable(String),
    #[error("not found")]
    NotFound,
    #[error("too many requests")]
    TooManyRequests,
    /// Igual a `TooManyRequests`, mas diz ao cliente QUANDO voltar
    /// (`Retry-After`, em segundos). É o que se usa em rotas novas: sem o
    /// cabeçalho, o cliente só pode adivinhar, e adivinhar é tentar já outra vez.
    #[error("too many requests")]
    RateLimited { retry_after_secs: u64 },
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
            ApiError::Unprocessable(m) => (StatusCode::UNPROCESSABLE_ENTITY, m.clone()),
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not found".into()),
            ApiError::TooManyRequests => {
                (StatusCode::TOO_MANY_REQUESTS, "too many requests".into())
            }
            ApiError::RateLimited { retry_after_secs } => {
                let mut res = (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(json!({ "error": "too many requests" })),
                )
                    .into_response();
                res.headers_mut().insert(
                    axum::http::header::RETRY_AFTER,
                    axum::http::HeaderValue::from(*retry_after_secs),
                );
                return res;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limited_leva_retry_after_e_o_mesmo_corpo() {
        let res = ApiError::RateLimited {
            retry_after_secs: 60,
        }
        .into_response();
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            res.headers()
                .get(axum::http::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok()),
            Some("60")
        );
    }
}
