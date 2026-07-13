use std::env;

const DEV_JWT: &str = "dev-only-secret-change-in-production";
const DEV_TURN: &str = "delonix_turn_dev_secret";
const DEV_DB: &str = "postgres://delonix:delonix_dev@localhost:5435/delonix_meet";

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: String,
    pub jwt_secret: String,
    pub turn_host: String,
    pub turn_secret: String,
    pub access_ttl_secs: i64,
    pub refresh_ttl_secs: i64,
    pub room_token_ttl_secs: i64,
    /// Origens permitidas para CORS (allowlist). Vazio => same-origin only.
    pub cors_origins: Vec<String>,
    pub cookie_secure: bool,
    /// Segredo partilhado que a camada de media (FreeSWITCH/provider) usa para
    /// chamar a API interna de IVR. Vazio => API interna de voz DESATIVADA.
    pub voice_internal_secret: String,
    /// Segredo de plataforma que autoriza o provisionamento de organizações via
    /// `POST /api/v1/admin/orgs` (ex.: o Odoo cria a org de cada empresa e
    /// recebe a chave de API). Vazio => endpoint de provisão DESATIVADO
    /// (fail-closed). Não é uma chave de org — é anterior a qualquer org.
    pub provisioning_secret: String,
    /// Tarifa estimada por minuto (inbound) para o cálculo de custo no CDR.
    pub voice_tariff_inbound: f64,
    /// Diretório onde as gravações são armazenadas (lido uma vez no arranque).
    pub recordings_dir: std::path::PathBuf,
    /// URL do Redis para pub/sub cross-nó (presença multi-instância).
    /// Opcional — se vazio, o servidor opera em modo single-node (sem Redis).
    pub redis_url: Option<String>,
    /// IP EXTERNO/alcançável que o SFU anuncia nos candidatos ICE (NAT 1:1).
    /// Em K8s, o IP da LB/nó — sem isto o SFU só anuncia o IP interno do pod
    /// (inalcançável) e a media não estabelece. Vazio => só host candidates
    /// (ok em local; em K8s a media depende do TURN relay). Ver sfu.rs.
    pub sfu_external_ip: Option<String>,
    /// Força a media a passar SEMPRE pelo TURN relay (`iceTransportPolicy: relay`
    /// no cliente e no SFU). Em K8s os host candidates do SFU não transportam
    /// media; sem relay-only o ICE liga por um par que passa o check mas fica
    /// preto. `FORCE_TURN_RELAY=1` exige coturn alcançável. Off em local.
    pub force_turn_relay: bool,
}

impl Config {
    pub fn from_env() -> Self {
        // Fail-closed: por omissão exige-se segredos fortes. Só se
        // DELONIX_ALLOW_INSECURE=1 (dev) é que se aceitam os defaults.
        let insecure = env::var("DELONIX_ALLOW_INSECURE").ok().as_deref() == Some("1");
        if insecure {
            tracing::warn!(
                "DELONIX_ALLOW_INSECURE=1 — a usar segredos de desenvolvimento. NÃO usar em produção."
            );
        }
        let cors_origins = env::var("CORS_ORIGINS")
            .ok()
            .map(|s| {
                s.split(',')
                    .map(|o| o.trim().to_string())
                    .filter(|o| !o.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        Self {
            database_url: secret("DATABASE_URL", DEV_DB, insecure, 0),
            bind_addr: env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8180".into()),
            jwt_secret: secret("JWT_SECRET", DEV_JWT, insecure, 32),
            turn_host: env::var("TURN_HOST").unwrap_or_else(|_| "localhost:3478".into()),
            turn_secret: secret("TURN_SECRET", DEV_TURN, insecure, 16),
            access_ttl_secs: 15 * 60,
            refresh_ttl_secs: 30 * 24 * 3600,
            room_token_ttl_secs: 5 * 60,
            cors_origins,
            cookie_secure: env::var("COOKIE_INSECURE").ok().as_deref() != Some("1"),
            voice_internal_secret: env::var("VOICE_INTERNAL_SECRET").unwrap_or_default(),
            provisioning_secret: env::var("PROVISIONING_SECRET").unwrap_or_default(),
            voice_tariff_inbound: env::var("VOICE_TARIFF_INBOUND")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0),
            recordings_dir: env::var("RECORDINGS_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| std::path::PathBuf::from("recordings")),
            redis_url: env::var("REDIS_URL").ok().filter(|s| !s.is_empty()),
            sfu_external_ip: env::var("SFU_EXTERNAL_IP").ok().filter(|s| !s.is_empty()),
            force_turn_relay: env::var("FORCE_TURN_RELAY").ok().as_deref() == Some("1"),
        }
    }
}

/// Lê um segredo do ambiente. Em produção (insecure=false) faz panic se estiver
/// ausente, igual ao default de dev, ou abaixo do comprimento mínimo.
fn secret(var: &str, dev_default: &str, insecure: bool, min_len: usize) -> String {
    match env::var(var) {
        Ok(v) if v == dev_default => {
            if insecure {
                v
            } else {
                panic!(
                    "{var} está com o valor default de dev — define um segredo forte em produção"
                )
            }
        }
        Ok(v) if v.len() < min_len => {
            panic!("{var} tem de ter pelo menos {min_len} caracteres")
        }
        Ok(v) => v,
        Err(_) => {
            if insecure {
                dev_default.to_string()
            } else {
                panic!("{var} tem de estar definido em produção (ou define DELONIX_ALLOW_INSECURE=1 em dev)")
            }
        }
    }
}
