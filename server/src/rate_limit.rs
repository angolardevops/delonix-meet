use axum::{
    extract::{ConnectInfo, Request, State},
    http::{header::RETRY_AFTER, HeaderMap, HeaderValue},
    middleware::Next,
    response::{IntoResponse, Response},
};
use dashmap::DashMap;
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    apikeys::{lookup_key, KeyLookup},
    error::ApiError,
    AppState,
};

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
        self.acquire(key).is_ok()
    }

    /// Como `check`, mas a recusa diz quanto falta para a janela abrir — o
    /// valor real do `Retry-After`, não uma constante.
    pub fn acquire(&self, key: &str) -> Result<(), Duration> {
        let now = Instant::now();
        let mut entry = self.hits.entry(key.to_string()).or_insert((now, 0));
        let (window_start, count) = *entry;
        let elapsed = now.duration_since(window_start);
        if elapsed > self.window {
            *entry = (now, 1);
            return Ok(());
        }
        if count >= self.limit {
            return Err(self.window - elapsed);
        }
        *entry = (window_start, count + 1);
        Ok(())
    }

    /// A chave está esgotada NESTA janela? Não conta como tentativa.
    ///
    /// É o par do `check` para os limitadores que só contam FALHAS (MFA,
    /// R131): pergunta-se antes de verificar, e só a falha chama `check`. Sem
    /// esta pergunta prévia, o código certo passava durante o bloqueio — e um
    /// travão que deixa passar a resposta certa não trava a força bruta.
    pub fn is_blocked(&self, key: &str) -> bool {
        match self.hits.get(key) {
            Some(e) => {
                let (window_start, count) = *e;
                Instant::now().duration_since(window_start) <= self.window && count >= self.limit
            }
            None => false,
        }
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

/// IP real do cliente.
///
/// Só olha para o `X-Forwarded-For` quando o peer é um proxy local/privado (o
/// Nginx, o ingress); numa ligação directa usa o IP da ligação.
///
/// E lê-o **da direita**. Os proxies deste produto ACRESCENTAM
/// (`$proxy_add_x_forwarded_for`): o que o cliente mandou fica à esquerda, e o
/// endereço que o proxy viu fica à direita. Ler o primeiro valor — como isto
/// fazia — era ler o que o cliente escreveu: um pedido com
/// `X-Forwarded-For: 1.2.3.4` ganhava um balde novo no limite por IP a cada
/// valor inventado. O valor certo é o que o proxy de fora escreveu, ou seja, o
/// `hops`-ésimo a contar do fim (`TRUSTED_PROXY_HOPS`, 1 por omissão: um proxy
/// entre o cliente e o servidor, que é o que o compose, o chart e os
/// manifestos montam).
///
/// Com MENOS entradas do que `hops` o pedido não atravessou todos os proxies
/// (alguém de dentro da rede a falar directamente com o servidor): usa-se a
/// mais à esquerda que houver. Um valor que não seja um endereço IP não serve
/// de chave: cai para o IP da ligação.
pub fn client_ip(headers: &HeaderMap, peer: IpAddr, hops: usize) -> String {
    let peer_is_proxy = peer.is_loopback()
        || matches!(peer, IpAddr::V4(v4) if v4.is_private())
        || matches!(peer, IpAddr::V6(v6) if v6.is_loopback());
    if peer_is_proxy {
        let entries: Vec<&str> = headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|s| s.split(','))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        let chosen = entries
            .len()
            .checked_sub(hops.max(1))
            .map_or(entries.first(), |i| entries.get(i));
        if let Some(ip) = chosen.and_then(|s| s.parse::<IpAddr>().ok()) {
            return ip.to_string();
        }
    }
    peer.to_string()
}

/// Applied to /api/auth/* — brute-force protection on credentials endpoints.
/// Limite de autenticação por IP.
///
/// **É por IP, e isso tem uma consequência que se paga em suporte:** uma
/// organização atrás de um único NAT apresenta-se toda com o mesmo endereço.
/// Com o default de 20/min, cinquenta pessoas a entrar às nove da manhã
/// recebem 429 — e do lado delas o sintoma é «a plataforma não deixa entrar».
/// Ajusta-se com `AUTH_RATE_PER_MIN` (ver config.rs). O travão por CONTA
/// (`login_limiter`, 8 em 5 min) é o que trava a força bruta a sério, e esse
/// não depende do IP.
pub async fn auth_rate_limit(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let ip = client_ip(
        request.headers(),
        addr.ip(),
        state.config.trusted_proxy_hops,
    );
    if !state.auth_limiter.check(&ip) {
        return Err(ApiError::TooManyRequests);
    }
    Ok(next.run(request).await)
}

/// `429` com o `Retry-After` real (segundos inteiros, arredondados para cima,
/// nunca 0 — um `Retry-After: 0` convida a repetir já).
fn too_many(retry_in: Duration) -> Response {
    let mut res = ApiError::TooManyRequests.into_response();
    let secs = retry_in.as_secs() + u64::from(retry_in.subsec_nanos() > 0);
    res.headers_mut()
        .insert(RETRY_AFTER, HeaderValue::from(secs.max(1)));
    res
}

/// Balde do limitador da v1 para este pedido: a chave, quando é uma chave
/// válida e não expirada; o IP em todos os outros casos.
///
/// Porque não o hash do que vier no cabeçalho: quem inventasse uma chave
/// diferente em cada pedido teria um balde novo em cada pedido, e o limite
/// deixava de existir. Só uma chave que existe ganha balde próprio.
pub fn v1_bucket(lookup: &KeyLookup, ip: &str) -> String {
    match &lookup.0 {
        Some(k) if k.is_usable(chrono::Utc::now()) => format!("apikey:{}", k.id),
        _ => ip.to_string(),
    }
}

/// Applied to /api/v1/* — limite POR CHAVE (ADR-0004 §4).
///
/// Uma organização atrás de um NAT pode ter várias integrações, cada uma com a
/// sua chave: por IP, a mais faladora esgotava o orçamento das outras. A chave
/// é procurada aqui uma vez e segue nas extensões para o `ApiKeyAuth`, que não
/// a volta a procurar.
pub async fn v1_rate_limit(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let ip = client_ip(
        request.headers(),
        addr.ip(),
        state.config.trusted_proxy_hops,
    );
    let lookup = lookup_key(&state, request.headers()).await?;
    if let Err(retry_in) = state.v1_limiter.acquire(&v1_bucket(&lookup, &ip)) {
        return Ok(too_many(retry_in));
    }
    request.extensions_mut().insert(lookup);
    Ok(next.run(request).await)
}

/// Applied to /api/ice-servers — limite por IP, partilhando o orçamento do
/// `v1_limiter`. Separado do `v1_rate_limit` de propósito: a `/api/ice-servers`
/// autentica por sessão, e escolher o balde por uma chave `dlx_` que a rota
/// nem lê deixava contornar o limite com várias chaves.
pub async fn ip_rate_limit(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let ip = client_ip(
        request.headers(),
        addr.ip(),
        state.config.trusted_proxy_hops,
    );
    if let Err(retry_in) = state.v1_limiter.acquire(&ip) {
        return Ok(too_many(retry_in));
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
    fn is_blocked_does_not_count_and_follows_check() {
        let limiter = RateLimiter::new(2, Duration::from_secs(60));
        for _ in 0..10 {
            assert!(!limiter.is_blocked("u"), "perguntar não gasta tentativas");
        }
        assert!(limiter.check("u"));
        assert!(!limiter.is_blocked("u"));
        assert!(limiter.check("u"));
        assert!(limiter.is_blocked("u"), "esgotado ao fim de 2");
        assert!(!limiter.is_blocked("outra"));
    }

    #[test]
    fn is_blocked_ends_with_the_window() {
        let limiter = RateLimiter::new(1, Duration::from_millis(10));
        assert!(limiter.check("u"));
        assert!(limiter.is_blocked("u"));
        std::thread::sleep(Duration::from_millis(15));
        assert!(!limiter.is_blocked("u"));
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
    fn acquire_diz_quanto_falta_da_janela() {
        let limiter = RateLimiter::new(1, Duration::from_secs(60));
        assert!(limiter.acquire("k").is_ok());
        std::thread::sleep(Duration::from_millis(20));
        let falta = limiter.acquire("k").unwrap_err();
        assert!(falta < Duration::from_secs(60), "{falta:?}");
        assert!(falta > Duration::from_secs(59), "{falta:?}");
    }

    #[test]
    fn retry_after_arredonda_para_cima_e_nunca_e_zero() {
        let h = |d| {
            too_many(d).headers()[RETRY_AFTER]
                .to_str()
                .unwrap()
                .to_string()
        };
        assert_eq!(h(Duration::from_millis(59_001)), "60");
        assert_eq!(h(Duration::from_secs(12)), "12");
        assert_eq!(h(Duration::ZERO), "1");
        assert_eq!(too_many(Duration::ZERO).status(), 429);
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

    fn xff(values: &[&str]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for v in values {
            h.append("x-forwarded-for", v.parse().unwrap());
        }
        h
    }

    #[test]
    fn xff_trusted_only_from_proxy() {
        let h = xff(&["203.0.113.9"]);
        let proxy: IpAddr = "127.0.0.1".parse().unwrap();
        // peer é o proxy (loopback) → o valor que ELE escreveu
        assert_eq!(client_ip(&h, proxy, 1), "203.0.113.9");
        // peer é público (ligação direta) → ignora XFF, usa o peer
        assert_eq!(
            client_ip(&h, "198.51.100.7".parse().unwrap(), 1),
            "198.51.100.7"
        );
    }

    /// O cliente manda um `X-Forwarded-For` seu e o proxy ACRESCENTA o endereço
    /// que viu. O valor forjado fica à esquerda e não pode ser a chave do
    /// limite — era, e bastava mudá-lo a cada pedido para nunca ser travado.
    #[test]
    fn a_forged_xff_does_not_choose_the_rate_limit_key() {
        let proxy: IpAddr = "10.0.0.7".parse().unwrap();
        for forged in ["1.2.3.4", "10.9.9.9", "8.8.8.8, 9.9.9.9"] {
            let h = xff(&[&format!("{forged}, 203.0.113.9")]);
            assert_eq!(client_ip(&h, proxy, 1), "203.0.113.9", "forjado: {forged}");
        }
        // O mesmo com o cabeçalho repetido em vez de separado por vírgulas.
        assert_eq!(
            client_ip(&xff(&["1.2.3.4", "203.0.113.9"]), proxy, 1),
            "203.0.113.9"
        );
    }

    #[test]
    fn hops_count_from_the_right() {
        let proxy: IpAddr = "10.0.0.7".parse().unwrap();
        // Dois proxies de confiança: cliente, depois o endereço do primeiro.
        let h = xff(&["6.6.6.6, 203.0.113.9, 10.0.0.1"]);
        assert_eq!(client_ip(&h, proxy, 2), "203.0.113.9");
        assert_eq!(client_ip(&h, proxy, 1), "10.0.0.1");
        // Menos entradas do que saltos: a mais à esquerda que houver.
        assert_eq!(client_ip(&xff(&["203.0.113.9"]), proxy, 2), "203.0.113.9");
        // `hops = 0` não existe: trata-se como 1, nunca como «o peer».
        assert_eq!(client_ip(&h, proxy, 0), "10.0.0.1");
    }

    #[test]
    fn a_value_that_is_not_an_address_is_never_the_key() {
        let proxy: IpAddr = "127.0.0.1".parse().unwrap();
        assert_eq!(client_ip(&xff(&["nao-e-um-ip"]), proxy, 1), "127.0.0.1");
        assert_eq!(client_ip(&HeaderMap::new(), proxy, 1), "127.0.0.1");
    }
}
