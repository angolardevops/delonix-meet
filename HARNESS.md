# Delonix Meet — AI Development Harness

> **Lê este ficheiro integralmente antes de qualquer tarefa.** Contém o contexto completo da plataforma, invariantes de segurança, convenções de código e o painel de revisores especializados.

---

## 1. Identidade e missão

**Delonix Meet** é uma plataforma de videoconferência **corporativa, self-hosted e SaaS**, construída de raiz para ser a alternativa séria ao Google Meet/Zoom/Teams em mercados com requisitos de **soberania de dados, conformidade regulatória (BNA/LGPD) e self-hosting**. O nome vem da *Delonix regia*, a flamboyant — exuberância e resiliência tropical.

**Público-alvo primário:** Empresas africanas e lusófonas, setor público, organizações que não podem ou não querem colocar comunicação crítica em cloud americana.

**Filosofia de produto:**
- Self-hosted first — funciona sem Internet externa
- Security by design — sem atalhos, sem "fazemos depois"
- Enterprise sem lock-in — open, webhooks, API keys
- Performance sem GC — backend 100% Rust

---

## 2. Stack técnica (estado real, julho 2026)

### Backend — `server/` (Rust)
| Crate/módulo | Função |
|---|---|
| axum 0.8 | HTTP server + router |
| webrtc-rs | SFU: DTLs/SRTP, RTP fan-out, simulcast |
| sqlx 0.8 | PostgreSQL async (migrações automáticas em main.rs) |
| tokio | Runtime async |
| argon2 | Hashing de passwords |
| jsonwebtoken | JWT access (15 min) + refresh (30 dias, rotativo) |
| reqwest 0.12 (rustls-tls) | Webhooks outbound (NÃO 0.13 — incompatível com rustls) |
| tower-http | CORS + tracing (features `cors`, `trace`). Os cabeçalhos de segurança NÃO vêm daqui — são postos à mão em `main.rs` (`nosniff`, `DENY`, HSTS) e no nginx; e não há camada de compressão. |

**Ficheiros principais:**
- `main.rs` — bootstrap, router, estado global (`AppState`), cron jobs (retention sweep)
- `config.rs` — lê env vars, **fail-closed sem segredos fortes** (panic no arranque)
- `auth.rs` — registo (cria org+admin), login, refresh, logout, room tokens
- `org.rs` — multi-tenant: organizations, branches, org_members, employee groups, salas presenciais, quotas, stats, SSO stubs
- `rooms.rs` — CRUD salas, `can_access_room` (isolamento cross-org), `insert_room` (helper reutilizado)
- `sfu.rs` — SFU Rust: Hub, Room, Publication, simulcast, PLI, gravação RTP→IVF/OGG
- `signaling.rs` — WebSocket `/ws` (room token): transporte SFU (offer/answer/ice) + moderação (admit/kick/lock/host-*) + chat/breakout-*/media
- `room_tools.rs` — contexto de colaboração in-room extraído de `signaling.rs`: sondagens, Q&A, temporizador, quadro branco (`impl SignalingHub::handle_tool_msg`)
- `presence.rs` — WebSocket `/rtc` (access token), chamadas WhatsApp-style: call-start/accept/decline/cancel, ring de reunião agendada
- `meetings.rs` — calendário, conflitos, quarentena, MoM, transcrição, webhooks de meeting
- `recordings.rs` — biblioteca de gravações, partilha read-only, sweep de retenção
- `recorder.rs` — gravação server-side: RTP→IVF(VP8)+OGG(Opus), ffmpeg post-stop (VP9+Opus webm), E2EE via decrypt_e2ee()
- `webhooks.rs` — CRUD webhooks org, fire() best-effort (Slack/Teams/Mattermost/generic+HMAC), SSRF guard
- `whiteboards.rs` — CRUD quadro branco persistente
- `voice.rs` — PSTN (stub, aguarda operador)
- `apikeys.rs` — API keys por org (hash + scopes)
- `audit.rs` — auditoria IMUTÁVEL e verificável: cada linha inclui o hash da anterior, numa cadeia por organização (migração 0037). Editar ou apagar uma linha parte a cadeia e é detectável em `/api/orgs/{id}/audit/verify` — mesmo por quem não confia em quem administra a base de dados, que é o adversário que interessa. Gatilhos recusam UPDATE/DELETE; a cadeia é a defesa que sobrevive a quem os possa remover. Ver R61
- `rate_limit.rs` — rate limit por IP/conta (DashMap, lockout login 8/5min)
- `error.rs` — `AppError` unificado → HTTP status + JSON body
- `metrics.rs` — contadores atómicos de observabilidade (WS, SFU, saturação das filas) expostos em `/metrics` (Prometheus). Das filas: `delonix_ws_queue_high_water` (marca de água — a folga real face a `WS_QUEUE_CAP`), `delonix_ws_queue_dropped_total` (efémeros perdidos), `delonix_ws_slow_consumer_kills_total` (sockets fechados por transbordo) e `delonix_nego_queue_dropped_total`. Marca de água e não profundidade instantânea: um gauge somado entre sockets vaza quando uma task de escrita morre a meio
- `users.rs` — perfis de utilizador (perfil público, `me`, update, pesquisa)
- `actions.rs` — agenda de reunião (tópicos com execução) + Plano de Ação 5W2H
- `mfa.rs` — segundo factor por TOTP (RFC 6238) e códigos de recuperação. O algoritmo é implementado aqui em vez de por dependência nova: HMAC-SHA-1, base64 e argon2 já eram dependências, e o RFC traz **vectores de teste oficiais** — uma verificação independente melhor do que confiar numa crate. Um código válido é CONSUMIDO, não só verificado (`last_step`, e `used_at` nos de recuperação): sem isso um TOTP apanhado por cima do ombro servia outra vez durante 30 s. Ver R53
- `mls.rs` — MLS key agreement para E2EE em grupo (key packages, welcome)
- `dlp.rs` — DLP (censura/redação de conteúdo sensível)
- `pubsub.rs` — Redis pub/sub para entrega cross-nó (presença/sinalização)
- `redis_state.rs` — estado in-room em Redis (whiteboard, timer, sondagens, settings) partilhado entre pods
- `ai.rs` — IA local via Ollama in-cluster: tradução de legendas em tempo real e resumo da ata. Fail-open sem `OLLAMA_URL` (o MoM cai para as regras do cliente); o texto das reuniões nunca sai para uma cloud externa
- `odoo.rs` — integração Odoo (módulo `nk_delonix_meet`): token `dlxo_<hex>`, provisionamento de utilizadores via `/api/v1/integration/odoo/provision`, descoberta da config Odoo de um utilizador (`org_odoo_config`)
- `odoo_sso.rs` — **login com conta Odoo**: autentica em `/web/session/authenticate`, cria a organização a partir da EMPRESA do utilizador (chave `(odoo_db, company_id)`) e sincroniza em segundo plano todos os utilizadores internos activos. Fail-closed sem `PLATFORM_ODOO_URL`/`PLATFORM_ODOO_DB`
- `meetings_v1.rs` — recurso `meetings` da API pública v1 (POST/PATCH/DELETE): cria REUNIÃO + sala com anfitrião humano (`host_email`) e convidados por email, idempotente por `external_ref`. É o que a integração de calendário usa — `/api/v1/rooms` cria salas sem anfitrião nem convidados, e ninguém consegue ser admitido nelas
- `sfu_e2e.rs` — testes ponta-a-ponta do SFU com `RTCPeerConnection`s reais no papel de browser (media a fluir nos dois sentidos + R13/glare). Só compila em `#[cfg(test)]`
- `storage.rs` — armazenamento remoto da plataforma (TrueNAS NFS / Nextcloud WebDAV); registo único em `platform_storage`, gerido pelo admin global

### Frontend — `web/src/` (React + TypeScript + Vite)
| Ficheiro/pasta | Função |
|---|---|
| `App.tsx` | Router por hash (`#/`), gate de auth, `PresenceProvider` acima do router |
| `api.ts` | Cliente REST (auth, rooms, orgs, meetings, recordings, apikeys, webhooks) |
| `signaling.ts` | Cliente WS `/ws` tipado |
| `webrtc.ts` | `SfuCall`: RTCPeerConnection, simulcast, screen share, `enhanceOpus()` (munge SDP recebido), QoS getStats |
| `presence.ts` | `PresenceProvider`: WS `/rtc`, modal de chamada, toasts de recusa |
| `e2ee.ts` | Insertable Streams AES-256-GCM, chave PBKDF2, worker |
| `media.ts` | `BackgroundEffect` (blur/RVM ONNX), `HeadTracker` (parallax 3D), `Transcriber` (Web Speech + Whisper fallback), `MeetingRecorder`, `LevelWatcher` |
| `whisperWorker.ts` | Whisper-tiny ONNX em worker, modelo self-hosted, env.allowRemoteModels=false |
| `matte.ts` | Matting RVM (fundo virtual com segmentação) |
| `i18n.ts` | i18next PT/EN, persist `dx_lang` |
| `pages/Room.tsx` | Sala: grelha↔palco, controls, breakouts, host controls, whiteboard, polls, Q&A, CC, timer, gravação |
| `pages/Landing.tsx` | Página pública (unauth na raiz) |
| `pages/Calendar.tsx` | Calendário mês/semana/agenda, agendamento, conflitos, ics |
| `pages/Recordings.tsx` | Biblioteca, viewer, tarefas do MoM |
| `pages/Analytics.tsx` | Admin org: KPIs, membros, webhooks, settings, SSO stubs |
| `pages/Directory.tsx` | Org/filiais/employees/grupos/salas presenciais, chamadas |
| `styles/` | SCSS com tokens em `:root`, temas via `[data-theme=...]` |

### Infraestrutura
| Serviço | Port (dev) | Uso |
|---|---|---|
| PostgreSQL | 5435 | Dados principais (migrações 0001–0037) |
| Redis | 6379 | Presença, pub/sub (multi-instância futura) |
| coturn | 3478/5349 | STUN/TURN para WebRTC NAT traversal |

---

## 3. Feature inventory (julho 2026)

### ✅ Feito e funcional
- Auth org-first: registo cria org+admin; login; refresh cookie HttpOnly; logout revoga
- Multi-tenant: isolamento cross-org em todos os endpoints (rooms, presence, search, analytics)
- SFU Rust: simulcast (q/h/f), screen share como track separada, E2EE server-side (decrypt), gravação server-side (VP9+Opus webm, ffmpeg composite multi-publicador)
- Sala de reunião: grelha Meet-style, palco com speaker detection, controles estilo Google Meet (pill dividida mic/câmara), whiteboard, breakouts (rename/add/move/timer/return-all), host controls (lock, share-only), CC (legendas partilhadas), reações, mão levantada, gravação
- Ferramentas in-room: timer, sondagens anónimas, Q&A com upvote
- Background effects: blur leve/forte, fundos virtuais, RVM ONNX (fallback MediaPipe)
- Transcrição: Web Speech (Chrome/Edge) + Whisper-tiny WASM (todos os browsers), multi-idioma
- Notas AI → MoM: gerado ao terminar, guardado em meetings, consulta em Recordings
- Calendário: mês/semana/agenda, conflitos real-time, quarentena, aceitar/recusar, MoM no evento, .ics
- Recordings: biblioteca, viewer (player+transcrição+MoM+tarefas), toggle cards/tabela, partilha read-only
- Analytics admin: KPIs 30d, série semanal, top organizadores, kind split, duração média, postura SSO/SCIM (stubs)
- Webhooks: Slack/Teams/Mattermost/generic+HMAC, SSRF guard, events: meeting.created/started/recording.ready
- API keys por org (hash + scopes)
- PWA: manifest + service worker
- Temas: Delonix (dark), NgolaCloud (claro quente), NgolaCloud-dark, Kaeso (corporativo flat)
- i18n PT/EN (Landing, Shell, Login, Home, Analytics, Roadmap — Room/Calendar/Recordings/Directory por traduzir)
- Status page pública (`/api/status`, `#/status`)
- PSTN/voz: voz.rs stub, aguarda operador externo

### ⬜ Próximas prioridades (ordenadas)
1. **SSO OIDC genérico** — config-driven, sem hardcode de provider
2. **App mobile Flutter** — chamadas voz/vídeo, push, CallKit/ConnectionService
3. **PSTN dial-in** — FreeSWITCH + Kamailio (docs em `docs/`)
4. **i18n completo** — Room, Calendar, Recordings, Directory
5. **SCIM provisioning** — auto-sync de utilizadores de IdP corporativo
6. **Whisper server-side** — diarização por track, qualidade superior ao WASM
7. **MLS key agreement** — substituir PBKDF2 por Signal/MLS para E2EE em grupo
8. **SDK público** — REST + WS tipado para integrações

---

## 4. Arquitetura — decisões não óbvias

### SFU próprio (não LiveKit/mediasoup)
O SFU está em `server/src/sfu.rs`. Decisão: **evoluir o SFU Rust** em vez de migrar para LiveKit. Razões: sem dependência externa, deploy simples (binário único), controlo total do pipeline RTP para E2EE + gravação side-car. Custo: simulcast e congestion control têm de ser feitos à mão.

### E2EE com gravação servidor
Aparente contradição resolvida por **key delegation**: o anfitrião cede a chave AES-256 (base64) no `server-record`; o servidor decifra os frames E2EE antes de gravar; a chave fica só em memória na `RecordingSession` — nunca em disco nem DB. O utilizador recebe confirm() explícito antes de gravar.

### Gravação IVF/OGG → ffmpeg
O `IVFWriter` da lib usa contador de frames como PTS (velocidade errada). `recorder.rs` usa PTS em ms reais do RTP. Dims VP8 lidos do keyframe e corrigidos no close. Stop → ffmpeg: 1 publicador = remux `-c copy`; N = xstack+amix.

### Grid layout (Room.tsx)
`useGridLayout` calcula best-fit 16:9 com `ResizeObserver`. **NÃO usar `var()` CSS para dimensões dos tiles** — transições congelam em janelas background. Dimensões inline por tile.

### Cookie HttpOnly para refresh
O refresh token vive em `dlx_refresh` (`HttpOnly; SameSite=Strict; Path=/api/auth; Secure`). O access token continua em Authorization header (localStorage). `COOKIE_INSECURE=1` só em dev HTTP puro.

### Vite proxy
`vite.config.ts` faz proxy de `/api`, `/ws` (ws:true) e `/rtc` (ws:true) para o backend. Mudanças no proxy exigem reinício do Vite (não HMR).

---

## 5. Design system

Tokens em `web/src/styles/` como custom properties CSS (`:root`). Hierarquia: **primitivos → semânticos → componentes**.

**Separação AÇÃO / MARCA:** o índigo é a cor de **ação** (botões primários, foco, links, nav ativo); o vermelho + dourado são a **marca** (logo, wordmark «Meet», landing, quadrado da sidebar). Nunca usar o vermelho para navegação nem o índigo para o logo.

| Token | Escuro | Claro | Uso |
| --- | --- | --- | --- |
| `--accent` | `#5c6cf2` | `#3947c9` | Ação primária, foco |
| `--accent-hi` | `#7c88f5` | `#4b5ad9` | Hover da ação |
| `--accent-text` | `#9aa5ff` | `#3947c9` | Índigo legível como texto/link |
| `--accent-soft` | `#242b4e` | `#e6e9fb` | Preenchimento de estado ativo/chip |
| `--bg` | `#14161d` | `#f4f5f7` | Fundo da página |
| `--surface` | `#1c1f28` | `#ffffff` | Cartões/modais |
| `--surface-2` | `#1a1d26` | `#f8f9fb` | Hover de linha, superfície aninhada |
| `--input-bg` | `#171a22` | `#ffffff` | Campos de formulário |
| `--border` | `#262a34` | `#e2e5eb` | Contorno de superfície |
| `--border-soft` | `#20242e` | `#eef0f4` | Separadores DENTRO do cartão |
| `--text` / `--text-2` | `#e8eaf0` / `#8b92a8` | `#1c2333` / `#5f6a82` | Texto primário/secundário |
| `--sb-bg` / `--sb-text` | `#12141a` / `#aeb4c6` | `#1e2a45` / `#c6cfe4` | **Rail de navegação — escuro nos DOIS temas** |
| `--hdr-bg` | `#14161d` | `#ffffff` | Barra de aplicação (topo) |
| `--accent-2` | `#EDA33B` | índigo escuro | Dourado de marca (wordmark) |
| `--brand` | `#D8352E` | `#C8201D` | Vermelho Delonix (logo, landing) |

**Regra da sala:** `.room-page` e `.waiting-page` reafirmam tokens dark **com `!important`** no fim de `styles.scss`. A sala é sempre escura independentemente do tema da app. Chrome da sala: fundo `#0d0f14`, barras `#12141a`, palco `linear-gradient(160deg,#1b2030,#12141c)`, painel lateral 320px encostado.

**Rail sempre escuro:** a barra lateral usa os tokens `--sb-*`, que são deliberadamente escuros também no tema claro (navy `#1e2a45`). Não a fazer seguir o tema — é âncora de identidade e evita que a navegação compita com o conteúdo.

**Sistema de controlo único (14/07/2026)** — referência completa em `docs/reference/design-system.md`:
- Tokens: `--radius-sm: 4px` (controlos) · `--radius-md: 6px` (superfícies) · `--radius-lg: 8px` · `--ctl-h: 30px` (altura única dos controlos). Camada de uniformização no FIM de `styles.scss` (3 tiers: ação / botão-ícone / superfícies) vence os valores históricos hardcoded.
- **Componentes novos usam o kit `web/src/components/ui.tsx`** (`Btn`/`IconBtn`/`Card`/`Field`/`TextInput`/`SelectCtl`/`Switch`) — nunca `<button className=…>` ad-hoc, nunca `border-radius`/`height` hardcoded na página. Variante nova = classe no CSS + entrada no kit. Migração do código existente é oportunista (referência: painel Ferramentas em `Room.tsx`).
- **Temas** = mapas de tokens em `styles/tokens.scss` emitidos sob `[data-theme=…]` — nunca overrides espalhados; testar os 4 temas + sala sempre escura (regressão #67).

**Camada CONSOLA (27/07/2026)** — no fim de `styles.scss`, DEPOIS do bloco de controlo único (à mesma especificidade, a última vence):

- Densidade: `html { font-size: 15px }`. A app dimensiona quase toda em `rem`, por isso a raiz é o botão único de densidade — não apertar tamanhos página a página.
- `.app-bar` (topo do conteúdo, em `Shell.tsx`): data, tema, «Nova reunião» e campo de código. Estas ações **saíram da Home** — não as duplicar lá.
- Estrutura do Shell: `.shell-main` (flex column, overflow hidden) → `.app-bar` + `.shell-body` (o que faz scroll). Páginas de altura total dentro do Shell usam `height: 100%`, nunca `100vh` (a barra já ocupa ~46px).
- Superfícies separam-se por **borda de 1px + luminância**, não por sombra: `--shadow` é 1px, `--border-soft` para separadores internos.
- Sala: controlos quadrados de 38px agrupados em `.ctrl-group` (dispositivos | sessão) + terminar solto. A pill Meet de 50px foi substituída; o chevron de dispositivo é um caret de 15px no canto.

**Fontes:** IBM Plex Sans (títulos e corpo) + IBM Plex Mono (código, horas, códigos de sala) — self-hosted via @fontsource. Família única de propósito: é o que dá a métrica de consola.

**Logo:** Globo vermelho com grelha dourada, anéis segmentados, 5 pinos. SVG em `web/public/logo.svg`. Usar `.brand-logo` para renderizar.

---

## 6. Invariantes de segurança (nunca quebrar)

1. **Segredos fail-closed:** `config.rs` faz panic sem `JWT_SECRET`/`TURN_SECRET`/`DATABASE_URL` fortes. `DELONIX_ALLOW_INSECURE=1` só em dev.
2. **Isolamento multi-tenant:** `rooms::can_access_room` e `org::org_co_members`/`admin_orgs_of_user` escopam TUDO à(s) org(s) do utilizador. Nunca devolver dados cross-org.
3. **Room tokens de curta duração:** JWT separado, âmbito = 1 sala, expira em 5 min. Sem room token válido → WS recusado.
4. **SSRF em webhooks:** validar host (bloquear IPs privados/loopback/link-local/metadata) na criação E na entrega. Sem redirects.
5. **Rate limit:** lockout por conta no login (8/5min); rate limit por IP em `/api/v1`; WS com rate limit por socket.
6. **Cookie Secure:** `dlx_refresh` sempre `Secure` exceto com `COOKIE_INSECURE=1` em dev HTTP.
7. **E2EE real:** chave AES-256 gerada no cliente, nunca vai ao servidor exceto para gravação (key delegation explícita com confirm() do utilizador).
8. **Validação no servidor:** autorização de host controls (lock/share-only/kick) validada em `signaling.rs`, não confiada no cliente.
9. **reqwest 0.12 com rustls-tls** (não 0.13) para compatibilidade com a versão do rustls no workspace.
10. **Uma conta tem UMA autoridade de autenticação, explícita:** `users.odoo_org_id` — a org que a gere, gravada quando a conta nasce de um Odoo e nunca reescrita por outra. NULL = conta local, autenticada localmente. **Nunca** resolver o provedor de autenticação por email nem por pertença a org (`LIMIT 1` sem ordem = escolher a autoridade por sorteio). E uma sincronização de directório **nunca reclama uma conta que já existe** — nem de outra org, nem local. As duas metades são precisas; fechar só uma deixa a porta entreaberta. Ver R25.

11. **Nenhuma fila de saída sem limite.** `WS_QUEUE_CAP` (default 512, por socket `/ws` e `/rtc`) e `NEGO_QUEUE_CAP` (default 64, renegociação do SFU por peer). Uma fila ilimitada transformava um consumidor lento — que na nossa rede-alvo é o caso NORMAL, não a excepção — num OOM que levava consigo todas as salas do pod. Cheia, descarta-se só o efémero e auto-substituível (`ServerMsg::is_droppable`: legenda parcial, traço de quadro, reacção) e conta-se; com protocolo ou estado fecha-se o socket e o cliente reentra. Nunca `send().await` nestes caminhos (os emissores correm dentro do lock do `DashMap` — R16): é sempre `try_send`. Ver R32/R33.

---

## 7. Workflow de desenvolvimento

```bash
# Arrancar tudo
make dev          # infra + backend + frontend

# Só o backend (com ALLOW_INSECURE)
bash deploy/run-dev-server.sh

# Migrações
cd server && cargo sqlx migrate run

# Build de produção
cargo build --release    # depois de migração nova, SEMPRE rebuild antes de restart
```

**Portas dev:** backend `8180`, frontend `5173`, Postgres `5435`, Redis `6379`, coturn `3478`.

**HTTPS:** obrigatório para câmara/microfone fora de localhost. Em rede local usar o nginx do `deploy/nginx-delonix.conf` com certificado self-signed.

**Testar chamada:** abrir duas abas `http://localhost:5173` em browsers diferentes, criar sala (SFU), juntar ambos, verificar vídeo e áudio bidirecionais.

**Deploy após alterações (REGRA):** depois de alterações merged, correr SEMPRE `make image-push` — gera imagens **versionadas** (tag `git describe`, ex.: `v1.0.0-16-g6d447e0`), faz `kind load` e **pina** a tag nos Deployments (`make pin`) com rollout status. Nunca confiar no `:latest` (imagem stale já causou "estilos perdidos" em stage). O ingress do **cluster kind** (VIP 172.30.0.200) é a porta de entrada única com cert **wildcard `*.delonix.local`** (mkcert, `deploy/certs/wildcard.*`): `meet.delonix.local` → Meet (in-cluster); `cloud.delonix.local` → delonix-engine no HOST :9443 (via ponte systemd user `ngolacloud-bridge.socket` 172.30.0.1→127.0.0.1, dispensável se o engine usar `--bind 0.0.0.0`); `pdf.delonix.local` → delonix-pdf no HOST :3000 (manifests em `deploy/k8s/06-external-apps.yaml`). nginx local e systemd delonix-* REMOVIDOS (13/07/2026); `publish-web.sh` obsoleto. Volumes docker locais `delonix-meet_pgdata/redisdata` preservados como backup da BD antiga (migrada para o cluster; dumps em `backups/`).

**Bases de dados de teste:** usar emails `@teste.local`. Limpar no fim: `DELETE FROM users WHERE email ~ '@teste\.local$'`. **⚠ Gravações: NUNCA `rm *.webm` em bloco** — apagar SÓ os ids criados pelo teste (regista-os ao criar), confirmando primeiro na BD que `uploader_id` é um utilizador `@teste.local`. Um `rm` cego já destruiu uma gravação real do utilizador (12/07/2026).

---

## 8. Padrões de código

### Rust
- `AppError` para todos os erros de handler — nunca `unwrap()` em código de produção
- Pool Postgres via `Extension<PgPool>` injetado pelo axum
- `sqlx::query` / `sqlx::query_as::<_, T>` — **API de runtime**, sem verificação
  em compile time. É o estado real: 118 chamadas, zero macros `query!`. A
  consequência tem de ser dita: um nome de coluna errado passa a compilação e
  só falha em execução, por isso qualquer alteração de esquema exige o teste
  que percorre o caminho. A alternativa (`query!` + `cargo sqlx prepare`)
  obrigaria a manter `.sqlx/` em dia e uma base acessível no build — não foi
  adoptada, e enquanto não for, não se escreve o contrário na documentação.
- Handlers async retornam `Result<impl IntoResponse, AppError>`
- Migrações em `server/migrations/` com prefixo numérico sequencial (`0001_`, `0002_`, …)
- Novos módulos: declarar em `main.rs` (`mod novo_modulo;`) + registar rotas no router

### TypeScript/React
- Componentes funcionais + hooks
- Estado global via Context (ver `PresenceProvider`, `RoomContext`)
- Mensagens WS tipadas com discriminant union (`type: "join" | "offer" | ...`)
- Tokens CSS via custom properties — nunca hardcode de cores
- i18n: `useTranslation()` + chave namespaced (`t('room.leave')`)
- **Nunca `var()` para dimensões de tiles** — ver Grid layout acima

---

## 9. Posicionamento competitivo

Ver `docs/competitive-positioning.md` para análise completa. Resumo:

| Funcionalidade | Zoom | Teams | Meet | **Delonix** |
|---|---|---|---|---|
| Self-hosted | ❌ | ❌ | ❌ | ✅ |
| SaaS | ✅ | ✅ | ✅ | ✅ |
| Backend open | ❌ | ❌ | ❌ | ✅ Rust |
| E2EE real | ✅ (pago) | ⚠️ parcial | ⚠️ parcial | ✅ sempre |
| Soberania de dados | ❌ | ❌ | ❌ | ✅ |
| Multi-tenant isolado | ✅ (conta) | ✅ (tenant) | ✅ (workspace) | ✅ (org+filial) |
| PSTN | ✅ (add-on) | ✅ (add-on) | ✅ (add-on) | ✅ (roadmap) |
| Whiteboard | ✅ | ✅ | ✅ | ✅ |
| MoM por AI | ✅ (Copilot pago) | ✅ (Copilot pago) | ✅ (Duet pago) | ✅ (incluso, local ou Claude API) |
| Preço self-host | ❌ | ❌ | ❌ | ✅ sem royalty |
| Compilado (Rust) | ❌ (Electron/JS) | ❌ (Electron) | N/A | ✅ binário único |

**O que Delonix faz que nenhum dos três faz:**
- Deploy de binário único sem runtime externo
- E2EE com gravação server-side (key delegation)
- Soberania de dados total (dados nunca saem do teu datacenter)
- Licença sem royalty para self-host
- Conformidade BNA/LGPD fora da caixa
- Hierarquia organizacional (org → filiais → grupos) nativa
- API keys + webhooks enterprise incluídos no core (não add-on)
- MoM por AI configurável com LLM local (Ollama) — sem cloud obrigatório

---

## 10. Painel de revisores especializados

**Subagentes autónomos** em `agents/` (invocar via Agent/`@nome`) — especialistas Delonix, cada um com o catálogo de regressões (`docs/reference/regressions.md`) no radar. Usar PROACTIVAMENTE nas áreas respetivas:

| Agente | Especialidade | Invocar para (ficheiros) |
|---|---|---|
| **delonix-code** | Rust supremo (nível criador): safety, ownership/lifetimes, async Tokio, unsafe, perf hot-path RTP | `server/src/*.rs` (sobretudo `sfu.rs`, `recorder.rs`, `signaling.rs`) |
| **delonix-devops** | Platform engineering: K8s, Docker, Ansible, Terraform, coturn/rede WebRTC, ingress, metallb, afinidade, media | `deploy/`, `deploy/k8s/`, Dockerfiles, Makefile, TURN |
| **delonix-frontend** | Frontend supremo: React/TS/CSS4/HTML5/JS + UX Meet/Teams/Zoom | `web/src/**` (`Room.tsx`, `webrtc.ts`, `presence.ts`, `styles/`) |
| **delonix-security-compliance** | Segurança (cripto/E2EE/TLS/DTLS/JWT/SSRF/rate-limit/cross-org) + compliance (eDiscovery/DLP/SCIM/audit/BNA/LGPD) | `auth.rs`, `e2ee.ts`, `webhooks.rs`, `config.rs`, `org.rs`, endpoints novos |
| **webrtc-sfu-reviewer** | WebRTC/SFU (Justin Uberti): ICE, simulcast, codecs, media num-só-sentido | `sfu.rs`, `webrtc.ts`, `e2ee.ts`, `recorder.rs` |
| **competitive-strategist** | Posicionamento vs Zoom/Teams/Meet, priorização de roadmap | features novas, decisões de produto |

Personas adicionais (invocar em prompt, perfis em `docs/ai-reviewers.md`): **Lars Bak** (WASM/Worker), **Lea Verou** (CSS/a11y), **Zoom Reliability Architect** (fallback/TURN/redes degradadas).

---

## 11. Gotchas conhecidos

- **Vite proxy:** `/ws` E `/rtc` precisam de `ws: true`. Reiniciar Vite após mudanças.
- **Grid tiles:** nunca `var()` CSS para largura/altura dos tiles — transições congelam em background.
- **rebuild release:** após nova migração, sempre `cargo build --release` antes de `systemctl restart`.
- **reqwest 0.12:** não atualizar para 0.13 — incompatível com a versão do rustls no workspace.
- **Web Speech API:** só Chrome/Edge. Firefox/Safari cai para Whisper WASM (mais lento).
- **getUserMedia/parallax/blur:** precisam de contexto seguro (HTTPS ou localhost).
- **Timer/animações Electron:** rAF/setInterval throttled em background — validar em ecrã real.
- **E2EE + gravação:** `e2eeKeyRef` capturado ANTES de `setKey()` — o buffer é transferido ao worker.
- **IVFWriter PTS:** a lib usa contador de frames (errado). `recorder.rs` usa PTS em ms do RTP — não reverter.
- **Regex i18n poda:** a poda de chaves por regex apagou `common.save` (greedy) — cuidado com chaves genéricas.
- **Salas presenciais vs salas virtuais:** `meeting_rooms` (presenciais, em org.rs) ≠ `rooms` (virtuais, em rooms.rs).
- **Email domain org:** utilizadores legados têm `email_domain=''` — o índice único ignora vazios (funciona).
- **Gravações no K8s = PVC partilhado** (`delonix-recordings`, montado em `/var/lib/delonix/recordings` nas réplicas do server — 02-server.yaml + RECORDINGS_DIR no configmap). Em kind (nó único) RWO chega (pods no mesmo nó partilham o PV local-path); **multi-nó exige RWX (NFS/CephFS) ou object storage (MinIO/S3)**. ⚠ `kubectl apply -f 02-server.yaml` repõe `image: :latest` (stale no nó!) → correr SEMPRE `make pin IMAGE_TAG=<tag>` a seguir a um apply avulso (o `make stage` já re-pina).
- **Afinidade por sala (K8s multi-réplica):** o SFU é in-memory por pod; o Redis propaga sinalização/presença, NÃO RTP. Todos os pares de uma sala têm de cair no MESMO pod → cliente envia `/ws?...&room=CODE` e o ingress faz `upstream-hash-by: $arg_room`. **CRÍTICO:** o `/ws` tem de usar um **Service DEDICADO** (`delonix-server-ws`, mesmos pods) — se `/ws` e `/api`/`/rtc` partilharem o mesmo Service, o ingress-nginx funde-os num backend e DESCARTA o `upstream-hash-by` (round-robin ganha) → afinidade não se aplica. Verificar: `curl .../ws?room=X` repetido → sempre o mesmo pod. Sem isto: media num só sentido, admissão e partilha de ecrã falham. `/rtc` (presença) é fanned por Redis, não precisa de afinidade.
- **Rate limit WS = token bucket** (`signaling.rs`): 600 burst / 300 sustained. Uma janela fixa baixa cortava o próprio anfitrião durante a rajada de ICE/renegociação — não voltar a uma janela fixa apertada.
- **Transcrição host-gated e distribuída:** só o anfitrião liga (`TranscriptionToggle`); o servidor difunde `Transcription`; CADA cliente transcreve o próprio microfone e difunde `transcript`. Motor: Web Speech (Chrome, mas envia áudio à Google) com fallback automático para **Whisper WASM local** em erro `network` — preferir local para soberania.
- **`vite.config.ts` só lê certos de dev no `serve`** (nunca no `build`) — senão a imagem Docker web falha no `npm run build`.
- **Oferta SFU na CONSTRUÇÃO (`webrtc.ts`):** a `SfuCall` envia o `sfu-offer` inicial no construtor, NÃO num `signal.on('joined')` interno. Porquê: a `SfuCall` é criada *dentro* do handler `joined` (o `callHolder` em `Room.tsx` adia a criação até `joined`), portanto um listener registado no construtor perderia o evento que já disparou → sem oferta → sem `pc connected`/`track published` → media morta. Regressão já custou uma sessão — não voltar a "gatear" a oferta pelo `joined`.
- **Convidado em espera NÃO monta a SFU (`Room.tsx` `callHolder`):** enquanto aguarda admissão, o convidado não pode criar `SfuCall` — senão gera oferta stale → glare/rollback em loop → flood → o rate-limit derruba → reload após admitir. A `SfuCall` só nasce no handler `joined` (após admissão real). `callHolder.start()` é idempotente (`if callRef.current || cancelled return`).
- **`.dockerignore` NÃO pode excluir `web/dist`:** o `Dockerfile.web.stage` faz `COPY web/dist` (usa o build local); excluir `web/dist` parte o `make stage`. Excluir sim: `server/target`, `web/node_modules`, `web/public/{ort,ort-rvm,models/*}`, `deploy/*.env`, `agents/worktrees` (contexto Docker de 4.5GB→<1MB; sem isto o cache serve imagem stale e o Rust não recompila).
- **Media K8s = relay-only via coturn (`FORCE_TURN_RELAY=1`):** em K8s o IP do pod (10.244.x) é inalcançável de fora e os host candidates do SFU não transportam media → sem relay-only o ICE "liga" mas fica preto. `FORCE_TURN_RELAY=1` põe `iceTransportPolicy:relay` no `/api/ice` E no `RTCConfiguration` do SFU; exige coturn alcançável (em stage: no HOST via `deploy/run-host-coturn.sh`, `TURN_HOST=172.30.0.1:3478`). Em local (systemd, mesmo host) NÃO ligar — host candidates chegam. **Aberto:** alocação TURN instável (`438 Stale nonce`/`allocation timeout`) → ver [[k8s-media-turn]] / `docs/reference/regressions.md`. Não é `/rtc` (presença = Redis, sem afinidade nem relay).
- **Servidor é autoritativo em ações de sala partilhadas:** `wb-open`/`wb-close` (quadro branco abre/fecha em TODOS; abrir só apresentador/anfitrião), `Presenting`/limpar apresentação ao parar screen-share, e abrir o painel de transcrição são difundidos/validados pelo servidor (`signaling.rs`) — o cliente NÃO decide sozinho. O painel de transcrição é host-only (não abre para todos ao ligar). **Partilha de ecrã de não-anfitrião exige `share-grant` do anfitrião** (grants por sala no hub; `ScreenShare` sem grant → Error — não confiar no cliente); fluxo: `share-request` → cartão Permitir/Negar no anfitrião → grant → partilha arranca no requerente. Controlo remoto: `remote-control request` só é entregue a quem está a apresentar (`presenter` por sala no hub).
- **Glare são DUAS metades:** adiar a oferta no servidor (`NegoMsg`) NÃO chega — o `rollback` do cliente descarta a oferta dele, por isso o cliente tem de **RE-OFERTAR** depois de responder. Guardado por `sfu_e2e.rs` + `glare.test.ts`. Ver R13.
- **Negociação SFU = canal único por peer (`NegoMsg`):** ofertas do cliente (ecrã, câmara), respostas e renegociações do servidor passam TODAS pela `negotiation_loop`. O webrtc-rs **não tem rollback** (nem implícito nem explícito a partir de `have-local-offer`), por isso uma oferta do cliente que chegue com a nossa pendente é **adiada**, nunca aplicada fora de estado — era aí que a partilha de ecrã se perdia em silêncio. Ver R13.
- **Sem PLI periódico:** keyframes só a pedido (subscrição nova, troca de camada, PLI/FIR reencaminhado do subscritor, rate-limit 1 s). O antigo ticker de 3 s por camada queimava bitrate para sempre. Ver R14.
- **Camada simulcast é reavaliada:** `reevaluate_peer` decide a partir do tamanho da sala **e** da perda reportada por RTCP (`Quality`). Chamada em cada entrada/saída e em cada mudança de nível de perda. Ver R15.
- **`touch_subs()` a seguir a QUALQUER alteração de subscritores:** a bomba de RTP usa um snapshot invalidado por `subs_version` (escritas fora do lock). Esquecer isto = media a ir para quem saiu. Ver R16.
- **Áudio remoto vive no `AudioSink`, nunca dentro de um tile:** esconder um tile não pode silenciar ninguém. Ver R19.
- **Publicar media exige fallback de negociação:** `replaceAudioTrack` reaproveita o transceiver `recvonly` e renegoceia — sem isso quem entra sem mic fica mudo para sempre. Ver R20.
- **Gravação só grava VP8/Opus** (`recordable_codec`); outro codec → track excluída + `error!`. Gravar VP9/H264 com o depacketizer VP8 produzia ficheiro corrompido sem erro. Ver R18.
- **Estado alimentado por timer tem de comparar antes de `setState`** (`sameSet` no `LevelWatcher`): sem isso a sala re-renderizava 5,5×/s em silêncio. Ver R21.
- **Seleção de oradores (top-N):** o SFU só reencaminha os 3 microfones mais ativos. Três armadilhas que silenciam gente: renumerar SEMPRE o áudio (`AudioMeter::next_seq` — sem isso a supressão parece perda e baixa o vídeo), decair a energia por TEMPO e não por pacote (com DTX quem se cala não envia nada e ficaria preso no top-N), e NUNCA suprimir microfones sem a extensão RFC 6464 negociada. Gravação, PSTN e áudio de ecrã recebem sempre tudo. Ver R22.
- **`video-interest` é enviado SEMPRE que o conjunto muda** — a página visível quando pagina, TODOS os peers quando não pagina. "Deixar de enviar" não significa "todos": o servidor ficaria com a última página. Ver R23.
- **Desligar a câmara liberta-a mesmo** (`disableVideo` → `replaceTrack(null)` + `track.stop()`), reutilizando o `videoSender` guardado — criar transceiver novo por religação faz crescer a SDP e perde o simulcast. Ver R24.
- **Harness:** manter `HARNESS.md`, `AGENTS.md`, `GEMINI.md` coerentes; a referência estável está em `docs/reference/architecture.md` (+ `docs/reference/regressions.md` = regressões a não reintroduzir); revisores autónomos em `agents/`.
