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
| axum 0.7 | HTTP server + router |
| webrtc-rs | SFU: DTLs/SRTP, RTP fan-out, simulcast |
| sqlx 0.7 | PostgreSQL async (migrações automáticas em main.rs) |
| tokio | Runtime async |
| argon2 | Hashing de passwords |
| jsonwebtoken | JWT access (15 min) + refresh (30 dias, rotativo) |
| reqwest 0.12 (rustls-tls) | Webhooks outbound (NÃO 0.13 — incompatível com rustls) |
| tower-http | CORS, compressão, cabeçalhos de segurança |

**Ficheiros principais:**
- `main.rs` — bootstrap, router, estado global (`AppState`), cron jobs (retention sweep)
- `config.rs` — lê env vars, **fail-closed sem segredos fortes** (panic no arranque)
- `auth.rs` — registo (cria org+admin), login, refresh, logout, room tokens
- `org.rs` — multi-tenant: organizations, branches, org_members, employee groups, salas presenciais, quotas, stats, SSO stubs
- `rooms.rs` — CRUD salas, `can_access_room` (isolamento cross-org), `insert_room` (helper reutilizado)
- `sfu.rs` — SFU Rust: Hub, Room, Publication, simulcast, PLI, gravação RTP→IVF/OGG
- `signaling.rs` — WebSocket `/ws` (room token), mensagens tipadas: join/offer/answer/ice/leave/chat/breakout-*/host-*/tools-*/wb-*/media
- `presence.rs` — WebSocket `/rtc` (access token), chamadas WhatsApp-style: call-start/accept/decline/cancel, ring de reunião agendada
- `meetings.rs` — calendário, conflitos, quarentena, MoM, transcrição, webhooks de meeting
- `recordings.rs` — biblioteca de gravações, partilha read-only, sweep de retenção
- `recorder.rs` — gravação server-side: RTP→IVF(VP8)+OGG(Opus), ffmpeg post-stop (VP9+Opus webm), E2EE via decrypt_e2ee()
- `webhooks.rs` — CRUD webhooks org, fire() best-effort (Slack/Teams/Mattermost/generic+HMAC), SSRF guard
- `whiteboards.rs` — CRUD quadro branco persistente
- `voice.rs` — PSTN (stub, aguarda operador)
- `apikeys.rs` — API keys por org (hash + scopes)
- `rate_limit.rs` — rate limit por IP/conta (DashMap, lockout login 8/5min)
- `error.rs` — `AppError` unificado → HTTP status + JSON body

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
| PostgreSQL | 5435 | Dados principais (migrações 0001–0015) |
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

| Token | Valor | Uso |
|---|---|---|
| `--accent` | `#C8201D` | Vermelho Delonix — CTAs primários |
| `--accent-hi` | `#F26430` | Hover/gradient start |
| `--accent-2` | `#EDA33B` | Dourado — acentos de texto, "Meet" no wordmark |
| `--bg` | `#07090D` | Fundo principal dark |
| `--surface` | `#0B0E13` | Cards/modais |
| `--surface-2` | `#12151C` | Hover/nested surfaces |
| `--text` | `#F4F6FA` | Texto primário |
| `--text-2` | `#9BA3B2` | Texto secundário |
| Sala: `--room-bg` | `#202124` | Cinza Meet — sala ignora temas claros |
| Sala: `--ctrl-bg` | `#3c4043` | Botões de controlo na sala |

**Regra da sala:** `.room-page` e `.waiting-page` reafirmam tokens dark **com `!important`** no fim de `styles.css`. A sala é sempre escura independentemente do tema da app.

**Fontes:** Space Grotesk (títulos), Instrument Sans (corpo), IBM Plex Mono (mono) — self-hosted via @fontsource.

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

**Bases de dados de teste:** usar emails `@teste.local`. Limpar no fim: `DELETE FROM users WHERE email ~ '@teste\.local$'` e apagar `.webm` órfãos em `recordings/`.

---

## 8. Padrões de código

### Rust
- `AppError` para todos os erros de handler — nunca `unwrap()` em código de produção
- Pool Postgres via `Extension<PgPool>` injetado pelo axum
- `sqlx::query!` / `sqlx::query_as!` — macros com verificação em compile time
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

Ver `docs/ai-reviewers.md` para perfis detalhados. Invocar como persona num prompt quando precisares de revisão especializada:

| Persona | Especialidade | Invocar para |
|---|---|---|
| **Graydon Hoare** | Criador do Rust | Safety, lifetimes, async Tokio, zero-cost abstractions |
| **Brendan Burns** | Co-criador do Kubernetes | Deploy, HA, resource limits, horizontal scaling, operador K8s |
| **Justin Uberti** | Co-criador do WebRTC (ex-Google Meet/Hangouts) | SFU design, ICE, simulcast, congestion control, codec negotiation |
| **Lars Bak** | Motor V8 (Google) → performance JS/WASM | WASM Whisper, Web Workers, JS engine bottlenecks |
| **Lea Verou** | CSS/Web Standards (ex-W3C) | Design tokens, CSS architecture, acessibilidade |
| **Adam Langley** | TLS/crypto (Google BoringSSL) | E2EE, TLS config, DTLS, key management |
| **Phil Karlton / Mike Belshe** | HTTP/2, SPDY (Google/Microsoft) | Protocolo WebSocket, multiplexing, cabeçalhos |
| **Zoom Platform Architect** (persona composta) | Reliability, escala de milhões de calls | Resiliência, fallback, TURN reliability, bitrate adaptation |
| **MS Teams Compliance Architect** (persona composta) | eDiscovery, DLP, auditoria | Retenção de dados, audit logs, SCIM, DLP hooks |

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
- **Afinidade por sala (K8s multi-réplica):** o SFU é in-memory por pod; o Redis propaga sinalização/presença, NÃO RTP. Todos os pares de uma sala têm de cair no MESMO pod → cliente envia `/ws?...&room=CODE` e o ingress faz `upstream-hash-by: $arg_room`. **CRÍTICO:** o `/ws` tem de usar um **Service DEDICADO** (`delonix-server-ws`, mesmos pods) — se `/ws` e `/api`/`/rtc` partilharem o mesmo Service, o ingress-nginx funde-os num backend e DESCARTA o `upstream-hash-by` (round-robin ganha) → afinidade não se aplica. Verificar: `curl .../ws?room=X` repetido → sempre o mesmo pod. Sem isto: media num só sentido, admissão e partilha de ecrã falham. `/rtc` (presença) é fanned por Redis, não precisa de afinidade.
- **Rate limit WS = token bucket** (`signaling.rs`): 600 burst / 300 sustained. Uma janela fixa baixa cortava o próprio anfitrião durante a rajada de ICE/renegociação — não voltar a uma janela fixa apertada.
- **Transcrição host-gated e distribuída:** só o anfitrião liga (`TranscriptionToggle`); o servidor difunde `Transcription`; CADA cliente transcreve o próprio microfone e difunde `transcript`. Motor: Web Speech (Chrome, mas envia áudio à Google) com fallback automático para **Whisper WASM local** em erro `network` — preferir local para soberania.
- **`vite.config.ts` só lê certos de dev no `serve`** (nunca no `build`) — senão a imagem Docker web falha no `npm run build`.
- **Harness:** manter `HARNESS.md`, `AGENTS.md`, `GEMINI.md` coerentes; a referência estável está em `docs/reference/architecture.md`; revisores autónomos em `agents/`.
