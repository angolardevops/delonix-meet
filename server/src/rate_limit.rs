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

/// Token bucket por-socket para o rate-limit dos WebSockets (`/ws`, `/rtc`).
/// Absorve rajadas legítimas (ICE/renegociação) até `burst` e limita o ritmo
/// sustentado a `refill_per_sec`. Substitui a janela fixa apertada que cortava o
/// próprio anfitrião durante a rajada de ICE — ver regressão **R6**. NÃO voltar a
/// janela fixa: os testes abaixo codificam esta invariante.
pub struct TokenBucket {
    tokens: f64,
    last: Instant,
    burst: f64,
    refill_per_sec: f64,
}

impl TokenBucket {
    pub fn new(burst: f64, refill_per_sec: f64) -> Self {
        Self {
            tokens: burst,
            last: Instant::now(),
            burst,
            refill_per_sec,
        }
    }

    /// Núcleo testável: consome 1 token no instante `now`. `true` = permitido.
    pub fn allow_at(&mut self, now: Instant) -> bool {
        self.tokens = (self.tokens
            + now.saturating_duration_since(self.last).as_secs_f64() * self.refill_per_sec)
            .min(self.burst);
        self.last = now;
        if self.tokens < 1.0 {
            return false;
        }
        self.tokens -= 1.0;
        true
    }

    /// Consome 1 token agora. `true` = permitido; `false` = flood → desligar.
    pub fn allow(&mut self) -> bool {
        self.allow_at(Instant::now())
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

    // ---- Regressão R6: rate-limit do WS = token bucket, NÃO janela fixa ----
    // Estes testes codificam a invariante que já custou uma sessão: uma janela
    // fixa apertada corta o anfitrião na rajada de ICE. O bucket TEM de absorver
    // a rajada até `burst` e só limitar o ritmo sustentado.

    #[test]
    fn r6_token_bucket_absorve_rajada_ate_burst() {
        let mut tb = TokenBucket::new(600.0, 300.0);
        let t0 = Instant::now();
        // A rajada inteira de 600 (ex.: ICE/renegociação) passa no MESMO instante.
        for i in 0..600 {
            assert!(tb.allow_at(t0), "token {i} da rajada devia passar");
        }
        // O 601.º no mesmo instante é cortado (bucket esgotado).
        assert!(!tb.allow_at(t0), "601.º sem refill devia ser cortado");
    }

    #[test]
    fn r6_token_bucket_refila_ao_ritmo_sustentado() {
        let mut tb = TokenBucket::new(600.0, 300.0);
        let t0 = Instant::now();
        for _ in 0..600 {
            tb.allow_at(t0);
        }
        assert!(!tb.allow_at(t0), "esgotado no instante inicial");
        // Passado 1s, refilaram 300 tokens (o ritmo sustentado) — nem mais nem menos.
        let t1 = t0 + Duration::from_secs(1);
        for i in 0..300 {
            assert!(tb.allow_at(t1), "token refilado {i} devia passar");
        }
        assert!(
            !tb.allow_at(t1),
            "301.º após 1s excede o sustentado de 300/s"
        );
    }

    #[test]
    fn r6_uma_janela_fixa_apertada_cortaria_a_rajada() {
        // Prova por contraste: a janela fixa antiga (80/s) cortaria a rajada de
        // ICE que o bucket absorve. Documenta PORQUÊ não voltar a janela fixa.
        let fixed = RateLimiter::new(80, Duration::from_secs(1));
        let mut cut_at = None;
        for i in 0..600 {
            if !fixed.check("host") {
                cut_at = Some(i);
                break;
            }
        }
        assert_eq!(cut_at, Some(80), "janela fixa 80/s corta ao 81.º da rajada");
        // ...enquanto o token bucket deixa passar os 600 (teste acima). Por isso R6.
    }

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
