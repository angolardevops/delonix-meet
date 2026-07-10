# GitHub Copilot Instructions — Delonix Meet

## Project overview
Delonix Meet is a self-hosted / SaaS corporate video conferencing platform. Backend is 100% Rust (axum + webrtc-rs + sqlx + PostgreSQL). Frontend is React + TypeScript + Vite. This is production software, not a tutorial project.

## Stack specifics

### Backend (server/src/)
- axum 0.7: `Router::new()`, handler pattern `async fn handler(Extension(pool): Extension<PgPool>, ...) -> Result<impl IntoResponse, AppError>`
- sqlx 0.7: `sqlx::query!` and `sqlx::query_as!` macros — always use macros, not string queries
- All errors via `AppError` in `error.rs` — never `unwrap()` or `expect()` in handler code
- New endpoints: declare module in `main.rs`, add routes in the axum router in `main.rs`
- Migrations: `server/migrations/NNNN_description.sql` — sequential prefix, run with `cargo sqlx migrate run`
- reqwest: version **0.12** with `rustls-tls` feature — DO NOT suggest upgrading to 0.13

### Frontend (web/src/)
- React 18 with functional components and hooks — no class components
- i18n: always `const { t } = useTranslation(); t('namespace.key')` — never hardcoded strings
- Design tokens: always use CSS custom properties (`var(--accent)`, `var(--bg)`, etc.) — never hardcode hex colors
- **EXCEPTION:** tile dimensions in the room grid MUST be inline styles, never CSS vars (causes frozen transitions in background tabs)
- WS messages: typed discriminant union — add the type to both Rust and TypeScript when adding new messages

### CSS/Design
- Tokens defined in `web/src/styles/` as `:root` custom properties
- Themes via `[data-theme="ngolacloud"]` etc. at end of `styles.css`
- Room is always dark: `.room-page` overrides tokens with `!important` — do not remove these
- Fonts: Space Grotesk, Instrument Sans, IBM Plex Mono — all self-hosted via @fontsource

## Security rules (never violate)
1. No `unwrap()` on user-controlled data in Rust handlers
2. Cross-org isolation: every query scoped to user's org via `can_access_room` / `org_co_members`
3. Room tokens: short-lived JWT (5min), one room scope — always verify before WS upgrade
4. SSRF: webhook URLs must be validated (no private IPs, no loopback, no redirects)
5. Rate limiting: all auth endpoints rate-limited; do not add unprotected public endpoints
6. Cookie: `dlx_refresh` is `HttpOnly; SameSite=Strict; Secure` — never send refresh token in JSON body

## What already exists — do not re-implement
- Auth (Argon2id + JWT + HttpOnly cookie refresh)
- Multi-tenant isolation (organizations, branches, groups)
- SFU WebRTC in Rust (sfu.rs) — do not suggest LiveKit
- E2EE via Insertable Streams (e2ee.ts)
- Server-side recording with ffmpeg post-processing (recorder.rs)
- Whisper WASM transcription (whisperWorker.ts, models in web/public/)
- Background effects blur + RVM ONNX matting (media.ts, matte.ts)
- Breakout rooms, host controls, whiteboard, polls, Q&A, timer (signaling.rs + Room.tsx)
- Webhooks with HMAC (webhooks.rs)
- API keys (apikeys.rs)
- Calendar with conflict detection and quarantine (meetings.rs + Calendar.tsx)
- PWA (sw.js + manifest.webmanifest)

## Database schema (key tables)
`users`, `organizations` (email_domain), `branches`, `org_members` (role: admin|member), `employee_groups`, `rooms` (code, topology: sfu|mesh), `refresh_tokens`, `meetings` (transcript, minutes columns), `recordings`, `recording_shares`, `room_participants`, `org_webhooks`, `api_keys`, `whiteboards`, `meet_quarantine`, `org_quotas`

Migrations 0001–0015 are applied. New migrations use prefix 0016+.

## Development
- Ports: backend 8180, frontend 5173, Postgres 5435
- `make dev` starts everything
- After new migration: `cargo build --release` required before systemd restart
- Test emails: use `@teste.local`, clean up after (`DELETE FROM users WHERE email ~ '@teste\.local$'`)
- Vite proxy: `/ws` and `/rtc` need `ws: true`, require Vite restart after changes

## Competitive context
Delonix is positioned as the self-hosted alternative for organizations with data sovereignty requirements (banks, government, healthcare). Key differentiators: Rust backend (no GC pauses), real E2EE always on, data stays on-premise, AI MoM with local Ollama option, no per-seat royalty.

## v2 — Critical invariants
- **Per-room affinity (K8s):** SFU is in-memory per pod; Redis fans signaling/presence, not RTP. Client sends `/ws?...&room=CODE`; ingress `upstream-hash-by: $arg_room`. Missing affinity = one-way media, broken admit/screen-share.
- **WS rate limit** is a token bucket (600 burst / 300 sustained) — never a tight fixed window (it disconnects the host mid-ICE).
- **Transcription** is host-gated and distributed: host toggles; every client transcribes its own mic; Web Speech falls back to local Whisper WASM (prefer local for sovereignty).
- Stable reference: `docs/reference/architecture.md`. Shared agent harness: `AGENTS.md`.
