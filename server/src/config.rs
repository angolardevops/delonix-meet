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
    /// Porque é que o `voice_internal_secret` NÃO serve, decidido uma vez no
    /// arranque (`voice_secret_refusal`). `Some` => as rotas de IVR respondem
    /// 503 com esta razão, sem sequer ler o cabeçalho. R154: o manifesto K8s
    /// trazia um valor escrito no repositório público, e quem o lesse validava
    /// PINs e injectava CDRs. Não faz panic — quem não usa voz não perde o
    /// servidor inteiro por causa disto.
    pub voice_secret_refusal: Option<&'static str>,
    /// Segredo de plataforma que autoriza o provisionamento de organizações via
    /// `POST /api/v1/admin/orgs` (ex.: o Odoo cria a org de cada empresa e
    /// recebe a chave de API). Vazio => endpoint de provisão DESATIVADO
    /// (fail-closed). Não é uma chave de org — é anterior a qualquer org.
    pub provisioning_secret: String,
    /// Porque é que o `provisioning_secret` NÃO serve, decidido uma vez no
    /// arranque (`provisioning_secret_refusal`). `Some` => `POST
    /// /api/v1/admin/orgs` responde 503 com esta razão. R155: o manifesto K8s
    /// trazia um valor escrito no repositório público.
    pub provisioning_secret_refusal: Option<&'static str>,
    /// Administradores da PLATAFORMA (`PLATFORM_ADMIN_USER_IDS`, UUIDs de
    /// utilizador separados por vírgula). Vazio (omissão) => ninguém administra
    /// a plataforma pela API (fail-closed).
    ///
    /// Porquê uma lista explícita e não «admin de uma org»: o registo público
    /// cria SEMPRE o autor como admin da org nova, por isso «admin de alguma
    /// org» era qualquer pessoa na Internet (auditoria 2026-09-16, S1).
    ///
    /// Porquê UUIDs e não emails: não há verificação de email no registo. Um
    /// email declarado antes de a conta existir podia ser registado primeiro
    /// por quem o soubesse — e herdava a plataforma. Um UUID só existe depois
    /// de a conta nascer, e é o operador que o vai buscar.
    pub platform_admin_user_ids: Vec<uuid::Uuid>,
    /// Ligações SMPP aos operadores móveis (ADR-0005):
    /// `smpp://system_id:password@host:2775?source_addr=DELONIX`. Ausente =>
    /// operador por contratar, e o encaminhamento não o escolhe. São da
    /// PLATAFORMA (o contrato é da Delonix), por isso vêm do ambiente e não de
    /// uma tabela — não há cifra de segredos em repouso (S5).
    pub sms_unitel_smpp: Option<String>,
    pub sms_movicel_smpp: Option<String>,
    pub sms_africell_smpp: Option<String>,
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
    /// Emissões em directo simultâneas por nó (`MAX_DIRECTOS`, default 2).
    ///
    /// O tecto existe porque o pod tem `limits.cpu: 1000m` e a sala em directo
    /// vive no MESMO pod que a serve (ADR-0001). Cada emissão copia o vídeo e
    /// só transcodifica o áudio — barato —, mas «barato» vezes N deixa de ser.
    /// Sem tecto, uma organização entusiasmada derruba as chamadas do nó.
    pub max_directos: usize,
    /// Destinos RTMP simultâneos POR emissão (`MAX_DESTINOS_POR_DIRECTO`,
    /// default 4) — o "multi-canal tipo StreamYard": um `ffmpeg` com N saídas
    /// `-f flv`. O tecto aqui é diferente do `max_directos`: aquele limita
    /// quantas SALAS emitem ao mesmo tempo; este limita quantas PLATAFORMAS
    /// uma única emissão alimenta — cada destino a mais é mais uma ligação
    /// TCP e mais banda de saída do mesmo pod, mesmo copiando o vídeo.
    pub max_destinos_por_directo: usize,
    /// Binário do ffmpeg (`FFMPEG_BIN`, default `ffmpeg`).
    ///
    /// Configurável porque nem toda a instalação tem o ffmpeg no PATH — e
    /// porque permite apontá-lo a um invólucro em desenvolvimento sem o
    /// instalar no host.
    pub ffmpeg_bin: String,
    /// Threads do ffmpeg de cada emissão (`DIRECTO_THREADS`, default 1).
    ///
    /// Um por emissão, não dois: a composição de uma gravação é diferível e
    /// pode gastar mais; um directo corre AO LADO de chamadas vivas.
    pub directo_threads: u32,
    /// Segundos que o servidor espera, depois do SIGTERM, para as salas
    /// esvaziarem antes de fechar (`DRAIN_GRACE_SECS`, default 40).
    ///
    /// Tem de ser MENOR que o `terminationGracePeriodSeconds` do K8s (45 s no
    /// `deploy/k8s/02-server.yaml`), senão o SIGKILL chega primeiro e o drain
    /// não serve para nada — que é exactamente o que acontecia antes.
    pub drain_grace_secs: u64,
    /// Quanto tempo um lugar fica reservado depois de o socket cair (R91),
    /// em segundos (`RECONNECT_GRACE_SECS`, por omissão 45).
    ///
    /// O tecto é curto de propósito. Enquanto o lugar está reservado o
    /// participante CONTA para a sala: ocupa quota, aparece no roster, e a
    /// gravação não finaliza. Uma janela generosa transforma um browser
    /// fechado numa sala que nunca esvazia.
    pub reconnect_grace_secs: u64,
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
            database_url: secret("DATABASE_URL", &DB_RULE, insecure),
            bind_addr: env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8180".into()),
            jwt_secret: secret("JWT_SECRET", &JWT_RULE, insecure),
            turn_host: env::var("TURN_HOST").unwrap_or_else(|_| "localhost:3478".into()),
            turn_secret: secret("TURN_SECRET", &TURN_RULE, insecure),
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
            voice_secret_refusal: {
                let v = env::var("VOICE_INTERNAL_SECRET").unwrap_or_default();
                let refusal = voice_secret_refusal(&v, insecure);
                if let Some(r) = refusal {
                    tracing::warn!("API interna de IVR (/api/voice/ivr/*) DESLIGADA: {r}");
                }
                refusal
            },
            provisioning_secret: env::var("PROVISIONING_SECRET").unwrap_or_default(),
            provisioning_secret_refusal: {
                let v = env::var("PROVISIONING_SECRET").unwrap_or_default();
                let refusal = provisioning_secret_refusal(&v, insecure);
                if let Some(r) = refusal {
                    tracing::warn!(
                        "Provisionamento de organizações (/api/v1/admin/orgs) DESLIGADO: {r}"
                    );
                }
                refusal
            },
            platform_admin_user_ids: uuid_list("PLATFORM_ADMIN_USER_IDS"),
            sms_unitel_smpp: env::var("SMS_UNITEL_SMPP").ok().filter(|v| !v.is_empty()),
            sms_movicel_smpp: env::var("SMS_MOVICEL_SMPP").ok().filter(|v| !v.is_empty()),
            sms_africell_smpp: env::var("SMS_AFRICELL_SMPP").ok().filter(|v| !v.is_empty()),
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
            reconnect_grace_secs: bounded_env("RECONNECT_GRACE_SECS", 45, 5, 300) as u64,
            drain_readiness_secs: bounded_env("DRAIN_READINESS_SECS", 12, 0, 300) as u64,
            drain_reconnect_ms: bounded_env("DRAIN_RECONNECT_MS", 2_000, 100, 60_000) as u64,
            ffmpeg_timeout_secs: bounded_env("FFMPEG_TIMEOUT_SECS", 3_600, 30, 86_400) as u64,
            ffmpeg_threads: bounded_env("FFMPEG_THREADS", 2, 1, 64) as u32,
            max_directos: bounded_env("MAX_DIRECTOS", 2, 0, 32),
            max_destinos_por_directo: bounded_env("MAX_DESTINOS_POR_DIRECTO", 4, 1, 8),
            directo_threads: bounded_env("DIRECTO_THREADS", 1, 1, 16) as u32,
            ffmpeg_bin: env::var("FFMPEG_BIN").unwrap_or_else(|_| "ffmpeg".into()),
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
/// Lista de UUIDs separados por vírgula. Um valor mal escrito faz panic no
/// arranque: ignorá-lo em silêncio deixava o operador convencido de que
/// declarou um administrador que o servidor nunca reconheceu.
fn uuid_list(var: &str) -> Vec<uuid::Uuid> {
    csv_env(var)
        .iter()
        .map(|v| {
            v.parse().unwrap_or_else(|_| {
                panic!("{var}: «{v}» não é um UUID de utilizador (SELECT id FROM users WHERE email = …)")
            })
        })
        .collect()
}

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

/// Segredo de voz mínimo: 32 caracteres, o mesmo chão do `JWT_SECRET`. O
/// Ansible gera 32 hexadecimais (128 bits); `openssl rand -hex 32` dá 64.
pub const VOICE_SECRET_MIN_LEN: usize = 32;

// ============================================================
//  Segredos QUEIMADOS (R154, R155).
//
//  Duas listas por segredo, e a diferença entre elas é o que deixa o dev
//  continuar a funcionar sem abrir a porta em produção:
//
//  - `BURNED_*` — valores que estiveram em ficheiros de DEPLOY versionados
//    deste repositório PÚBLICO (`deploy/k8s/01-config.yaml`, helm-values).
//    Nunca foram de dev: foram aplicados a clusters. Recusados em QUALQUER
//    modo, incluindo `DELONIX_ALLOW_INSECURE=1` — um valor que está no GitHub
//    não é «um segredo de dev», é um segredo de outra pessoa.
//  - `DEV_*` — valores de desenvolvimento conhecidos (docker-compose, Makefile,
//    scripts de demo, marcadores `CHANGE_ME`). Aceites com
//    `DELONIX_ALLOW_INSECURE=1`, recusados em produção.
//
//  Cada `BURNED_*` tem a decisão escrita em `scripts/leaked-secrets-accepted.txt`
//  (um teste garante-o) e o `check-repo-hygiene.sh` impede-o de voltar a um
//  ficheiro seguido. O histórico do git não se reescreve.
// ============================================================

/// `VOICE_INTERNAL_SECRET` publicado. De 98f5b28 (2026-07-10) até R154.
pub const BURNED_VOICE_SECRETS: &[&str] = &["voice-internal-secret-for-pstn"];
/// `VOICE_INTERNAL_SECRET` de dev: `Makefile`, `VOICE_SECRET ?=`.
pub const DEV_VOICE_SECRETS: &[&str] = &["dev-voice-secret-abc123"];

/// `JWT_SECRET` publicado em `deploy/k8s/01-config.yaml` (98f5b28 → R155).
/// Quem o tem assina tokens de sessão para QUALQUER utilizador.
pub const BURNED_JWT_SECRETS: &[&str] = &["stage-jwt-secret-min-32-chars-abcdef123456"];
/// `TURN_SECRET` publicado em `deploy/k8s/01-config.yaml` (98f5b28 → R155).
/// Quem o tem gera credenciais TURN válidas e usa o coturn como relay aberto.
pub const BURNED_TURN_SECRETS: &[&str] = &["stage-turn-secret-key"];
/// Passwords do Postgres publicadas: `01-config.yaml` e
/// `helm-values/postgres-stage-values.yaml` (`delonix_dev_pass`),
/// `helm-values/postgres-values.yaml` (`delonix_prod_pass`). Verificadas na
/// password embutida no `DATABASE_URL`, seja qual for o host.
pub const BURNED_DB_PASSWORDS: &[&str] = &["delonix_dev_pass", "delonix_prod_pass"];
/// Passwords de dev e marcadores por trocar (`docker-compose.yml`, CI,
/// `deploy/delonix.env.example`, `50-data.yaml`).
pub const DEV_DB_PASSWORDS: &[&str] = &[
    "delonix_dev",
    "CHANGE_ME",
    "CHANGE_ME_same_as_app_DATABASE_URL",
    "TROCAR_PASSWORD_FORTE",
];
/// `PROVISIONING_SECRET` publicado em `deploy/k8s/01-config.yaml`
/// (8fdaab8, 2026-07-14 → R155). Quem o tem cria organizações e recebe a
/// chave de API de cada uma.
pub const BURNED_PROVISIONING_SECRETS: &[&str] =
    &["dlxprov_bcdc13c52115d2b67942298b6d548b65f47980470090a2e0"];
/// `PROVISIONING_SECRET` de dev: `deploy/demo-kaeso.sh` (com
/// `DELONIX_ALLOW_INSECURE=1`).
pub const DEV_PROVISIONING_SECRETS: &[&str] = &["kaeso_demo_provisioning_secret"];
/// Chão do `PROVISIONING_SECRET`: o mesmo do `JWT_SECRET`.
pub const PROVISIONING_SECRET_MIN_LEN: usize = 32;

/// `value` é um dos `list`? Comparação em tempo constante e SEM sair ao
/// primeiro acerto: o tempo não diz qual nem se algum.
pub(crate) fn is_one_of(value: &str, list: &[&str]) -> bool {
    list.iter().fold(false, |hit, known| {
        hit | crate::apikeys::ct_eq(value.as_bytes(), known.as_bytes())
    })
}

/// `None` se o segredo de voz serve; `Some(razão)` se não. Com
/// `DELONIX_ALLOW_INSECURE=1` aceita-se o valor de dev do Makefile (e qualquer
/// outro não vazio), mas NUNCA um valor publicado.
pub fn voice_secret_refusal(secret: &str, insecure: bool) -> Option<&'static str> {
    if secret.is_empty() {
        return Some("VOICE_INTERNAL_SECRET não está definido");
    }
    if is_one_of(secret, BURNED_VOICE_SECRETS) {
        return Some(
            "VOICE_INTERNAL_SECRET é um valor de exemplo publicado no repositório — gera um novo",
        );
    }
    if insecure {
        return None;
    }
    if is_one_of(secret, DEV_VOICE_SECRETS) {
        return Some("VOICE_INTERNAL_SECRET é o valor de desenvolvimento do Makefile");
    }
    if secret.len() < VOICE_SECRET_MIN_LEN {
        return Some("VOICE_INTERNAL_SECRET tem menos de 32 caracteres");
    }
    None
}

/// `None` se o `PROVISIONING_SECRET` serve; `Some(razão)` se não — e então
/// `POST /api/v1/admin/orgs` responde 503 com a razão (R155). Não faz panic:
/// quem não provisiona organizações pelo Odoo não perde o servidor.
pub fn provisioning_secret_refusal(secret: &str, insecure: bool) -> Option<&'static str> {
    if secret.is_empty() {
        return Some("PROVISIONING_SECRET não está definido");
    }
    if is_one_of(secret, BURNED_PROVISIONING_SECRETS) {
        return Some("PROVISIONING_SECRET é um valor publicado no repositório — gera um novo");
    }
    if insecure {
        return None;
    }
    if is_one_of(secret, DEV_PROVISIONING_SECRETS) {
        return Some("PROVISIONING_SECRET é um valor de demonstração");
    }
    if secret.len() < PROVISIONING_SECRET_MIN_LEN {
        return Some("PROVISIONING_SECRET tem menos de 32 caracteres");
    }
    None
}

/// A password embutida num `DATABASE_URL` (`postgres://user:PASS@host/db`),
/// tal como está escrita. `None` se o URL não tiver password ou não for um URL.
fn db_url_password(url: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.password().map(str::to_string))
}

/// Lê um segredo do ambiente. Em produção (insecure=false) faz panic se estiver
/// ausente, igual ao default de dev, abaixo do comprimento mínimo, ou — em
/// qualquer modo — igual a um valor publicado no repositório.
fn secret(var: &str, rule: &SecretRule, insecure: bool) -> String {
    match check_secret(var, env::var(var).ok().as_deref(), rule, insecure) {
        Ok(v) => v,
        Err(why) => panic!("{why}"),
    }
}

/// O que torna um segredo lido por `secret` aceitável.
struct SecretRule {
    /// Usado quando a variável falta e `DELONIX_ALLOW_INSECURE=1`.
    dev_default: &'static str,
    min_len: usize,
    /// Valores publicados: recusados em qualquer modo.
    burned: &'static [&'static str],
    /// Valores de dev conhecidos: recusados só em produção.
    dev_known: &'static [&'static str],
    /// Compara as listas com a password do URL e não com o valor inteiro.
    is_db_url: bool,
}

const JWT_RULE: SecretRule = SecretRule {
    dev_default: DEV_JWT,
    min_len: 32,
    burned: BURNED_JWT_SECRETS,
    dev_known: &[],
    is_db_url: false,
};
const TURN_RULE: SecretRule = SecretRule {
    dev_default: DEV_TURN,
    min_len: 16,
    burned: BURNED_TURN_SECRETS,
    dev_known: &[],
    is_db_url: false,
};
const DB_RULE: SecretRule = SecretRule {
    dev_default: DEV_DB,
    min_len: 0,
    burned: BURNED_DB_PASSWORDS,
    dev_known: DEV_DB_PASSWORDS,
    is_db_url: true,
};

/// A decisão de `secret`, sem ambiente nem panic, para se poder testar.
fn check_secret(
    var: &str,
    value: Option<&str>,
    rule: &SecretRule,
    insecure: bool,
) -> Result<String, String> {
    let Some(v) = value else {
        return if insecure {
            Ok(rule.dev_default.to_string())
        } else {
            Err(format!(
                "{var} tem de estar definido em produção (ou define DELONIX_ALLOW_INSECURE=1 em dev)"
            ))
        };
    };
    let compared = if rule.is_db_url {
        db_url_password(v).unwrap_or_default()
    } else {
        v.to_string()
    };
    if !compared.is_empty() && is_one_of(&compared, rule.burned) {
        return Err(format!(
            "{var} usa um valor PUBLICADO no repositório (scripts/leaked-secrets-accepted.txt) — \
             gera um novo e roda-o (docs/deployment.md §6)"
        ));
    }
    let dev =
        v == rule.dev_default || (!compared.is_empty() && is_one_of(&compared, rule.dev_known));
    if dev && !insecure {
        return Err(format!(
            "{var} está com um valor de desenvolvimento — define um segredo forte em produção"
        ));
    }
    if v.len() < rule.min_len {
        return Err(format!(
            "{var} tem de ter pelo menos {} caracteres",
            rule.min_len
        ));
    }
    Ok(v.to_string())
}

impl Config {
    /// A janela de graça como `Duration`. Existe para os chamadores não terem
    /// de se lembrar da unidade — um `45` lido como milissegundos daria uma
    /// janela de 45 ms e a reclamação nunca aconteceria.
    pub fn reconnect_grace(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.reconnect_grace_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STRONG_JWT: &str = "9f1c3e5a7b9d0f2a4c6e8b0d2f4a6c8e0b2d4f6a8c0e2b4d";
    const STRONG_DB: &str =
        "postgres://delonix:Q7vX2mR9kL4pZ8wN3bT6yH1cF5jD0sG@db.interno:5432/delonix_meet";

    /// R155 — um `JWT_SECRET`/`TURN_SECRET` publicado no repositório NÃO
    /// arranca o servidor, nem em produção nem com `DELONIX_ALLOW_INSECURE=1`.
    /// O JWT publicado tem 42 caracteres: é a lista que o recusa, não o chão.
    #[test]
    fn published_jwt_and_turn_are_refused_in_every_mode() {
        for insecure in [false, true] {
            for v in BURNED_JWT_SECRETS {
                let e = check_secret("JWT_SECRET", Some(v), &JWT_RULE, insecure).unwrap_err();
                assert!(e.contains("PUBLICADO"), "insecure={insecure}: {e}");
            }
            for v in BURNED_TURN_SECRETS {
                let e = check_secret("TURN_SECRET", Some(v), &TURN_RULE, insecure).unwrap_err();
                assert!(e.contains("PUBLICADO"), "insecure={insecure}: {e}");
            }
        }
    }

    /// A password publicada é recusada DENTRO do URL, seja qual for o host,
    /// o utilizador ou a base — mudar só o host não a torna segura.
    #[test]
    fn published_db_password_is_refused_whatever_the_host() {
        for pass in BURNED_DB_PASSWORDS {
            for host in [
                "delonix-postgres-postgresql.delonix-meet.svc.cluster.local:5432",
                "10.0.0.5:5432",
                "localhost",
            ] {
                let url = format!("postgres://outro:{pass}@{host}/qualquer");
                for insecure in [false, true] {
                    let e =
                        check_secret("DATABASE_URL", Some(&url), &DB_RULE, insecure).unwrap_err();
                    assert!(e.contains("PUBLICADO"), "{url}: {e}");
                }
            }
        }
    }

    /// «Dev conhecido» não é «publicado»: o `make dev`, o docker-compose e o CI
    /// usam `DELONIX_ALLOW_INSECURE=1` com estes valores e têm de continuar a
    /// arrancar; em produção os mesmos valores param o arranque.
    #[test]
    fn dev_values_start_only_with_allow_insecure() {
        let dev_url = "postgres://delonix:delonix_dev@dlxmeet-db:5432/delonix_meet";
        assert!(check_secret("DATABASE_URL", Some(dev_url), &DB_RULE, true).is_ok());
        assert!(check_secret("DATABASE_URL", Some(dev_url), &DB_RULE, false).is_err());
        assert!(check_secret("DATABASE_URL", Some(DEV_DB), &DB_RULE, false).is_err());
        let placeholder = "postgres://delonix:CHANGE_ME@postgres:5432/delonix_meet";
        assert!(check_secret("DATABASE_URL", Some(placeholder), &DB_RULE, false).is_err());
        assert!(check_secret("JWT_SECRET", Some(DEV_JWT), &JWT_RULE, true).is_ok());
        assert!(check_secret("JWT_SECRET", Some(DEV_JWT), &JWT_RULE, false).is_err());
        assert!(check_secret("TURN_SECRET", Some(DEV_TURN), &TURN_RULE, false).is_err());
        // Ausente: dev cai no default, produção recusa.
        assert_eq!(
            check_secret("JWT_SECRET", None, &JWT_RULE, true).as_deref(),
            Ok(DEV_JWT)
        );
        assert!(check_secret("JWT_SECRET", None, &JWT_RULE, false).is_err());
    }

    /// A metade que tem de continuar a passar: um segredo forte e novo arranca.
    #[test]
    fn strong_fresh_secrets_are_accepted() {
        assert!(check_secret("JWT_SECRET", Some(STRONG_JWT), &JWT_RULE, false).is_ok());
        assert!(check_secret("TURN_SECRET", Some(&STRONG_JWT[..32]), &TURN_RULE, false).is_ok());
        assert!(check_secret("DATABASE_URL", Some(STRONG_DB), &DB_RULE, false).is_ok());
        // Curto continua a ser recusado, como antes.
        assert!(check_secret("JWT_SECRET", Some("curto"), &JWT_RULE, true).is_err());
        // Uma password que só CONTÉM a publicada não é a publicada.
        let parecida = "postgres://delonix:delonix_dev_pass_mas_nova_9Xk2@db:5432/d";
        assert!(check_secret("DATABASE_URL", Some(parecida), &DB_RULE, false).is_ok());
    }

    /// R155 — o `PROVISIONING_SECRET` fecha sem panic: vazio, publicado, de
    /// demo ou curto dá razão (→ 503); forte passa. O publicado é recusado
    /// também com `DELONIX_ALLOW_INSECURE=1`; o de demo não.
    #[test]
    fn provisioning_secret_refusal_by_value() {
        assert!(provisioning_secret_refusal("", false).is_some());
        assert!(provisioning_secret_refusal("", true).is_some());
        for v in BURNED_PROVISIONING_SECRETS {
            for insecure in [false, true] {
                let r = provisioning_secret_refusal(v, insecure).unwrap_or_default();
                assert!(r.contains("publicado"), "insecure={insecure}: {r}");
            }
        }
        for v in DEV_PROVISIONING_SECRETS {
            assert!(provisioning_secret_refusal(v, false).is_some());
            assert_eq!(provisioning_secret_refusal(v, true), None);
        }
        assert!(provisioning_secret_refusal("0123456789abcdef0123456789abcde", false).is_some());
        assert_eq!(provisioning_secret_refusal(STRONG_JWT, false), None);
    }

    /// O valor de voz publicado deixa de passar com `DELONIX_ALLOW_INSECURE=1`
    /// (em R154 passava); o de dev do Makefile continua a passar.
    #[test]
    fn published_voice_secret_is_refused_even_in_dev() {
        for v in BURNED_VOICE_SECRETS {
            assert!(voice_secret_refusal(v, true).is_some());
        }
        for v in DEV_VOICE_SECRETS {
            assert_eq!(voice_secret_refusal(v, true), None);
            assert!(voice_secret_refusal(v, false).is_some());
        }
    }

    #[test]
    fn is_one_of_matches_whole_values_only() {
        assert!(is_one_of("abc", &["x", "abc"]));
        assert!(!is_one_of("ab", &["abc"]));
        assert!(!is_one_of("abcd", &["abc"]));
        assert!(!is_one_of("abc", &[]));
    }

    /// Cada valor recusado por estar PUBLICADO tem a decisão escrita no livro
    /// que o `check-repo-hygiene.sh` usa. Sem isto, o servidor e o portão
    /// divergiam em silêncio.
    #[test]
    fn every_burned_value_is_in_the_ledger() {
        let ledger = include_str!("../../scripts/leaked-secrets-accepted.txt");
        let lines: Vec<&str> = ledger
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        for v in BURNED_VOICE_SECRETS
            .iter()
            .chain(BURNED_JWT_SECRETS)
            .chain(BURNED_TURN_SECRETS)
            .chain(BURNED_DB_PASSWORDS)
            .chain(BURNED_PROVISIONING_SECRETS)
        {
            assert!(lines.contains(v), "«{v}» não está no livro");
        }
    }
}
