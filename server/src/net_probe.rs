//! Sondagem de rede para o ecrã de pré-entrada: descarga e subida medidas
//! contra ESTE servidor (o mesmo caminho que a reunião vai usar), em vez de um
//! serviço de terceiros.
//!
//! - `GET  /api/net-probe?bytes=N` → N bytes pseudo-aleatórios, `no-store`.
//!   O cliente mede o tempo de descarga.
//! - `POST /api/net-probe` (corpo binário) → `{bytes, server_ms}`: quantos bytes
//!   chegaram e quanto tempo levou a ler o corpo do lado do servidor.
//!
//! Os bytes não precisam de ser criptográficos: servem só para não serem
//! comprimíveis por um proxy pelo caminho (o que falsearia a medida). Por isso
//! é um xorshift e não o gerador de chaves.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use axum::{
    body::Body,
    extract::Query,
    http::{header, HeaderValue},
    response::{IntoResponse, Response},
    Json,
};
use futures_util::StreamExt;
use serde::Deserialize;

use crate::{auth::AuthUser, error::ApiError, rate_limit::RateLimiter};

/// Tecto de uma sondagem, nos dois sentidos.
pub const MAX_PROBE_BYTES: usize = 4 * 1024 * 1024;
const DEFAULT_PROBE_BYTES: usize = 256 * 1024;

/// Sondagens por conta por minuto (descarga e subida contam juntas). Um teste
/// de pré-entrada faz 2–6; isto só trava quem use a rota como gerador de
/// tráfego.
static LIMITER: LazyLock<RateLimiter> =
    LazyLock::new(|| RateLimiter::new(30, Duration::from_secs(60)));

#[derive(Deserialize)]
pub struct ProbeQuery {
    #[serde(default)]
    bytes: Option<usize>,
}

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

fn clamp_bytes(pedido: Option<usize>) -> usize {
    pedido
        .unwrap_or(DEFAULT_PROBE_BYTES)
        .clamp(1, MAX_PROBE_BYTES)
}

pub async fn download(auth: AuthUser, Query(q): Query<ProbeQuery>) -> Result<Response, ApiError> {
    if !LIMITER.check(&auth.user_id.to_string()) {
        return Err(ApiError::TooManyRequests);
    }
    let n = clamp_bytes(q.bytes);
    let mut r = noise(n).into_response();
    let h = r.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(r)
}

pub async fn upload(auth: AuthUser, body: Body) -> Result<Json<serde_json::Value>, ApiError> {
    if !LIMITER.check(&auth.user_id.to_string()) {
        return Err(ApiError::TooManyRequests);
    }
    let inicio = Instant::now();
    let mut stream = body.into_data_stream();
    let mut total = 0usize;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| ApiError::BadRequest("corpo interrompido".into()))?;
        total += chunk.len();
        if total > MAX_PROBE_BYTES {
            return Err(ApiError::BadRequest("sondagem acima do tecto".into()));
        }
    }
    Ok(Json(serde_json::json!({
        "bytes": total,
        "server_ms": inicio.elapsed().as_secs_f64() * 1000.0,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_tamanho_pedido_fica_dentro_do_tecto() {
        assert_eq!(clamp_bytes(None), DEFAULT_PROBE_BYTES);
        assert_eq!(clamp_bytes(Some(0)), 1);
        assert_eq!(clamp_bytes(Some(usize::MAX)), MAX_PROBE_BYTES);
    }

    #[test]
    fn o_ruido_tem_o_tamanho_pedido_e_nao_e_constante() {
        let v = noise(1000);
        assert_eq!(v.len(), 1000);
        let distintos: std::collections::HashSet<u8> = v.iter().copied().collect();
        assert!(
            distintos.len() > 100,
            "comprimível demais: {}",
            distintos.len()
        );
    }
}
