use axum::{
    extract::{ConnectInfo, Request, State},
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

/// Fixed-window in-memory rate limiter, keyed by client IP.
/// Enough for a single instance; swap for Redis when scaling horizontally.
pub struct RateLimiter {
    limit: u32,
    window: Duration,
    hits: DashMap<IpAddr, (Instant, u32)>,
}

impl RateLimiter {
    pub fn new(limit: u32, window: Duration) -> Self {
        Self { limit, window, hits: DashMap::new() }
    }

    pub fn check(&self, ip: IpAddr) -> bool {
        let now = Instant::now();
        let mut entry = self.hits.entry(ip).or_insert((now, 0));
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

/// Applied to /api/auth/* — brute-force protection on credentials endpoints.
pub async fn auth_rate_limit(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    if !state.auth_limiter.check(addr.ip()) {
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
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        assert!(limiter.check(ip));
        assert!(limiter.check(ip));
        assert!(limiter.check(ip));
        assert!(!limiter.check(ip), "4th request in window must be blocked");
    }

    #[test]
    fn different_ips_do_not_interfere() {
        let limiter = RateLimiter::new(1, Duration::from_secs(60));
        let a: IpAddr = "10.0.0.1".parse().unwrap();
        let b: IpAddr = "10.0.0.2".parse().unwrap();
        assert!(limiter.check(a));
        assert!(!limiter.check(a));
        assert!(limiter.check(b), "another IP has its own budget");
    }

    #[test]
    fn window_resets() {
        let limiter = RateLimiter::new(1, Duration::from_millis(10));
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        assert!(limiter.check(ip));
        assert!(!limiter.check(ip));
        std::thread::sleep(Duration::from_millis(15));
        assert!(limiter.check(ip), "budget must reset after the window");
    }
}
