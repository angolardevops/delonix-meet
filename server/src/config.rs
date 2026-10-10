use std::env;

const DEV_JWT: &str = "dev-only-secret-change-in-production";
const DEV_TURN: &str = "delonix_turn_dev_secret";
const DEV_DB: &str = "postgres://delonix:delonix_dev@localhost:5435/delonix_meet";

#[derive(Clone)]
pub struct Config {
    /// Perfil da instalação (`DELONIX_EDITION`, por omissão `saas` — o
    /// comportamento histórico). Fixa os valores por omissão das políticas
    /// abaixo; cada uma pode ser sobreposta (ADR-0006 §2).
    pub edition: delonix_meet_core::edition::Edition,
    /// Quem pode criar conta (`REGISTRATION_MODE`).
    pub registration_mode: delonix_meet_core::edition::RegistrationMode,
    /// Domínios aceites em `REGISTRATION_MODE=domain` (`REGISTRATION_DOMAINS`, csv).
    pub registration_domains: Vec<String>,
    /// Uma org por empresa, ou uma só org na instalação (`TENANCY_MODE`).
    pub tenancy_mode: delonix_meet_core::edition::TenancyMode,
    /// Correr as migrações no arranque (`DELONIX_MIGRATE`, omissão `1`). Em
    /// SaaS com várias réplicas corre-se `delonix-server migrate` num Job e
    /// põe-se `0` no Deployment, para não haver N réplicas a migrar ao mesmo tempo.
    pub migrate_on_start: bool,
    /// `LOG_FORMAT=json` para os logs saírem estruturados (K8s/Loki).
    pub log_json: bool,
    /// Listener HTTP INTERNO (`INTERNAL_BIND_ADDR`): a API de IVR e o
    /// `/metrics` saem da árvore pública e passam a viver aqui, numa porta que
    /// nenhum ingress publica. Vazio => tudo fica no listener público, como antes.
    pub internal_bind_addr: Option<String>,
    /// Listener gRPC interno (`GRPC_BIND_ADDR`). Vazio => desligado.
    pub grpc_bind_addr: Option<String>,
    /// mTLS do gRPC (`GRPC_TLS_CERT`, `GRPC_TLS_KEY`, `GRPC_CLIENT_CA`: caminhos).
    pub grpc_tls_cert: Option<String>,
    pub grpc_tls_key: Option<String>,
    pub grpc_client_ca: Option<String>,
    /// Directório da SPA a servir pelo próprio binário (`UI_DIR`). Vazio =>
    /// a UI é servida à parte (nginx/CDN), como antes.
    pub ui_dir: Option<std::path::PathBuf>,
    /// Cifra de segredos em repouso (`DATA_ENCRYPTION_KEYS="kid:base64,…"`,
    /// ADR-0006 / S5). Sem a variável: em desenvolvimento uma chave derivada;
    /// em produção `None`, e as capacidades NOVAS que guardam segredos recusam
    /// (422) em vez de os escrever em claro.
    pub secret_box: Option<std::sync::Arc<delonix_meet_core::secret_box::SecretBox>>,
    /// Participantes que este nó aguenta (`NODE_PEER_CAPACITY`), declarado pelo
    /// operador a partir de testes de carga. Sem ele o inventário não inventa
    /// uma ocupação.
    pub node_peer_capacity: Option<u32>,
    /// Ligações `/ws` simultâneas que UMA conta pode ter neste nó
    /// (`MAX_WS_PER_USER`, por omissão 16; `0` desliga). Impede que uma conta
    /// ocupe o nó com sockets. Não conta bots nem fontes de estúdio.
    pub max_ws_per_user: Option<u32>,
    /// Participantes concorrentes que uma organização pode ter NESTE nó quando
    /// o operador não lhe fixou um tecto (`ORG_MAX_PARTICIPANTS`). Sem ele, uma
    /// org sem tecto próprio é ilimitada.
    pub org_max_participants: Option<u32>,
    /// `DELONIX_ALLOW_INSECURE=1`: segredos de dev aceites e CORS permissivo.
    /// Lido UMA vez aqui — nenhum outro módulo lê o ambiente.
    pub allow_insecure: bool,
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
    /// Hosts isentos da guarda de saída para URLs escritos por clientes
    /// (`OUTBOUND_ALLOW_HOSTS`, separados por vírgula): webhooks, `odoo_url` da
    /// organização, emissor OIDC. Vazio (omissão) => nenhum destino interno é
    /// alcançável, que é o comportamento seguro. Existe porque o integrador
    /// típico on-prem — Odoo ou Keycloak em `10.x` — seria recusado. Nomes
    /// exactos, nunca redes. O host de `PLATFORM_ODOO_URL` entra sozinho (ver
    /// `net_guard`).
    pub outbound_allow_hosts: Vec<String>,
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
    /// `POST /api/operator/v1/organizations` (ex.: o Odoo cria a org de cada empresa e
    /// recebe a chave de API). Vazio => endpoint de provisão DESATIVADO
    /// (fail-closed). Não é uma chave de org — é anterior a qualquer org.
    pub provisioning_secret: String,
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
    /// `smpp://system_id:password@host:2775?source_addr=DELONIX` (texto
    /// simples) ou `smpps://…` (a mesma sintaxe, envolvida em TLS antes do
    /// bind — validação contra as raízes do sistema por omissão). Ausente =>
    /// operador por contratar, e o encaminhamento não o escolhe. São da
    /// PLATAFORMA (o contrato é da Delonix), por isso vêm do ambiente e não de
    /// uma tabela — não há cifra de segredos em repouso (S5).
    pub sms_unitel_smpp: Option<String>,
    pub sms_movicel_smpp: Option<String>,
    pub sms_africell_smpp: Option<String>,
    /// Feixe de CA (PEM, um ou mais certificados) para validar o SMSC de cada
    /// operador quando `smpps://` e a CA não é pública — carrega-se de um
    /// ficheiro no disco, nunca em claro no ambiente. Sem isto e com
    /// `smpps://`, valida-se contra as raízes do sistema (`webpki-roots`).
    /// Ignorado com `smpp://` (texto simples).
    pub sms_unitel_smpp_ca: Option<String>,
    pub sms_movicel_smpp_ca: Option<String>,
    pub sms_africell_smpp_ca: Option<String>,
    /// Telefonia (ADR-0009). Event Socket do FreeSWITCH (`host:porta`,
    /// normalmente `127.0.0.1:8021` ou o serviço interno). Ausente => nenhuma
    /// operação que precise do media server corre, e a API responde
    /// `not_configured` — nunca um estado inventado.
    pub telephony_esl_addr: Option<String>,
    /// Password do Event Socket (`TELEPHONY_ESL_PASSWORD`). Obrigatória com o endereço.
    pub telephony_esl_password: String,
    /// Perfil sofia onde vivem os gateways dos troncos (`external`).
    pub telephony_sofia_profile: String,
    /// JSON-RPC do Kamailio (`jsonrpcs`, ex.: `http://sbc-01:5071/RPC`). Ausente =>
    /// o SBC aparece como `not_configured`.
    pub telephony_kamailio_rpc_url: Option<String>,
    /// Números de emergência, que nunca se gravam nem bloqueiam
    /// (`TELEPHONY_EMERGENCY_NUMBERS`, omissão `112,113,115`).
    pub telephony_emergency_numbers: Vec<String>,
    /// Indicativo do país da instalação, sem `+` (`244`), e comprimento de um
    /// número nacional completo (`9`) — para normalizar o que se marca.
    pub telephony_country_code: String,
    pub telephony_national_len: usize,
    /// Tarifa estimada por minuto (inbound) para o cálculo de custo no CDR.
    pub voice_tariff_inbound: f64,
    /// Sufixo do domínio SIP dos ramais internos (`VOICE_RAMAIS_DOMAIN_SUFFIX`):
    /// cada org fala em `<slug>.<sufixo>` (ex.: `acme.ramais.delonix.meet`).
    /// O slug distingue as orgs — necessário porque `extension` só é única
    /// DENTRO da org (ver migração 0064); sem isto, o ramal "101" da Acme e o
    /// "101" da Zeta colidiriam no mesmo directório SIP.
    pub voice_ramais_domain_suffix: String,
    /// Só laboratório e testes (ADR-0023): o URL do operador a que o servidor entrega o pedido de «acordar»
    /// de um aparelho `lab`. Vazio = o fornecedor `lab` não está configurado (não acorda ninguém).
    pub push_lab_url: Option<String>,
    /// O serviço `delonix-push` (ADR-0023): URL base (`PUSH_DELONIX_URL`) e chave de servidor do projecto
    /// (`PUSH_DELONIX_KEY`, segredo). Qualquer um ausente = o fornecedor `delonix` não está configurado.
    pub push_delonix_url: Option<String>,
    pub push_delonix_key: Option<String>,
    /// Quantos segundos uma chamada que o SERVIDOR origina para um ramal sem registo espera que o aparelho acorde
    /// (push) e se registe (ADR-0023). 0 (por omissão) desliga. É a contraparte, para chamadas do servidor, do
    /// `DELONIX_PUSH_WAIT_SECS` do FreeSWITCH, que só vê as chamadas que passam pelo seu dialplan.
    pub push_wait_secs: u32,
    /// Número curto RESERVADO que um ramal marca para entrar numa reunião
    /// (`VOICE_MEETING_ACCESS_NUMBER`, 3–5 dígitos sem zero à esquerda; por
    /// omissão `8000`). O FreeSWITCH não o conhece: pergunta-o ao servidor em
    /// `resolve-extension`, por isso este é o único sítio onde se configura.
    /// Nenhum ramal pode ter este número (`ramais.extension_reserved`).
    pub voice_meeting_access_number: String,
    /// Endereço PÚBLICO do servidor SIP dos ramais — o que um softphone põe em
    /// «servidor/proxy»: `VOICE_RAMAIS_PUBLIC_HOST` (nome DNS ou IP, sem
    /// esquema nem porta), `VOICE_RAMAIS_PUBLIC_PORT` (omissão 5070) e
    /// `VOICE_RAMAIS_PUBLIC_TRANSPORT` (`udp`|`tcp`|`tls`, omissão `udp`).
    /// Sem host (ou com um host mal formado) fica `None` e a API devolve
    /// `sip_server: null` — o servidor não adivinha por onde é alcançável.
    pub voice_ramais_public: Option<delonix_meet_domain::telephony::extension::SipServer>,
    /// Diretório onde as gravações são armazenadas (lido uma vez no arranque).
    pub recordings_dir: std::path::PathBuf,
    /// Ficheiros ZIP de «os meus dados» (`DATA_EXPORTS_DIR`). Por omissão
    /// `<RECORDINGS_DIR>/exports`, no mesmo armazenamento das gravações.
    pub data_exports_dir: std::path::PathBuf,
    /// Chaves de acesso (WebAuthn, ADR-0011): o RP ID (`WEBAUTHN_RP_ID`, o
    /// domínio, p.ex. `meet.delonix.co.ao`) e a origem do web
    /// (`WEBAUTHN_RP_ORIGIN`, `https://meet.delonix.co.ao`). Sem os dois, as
    /// chaves de acesso ficam `not_configured` — não se adivinha a origem a
    /// partir de um cabeçalho do pedido.
    pub webauthn_rp_id: Option<String>,
    pub webauthn_rp_origin: Option<String>,
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
    /// Relay de correio do operador (`SMTP_HOST`). Vazio => **o correio fica
    /// desligado** e quem enfileira recebe-o dito, em vez de enfileirar
    /// mensagens que ninguém envia. O D7 (2026-10-09, ADR-0025) escolheu o
    /// relay primeiro: não há SMTP por organização, e por isso também não há
    /// host escolhido pelo inquilino — o `net_guard` só guarda URLs HTTP e não
    /// cobre uma ligação SMTP.
    pub smtp_host: Option<String>,
    /// Porta do relay (`SMTP_PORT`, 587, 1..=65535). 587 = submissão com
    /// STARTTLS, que é o que o `smtp_starttls` assume.
    pub smtp_port: u16,
    /// Conta no relay (`SMTP_USERNAME`). Vazio => entrega sem autenticação,
    /// que só faz sentido num relay da rede interna.
    pub smtp_username: Option<String>,
    /// Password no relay (`SMTP_PASSWORD`). Nunca aparece em log nem em
    /// resposta de API.
    pub smtp_password: Option<String>,
    /// Remetente (`SMTP_FROM`), p.ex. `Delonix Meet <nao-responda@exemplo.ao>`.
    /// Sem ele o correio fica desligado: uma mensagem sem remetente é recusada
    /// por qualquer relay sério.
    pub smtp_from: Option<String>,
    /// `SMTP_STARTTLS=0` desliga o STARTTLS (1 por omissão). Só para um relay
    /// em `localhost`: sem isto a password da conta viaja em claro.
    pub smtp_starttls: bool,
    /// O endereço público da web (`PUBLIC_URL`, p.ex. `https://meet.exemplo.ao`,
    /// sem barra no fim). É de onde saem os links que o servidor escreve num
    /// email (prova de endereço, reposição de password). NUNCA se tira do
    /// cabeçalho `Host` do pedido: quem pede escolhia o domínio para onde vai o
    /// token — o envenenamento de links de reposição. Sem ele, o correio que
    /// leva um link recusa-se (`mail.public_url_missing`), como as chaves de
    /// acesso sem `WEBAUTHN_RP_ORIGIN`.
    pub public_url: Option<String>,
    /// URL do Ollama in-cluster (LLM local — soberania: o texto nunca sai do
    /// datacenter). Vazio => IA desligada, fail-open: o MoM fica por regras
    /// (cliente) e a tradução de legendas não aparece.
    pub ollama_url: Option<String>,
    /// Modelo para tradução das legendas (rápido; ex.: qwen2.5:1.5b).
    pub ollama_model_translate: String,
    /// Modelo para o resumo da ata (qualidade; ex.: qwen2.5:7b em prod).
    pub ollama_model_summary: String,
    /// Modelo das tarefas do Estúdio (`OLLAMA_MODEL_STUDIO`; por omissão, o
    /// do resumo): resumo e capítulos, texto de publicação, bordões.
    pub ollama_model_studio: String,
    /// Tecto de uma tarefa do Estúdio (`OLLAMA_TIMEOUT_SECS`, 120, 5..=900).
    /// O pedido é síncrono: o ecrã espera pela resposta ou pelo erro.
    pub ollama_timeout_secs: u64,
    /// Tarefas do Estúdio em simultâneo POR ORGANIZAÇÃO
    /// (`AI_STUDIO_CONCURRENCY_PER_ORG`, 1, 1..=8). O modelo local é um só e
    /// partilhado: sem este tecto, uma organização monopolizava-o.
    pub ai_studio_concurrency_per_org: usize,
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
    /// Composições de gravação a correr ao mesmo tempo neste pod
    /// (`FFMPEG_MAX_CONCURRENT`, default 1). As restantes esperam a vez.
    ///
    /// Medido a 2026-09-17 (`docs/ops/teste-de-carga-2026-09-17.md`): 20
    /// composições em simultâneo, com `FFMPEG_THREADS=2`, tomaram ~12 núcleos e
    /// deixaram as chamadas VIVAS do mesmo nó com 64% de perda. `FFMPEG_THREADS`
    /// limita UM ffmpeg; sem este tecto, N gravações a acabar juntas (o fim de
    /// uma reunião grande, ou de várias) somam N× esse limite. O custo de
    /// esperar é tempo até a gravação aparecer na biblioteca; o de não esperar
    /// é a chamada de quem ainda está a falar.
    pub ffmpeg_max_concurrent: usize,
    /// Emissões em directo simultâneas por nó (`MAX_DIRECTOS`, default 2).
    ///
    /// O tecto existe porque a sala em directo vive no MESMO pod que a serve
    /// (ADR-0001), e o CPU desse pod é finito. Cada emissão copia o vídeo e
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
    /// Binário do ffprobe (`FFPROBE_BIN`, default `ffprobe`). Mede duração,
    /// resolução, fps e codecs de cada gravação (ver `media_probe.rs`).
    pub ffprobe_bin: String,
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
    /// Segundos que o drain dá às composições de gravação em curso depois de
    /// as salas esvaziarem (`DRAIN_COMPOSE_SECS`, por omissão 5).
    ///
    /// Curto por obrigação, não por escolha, e a conta tem de fechar: o
    /// `terminationGracePeriodSeconds` é **60 s** (`deploy/k8s/02-server.yaml`),
    /// e 12 (readiness) + 40 (salas) + 5 = **57 s** deixa 3 s de folga antes do
    /// SIGKILL. Com 8 dava 60 exactos — o SIGKILL chegava no mesmo instante em
    /// que a espera terminava, e um drain que acaba ao mesmo tempo que é morto
    /// não é um drain. Quem subir um dos três tem de subir a graça do K8s.
    ///
    /// Serve o caso frequente: a composição que estava quase a acabar acaba. A
    /// que não acabar JÁ NÃO SE PERDE — fica `processing` com manifesto e o nó
    /// seguinte retoma-a (`recorder::resume_due`, migração 0099). Esta espera é
    /// uma optimização; a garantia é a retoma.
    pub drain_compose_secs: u64,
    /// Atraso que se pede ao cliente antes de reconectar (`DRAIN_RECONNECT_MS`,
    /// default 2000). O cliente acrescenta jitter por cima — sem isso, uma sala
    /// inteira reconecta no mesmo milissegundo e o pod novo leva com tudo de uma vez.
    pub drain_reconnect_ms: u64,
    /// Quantos proxies de confiança há entre o cliente e este servidor
    /// (`TRUSTED_PROXY_HOPS`, default 1). Diz qual entrada do `X-Forwarded-For`
    /// é o endereço do cliente: a `n`-ésima a contar do FIM, que é a que o
    /// proxy de fora escreveu. Tudo o que está à esquerda dela veio do cliente
    /// e não é de confiar — ver `rate_limit::client_ip`. Errar para menos faz
    /// toda a gente partilhar o endereço de um proxy (e o limite por IP);
    /// errar para mais devolve a chave ao cliente.
    pub trusted_proxy_hops: usize,
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
    /// Entradas de convidado sem conta por IP, por minuto
    /// (`GUEST_JOIN_PER_IP_PER_MIN`, 10 por omissão). A rota é pública e emite
    /// credenciais TURN: sem travão, qualquer um esgotava o relay ou enchia
    /// salas de espera alheias.
    pub guest_join_per_ip_per_min: usize,
    /// Entradas de convidado por SALA, por minuto
    /// (`GUEST_JOIN_PER_ROOM_PER_MIN`, 30 por omissão). O travão por IP não
    /// chega contra quem tem muitos IPs: este protege o anfitrião de ver a sala
    /// de espera inundada.
    pub guest_join_per_room_per_min: usize,
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
    /// IPs dos FreeSWITCH aceites pela ponte telefone↔sala, tanto no SIP como
    /// no RTP (`PHONE_BRIDGE_FREESWITCH_IPS`, lista separada por vírgulas;
    /// aceita-se também o antigo `PSTN_BRIDGE_FREESWITCH_IP` singular, que os
    /// deploys da Abordagem B já usam). **Fail-closed**: vazio/ausente => a
    /// ponte não aceita NADA — nem um `INVITE`, nem um pacote RTP. Sem esta
    /// lista, aceitar de qualquer origem deixaria qualquer host da rede
    /// injectar áudio numa reunião fingindo ser o FreeSWITCH.
    ///
    /// Passou de um IP a uma lista porque o `dispatcher.list` do Kamailio
    /// cresce em produção e o SBC pode reenviar de mais que um nó (ADR-0009).
    pub phone_bridge_freeswitch_ips: Vec<std::net::IpAddr>,
    /// Os **nomes** de `PHONE_BRIDGE_FREESWITCH_IPS` (a mesma variável aceita IPs e nomes
    /// de máquina, separados por vírgulas). Resolvem-se ao arrancar e de tempos a tempos,
    /// e só valem os endereços **privados** a que resolvem (`phone_bridge::origens`): um
    /// orquestrador que dá um IP novo a cada arranque deixa de pedir um IP fixo.
    pub phone_bridge_freeswitch_names: Vec<String>,
    /// De quantos em quantos segundos se voltam a resolver os nomes
    /// (`PHONE_BRIDGE_RESOLVE_SECS`, 5–300, por omissão 15).
    pub phone_bridge_resolve_secs: u64,
    /// `PSTN_BRIDGE_HOST`/`SFU_EXTERNAL_IP` foi mesmo definido? Se não, a morada que a
    /// ponte dá ao FreeSWITCH é o IP local detectado, e não o `127.0.0.1` de omissão.
    pub pstn_bridge_host_explicit: bool,
    /// Onde o UA SIP da ponte escuta (`PHONE_BRIDGE_SIP_BIND`, p.ex.
    /// `0.0.0.0:5090`). Ausente => a ponte NÃO arranca e as rotas de canais
    /// respondem `channels.bridge_not_configured` — nunca um estado inventado.
    pub phone_bridge_sip_bind: Option<std::net::SocketAddr>,
    /// Host:porta que o control plane dá ao FreeSWITCH como destino do
    /// `bridge` SIP (`PHONE_BRIDGE_SIP_ADVERTISE`). Por omissão o
    /// `PSTN_BRIDGE_HOST`/`SFU_EXTERNAL_IP` com a porta do bind — em K8s o
    /// Service e o bind não têm de coincidir.
    pub phone_bridge_sip_advertise: Option<String>,
    /// IP onde as pernas abrem RTP e que vai no SDP (`PHONE_BRIDGE_RTP_IP`).
    /// Por omissão o mesmo IP do bind SIP.
    pub phone_bridge_rtp_ip: Option<std::net::IpAddr>,
    /// Intervalo de portas RTP das pernas, inclusive
    /// (`PHONE_BRIDGE_RTP_MIN`/`MAX`). Ausente => porta efémera do SO, que não
    /// se pode expor no K8s.
    pub phone_bridge_rtp_ports: Option<(u16, u16)>,
    /// A perna da ponte negoceia Opus (`PHONE_BRIDGE_WIDEBAND`, por omissão
    /// ligado — ADR-0018): o servidor manda `OPUS,PCMA` na dial string e a voz
    /// de um softphone chega à sala sem passar por 8 kHz. `0` (ou `false`,
    /// `off`, `no`, ou qualquer valor que não se perceba) repõe `PCMA`, o
    /// caminho G.711 de sempre, sem reconstruir nada.
    pub phone_bridge_wideband: bool,
    /// Host que o control plane devolve ao IVR (`PSTN_BRIDGE_HOST`). Por
    /// omissão o mesmo `SFU_EXTERNAL_IP`; "127.0.0.1" se nenhum dos dois
    /// estiver definido (dev local, tudo na mesma máquina).
    pub pstn_bridge_host: String,
}

/// De onde a configuração se lê. Em produção é o ambiente do processo; nos
/// testes é um mapa, para que dois testes em paralelo não disputem
/// `std::env` (que é global ao processo).
pub struct Source<'a>(pub &'a dyn Fn(&str) -> Option<String>);

impl Source<'_> {
    fn var(&self, name: &str) -> Result<String, ()> {
        (self.0)(name).ok_or(())
    }
}

impl Config {
    pub fn from_env() -> Self {
        Self::from_source(&Source(&|k| env::var(k).ok()))
    }

    /// Constrói a configuração a partir de um mapa (testes de integração).
    /// As mesmas regras de `from_env`: fail-closed sem segredos fortes.
    pub fn from_map(vars: &std::collections::HashMap<&str, &str>) -> Self {
        Self::from_source(&Source(&|k| vars.get(k).map(|v| v.to_string())))
    }

    pub fn from_source(src: &Source) -> Self {
        // Fail-closed: por omissão exige-se segredos fortes. Só se
        // DELONIX_ALLOW_INSECURE=1 (dev) é que se aceitam os defaults.
        let insecure = src.var("DELONIX_ALLOW_INSECURE").ok().as_deref() == Some("1");
        let cors_origins = csv_env(src, "CORS_ORIGINS");
        use delonix_meet_core::edition::{Edition, RegistrationMode, TenancyMode};
        let edition = match src.var("DELONIX_EDITION") {
            Err(_) => Edition::Saas,
            Ok(v) => Edition::parse(&v).unwrap_or_else(|| {
                panic!("DELONIX_EDITION: «{v}» não é saas | enterprise | personal")
            }),
        };
        let registration_mode = match src.var("REGISTRATION_MODE") {
            Err(_) => edition.default_registration(),
            Ok(v) => RegistrationMode::parse(&v).unwrap_or_else(|| {
                panic!("REGISTRATION_MODE: «{v}» não é open | domain | invite | closed")
            }),
        };
        let tenancy_mode = match src.var("TENANCY_MODE") {
            Err(_) => edition.default_tenancy(),
            Ok(v) => TenancyMode::parse(&v)
                .unwrap_or_else(|| panic!("TENANCY_MODE: «{v}» não é multi | single")),
        };
        let registration_domains: Vec<String> = csv_env(src, "REGISTRATION_DOMAINS")
            .into_iter()
            .map(|d| d.to_lowercase())
            .collect();
        if registration_mode == RegistrationMode::Domain && registration_domains.is_empty() {
            // Fail-closed e em voz alta: «domain» sem domínios fecharia o registo
            // a toda a gente sem que o operador percebesse porquê.
            panic!("REGISTRATION_MODE=domain exige REGISTRATION_DOMAINS (csv)");
        }
        let opt = |k: &str| src.var(k).ok().filter(|v| !v.trim().is_empty());
        let jwt_secret = secret(src, "JWT_SECRET", DEV_JWT, insecure, 32);
        let secret_box = match opt("DATA_ENCRYPTION_KEYS") {
            Some(spec) => Some(std::sync::Arc::new(
                delonix_meet_core::secret_box::SecretBox::from_spec(&spec)
                    .unwrap_or_else(|e| panic!("DATA_ENCRYPTION_KEYS: {e}")),
            )),
            None if insecure => Some(std::sync::Arc::new(
                delonix_meet_core::secret_box::SecretBox::derived_for_dev(&jwt_secret),
            )),
            // Em produção a chave é obrigatória. Antes, o servidor arrancava
            // sem ela e cada escrita de um segredo dava `422` — um deploy
            // «a funcionar» em que guardar um webhook, um tronco ou um destino
            // de directo falhava, e ninguém o sabia até tentar. O campo
            // continua `Option` porque os testes o esvaziam para exercitar
            // essa recusa, que fica como defesa em profundidade.
            None => panic!(
                "DATA_ENCRYPTION_KEYS tem de estar definida em produção: é a chave que cifra os \
                 segredos guardados na base (formato kid:base64 de 32 bytes — \
                 `echo \"k1:$(openssl rand -base64 32)\"`; o `make bootstrap` gera-a). \
                 Em desenvolvimento, DELONIX_ALLOW_INSECURE=1 deriva uma."
            ),
        };
        let ollama_model_summary = src
            .var("OLLAMA_MODEL_SUMMARY")
            .unwrap_or_else(|_| "qwen2.5:1.5b".into());
        Self {
            edition,
            registration_mode,
            registration_domains,
            tenancy_mode,
            node_peer_capacity: opt("NODE_PEER_CAPACITY").map(|v| {
                v.trim()
                    .parse::<u32>()
                    .ok()
                    .filter(|n| *n > 0)
                    .unwrap_or_else(|| {
                        panic!("NODE_PEER_CAPACITY: «{v}» não é um inteiro positivo")
                    })
            }),
            max_ws_per_user: match bounded_env(src, "MAX_WS_PER_USER", 16, 0, 100_000) {
                0 => None,
                n => Some(n as u32),
            },
            org_max_participants: opt("ORG_MAX_PARTICIPANTS").map(|v| {
                v.trim()
                    .parse::<u32>()
                    .ok()
                    .filter(|n| *n > 0)
                    .unwrap_or_else(|| {
                        panic!("ORG_MAX_PARTICIPANTS: «{v}» não é um inteiro positivo")
                    })
            }),
            migrate_on_start: src.var("DELONIX_MIGRATE").ok().as_deref() != Some("0"),
            log_json: src.var("LOG_FORMAT").ok().as_deref() == Some("json"),
            internal_bind_addr: opt("INTERNAL_BIND_ADDR"),
            grpc_bind_addr: opt("GRPC_BIND_ADDR"),
            grpc_tls_cert: opt("GRPC_TLS_CERT"),
            grpc_tls_key: opt("GRPC_TLS_KEY"),
            grpc_client_ca: opt("GRPC_CLIENT_CA"),
            ui_dir: opt("UI_DIR").map(std::path::PathBuf::from),
            allow_insecure: insecure,
            database_url: {
                let url = secret(src, "DATABASE_URL", DEV_DB, insecure, 0);
                if !insecure && database_url_uses_burned_password(&url) {
                    // Aviso e não recusa: rodar a password de uma base é um
                    // `ALTER USER` com a aplicação parada, e a base não está
                    // exposta para fora do cluster. Os outros três abrem a
                    // API a quem os tem — esses recusam-se.
                    tracing::warn!(
                        "DATABASE_URL usa uma password que esteve publicada no repositório \
                         (deploy/k8s) — roda-a: está ao alcance de qualquer clone"
                    );
                }
                url
            },
            bind_addr: src
                .var("BIND_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:8180".into()),
            jwt_secret,
            secret_box,
            turn_host: src
                .var("TURN_HOST")
                .unwrap_or_else(|_| "localhost:3478".into()),
            turn_secret: secret(src, "TURN_SECRET", DEV_TURN, insecure, 16),
            access_ttl_secs: 15 * 60,
            refresh_ttl_secs: 30 * 24 * 3600,
            room_token_ttl_secs: 5 * 60,
            cors_origins,
            platform_odoo_url: src
                .var("PLATFORM_ODOO_URL")
                .ok()
                .map(|u| u.trim_end_matches('/').to_string())
                .filter(|u| !u.is_empty()),
            platform_odoo_db: src.var("PLATFORM_ODOO_DB").ok().filter(|d| !d.is_empty()),
            outbound_allow_hosts: csv_env(src, "OUTBOUND_ALLOW_HOSTS"),
            cookie_secure: src.var("COOKIE_INSECURE").ok().as_deref() != Some("1"),
            voice_internal_secret: src.var("VOICE_INTERNAL_SECRET").unwrap_or_default(),
            voice_secret_refusal: {
                let v = src.var("VOICE_INTERNAL_SECRET").unwrap_or_default();
                let refusal = voice_secret_refusal(&v, insecure);
                if let Some(r) = refusal {
                    tracing::warn!("API interna de IVR (/internal/v1/voice/ivr/*) DESLIGADA: {r}");
                }
                refusal
            },
            provisioning_secret: {
                let v = src.var("PROVISIONING_SECRET").unwrap_or_default();
                refuse_burned("PROVISIONING_SECRET", &v, insecure);
                v
            },
            platform_admin_user_ids: uuid_list(src, "PLATFORM_ADMIN_USER_IDS"),
            sms_unitel_smpp: opt("SMS_UNITEL_SMPP"),
            sms_movicel_smpp: opt("SMS_MOVICEL_SMPP"),
            sms_africell_smpp: opt("SMS_AFRICELL_SMPP"),
            sms_unitel_smpp_ca: opt("SMS_UNITEL_SMPP_CA"),
            sms_movicel_smpp_ca: opt("SMS_MOVICEL_SMPP_CA"),
            sms_africell_smpp_ca: opt("SMS_AFRICELL_SMPP_CA"),
            telephony_esl_addr: opt("TELEPHONY_ESL_ADDR"),
            telephony_esl_password: {
                let pw = src.var("TELEPHONY_ESL_PASSWORD").unwrap_or_default();
                if opt("TELEPHONY_ESL_ADDR").is_some() && pw.trim().is_empty() {
                    panic!("TELEPHONY_ESL_ADDR exige TELEPHONY_ESL_PASSWORD");
                }
                pw
            },
            telephony_sofia_profile: opt("TELEPHONY_SOFIA_PROFILE")
                .unwrap_or_else(|| "external".into()),
            telephony_kamailio_rpc_url: opt("TELEPHONY_KAMAILIO_RPC_URL"),
            telephony_emergency_numbers: {
                let spec =
                    opt("TELEPHONY_EMERGENCY_NUMBERS").unwrap_or_else(|| "112,113,115".into());
                delonix_meet_domain::telephony::dial_plan::parse_emergency_numbers(&spec)
            },
            telephony_country_code: opt("TELEPHONY_COUNTRY_CODE")
                .map(|c| c.trim_start_matches('+').to_string())
                .filter(|c| !c.is_empty() && c.bytes().all(|b| b.is_ascii_digit()))
                .unwrap_or_else(|| "244".into()),
            telephony_national_len: opt("TELEPHONY_NATIONAL_LEN")
                .and_then(|v| v.parse().ok())
                .filter(|n| (4..=15).contains(n))
                .unwrap_or(9),
            voice_tariff_inbound: src
                .var("VOICE_TARIFF_INBOUND")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0),
            push_lab_url: opt("PUSH_LAB_URL"),
            push_delonix_url: opt("PUSH_DELONIX_URL"),
            push_delonix_key: opt("PUSH_DELONIX_KEY"),
            push_wait_secs: opt("PUSH_WAIT_SECS")
                .and_then(|v| v.parse().ok())
                .filter(|n| *n <= 60)
                .unwrap_or(0),
            voice_ramais_domain_suffix: opt("VOICE_RAMAIS_DOMAIN_SUFFIX")
                .unwrap_or_else(|| "ramais.delonix.meet".into()),
            voice_meeting_access_number: {
                use delonix_meet_domain::telephony::extension as ext;
                bounded_env(
                    src,
                    "VOICE_MEETING_ACCESS_NUMBER",
                    ext::DEFAULT_MEETING_ACCESS_NUMBER,
                    ext::MEETING_ACCESS_NUMBER_MIN,
                    ext::MEETING_ACCESS_NUMBER_MAX,
                )
                .to_string()
            },
            voice_ramais_public: opt("VOICE_RAMAIS_PUBLIC_HOST").and_then(|host| {
                use delonix_meet_domain::telephony::extension as ext;
                let port = bounded_env(
                    src,
                    "VOICE_RAMAIS_PUBLIC_PORT",
                    ext::DEFAULT_SIP_PUBLIC_PORT,
                    1,
                    65_535,
                ) as u16;
                let transport = match opt("VOICE_RAMAIS_PUBLIC_TRANSPORT") {
                    None => ext::SipTransport::Udp,
                    Some(v) => ext::SipTransport::parse(&v).unwrap_or_else(|| {
                        tracing::warn!(
                            "VOICE_RAMAIS_PUBLIC_TRANSPORT='{v}' inválido (esperado udp, tcp ou tls) — a usar udp"
                        );
                        ext::SipTransport::Udp
                    }),
                };
                let server = ext::SipServer::new(&host, port, transport);
                if server.is_none() {
                    tracing::warn!(
                        "VOICE_RAMAIS_PUBLIC_HOST='{host}' não é um nome DNS nem um IP — os ramais saem sem endereço público (sip_server: null)"
                    );
                }
                server
            }),
            recordings_dir: src
                .var("RECORDINGS_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| std::path::PathBuf::from("recordings")),
            data_exports_dir: src
                .var("DATA_EXPORTS_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| {
                    src.var("RECORDINGS_DIR")
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|_| std::path::PathBuf::from("recordings"))
                        .join("exports")
                }),
            webauthn_rp_id: src.var("WEBAUTHN_RP_ID").ok().filter(|s| !s.is_empty()),
            webauthn_rp_origin: src.var("WEBAUTHN_RP_ORIGIN").ok().filter(|s| !s.is_empty()),
            redis_url: src.var("REDIS_URL").ok().filter(|s| !s.is_empty()),
            sfu_external_ip: src.var("SFU_EXTERNAL_IP").ok().filter(|s| !s.is_empty()),
            sfu_udp_min: bounded_env(
                src,
                "SFU_UDP_MIN",
                crate::sfu::SFU_UDP_MIN as usize,
                1_024,
                65_534,
            ) as u16,
            sfu_udp_max: bounded_env(
                src,
                "SFU_UDP_MAX",
                crate::sfu::SFU_UDP_MAX as usize,
                1_025,
                65_535,
            ) as u16,
            force_turn_relay: src.var("FORCE_TURN_RELAY").ok().as_deref() == Some("1"),
            smtp_host: opt("SMTP_HOST"),
            smtp_port: bounded_env(src, "SMTP_PORT", 587, 1, 65_535) as u16,
            smtp_username: opt("SMTP_USERNAME"),
            smtp_password: opt("SMTP_PASSWORD"),
            smtp_from: opt("SMTP_FROM"),
            smtp_starttls: src.var("SMTP_STARTTLS").ok().as_deref() != Some("0"),
            public_url: opt("PUBLIC_URL").map(|v| {
                let v = v.trim().trim_end_matches('/').to_string();
                // Fail-closed e em voz alta: um link de email para um esquema
                // estranho (ou sem esquema) seria um link partido em todas as
                // caixas de correio, e só se descobria quando alguém clicasse.
                if !(v.starts_with("https://") || v.starts_with("http://")) {
                    panic!("PUBLIC_URL: «{v}» tem de começar por https:// (ou http:// num laboratório)");
                }
                v
            }),
            ollama_url: src.var("OLLAMA_URL").ok().filter(|s| !s.is_empty()),
            ollama_model_translate: src
                .var("OLLAMA_MODEL_TRANSLATE")
                .unwrap_or_else(|_| "qwen2.5:1.5b".into()),
            ollama_model_summary: ollama_model_summary.clone(),
            ollama_model_studio: opt("OLLAMA_MODEL_STUDIO").unwrap_or(ollama_model_summary),
            ollama_timeout_secs: bounded_env(src, "OLLAMA_TIMEOUT_SECS", 120, 5, 900) as u64,
            ai_studio_concurrency_per_org: bounded_env(
                src,
                "AI_STUDIO_CONCURRENCY_PER_ORG",
                1,
                1,
                8,
            ),
            ws_queue_cap: bounded_env(src, "WS_QUEUE_CAP", 512, 32, 65_536),
            nego_queue_cap: bounded_env(src, "NEGO_QUEUE_CAP", 64, 4, 4_096),
            rec_queue_cap: bounded_env(src, "REC_QUEUE_CAP", 2_048, 64, 65_536),
            trusted_proxy_hops: bounded_env(src, "TRUSTED_PROXY_HOPS", 1, 1, 8),
            auth_rate_per_min: bounded_env(src, "AUTH_RATE_PER_MIN", 20, 5, 10_000),
            guest_join_per_ip_per_min: bounded_env(src, "GUEST_JOIN_PER_IP_PER_MIN", 10, 1, 1_000),
            guest_join_per_room_per_min: bounded_env(
                src,
                "GUEST_JOIN_PER_ROOM_PER_MIN",
                30,
                1,
                1_000,
            ),
            drain_grace_secs: bounded_env(src, "DRAIN_GRACE_SECS", 40, 1, 3_600) as u64,
            reconnect_grace_secs: bounded_env(src, "RECONNECT_GRACE_SECS", 45, 5, 300) as u64,
            drain_readiness_secs: bounded_env(src, "DRAIN_READINESS_SECS", 12, 0, 300) as u64,
            drain_compose_secs: bounded_env(src, "DRAIN_COMPOSE_SECS", 5, 0, 3_600) as u64,
            drain_reconnect_ms: bounded_env(src, "DRAIN_RECONNECT_MS", 2_000, 100, 60_000) as u64,
            ffmpeg_timeout_secs: bounded_env(src, "FFMPEG_TIMEOUT_SECS", 3_600, 30, 86_400) as u64,
            ffmpeg_threads: bounded_env(src, "FFMPEG_THREADS", 2, 1, 64) as u32,
            ffmpeg_max_concurrent: bounded_env(src, "FFMPEG_MAX_CONCURRENT", 1, 1, 64),
            max_directos: bounded_env(src, "MAX_DIRECTOS", 2, 0, 32),
            max_destinos_por_directo: bounded_env(src, "MAX_DESTINOS_POR_DIRECTO", 4, 1, 8),
            directo_threads: bounded_env(src, "DIRECTO_THREADS", 1, 1, 16) as u32,
            ffmpeg_bin: src.var("FFMPEG_BIN").unwrap_or_else(|_| "ffmpeg".into()),
            ffprobe_bin: src.var("FFPROBE_BIN").unwrap_or_else(|_| "ffprobe".into()),
            phone_bridge_freeswitch_ips: {
                let mut v = origens_env(src, "PHONE_BRIDGE_FREESWITCH_IPS").0;
                if let Some(um) = ip_env(src, "PSTN_BRIDGE_FREESWITCH_IP") {
                    if !v.contains(&um) {
                        v.push(um);
                    }
                }
                v
            },
            phone_bridge_freeswitch_names: origens_env(src, "PHONE_BRIDGE_FREESWITCH_IPS").1,
            phone_bridge_resolve_secs: bounded_env(src, "PHONE_BRIDGE_RESOLVE_SECS", 15, 5, 300)
                as u64,
            pstn_bridge_host_explicit: opt("PSTN_BRIDGE_HOST")
                .or_else(|| opt("SFU_EXTERNAL_IP"))
                .is_some(),
            phone_bridge_sip_bind: match src.var("PHONE_BRIDGE_SIP_BIND") {
                Ok(v) if !v.trim().is_empty() => match v.trim().parse() {
                    Ok(a) => Some(a),
                    Err(_) => {
                        tracing::warn!(
                            "PHONE_BRIDGE_SIP_BIND: «{v}» não é host:porta — ponte telefone↔sala desligada"
                        );
                        None
                    }
                },
                _ => None,
            },
            phone_bridge_sip_advertise: opt("PHONE_BRIDGE_SIP_ADVERTISE"),
            phone_bridge_rtp_ip: ip_env(src, "PHONE_BRIDGE_RTP_IP"),
            phone_bridge_rtp_ports: {
                let min = bounded_env(src, "PHONE_BRIDGE_RTP_MIN", 0, 0, 65_535) as u16;
                let max = bounded_env(src, "PHONE_BRIDGE_RTP_MAX", 0, 0, 65_535) as u16;
                (min > 0 && max >= min).then_some((min, max))
            },
            // Quem escreve `false` ou `off` quer desligar: um valor que não se
            // percebe NÃO pode cair no «ligado» por omissão.
            phone_bridge_wideband: match opt("PHONE_BRIDGE_WIDEBAND") {
                None => true,
                Some(v) => match v.trim().to_ascii_lowercase().as_str() {
                    "1" | "true" | "on" | "yes" => true,
                    "0" | "false" | "off" | "no" => false,
                    outro => {
                        tracing::warn!(
                            "PHONE_BRIDGE_WIDEBAND: «{outro}» não é 0 nem 1 — banda larga da ponte DESLIGADA"
                        );
                        false
                    }
                },
            },
            pstn_bridge_host: opt("PSTN_BRIDGE_HOST")
                .or_else(|| opt("SFU_EXTERNAL_IP"))
                .unwrap_or_else(|| "127.0.0.1".into()),
        }
    }
}

/// Lê um tamanho de fila do ambiente, preso a `[min, max]`. Um valor
/// inválido ou fora do intervalo cai no default com um aviso em vez de fazer
/// panic: uma fila mal configurada não deve impedir o servidor de arrancar,
/// mas também não pode virar «ilimitada por engano» com um 0 ou um u32 inteiro.
fn bounded_env(src: &Source, var: &str, default: usize, min: usize, max: usize) -> usize {
    match src.var(var) {
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
fn uuid_list(src: &Source, var: &str) -> Vec<uuid::Uuid> {
    csv_env(src, var)
        .iter()
        .map(|v| {
            v.parse().unwrap_or_else(|_| {
                panic!("{var}: «{v}» não é um UUID de utilizador (SELECT id FROM users WHERE email = …)")
            })
        })
        .collect()
}

/// Lê um único IP do ambiente. Ausente => `None` (fail-closed, ver
/// `Config::phone_bridge_freeswitch_ips`).
/// Presente mas ilegível como IP => aviso + `None` — o mesmo tratamento que
/// `bounded_env` dá a um valor fora do intervalo: nunca um panic por uma
/// variável de configuração de uma funcionalidade opcional, mas também nunca
/// um valor absurdo aceite em silêncio.
/// Lista de IPs separados por vírgulas (allowlist da ponte telefone↔sala).
/// Vazia => a ponte recusa tudo. Entradas ilegíveis avisam e saem — nunca um
/// panic por configuração de uma funcionalidade opcional, nunca um valor
/// absurdo aceite em silêncio.
/// Uma lista de origens (`a,b,c`): IPs e nomes de máquina. Uma entrada que não é nem uma
/// coisa nem outra avisa e cai — nunca vira «aceitar tudo».
fn origens_env(src: &Source, var: &str) -> (Vec<std::net::IpAddr>, Vec<String>) {
    use crate::phone_bridge::origens::{parse_origem, Origem};
    let Ok(raw) = src.var(var) else {
        return (Vec::new(), Vec::new());
    };
    let (mut ips, mut nomes) = (Vec::new(), Vec::new());
    for s in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        match parse_origem(s) {
            Some(Origem::Ip(ip)) => ips.push(ip),
            Some(Origem::Nome(n)) => nomes.push(n),
            None => tracing::warn!("{var}: «{s}» não é um IP nem um nome de máquina — ignorado"),
        }
    }
    (ips, nomes)
}

fn ip_env(src: &Source, var: &str) -> Option<std::net::IpAddr> {
    match src.var(var) {
        Err(_) => None,
        Ok(v) if v.trim().is_empty() => None,
        Ok(v) => match v.trim().parse() {
            Ok(ip) => Some(ip),
            Err(_) => {
                tracing::warn!("{var}='{v}' não é um IP válido — ponte PSTN↔SFU fica desactivada");
                None
            }
        },
    }
}

fn csv_env(src: &Source, var: &str) -> Vec<String> {
    src.var(var)
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

/// Valores de `VOICE_INTERNAL_SECRET` que já estiveram escritos em ficheiros
/// versionados deste repositório PÚBLICO. Estão queimados: qualquer clone os
/// tem, e o histórico do git não se reescreve (ver
/// `scripts/leaked-secrets-accepted.txt`). Um deploy que ainda os use é
/// recusado, e não aceite por ter comprimento suficiente.
pub const BURNED_VOICE_SECRETS: &[&str] = &[
    // deploy/k8s/01-config.yaml, de 98f5b28 (2026-07-10) até R154.
    "voice-internal-secret-for-pstn",
    // Makefile, `VOICE_SECRET ?=` — valor de dev.
    "dev-voice-secret-abc123",
];

/// `None` se o segredo de voz serve; `Some(razão)` se não. Com
/// `DELONIX_ALLOW_INSECURE=1` aceita-se qualquer valor não vazio — é o que
/// deixa o `make dev` continuar a usar o valor de dev do Makefile.
pub fn voice_secret_refusal(secret: &str, insecure: bool) -> Option<&'static str> {
    if secret.is_empty() {
        return Some("VOICE_INTERNAL_SECRET não está definido");
    }
    if insecure {
        return None;
    }
    if BURNED_VOICE_SECRETS.contains(&secret) {
        return Some(
            "VOICE_INTERNAL_SECRET é um valor de exemplo publicado no repositório — gera um novo",
        );
    }
    if secret.len() < VOICE_SECRET_MIN_LEN {
        return Some("VOICE_INTERNAL_SECRET tem menos de 32 caracteres");
    }
    None
}

/// Segredos da aplicação que estiveram escritos em `deploy/k8s/01-config.yaml`
/// (o Secret `delonix-secrets`) até 2026-10-04, num repositório PÚBLICO. Estão
/// queimados, como os de voz acima: com o `JWT_SECRET` forja-se a sessão de
/// qualquer conta, com o `PROVISIONING_SECRET` cria-se uma organização, com o
/// `TURN_SECRET` usa-se o relay de media. Um servidor em produção que ainda os
/// traga recusa arrancar — ver `scripts/leaked-secrets-accepted.txt`.
pub const BURNED_SECRETS: &[(&str, &str)] = &[
    ("JWT_SECRET", "stage-jwt-secret-min-32-chars-abcdef123456"),
    ("TURN_SECRET", "stage-turn-secret-key"),
    (
        "PROVISIONING_SECRET",
        "dlxprov_bcdc13c52115d2b67942298b6d548b65f47980470090a2e0",
    ),
];

/// Passwords de base de dados publicadas no mesmo sítio e nos ficheiros de
/// valores do Postgres (`deploy/k8s/helm-values/`).
pub const BURNED_DB_PASSWORDS: &[&str] =
    &["delonix_dev_pass", "delonix_prod_pass", "repl_prod_pass"];

/// `true` se `value` é o valor publicado de `var`.
pub fn is_burned(var: &str, value: &str) -> bool {
    BURNED_SECRETS
        .iter()
        .any(|(v, burned)| *v == var && *burned == value)
}

/// `true` se a password dentro do URL é uma das publicadas.
pub fn database_url_uses_burned_password(url: &str) -> bool {
    BURNED_DB_PASSWORDS
        .iter()
        .any(|p| url.contains(&format!(":{p}@")))
}

/// Em produção, um segredo publicado no repositório faz o arranque falhar em
/// voz alta. Com `DELONIX_ALLOW_INSECURE=1` aceita-se — é desenvolvimento.
fn refuse_burned(var: &str, value: &str, insecure: bool) {
    if !insecure && is_burned(var, value) {
        panic!(
            "{var} é um valor que esteve publicado neste repositório (deploy/k8s/01-config.yaml) — \
             está queimado. Gera um novo fora do repo (`make bootstrap`) e roda-o no cluster."
        );
    }
}

/// Lê um segredo do ambiente. Em produção (insecure=false) faz panic se estiver
/// ausente, igual ao default de dev, publicado no repositório, ou abaixo do
/// comprimento mínimo.
fn secret(src: &Source, var: &str, dev_default: &str, insecure: bool, min_len: usize) -> String {
    if let Ok(v) = src.var(var) {
        refuse_burned(var, &v, insecure);
    }
    match src.var(var) {
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

impl Config {
    /// A política de registo do domínio, montada a partir da configuração.
    pub fn registration_policy(
        &self,
    ) -> delonix_meet_domain::identity::registration::RegistrationPolicy {
        delonix_meet_domain::identity::registration::RegistrationPolicy {
            edition: self.edition,
            mode: self.registration_mode,
            tenancy: self.tenancy_mode,
            allowed_domains: self.registration_domains.clone(),
        }
    }

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
    use std::collections::HashMap;

    const KEY: &str = "k1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    const STRONG_JWT: &str = "0123456789abcdef0123456789abcdef0123456789abcdef";
    const STRONG_TURN: &str = "0123456789abcdef01234567";

    /// Um ambiente de produção que arranca: sem `DELONIX_ALLOW_INSECURE`, com
    /// os três segredos fortes e a chave de cifra. Cada teste troca UMA coisa.
    fn production<'a>(change: &[(&'a str, Option<&'a str>)]) -> Config {
        let mut vars: HashMap<&str, &str> = HashMap::from([
            ("JWT_SECRET", STRONG_JWT),
            ("TURN_SECRET", STRONG_TURN),
            (
                "DATABASE_URL",
                "postgres://delonix:uma-password-forte@db:5432/delonix_meet",
            ),
            ("DATA_ENCRYPTION_KEYS", KEY),
        ]);
        for (k, v) in change {
            match v {
                Some(v) => vars.insert(k, v),
                None => vars.remove(k),
            };
        }
        Config::from_map(&vars)
    }

    /// Controlo positivo: sem ele, os `should_panic` abaixo podiam estar a
    /// medir outra coisa que falta no mapa.
    #[test]
    fn production_with_strong_secrets_and_the_key_starts() {
        let c = production(&[]);
        assert!(!c.allow_insecure);
        assert!(c.secret_box.is_some());
    }

    #[test]
    #[should_panic(expected = "DATA_ENCRYPTION_KEYS tem de estar definida em produção")]
    fn production_without_the_encryption_key_refuses_to_start() {
        production(&[("DATA_ENCRYPTION_KEYS", None)]);
    }

    #[test]
    fn development_derives_a_key_when_none_is_given() {
        let c = production(&[
            ("DATA_ENCRYPTION_KEYS", None),
            ("DELONIX_ALLOW_INSECURE", Some("1")),
        ]);
        assert!(c.secret_box.is_some());
    }

    #[test]
    #[should_panic(expected = "JWT_SECRET é um valor que esteve publicado")]
    fn the_published_jwt_secret_is_refused() {
        production(&[(
            "JWT_SECRET",
            Some("stage-jwt-secret-min-32-chars-abcdef123456"),
        )]);
    }

    #[test]
    #[should_panic(expected = "TURN_SECRET é um valor que esteve publicado")]
    fn the_published_turn_secret_is_refused() {
        production(&[("TURN_SECRET", Some("stage-turn-secret-key"))]);
    }

    #[test]
    #[should_panic(expected = "PROVISIONING_SECRET é um valor que esteve publicado")]
    fn the_published_provisioning_secret_is_refused() {
        production(&[(
            "PROVISIONING_SECRET",
            Some("dlxprov_bcdc13c52115d2b67942298b6d548b65f47980470090a2e0"),
        )]);
    }

    /// Em desenvolvimento os valores publicados passam: é o que deixa um
    /// laboratório antigo continuar a arrancar com `DELONIX_ALLOW_INSECURE=1`.
    #[test]
    fn development_accepts_the_published_values() {
        let c = production(&[
            ("DELONIX_ALLOW_INSECURE", Some("1")),
            (
                "JWT_SECRET",
                Some("stage-jwt-secret-min-32-chars-abcdef123456"),
            ),
            ("TURN_SECRET", Some("stage-turn-secret-key")),
        ]);
        assert!(c.allow_insecure);
    }

    #[test]
    fn every_burned_secret_is_recognised_only_under_its_own_name() {
        for (var, value) in BURNED_SECRETS {
            assert!(is_burned(var, value));
            assert!(!is_burned("OUTRA_VARIAVEL", value));
        }
        assert!(!is_burned("JWT_SECRET", STRONG_JWT));
    }

    #[test]
    fn a_database_url_with_a_published_password_is_recognised() {
        for p in BURNED_DB_PASSWORDS {
            assert!(database_url_uses_burned_password(&format!(
                "postgres://delonix:{p}@delonix-postgres:5432/delonix_meet"
            )));
        }
        // O nome do UTILIZADOR ou da base não conta: só a password.
        assert!(!database_url_uses_burned_password(
            "postgres://delonix_dev_pass:forte@db:5432/delonix_dev_pass"
        ));
        assert!(!database_url_uses_burned_password(DEV_DB));
    }
}
