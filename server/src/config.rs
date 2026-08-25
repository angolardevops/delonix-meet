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
    /// Odoo da PLATAFORMA (`PLATFORM_ODOO_URL` / `PLATFORM_ODOO_DB`): a
    /// instância contra a qual se validam credenciais de quem ainda NÃO tem
    /// conta aqui. É o que permite entrar com a conta Odoo e ver a
    /// organização e os colegas aparecerem sozinhos (ver odoo_sso.rs).
    /// Vazio (omissão) => o login por conta Odoo está DESLIGADO e o
    /// comportamento é o de sempre: só entra quem já foi provisionado.
    pub platform_odoo_url: Option<String>,
    pub platform_odoo_db: Option<String>,
    /// Hosts isentos da guarda anti-SSRF dos webhooks (`WEBHOOK_ALLOW_HOSTS`,
    /// separados por vírgula). Vazio (omissão) => nenhum destino interno é
    /// alcançável, que é o comportamento seguro. Existe porque o integrador
    /// típico — um Odoo on-prem em `10.x` ou `localhost` — seria bloqueado e
    /// ficaria sem o webhook de aceleração. Nomes exactos, nunca redes.
    pub webhook_allow_hosts: Vec<String>,
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
    /// Intervalo de portas UDP da media. O fixo (50000–50200) é o que o K8s
    /// expõe; muda-se quando duas instâncias partilham o mesmo host (R57).
    pub sfu_udp_min: u16,
    pub sfu_udp_max: u16,
    /// Força a media a passar SEMPRE pelo TURN relay (`iceTransportPolicy: relay`
    /// no cliente e no SFU). Em K8s os host candidates do SFU não transportam
    /// media; sem relay-only o ICE liga por um par que passa o check mas fica
    /// preto. `FORCE_TURN_RELAY=1` exige coturn alcançável. Off em local.
    pub force_turn_relay: bool,
    /// URL do Ollama in-cluster (LLM local — soberania: o texto nunca sai do
    /// datacenter). Vazio => IA desligada, fail-open: o MoM fica por regras
    /// (cliente) e a tradução de legendas não aparece.
    pub ollama_url: Option<String>,
    /// Modelo para tradução das legendas (rápido; ex.: qwen2.5:1.5b).
    pub ollama_model_translate: String,
    /// Modelo para o resumo da ata (qualidade; ex.: qwen2.5:7b em prod).
    pub ollama_model_summary: String,
    /// Capacidade da fila de saída de CADA WebSocket (`WS_QUEUE_CAP`). As filas
    /// são LIMITADAS por desenho: um cliente cujo socket TCP estagna (rede
    /// degradada, aba suspensa, cliente parado no depurador) deixa de drenar a
    /// fila, e uma fila ilimitada cresce até à memória do nó acabar — uma sala
    /// com um único consumidor lento derrubava o pod inteiro. Cheia:
    /// descarta-se o que é efémero (legenda parcial, traço, reacção) e
    /// fecha-se o socket se a mensagem for de protocolo. Ver `PeerTx`.
    pub ws_queue_cap: usize,
    /// Tecto de tempo para a composição `ffmpeg` de uma gravação
    /// (`FFMPEG_TIMEOUT_SECS`, default 3600). Sem tecto, um input malformado
    /// pendura o processo para sempre: o directório temporário nunca é
    /// limpo, a gravação nunca entra na biblioteca, e ninguém dá por isso.
    pub ffmpeg_timeout_secs: u64,
    /// Threads que o `ffmpeg` pode usar (`FFMPEG_THREADS`, default 2). É o
    /// travão de CPU que temos sem cgroups: sem ele o `ffmpeg` toma todos os
    /// núcleos do nó e a composição de uma gravação degrada as chamadas VIVAS
    /// que estão a decorrer no mesmo pod.
    pub ffmpeg_threads: u32,
    /// Segundos que o servidor espera, depois do SIGTERM, para as salas
    /// esvaziarem antes de fechar (`DRAIN_GRACE_SECS`, default 40).
    ///
    /// Tem de ser MENOR que o `terminationGracePeriodSeconds` do K8s (45 s no
    /// `deploy/k8s/02-server.yaml`), senão o SIGKILL chega primeiro e o drain
    /// não serve para nada — que é exactamente o que acontecia antes.
    pub drain_grace_secs: u64,
    /// Segundos entre pôr a readiness em 503 e avisar os clientes
    /// (`DRAIN_READINESS_SECS`, default 12).
    ///
    /// Existe porque a ordem importa: avisar primeiro e retirar o pod depois
    /// faz os clientes reconectarem e o balanceador mandá-los de volta para
    /// aqui. O default cobre um `periodSeconds: 10` de readiness com folga.
    pub drain_readiness_secs: u64,
    /// Atraso que se pede ao cliente antes de reconectar (`DRAIN_RECONNECT_MS`,
    /// default 2000). O cliente acrescenta jitter por cima — sem isso, uma sala
    /// inteira reconecta no mesmo milissegundo e o pod novo leva com tudo de uma vez.
    pub drain_reconnect_ms: u64,
    /// Pedidos de autenticação aceites por IP e por minuto (`AUTH_RATE_PER_MIN`,
    /// default 20 — o valor que estava escrito no código).
    ///
    /// Passa a ser configurável por uma razão concreta e não por causa dos
    /// testes: o limite é **por IP**, e uma organização atrás de um único NAT
    /// apresenta-se toda com o mesmo endereço. Cinquenta pessoas a entrar às
    /// nove da manhã esgotam vinte pedidos por minuto e recebem 429 — e o
    /// sintoma, do lado delas, é «a plataforma não deixa entrar».
    ///
    /// O default NÃO muda: quem não configurar nada mantém exactamente o
    /// comportamento anterior. E o valor é preso a um intervalo — isto é um
    /// controlo de segurança, e um `0` ou um número absurdo não podem entrar
    /// por descuido.
    pub auth_rate_per_min: usize,
    /// Capacidade da fila de escrita de CADA track em gravação
    /// (`REC_QUEUE_CAP`, default 2048 ≈ vários segundos de vídeo). A escrita
    /// corre numa thread dedicada; a fila é o que impede um disco lento de
    /// virar consumo de memória sem fim. Cheia, perdem-se pacotes — contados
    /// em `delonix_recording_packets_dropped_total`, nunca em silêncio.
    pub rec_queue_cap: usize,
    /// Capacidade da fila de renegociação do SFU por peer (`NEGO_QUEUE_CAP`).
    /// Coalescível: o estado de subscrição mais recente vence, por isso
    /// transbordar descarta o pedido mais novo e conta a métrica.
    pub nego_queue_cap: usize,
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
        let cors_origins = csv_env("CORS_ORIGINS");
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
            platform_odoo_url: env::var("PLATFORM_ODOO_URL")
                .ok()
                .map(|u| u.trim_end_matches('/').to_string())
                .filter(|u| !u.is_empty()),
            platform_odoo_db: env::var("PLATFORM_ODOO_DB").ok().filter(|d| !d.is_empty()),
            webhook_allow_hosts: csv_env("WEBHOOK_ALLOW_HOSTS"),
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
            sfu_udp_min: bounded_env(
                "SFU_UDP_MIN",
                crate::sfu::SFU_UDP_MIN as usize,
                1_024,
                65_534,
            ) as u16,
            sfu_udp_max: bounded_env(
                "SFU_UDP_MAX",
                crate::sfu::SFU_UDP_MAX as usize,
                1_025,
                65_535,
            ) as u16,
            force_turn_relay: env::var("FORCE_TURN_RELAY").ok().as_deref() == Some("1"),
            ollama_url: env::var("OLLAMA_URL").ok().filter(|s| !s.is_empty()),
            ollama_model_translate: env::var("OLLAMA_MODEL_TRANSLATE")
                .unwrap_or_else(|_| "qwen2.5:1.5b".into()),
            ollama_model_summary: env::var("OLLAMA_MODEL_SUMMARY")
                .unwrap_or_else(|_| "qwen2.5:1.5b".into()),
            ws_queue_cap: bounded_env("WS_QUEUE_CAP", 512, 32, 65_536),
            nego_queue_cap: bounded_env("NEGO_QUEUE_CAP", 64, 4, 4_096),
            rec_queue_cap: bounded_env("REC_QUEUE_CAP", 2_048, 64, 65_536),
            auth_rate_per_min: bounded_env("AUTH_RATE_PER_MIN", 20, 5, 10_000),
            drain_grace_secs: bounded_env("DRAIN_GRACE_SECS", 40, 1, 3_600) as u64,
            drain_readiness_secs: bounded_env("DRAIN_READINESS_SECS", 12, 0, 300) as u64,
            drain_reconnect_ms: bounded_env("DRAIN_RECONNECT_MS", 2_000, 100, 60_000) as u64,
            ffmpeg_timeout_secs: bounded_env("FFMPEG_TIMEOUT_SECS", 3_600, 30, 86_400) as u64,
            ffmpeg_threads: bounded_env("FFMPEG_THREADS", 2, 1, 64) as u32,
        }
    }
}

/// Lê um tamanho de fila do ambiente, preso a `[min, max]`. Um valor
/// inválido ou fora do intervalo cai no default com um aviso em vez de fazer
/// panic: uma fila mal configurada não deve impedir o servidor de arrancar,
/// mas também não pode virar «ilimitada por engano» com um 0 ou um u32 inteiro.
fn bounded_env(var: &str, default: usize, min: usize, max: usize) -> usize {
    match env::var(var) {
        Err(_) => default,
        Ok(v) => match v.trim().parse::<usize>() {
            Ok(n) if (min..=max).contains(&n) => n,
            _ => {
                tracing::warn!(
                    "{var}='{v}' inválido (esperado inteiro em {min}..={max}) — a usar {default}"
                );
                default
            }
        },
    }
}

/// Lê uma variável de ambiente com valores separados por vírgula.
fn csv_env(var: &str) -> Vec<String> {
    env::var(var)
        .ok()
        .map(|s| {
            s.split(',')
                .map(|o| o.trim().to_string())
                .filter(|o| !o.is_empty())
                .collect()
        })
        .unwrap_or_default()
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
