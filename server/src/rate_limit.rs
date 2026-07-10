use axum::{
    extract::{ConnectInfo, Request, State},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};
use dashmap::DashMap;
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{error::ApiError, AppState};

/// Fixed-window in-memory rate limiter, keyed by an arbitrary string (IP ou
/// conta). Suficiente para uma instância; trocar por Redis ao escalar.
pub struct RateLimiter {
    limit: u32,
    window: Duration,
    hits: DashMap<String, (Instant, u32)>,
}

impl RateLimiter {
    pub fn new(limit: u32, window: Duration) -> Self {
        Self {
            limit,
            window,
            hits: DashMap::new(),
        }
    }

    pub fn check(&self, key: &str) -> bool {
        let now = Instant::now();
        let mut entry = self.hits.entry(key.to_string()).or_insert((now, 0));
        let (window_start, count) = *entry;
        if now.duration_since(window_start) > self.window {
            *entry = (now, 1);
            return true;
        }
        if count >= self.limit {
            return false;
        }
        *entry = (window_start, count + 1);
        true
    }
}

/// IP real do cliente. Só confia em `X-Forwarded-For` quando o peer é um proxy
/// local/privado (o Nginx); caso contrário usa o IP da ligação. Impede que um
/// atacante direto forje o XFF para escapar ao rate-limit.
pub fn client_ip(headers: &HeaderMap, peer: IpAddr) -> String {
    let peer_is_proxy = peer.is_loopback()
        || matches!(peer, IpAddr::V4(v4) if v4.is_private())
        || matches!(peer, IpAddr::V6(v6) if v6.is_loopback());
    if peer_is_proxy {
        if let Some(first) = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').next())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return first.to_string();
        }
    }
    peer.to_string()
}

/// Applied to /api/auth/* — brute-force protection on credentials endpoints.
pub async fn auth_rate_limit(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let ip = client_ip(request.headers(), addr.ip());
    if !state.auth_limiter.check(&ip) {
        return Err(ApiError::TooManyRequests);
    }
    Ok(next.run(request).await)
}

/// Applied to /api/v1/* — protege a superfície autenticada por chave de API.
pub async fn v1_rate_limit(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let ip = client_ip(request.headers(), addr.ip());
    if !state.v1_limiter.check(&ip) {
        return Err(ApiError::TooManyRequests);
    }
    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_after_limit_within_window() {
        let limiter = RateLimiter::new(3, Duration::from_secs(60));
        assert!(limiter.check("10.0.0.1"));
        assert!(limiter.check("10.0.0.1"));
        assert!(limiter.check("10.0.0.1"));
        assert!(
            !limiter.check("10.0.0.1"),
            "4th request in window must be blocked"
        );
    }

    #[test]
    fn different_keys_do_not_interfere() {
        let limiter = RateLimiter::new(1, Duration::from_secs(60));
        assert!(limiter.check("10.0.0.1"));
        assert!(!limiter.check("10.0.0.1"));
        assert!(limiter.check("10.0.0.2"), "another key has its own budget");
        assert!(
            limiter.check("acct:user@example.com"),
            "account key is independent"
        );
    }

    #[test]
    fn window_resets() {
        let limiter = RateLimiter::new(1, Duration::from_millis(10));
        assert!(limiter.check("10.0.0.1"));
        assert!(!limiter.check("10.0.0.1"));
        std::thread::sleep(Duration::from_millis(15));
        assert!(
            limiter.check("10.0.0.1"),
            "budget must reset after the window"
        );
    }

    #[test]
    fn xff_trusted_only_from_proxy() {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", "203.0.113.9, 10.0.0.1".parse().unwrap());
        // peer é o proxy (loopback) → confia no primeiro XFF
        assert_eq!(client_ip(&h, "127.0.0.1".parse().unwrap()), "203.0.113.9");
        // peer é público (ligação direta) → ignora XFF, usa o peer
        assert_eq!(
            client_ip(&h, "198.51.100.7".parse().unwrap()),
            "198.51.100.7"
        );
    }
}
