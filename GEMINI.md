# Delonix Meet — AI Development Harness (Gemini)

> **Read this entire file before any task.** It provides complete platform context to avoid common mistakes.

---

## Project identity

**Delonix Meet** is a **self-hosted / SaaS corporate video conferencing platform** — a real alternative to Google Meet, Zoom, and Microsoft Teams for organizations that require **data sovereignty, local compliance (BNA/LGPD), and air-gap deployability**.

- Backend: 100% Rust (axum, webrtc-rs, tokio, sqlx, PostgreSQL)
- Frontend: React + TypeScript + Vite
- Mobile: Flutter (in progress)
- Infrastructure: Docker Compose (dev), nginx + systemd (prod)
- No external cloud dependencies — fully self-contained

**This is production software, not a demo.** It runs real video calls with E2EE, multi-tenant isolation, server-side recording, AI-generated meeting minutes, and enterprise webhooks.

---

## Tech stack (exact versions that matter)

| Component | Version | Why it matters |
|---|---|---|
| Rust | 1.80+ | Edition 2021, async closures, `impl Trait` in fn params |
| axum | 0.7 | `Router::new()`, `Extension` extractors, `MethodRouter` |
| sqlx | 0.7 | `query!` macros with compile-time checking (requires `DATABASE_URL` at build) |
| reqwest | **0.12** (rustls-tls) | **Do NOT upgrade to 0.13** — rustls version conflict |
| webrtc-rs | Latest compatible | SFU: DTLs, SRTP, RTP fan-out, simulcast |
| React | 18 | Concurrent features, `useTransition`, `useDeferredValue` |
| Vite | 5 | HMR, proxy config (must include `/ws` and `/rtc` with `ws: true`) |
| i18next | 23 | PT/EN, persisted in `dx_lang` localStorage |

---

## What currently works (July 2026)

Complete feature inventory — do not implement these again, they exist:

✅ **Auth:** Org-first registration (creates org+admin), JWT access tokens (15min), refresh tokens (HttpOnly cookie `dlx_refresh`), token rotation, logout revocation  
✅ **Multi-tenant:** Organizations, branches, employee groups, cross-org isolation enforced on every endpoint  
✅ **SFU:** Rust WebRTC SFU (webrtc-rs), simulcast (q/h/f layers), screen share as separate track, server-side E2EE decrypt for recording  
✅ **Room features:** Google Meet-style grid, stage/audience view, split-pill mic/camera controls, whiteboard (persistent), breakout rooms (full), host controls (lock/share-only/kick), closed captions, reactions, raise hand, recording  
✅ **In-room tools:** Meeting timer, anonymous polls, Q&A with upvotes  
✅ **Background effects:** Blur (light/strong), virtual backgrounds, RVM ONNX matting  
✅ **Transcription:** Web Speech API (Chrome/Edge) + Whisper-tiny WASM fallback (all browsers), multi-language  
✅ **AI MoM:** Generated at meeting end, stored in `meetings.transcript`/`meetings.minutes`, viewable in Recordings  
✅ **Calendar:** Month/week/agenda views, real-time conflict detection, quarantine system, .ics export  
✅ **Recordings:** Library, viewer (player + transcript + MoM + tasks), card/table toggle, read-only share  
✅ **Admin analytics:** 30-day KPIs, weekly series, top organizers, kind split, SSO/SCIM stubs  
✅ **Webhooks:** Slack/Teams/Mattermost/generic + HMAC signing, SSRF guard, events: meeting.created/started/recording.ready  
✅ **API keys:** Per-org, hashed, with scopes  
✅ **PWA:** manifest + service worker  
✅ **Themes:** Delonix (dark default), NgolaCloud (light warm), NgolaCloud-dark, Kaeso (flat corporate)  
✅ **i18n:** PT/EN on Landing, Shell, Login, Home, Analytics, Roadmap  
✅ **Status page:** `/api/status` (public) + `#/status` route  
✅ **E2EE:** Insertable Streams AES-256-GCM, key delegation for server recording, security code badge  

⬜ **Not yet done:** SSO OIDC real (stubs exist), Flutter mobile, PSTN dial-in, i18n on Room/Calendar/Recordings/Directory, MLS key agreement, SCIM, remote desktop control, DLP hooks

---

## Architecture — non-obvious decisions

### Why a custom Rust SFU (not LiveKit/mediasoup)
Decision made and locked: evolve the Rust SFU in `server/src/sfu.rs`. Reasons: single binary deploy, full control of RTP pipeline for E2EE + recording, no external dependency. Do not suggest migrating to LiveKit.

### E2EE + server recording (not a contradiction)
The host explicitly delegates the AES-256 key (base64) in the `server-record` message. The server decrypts frames only to record; the key lives only in `RecordingSession` memory — never on disk or in the database. The user sees an explicit `confirm()` dialog.

### Grid layout must use inline styles, not CSS vars
`useGridLayout` in Room.tsx computes tile dimensions with `ResizeObserver` and sets them as inline styles. **Do NOT replace with CSS custom properties** — transitions freeze in background windows (browser throttles rAF/setInterval for background tabs; inline styles bypass the issue).

### reqwest 0.12, not 0.13
The workspace pins `reqwest = { version = "0.12", features = ["rustls-tls"] }`. Upgrading to 0.13 breaks the rustls version compatibility. Do not upgrade.

---

## Security invariants — never break these

1. **Fail-closed:** Server panics on startup without strong `JWT_SECRET`/`TURN_SECRET`/`DATABASE_URL`. `DELONIX_ALLOW_INSECURE=1` only in dev.
2. **Cross-org isolation:** `rooms::can_access_room` and `org::org_co_members`/`admin_orgs_of_user` scope ALL data to the user's org(s). Never return cross-org data.
3. **Room tokens:** Short-lived JWT (5 min), scope = 1 room. Rejected WS without valid token.
4. **SSRF:** Webhook hosts validated (block private/loopback/link-local/metadata) on create AND delivery. No redirects.
5. **Rate limiting:** Login lockout 8 attempts/5min; `/api/v1` rate-limited by IP; WS rate-limited per socket.
6. **Cookie security:** `dlx_refresh` always `Secure` except with `COOKIE_INSECURE=1`.
7. **Server-side authorization:** Host controls (lock/kick/share-only) validated in `signaling.rs`, never trusted from client.

---

## Design system

CSS custom properties in `web/src/styles/`. Never hardcode colors.

| Token | Value | Use |
|---|---|---|
| `--accent` | `#C8201D` | Delonix red — primary CTAs |
| `--accent-hi` | `#F26430` | Hover / gradient |
| `--accent-2` | `#EDA33B` | Gold — text accents, "Meet" wordmark |
| `--bg` | `#07090D` | Main dark background |
| `--surface` | `#0B0E13` | Cards/modals |
| `--room-bg` | `#202124` | Meet-gray — room is always dark |
| `--ctrl-bg` | `#3c4043` | Room control buttons |

**Room always dark:** `.room-page` reaffirms dark tokens with `!important` at end of `styles.css` — room ignores light themes.

**Unified control system (2026-07-14):** full reference in `docs/reference/design-system.md`. Tokens `--radius-sm/md/lg` = 4/6/8px, `--ctl-h` = 30px; uniformization layer at the END of `styles.scss` (3 tiers). New controls MUST use the kit `web/src/components/ui.tsx` (`Btn`/`IconBtn`/`Card`/`Field`/`SelectCtl`/`Switch`) — no ad-hoc buttons, no hardcoded radius/height. Themes = token maps in `styles/tokens.scss` under `[data-theme=…]`, never scattered overrides.

**Fonts:** Space Grotesk (headings), Instrument Sans (body), IBM Plex Mono (mono) — self-hosted via @fontsource.

---

## What makes Delonix unique vs Zoom/Teams/Meet

| Feature | Zoom | Teams | Meet | Delonix |
|---|---|---|---|---|
| Self-hosted | ❌ | ❌ | ❌ | ✅ |
| Data sovereignty | ❌ | ❌ | ❌ | ✅ |
| Rust backend (no GC) | ❌ | ❌ | ❌ | ✅ |
| E2EE always on | ❌ (paid) | ⚠️ 1:1 only | ⚠️ TLS only | ✅ |
| E2EE + server recording | ❌ | ❌ | ❌ | ✅ (key delegation) |
| AI MoM local (Ollama) | ❌ | ❌ | ❌ | ✅ |
| API keys + webhooks (core) | add-on | add-on | ❌ | ✅ |
| Org hierarchy (branches) | ❌ | ❌ | ❌ | ✅ |
| License: no royalty self-host | ❌ | ❌ | ❌ | ✅ |

See `docs/competitive-positioning.md` for full analysis.

---

## Development workflow

```bash
make dev          # Start everything: infra + backend + frontend
# Ports: backend 8180, frontend 5173, Postgres 5435

# After new migration: always rebuild release binary
cargo build --release

# Test accounts: use @teste.local emails
# Clean up: DELETE FROM users WHERE email ~ '@teste\.local$'
```

**HTTPS required** for camera/mic outside localhost. Use `deploy/nginx-delonix.conf` with self-signed cert for LAN testing.

---

## Code conventions

### Rust
- All handler errors via `AppError` — no `unwrap()` in production code
- `sqlx::query!` macros with compile-time verification
- Handlers return `Result<impl IntoResponse, AppError>`
- New modules: declare in `main.rs` (`mod new_module;`) + register routes in router
- Migrations: `server/migrations/NNNN_name.sql` with sequential prefix

### TypeScript
- Functional components + hooks
- WS messages: typed discriminant union
- i18n: always use `t('namespace.key')`, never hardcoded strings
- **No `var()` for tile dimensions** — inline styles only in grid layout

---

## Known gotchas

- **Glare has TWO halves:** deferring the client offer server-side is NOT enough — the client's `rollback` discards its own offer, so the client must **re-offer** after answering. Guarded by `sfu_e2e.rs` + `glare.test.ts` (R13).
- **SFU negotiation goes through ONE per-peer channel** (`NegoMsg` → `negotiation_loop`): client offers (screen share, camera on), client answers and server renegotiations are all serialized. webrtc-rs has no rollback — a client offer arriving while ours is pending is **deferred**, never applied out of state (R13).
- **No periodic PLI** — keyframes on demand only: new subscription, layer switch, or subscriber PLI/FIR forwarded to the publisher (1 s rate limit) (R14).
- **Simulcast layer is re-evaluated** from room size AND the subscriber's RTCP-reported loss, on every join/leave and loss-level change (R15).
- **Call `touch_subs()` after ANY subscriber change** — the RTP pump uses a snapshot invalidated by `subs_version` (R16).
- **Remote audio lives in `AudioSink`, never inside a tile** — hiding a tile must never mute anyone (R19).
- **Active-speaker selection (top-N):** the SFU forwards only the 3 loudest mics. Three traps that silence people: always renumber forwarded audio (suppression would otherwise look like packet loss and downgrade their video), decay energy on a TIMER not per packet (with DTX a silent speaker sends nothing and would stay pinned in the top-N), and never suppress mics whose RFC 6464 level extension was not negotiated. Recording, PSTN and screen audio always get everything (R22).
- **`video-interest` is sent on every change** — the visible page while paginating, ALL peers when not. Going silent does not mean "all" (R23).
- **Publishing media needs a negotiation fallback** — `replaceAudioTrack` reuses the `recvonly` transceiver and renegotiates (R20).

- Vite proxy: `/ws` and `/rtc` need `ws: true`. Changes require Vite restart.
- Web Speech API: Chrome/Edge only. Firefox falls back to Whisper WASM.
- `getUserMedia`: requires secure context (HTTPS or localhost).
- `e2eeKeyRef` captured BEFORE `setKey()` — buffer is transferred to worker.
- IVF PTS: library uses frame counter (wrong). `recorder.rs` uses real RTP ms timestamps — do not revert.
- Existing orgs with `email_domain=''` work fine — partial unique index ignores empty strings.
- Physical meeting rooms (`meeting_rooms` in org.rs) ≠ virtual rooms (`rooms` in rooms.rs).

---

## Specialized reviewers (see docs/ai-reviewers.md)

| Persona | Invoke for |
|---|---|
| **Graydon Hoare** (Rust creator) | Rust safety, lifetimes, async patterns, zero-cost abstractions |
| **Brendan Burns** (Kubernetes co-creator) | Deploy, HA, scaling, K8s operator design |
| **Justin Uberti** (WebRTC co-creator, ex-Google Meet) | SFU design, ICE, simulcast, codec negotiation, E2EE |
| **Adam Langley** (Google BoringSSL) | TLS, crypto, auth, SSRF, CSP, key management |
| **Lars Bak** (V8 creator) | WASM performance, Web Workers, JS engine bottlenecks |
| **MS Teams Compliance Architect** (persona) | eDiscovery, DLP, SCIM, audit logs, retention |
| **Zoom Platform Architect** (persona) | Call reliability, bitrate adaptation, failover, reconnect |

---

## v2 — Critical updates (keep in sync with `AGENTS.md` / `docs/reference/architecture.md`)

- **Per-room affinity (K8s multi-replica):** the SFU is in-memory per pod; Redis fans signaling/presence, NOT RTP. All peers of a room MUST hit the same pod → client sends `/ws?...&room=CODE`, ingress uses `upstream-hash-by: $arg_room`. Without it: one-way media, admission and screen-share fail. `/rtc` (presence) is Redis-fanned, no affinity needed.
- **WS rate limit is a token bucket** (600 burst / 300 sustained) — a low fixed window used to disconnect the host during the ICE/renegotiation burst.
- **Transcription is host-gated + distributed:** only the host toggles it (`TranscriptionToggle` → broadcast `Transcription`); EVERY client transcribes its own mic and broadcasts `transcript`. Engine: Web Speech (Chrome — but sends audio to Google) with automatic fallback to **local Whisper WASM** on `network` error. Prefer local for sovereignty. The transcription panel is host-only (does not open for everyone when enabled).
- **`/ws` needs a DEDICATED Service** (`delonix-server-ws`) — sharing it with `/api`/`/rtc` makes ingress-nginx merge the backends and DROP `upstream-hash-by` → affinity lost → one-way media.
- **SFU initial offer is sent in the `SfuCall` CONSTRUCTOR**, not gated by an internal `signal.on('joined')` (the call is created *inside* the `joined` handler, so a constructor listener would miss the already-fired event → dead media). And a **waiting guest must NOT build the `SfuCall`** (stale offer → glare loop → flood → reload after admit).
- **K8s media is relay-only** (`FORCE_TURN_RELAY=1` → `iceTransportPolicy:relay` on `/api/ice` and SFU `RTCConfiguration`) with a reachable coturn (stage: on the HOST via `deploy/run-host-coturn.sh`). Without it ICE connects but the tile stays black. Do NOT enable on local (systemd, same host). Open issue: unstable TURN allocation (`438 Stale nonce`).
- **`.dockerignore` must NOT exclude `web/dist`** (`Dockerfile.web.stage` copies it); `vite.config.ts` reads dev certs only in `serve`.
- **Server is authoritative** for shared room actions: `wb-close`, `Presenting`/clear-presentation on stop-share are broadcast/validated in `signaling.rs`; the client does not decide alone.
- **Reference:** stable knowledge base in `docs/reference/architecture.md`; **regressions never to reintroduce in `docs/reference/regressions.md` (R1–R24)**; autonomous reviewer subagents in `agents/`.
