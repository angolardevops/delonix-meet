//! Sondagem de rede para a «qualidade prevista» da pré-entrada: descarga e
//! subida medidas contra ESTE servidor (o mesmo caminho que a reunião vai
//! usar), em vez de um serviço de terceiros.
//!
//! - `GET  /api/net-probe?bytes=N` → N bytes incomprimíveis, `no-store`. O
//!   cliente mede o tempo de descarga.
//! - `POST /api/net-probe` (corpo binário) → `{bytes, server_ms}`: quantos bytes
//!   chegaram e quanto tempo levou a ler o corpo, medido no servidor.
//!
//! **Isolamento e dados pessoais.** Não há dados de nenhuma organização nem de
//! nenhuma pessoa na rota: pede sessão (não é um gerador de tráfego aberto),
//! não lê nem escreve a base, não regista o IP nem a conta, e a resposta só
//! leva contagens. O limite é por IP ([`AppState::net_probe_limiter`]), e o IP
//! só vive como chave desse limitador em memória, durante a janela.
//!
//! Os bytes não precisam de ser criptográficos: servem só para não serem
//! comprimíveis por um proxy pelo caminho (o que falsearia a medida). Por isso
//! é um xorshift e não o gerador de chaves.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use axum::{
    body::Body,
    extract::{ConnectInfo, Query, State},
    http::{header, HeaderMap, HeaderValue},
    response::{IntoResponse, Response},
    Json,
};
use delonix_meet_core::DomainError;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use crate::{auth::AuthUser, error::ApiError, rate_limit::client_ip, AppState};

/// Tecto de uma sondagem, nos dois sentidos. A pré-entrada usa 128 KiB.
pub const MAX_PROBE_BYTES: usize = 1024 * 1024;
const DEFAULT_PROBE_BYTES: usize = 256 * 1024;
/// Sondagens por IP por minuto (descarga e subida contam juntas). Uma
/// pré-entrada faz 2; o tecto aguenta uma organização atrás de um NAT a entrar
/// ao mesmo tempo e trava quem use a rota para gerar tráfego (60 MiB/min).
pub const PROBES_PER_IP_PER_MINUTE: u32 = 60;

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ProbeQuery {
    /// Bytes a descarregar, `1..=1048576`; omissão 262144. Fora do intervalo
    /// fica no limite mais próximo.
    #[serde(default)]
    pub bytes: Option<usize>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[schema(as = NetProbeUpload)]
pub struct UploadResult {
    /// Bytes recebidos.
    pub bytes: usize,
    /// Tempo a ler o corpo, no servidor (ms).
    pub server_ms: f64,
}

/// Bytes da descarga (só para o spec).
#[derive(utoipa::ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
pub struct ProbeBytes(Vec<u8>);

#[derive(utoipa::OpenApi)]
#[openapi(paths(download, upload), components(schemas(UploadResult)))]
pub struct ApiDoc;

fn noise(n: usize) -> Vec<u8> {
    let mut x: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
        | 1;
    let mut v = Vec::with_capacity(n + 8);
    while v.len() < n {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        v.extend_from_slice(&x.to_le_bytes());
    }
    v.truncate(n);
    v
}

fn clamp_bytes(requested: Option<usize>) -> usize {
    requested
        .unwrap_or(DEFAULT_PROBE_BYTES)
        .clamp(1, MAX_PROBE_BYTES)
}

/// `Some(429)`, com o `Retry-After` real do limitador por IP, quando este
/// endereço já gastou as suas sondagens.
fn limited(state: &AppState, headers: &HeaderMap, addr: SocketAddr) -> Option<Response> {
    let ip = client_ip(headers, addr.ip());
    match state.net_probe_limiter.acquire(&format!("net-probe:{ip}")) {
        Ok(()) => None,
        Err(retry_in) => {
            let mut res = ApiError::Domain(DomainError::new(
                delonix_meet_core::ErrorKind::ResourceExhausted,
                "net_probe.rate_limited",
                "sondagens de rede a mais deste endereço; tenta daqui a pouco",
            ))
            .into_response();
            let secs = retry_in.as_secs() + u64::from(retry_in.subsec_nanos() > 0);
            res.headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(secs.max(1)));
            Some(res)
        }
    }
}

/// Descarga para medir o débito de chegada.
#[utoipa::path(
    get, path = "/api/net-probe", tag = "rooms",
    security(("session" = [])),
    params(ProbeQuery),
    responses(
        (status = 200, body = inline(ProbeBytes), content_type = "application/octet-stream"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 429, body = crate::openapi::ErrorBody, headers(("Retry-After" = u64)), description = "`net_probe.rate_limited`: 60 sondagens por IP por minuto."),
    )
)]
pub async fn download(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    _auth: AuthUser,
    Query(q): Query<ProbeQuery>,
) -> Response {
    if let Some(res) = limited(&state, &headers, addr) {
        return res;
    }
    let mut res = noise(clamp_bytes(q.bytes)).into_response();
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

/// Subida para medir o débito de saída: o servidor conta os bytes e o tempo.
#[utoipa::path(
    post, path = "/api/net-probe", tag = "rooms",
    security(("session" = [])),
    request_body(content = inline(ProbeBytes), content_type = "application/octet-stream"),
    responses(
        (status = 200, body = UploadResult),
        (status = 400, body = crate::openapi::ErrorBody, description = "`net_probe.interrupted`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 413, body = crate::openapi::ErrorBody, description = "`net_probe.too_large`: corpo acima de 1 MiB."),
        (status = 429, body = crate::openapi::ErrorBody, headers(("Retry-After" = u64)), description = "`net_probe.rate_limited`"),
    )
)]
pub async fn upload(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    _auth: AuthUser,
    body: Body,
) -> Response {
    if let Some(res) = limited(&state, &headers, addr) {
        return res;
    }
    let started = Instant::now();
    let mut stream = body.into_data_stream();
    let mut total = 0usize;
    // O `Body` em bruto não passa pelo `DefaultBodyLimit`: o tecto é este.
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            return ApiError::Domain(DomainError::invalid(
                "net_probe.interrupted",
                "o corpo da sondagem foi interrompido",
            ))
            .into_response();
        };
        total += chunk.len();
        if total > MAX_PROBE_BYTES {
            let mut res = ApiError::Domain(DomainError::invalid(
                "net_probe.too_large",
                format!("uma sondagem tem no máximo {MAX_PROBE_BYTES} bytes"),
            ))
            .into_response();
            *res.status_mut() = axum::http::StatusCode::PAYLOAD_TOO_LARGE;
            return res;
        }
    }
    Json(UploadResult {
        bytes: total,
        server_ms: started.elapsed().as_secs_f64() * 1000.0,
    })
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_size_stays_within_the_ceiling() {
        assert_eq!(clamp_bytes(None), DEFAULT_PROBE_BYTES);
        assert_eq!(clamp_bytes(Some(0)), 1);
        assert_eq!(clamp_bytes(Some(usize::MAX)), MAX_PROBE_BYTES);
    }

    #[test]
    fn noise_has_the_requested_size_and_is_not_constant() {
        let v = noise(1000);
        assert_eq!(v.len(), 1000);
        let distinct: std::collections::HashSet<u8> = v.iter().copied().collect();
        assert!(
            distinct.len() > 100,
            "comprimível demais: {}",
            distinct.len()
        );
    }
}
