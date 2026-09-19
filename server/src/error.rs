//! Erro das superfícies HTTP e o seu envelope.
//!
//! Envelope (todas as superfícies — BFF, v1, operador, integrações):
//!
//! ```json
//! {"error": "título demasiado longo", "code": "meeting.title_too_long",
//!  "details": [{"field": "title", "description": "máximo 140"}], "request_id": "…"}
//! ```
//!
//! - `code` é **contrato** (estável); `error` é a mensagem para pessoas.
//! - Porquê plano e não `{"error": {"code": …}}` (a forma que o ADR-0004 §4
//!   previa): o `web/src/api.ts` e o módulo Odoo `nk_delonix_meet` lêem
//!   `body.error` como texto. Aninhar partia os dois — e a v1 só quebra com v2.
//!   O plano acrescenta sem remover (ADR-0006 §3).
//! - `request_id` é o mesmo do cabeçalho `X-Request-Id` e dos logs: é o que se
//!   pede a quem reporta um erro.

use axum::{
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use delonix_meet_core::{DomainError, ErrorKind};
use serde_json::json;

tokio::task_local! {
    /// Identificador do pedido em curso, posto pelo middleware `request_id`.
    pub static REQUEST_ID: String;
}

/// O id do pedido em curso, se o handler correr dentro do middleware.
pub fn current_request_id() -> Option<String> {
    REQUEST_ID.try_with(|id| id.clone()).ok()
}

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
    /// Este nó não pode servir AGORA, mas outro pode — é o caso do drain. É
    /// diferente de `Unauthorized` (nunca pode) e de `Internal` (avariou): diz
    /// ao cliente para voltar a tentar, e o balanceador manda-o para outro pod.
    #[error("{0}")]
    ServiceUnavailable(String),
    #[error("internal error: {0}")]
    Internal(String),
    /// Erro do domínio, com código estável próprio. É a forma de código NOVO:
    /// as variantes acima dão códigos genéricos e ficam para o código herdado.
    #[error(transparent)]
    Domain(#[from] DomainError),
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

    /// Estado HTTP, código estável e mensagem pública.
    fn parts(&self) -> (StatusCode, &'static str, String) {
        match self {
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, "invalid_argument", m.clone()),
            ApiError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "auth.unauthenticated",
                "unauthorized".into(),
            ),
            ApiError::Forbidden => (
                StatusCode::FORBIDDEN,
                "permission_denied",
                "forbidden".into(),
            ),
            ApiError::Conflict(m) => (StatusCode::CONFLICT, "conflict", m.clone()),
            ApiError::Unprocessable(m) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "failed_precondition",
                m.clone(),
            ),
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not_found", "not found".into()),
            ApiError::TooManyRequests => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "too many requests".into(),
            ),
            ApiError::ServiceUnavailable(m) => {
                (StatusCode::SERVICE_UNAVAILABLE, "unavailable", m.clone())
            }
            ApiError::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal error".into(),
            ),
            ApiError::Domain(e) => {
                let msg = if e.kind == ErrorKind::Internal {
                    "internal error".to_string()
                } else {
                    e.message.clone()
                };
                (status_of(e.kind), e.code, msg)
            }
        }
    }
}

/// Tradução canónica classe → HTTP. É a única tabela; o gRPC tem a sua em
/// paralelo (`delonix-meet-api`), a partir da mesma `ErrorKind`.
pub fn status_of(kind: ErrorKind) -> StatusCode {
    match kind {
        ErrorKind::InvalidArgument => StatusCode::BAD_REQUEST,
        ErrorKind::FailedPrecondition => StatusCode::UNPROCESSABLE_ENTITY,
        ErrorKind::Unauthenticated => StatusCode::UNAUTHORIZED,
        ErrorKind::PermissionDenied => StatusCode::FORBIDDEN,
        ErrorKind::NotFound => StatusCode::NOT_FOUND,
        ErrorKind::Conflict => StatusCode::CONFLICT,
        ErrorKind::ResourceExhausted => StatusCode::TOO_MANY_REQUESTS,
        ErrorKind::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        ErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
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
        match &self {
            ApiError::Internal(e) => tracing::error!(error = %e, "internal error"),
            ApiError::Domain(e) if e.kind == ErrorKind::Internal => {
                tracing::error!(error = %e.message, code = e.code, "internal error")
            }
            _ => {}
        }
        let (status, code, msg) = self.parts();
        let details = match &self {
            ApiError::Domain(e) => json!(e.details),
            _ => json!([]),
        };
        let request_id = current_request_id();
        let mut res = (
            status,
            Json(json!({
                "error": msg,
                "code": code,
                "details": details,
                "request_id": request_id,
            })),
        )
            .into_response();
        if status == StatusCode::TOO_MANY_REQUESTS {
            res.headers_mut()
                .insert("Retry-After", HeaderValue::from_static("60"));
        }
        res
    }
}

/// Põe no envelope as respostas de erro que não passaram por `ApiError`: as
/// recusas dos extractores do axum (`Json` mal formado dá 400/415/422 em texto),
/// o 405 e o 404 de rota inexistente. Sem isto o cliente tinha dois formatos
/// de erro — e o gerado a partir do OpenAPI só conhece um.
///
/// Não toca em: respostas de sucesso, respostas que já são JSON, upgrades de
/// WebSocket, e as sondas (`/health`, `/ready`), cujo corpo é texto de
/// propósito.
pub async fn normalize_error_body(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let path = req.uri().path().to_owned();
    let res = next.run(req).await;
    let status = res.status();
    if status.as_u16() < 400 || path == "/health" || path == "/ready" {
        return res;
    }
    let is_json = res
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("application/json"));
    if is_json {
        return res;
    }
    let (parts, body) = res.into_parts();
    // As recusas do axum são frases curtas; um corpo grande não é uma recusa.
    let text = match axum::body::to_bytes(body, 8 * 1024).await {
        Ok(b) => String::from_utf8_lossy(&b).trim().to_string(),
        Err(_) => String::new(),
    };
    let code = match status {
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => "invalid_argument",
        StatusCode::NOT_FOUND => "not_found",
        StatusCode::METHOD_NOT_ALLOWED => "method_not_allowed",
        StatusCode::PAYLOAD_TOO_LARGE => "payload_too_large",
        StatusCode::UNSUPPORTED_MEDIA_TYPE => "unsupported_media_type",
        StatusCode::UNAUTHORIZED => "auth.unauthenticated",
        StatusCode::FORBIDDEN => "permission_denied",
        StatusCode::TOO_MANY_REQUESTS => "rate_limited",
        s if s.is_server_error() => "internal",
        _ => "error",
    };
    let msg = if text.is_empty() || status.is_server_error() {
        status.canonical_reason().unwrap_or("error").to_lowercase()
    } else {
        text
    };
    let mut out = (
        status,
        Json(
            json!({"error": msg, "code": code, "details": [], "request_id": current_request_id()}),
        ),
    )
        .into_response();
    for (k, v) in parts.headers.iter() {
        if k != axum::http::header::CONTENT_TYPE && k != axum::http::header::CONTENT_LENGTH {
            out.headers_mut().insert(k.clone(), v.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn envelope_is_flat_and_keeps_error_text() {
        let res = REQUEST_ID
            .scope("req-1".into(), async {
                ApiError::Domain(
                    DomainError::invalid("meeting.title_too_long", "título demasiado longo")
                        .with_field("title", "máximo 140"),
                )
                .into_response()
            })
            .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["error"], "título demasiado longo");
        assert_eq!(v["code"], "meeting.title_too_long");
        assert_eq!(v["details"][0]["field"], "title");
        assert_eq!(v["request_id"], "req-1");
    }

    #[tokio::test]
    async fn internal_never_leaks_detail() {
        let res = ApiError::internal("password=segredo").into_response();
        let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(!text.contains("segredo"));
        assert!(text.contains("\"code\":\"internal\""));
    }

    #[test]
    fn precondition_is_422() {
        assert_eq!(
            status_of(ErrorKind::FailedPrecondition),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    #[tokio::test]
    async fn rate_limited_sets_retry_after() {
        let res = ApiError::TooManyRequests.into_response();
        assert_eq!(res.headers()["Retry-After"], "60");
    }
}
