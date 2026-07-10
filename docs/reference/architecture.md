# Delonix Meet — Referência de Arquitetura

> **Base de conhecimento tool-agnóstica.** Referência estável do sistema para humanos e agentes (Gemini/Codex/Copilot). O contexto operacional vivo está em [`HARNESS.md`](../../HARNESS.md) / [`AGENTS.md`](../../AGENTS.md); este documento é o mapa de referência para o **crescimento** da plataforma. Manter atualizado quando a arquitetura mudar (não a cada commit).

---

## 1. Visão de sistema

```
                         ┌───────────────── Browser (React/TS) ─────────────────┐
                         │  webrtc.ts (SfuCall)   signaling.ts (/ws)  presence.ts (/rtc)  │
                         │  e2ee.ts (worker)   media.ts (RVM/Whisper)  Room.tsx           │
                         └───────┬───────────────────┬────────────────────┬───────────────┘
                          HTTPS  │            WSS /ws │           WSS /rtc  │
                         ┌───────▼───────────────────▼────────────────────▼───────────────┐
                         │                    nginx / ingress-nginx (TLS)                   │
                         │        /api → server   /ws → server (afinidade por sala)         │
                         │        /rtc → server   / → web (SPA estática)                    │
                         └───────┬──────────────────────────────────────────┬──────────────┘
                                 │                                          │
                       ┌─────────▼─────────┐                      ┌─────────▼─────────┐
                       │  delonix-server    │  Redis pub/sub       │   delonix-web     │
                       │  (Rust / axum)     │◄────(sinalização,    │  (nginx estático) │
                       │  ┌──────────────┐  │      presença)       └───────────────────┘
                       │  │ SFU (in-mem) │  │
                       │  │ signaling hub│  │      ┌──────────┐   ┌──────────┐
                       │  │ presence hub │  │──────│ Postgres │   │  coturn  │ STUN/TURN
                       │  └──────────────┘  │      └──────────┘   └──────────┘
                       └────────────────────┘
```

**Regra de ouro do scaling:** o SFU e os hubs de sinalização/presença são **estado em memória por processo**. O Redis propaga *eventos* (chat, presença, sala de espera, whiteboard, tools) entre pods — **nunca RTP**. Logo:
- **`/ws` (media/SFU)** precisa de **afinidade por sala** — todos os pares de uma sala no mesmo pod (`upstream-hash-by: $arg_room`; cliente envia `?room=CODE`).
- **`/rtc` (presença)** e os eventos de sinalização não-media são fanned por Redis → não precisam de afinidade.

## 2. Módulos do backend (`server/src/`)

| Módulo | Responsabilidade |
|---|---|
| `main.rs` | Bootstrap, router, `AppState`, cron (retention/quarentena), embed de migrações |
| `config.rs` | Env vars, **fail-closed** sem segredos fortes |
| `auth.rs` | Registo (org+admin), login, refresh rotativo, logout, room tokens, `Claims` |
| `org.rs` | Multi-tenant: orgs, filiais, membros, grupos, salas presenciais, quotas, stats |
| `rooms.rs` | CRUD salas, `room_access` (direto vs espera), `can_access_room`, `room_admitters` |
| `sfu.rs` | SFU: Hub, Room, Publication, simulcast, PLI, gravação RTP→IVF/OGG |
| `signaling.rs` | WS `/ws`: join/offer/answer/ice/chat/breakout/host/tools/wb/media/transcription |
| `presence.rs` | WS `/rtc`: chamadas WhatsApp-style (start/accept/decline), ring de reunião |
| `meetings.rs` | Calendário, conflitos, quarentena, MoM, transcrição, webhooks de meeting |
| `recordings.rs` | Biblioteca, partilha read-only, RBAC download, sweep de retenção |
| `recorder.rs` | Gravação server-side: RTP→IVF(VP8)+OGG(Opus)→ffmpeg webm, E2EE decrypt |
| `webhooks.rs` | CRUD webhooks, fire() (Slack/Teams/Mattermost/generic+HMAC), SSRF guard |
| `whiteboards.rs` · `voice.rs` · `apikeys.rs` · `rate_limit.rs` · `error.rs` · `dlp.rs` · `pubsub.rs`/`redis_state.rs` | quadro branco · PSTN (stub) · API keys · rate limit · `AppError` · censura DLP · multi-nó Redis |

## 3. Modelo de dados (Postgres)

Entidades-núcleo e relações (ver `server/migrations/` para o esquema exato):

- **users** (email único por org via `email_domain`) → **org_members** → **organizations** → **branches** → **employee_groups**.
- **rooms** (`code`, `owner_id`, `topology` p2p/sfu, `waiting_room`, `e2ee`, `format` normal/training) → **room_participants**, **room_admitters** (co-anfitriões de admissões).
- **meetings** (`room_code`, agenda) → **meeting_invitees** (attendees da agenda → entrada direta).
- **recordings** (+ share links read-only, retenção) · **whiteboards** · **webhooks** · **api_keys** · **org_quotas** · **voice_*** (PSTN).

**Dois conceitos de "sala" a não confundir:** `meeting_rooms` (presenciais, em `org.rs`) ≠ `rooms` (virtuais, em `rooms.rs`).

## 4. Fluxos-chave

**Entrada numa sala:** `join_room` calcula `room_access` → `direct` (dono OU attendee da agenda OU co-anfitrião persistido) entra logo; os restantes vão para **sala de espera** e são admitidos pelo anfitrião ou por um co-anfitrião promovido (`room_admitters`, persiste entre reconexões). Emite um **room token** curto → WS `/ws`.

**Media (SFU):** cliente publica tracks (simulcast q/h/f + ecrã como track separada) → servidor faz fan-out (`subscribe_to_existing`, `consider_subscribe`) → renegociação server-driven (`SfuOffer`/`answer_slot`). E2EE cifra frames antes de saírem do cliente; o servidor só vê cifrado (exceto gravação por key delegation).

**Transcrição (distribuída, host-gated):** anfitrião liga → servidor difunde `Transcription` → **cada** cliente transcreve o próprio microfone (Web Speech, senão Whisper WASM local) e difunde `transcript` → agregação central legendada por orador → MoM guardado (auto ao encerrar).

**Presença/chamadas:** WS `/rtc` (access token) → ring estilo WhatsApp/Teams, propagado por Redis entre pods.

## 5. Frontend (`web/src/`)

`App.tsx` (router por hash, gate de auth, `PresenceProvider`) · `api.ts` (REST) · `signaling.ts`/`webrtc.ts`/`presence.ts` (tempo real) · `e2ee.ts`+worker · `media.ts` (BackgroundEffect RVM ONNX, Transcriber, Denoiser RNNoise, MeetingRecorder) · `matte.ts`/`whisperWorker.ts` · `i18n.ts` (PT/EN) · `pages/` (Room, Landing, Calendar, Recordings, Analytics, Directory) · `styles/` (tokens CSS → temas via `[data-theme]`; a sala é sempre dark, palette Meet).

## 6. Modelo de deploy

| Alvo | Como | Notas |
|---|---|---|
| **Dev** | `make dev` | infra compose + backend + Vite (8180/5173) |
| **Single-node** | systemd `delonix-server` + nginx (`deploy/nginx-delonix.conf`) + `publish-web.sh` | mkcert TLS; SFU num processo → sem problema de afinidade |
| **Kubernetes** | `deploy/k8s/` (kustomize) + `deploy/build-images.sh` | ingress-nginx + cert-manager; **exige afinidade por sala no `/ws`**; imagens rootless; HPA opt-in |

## 7. Pontos de extensão (para crescer)

- **SSO OIDC genérico** — config-driven, sem hardcode de provider (stub em `org.rs`).
- **SCIM** — provisioning/desativação a partir do IdP corporativo.
- **PSTN dial-in** — FreeSWITCH + Kamailio (docs em `docs/pstn-*`), ponte SFU↔SIP.
- **Whisper server-side** — diarização por track (worker GPU em `ai-worker/`), qualidade > WASM.
- **MLS** — substituir PBKDF2 por key agreement de grupo (Signal/MLS) no E2EE.
- **SDK público** — REST + WS tipado para bots/integrações headless.
- **Multi-região** — ver [`docs/multi-region-scaling.md`](../multi-region-scaling.md); data residency por tenant.

## 8. Invariantes que qualquer mudança tem de respeitar

Ver a lista completa em [`AGENTS.md` §3](../../AGENTS.md) e [`HARNESS.md` §6](../../HARNESS.md). Resumo: segredos fail-closed · isolamento multi-tenant em TODOS os endpoints · room tokens curtos · SSRF guard nos webhooks · rate limit (token bucket no WS) · cookie refresh Secure · E2EE real com key delegation explícita · autorização de host controls no servidor · afinidade por sala em multi-réplica.
