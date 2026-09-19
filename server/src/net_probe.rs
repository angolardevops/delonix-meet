//! Sondagem de rede antes de entrar numa sala: descarga e subida contra
//! ESTE servidor, não um CDN — um CDN pode estar mais perto do que o SFU e
//! mentir sobre a rede que a chamada vai realmente usar.
//!
//! Sem afinidade por sala (ADR-0001): qualquer pod responde, o pedido não
//! tem sala nenhuma associada.

use std::sync::Arc;
use std::time::Instant;

use axum::{
    body::Bytes, extract::Query, extract::State, http::header, response::IntoResponse, Json,
};
use serde::{Deserialize, Serialize};

use crate::{auth::AuthUser, error::ApiError, AppState};

/// Tecto por pedido, para descarga e para subida.
pub const MAX_PROBE_BYTES: usize = 4 * 1024 * 1024;
const DEFAULT_PROBE_BYTES: usize = 256 * 1024;

#[derive(Deserialize, utoipa::IntoParams)]
pub struct ProbeQuery {
    /// Bytes a devolver; por omissão 256 KiB, tecto 4 MiB.
    #[serde(default)]
    bytes: Option<usize>,
}

/// Marcador só para o spec — o corpo é binário puro, sem forma.
#[derive(utoipa::ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
pub struct ProbeBytes(Vec<u8>);

/// Sonda de descarga: devolve `bytes` de enchimento. O cliente mede o tempo
/// desde o pedido até ao fim da resposta — por isso `Cache-Control: no-store`
/// é obrigatório, uma resposta em cache mediria a rede local, não o servidor.
#[utoipa::path(
    get, path = "/api/net-probe", tag = "diagnostico",
    security(("session" = [])),
    params(ProbeQuery),
    responses(
        (status = 200, body = inline(ProbeBytes), content_type = "application/octet-stream"),
        (status = 400, description = "`net_probe.invalid_bytes`: fora de 1..=4194304.", body = crate::openapi::ErrorBody),
        (status = 401, description = "Sem sessão.", body = crate::openapi::ErrorBody),
        (status = 429, description = "Mais de 30 sondagens por conta no último minuto.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn download(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<ProbeQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !state.net_probe_limiter.check(&auth.user_id.to_string()) {
        return Err(ApiError::TooManyRequests);
    }
    let bytes = q.bytes.unwrap_or(DEFAULT_PROBE_BYTES);
    if bytes == 0 || bytes > MAX_PROBE_BYTES {
        return Err(delonix_meet_core::DomainError::invalid(
            "net_probe.invalid_bytes",
            format!("bytes tem de estar entre 1 e {MAX_PROBE_BYTES}"),
        )
        .into());
    }
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        vec![0u8; bytes],
    ))
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct UploadProbeResult {
    pub bytes: usize,
    /// Tempo a LER o corpo, medido no servidor — o que o browser não consegue
    /// medir sozinho (o "enviado" dele não é o "recebido" do servidor).
    pub server_ms: u64,
}

/// Sonda de subida. Recebe o corpo em bruto directamente (não pelo
/// extractor `Bytes`, que já teria consumido o corpo antes deste código
/// correr): o relógio arranca mesmo antes de ler, para medir a leitura em
/// si, não o tempo de fila do servidor.
#[utoipa::path(
    post, path = "/api/net-probe", tag = "diagnostico",
    security(("session" = [])),
    request_body(content = inline(ProbeBytes), content_type = "application/octet-stream", description = "Até 4 MiB; o conteúdo é ignorado."),
    responses(
        (status = 200, body = UploadProbeResult),
        (status = 401, description = "Sem sessão.", body = crate::openapi::ErrorBody),
        (status = 413, description = "Corpo acima de 4 MiB."),
        (status = 429, description = "Mais de 30 sondagens por conta no último minuto.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn upload(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    request: axum::extract::Request,
) -> Result<Json<UploadProbeResult>, ApiError> {
    if !state.net_probe_limiter.check(&auth.user_id.to_string()) {
        return Err(ApiError::TooManyRequests);
    }
    let t0 = Instant::now();
    let body: Bytes = axum::body::to_bytes(request.into_body(), MAX_PROBE_BYTES)
        .await
        .map_err(|e| ApiError::BadRequest(format!("corpo ilegível: {e}")))?;
    let server_ms = t0.elapsed().as_millis() as u64;
    Ok(Json(UploadProbeResult {
        bytes: body.len(),
        server_ms: server_ms.max(1),
    }))
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(download, upload),
    components(schemas(ProbeBytes, UploadProbeResult))
)]
pub struct ApiDoc;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_probe_bytes_is_four_mebibytes() {
        assert_eq!(MAX_PROBE_BYTES, 4 * 1024 * 1024);
    }
}
