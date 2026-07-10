# AGENTS.md — Delonix Meet

> **Harness universal de agentes.** Lido por OpenAI Codex CLI, e por qualquer ferramenta de agente que siga a convenção `AGENTS.md`. O ficheiro-mãe é [`HARNESS.md`](HARNESS.md) (contexto completo); para Gemini é [`GEMINI.md`](GEMINI.md). Este ficheiro é o **resumo operacional partilhado** — os três devem manter-se coerentes.

---

## 1. O que é o Delonix Meet

Plataforma de videoconferência **corporativa, self-hosted e SaaS**, feita de raiz para mercados com requisitos de **soberania de dados, conformidade (BNA/LGPD) e self-hosting**. Alternativa séria a Zoom/Teams/Meet para empresas africanas e lusófonas, setor público e organizações que não podem pôr comunicação crítica em cloud estrangeira.

Princípios: **self-hosted first** · **security by design** · **enterprise sem lock-in** · **backend 100% Rust (sem GC)**.

## 2. Stack (estado real)

**Backend** `server/` (Rust): axum 0.7 (HTTP), webrtc-rs (SFU: DTLS/SRTP, simulcast, RTP fan-out), sqlx 0.7 (Postgres, migrações auto), tokio, argon2, jsonwebtoken (JWT access 15 min + refresh 30 d rotativo), reqwest 0.12 **rustls-tls** (NÃO 0.13). Redis pub/sub para multi-nó (presença + sinalização).

**Frontend** `web/src/` (React + TS + Vite): `webrtc.ts` (SfuCall), `signaling.ts` (WS `/ws`), `presence.ts` (WS `/rtc`), `e2ee.ts` (Insertable Streams AES-256-GCM), `media.ts` (efeitos de fundo RVM ONNX, Transcriber, MeetingRecorder), `pages/Room.tsx` (sala Meet-style).

**Infra**: Postgres (5435 dev), Redis (6379), coturn (3478/5349). Deploy: systemd + nginx (single-node) **e** Kubernetes (`deploy/k8s/`, ingress-nginx + cert-manager). Ver [`docs/reference/architecture.md`](docs/reference/architecture.md) para o mapa completo.

## 3. Invariantes de segurança (NUNCA quebrar)

1. **Segredos fail-closed**: `config.rs` faz panic sem `JWT_SECRET`/`TURN_SECRET`/`DATABASE_URL` fortes. `DELONIX_ALLOW_INSECURE=1` só em dev.
2. **Isolamento multi-tenant**: `rooms::can_access_room` / `room_access` e `org::*` escopam TUDO à(s) org(s) do utilizador. Nunca devolver dados cross-org.
3. **Room tokens curtos**: JWT separado, âmbito = 1 sala, expira em minutos. Sem token válido → WS recusado.
4. **SSRF em webhooks**: validar host (bloquear privados/loopback/link-local/metadata) na criação E na entrega. Sem redirects.
5. **Rate limit**: lockout de login por conta; rate limit por IP em `/api/v1`; WS com token bucket por socket (600 burst / 300 sustained — tolera a rajada de ICE).
6. **Cookie `dlx_refresh`** sempre `HttpOnly; SameSite=Strict; Secure` (exceto `COOKIE_INSECURE=1` em dev HTTP).
7. **E2EE real**: chave AES-256 gerada no cliente, nunca vai ao servidor exceto para gravação (key delegation explícita com confirm()).
8. **Validação no servidor**: host controls (lock/share-only/kick/promote-admit) validados em `signaling.rs`, nunca confiados no cliente.

## 4. Arquitetura — decisões não óbvias

- **SFU próprio** (`sfu.rs`), não LiveKit/mediasoup — binário único, controlo total do pipeline RTP para E2EE + gravação side-car. Custo: simulcast/congestion à mão.
- **SFU é in-memory por pod.** O Redis propaga **sinalização/presença**, NÃO RTP. Em multi-réplica, TODOS os pares de uma sala têm de cair no MESMO pod → **afinidade por sala** no ingress (`upstream-hash-by: $arg_room`; o cliente envia `/ws?...&room=CODE`). Sem isto: media num só sentido, admissão e partilha de ecrã falham.
- **E2EE + gravação servidor**: key delegation — o anfitrião cede a chave AES-256 no `server-record`; o servidor decifra antes de gravar; a chave fica só em memória.
- **Gravação IVF/OGG → ffmpeg**: `recorder.rs` usa PTS em ms reais do RTP (o `IVFWriter` da lib usa contador de frames — errado, não reverter).
- **Transcrição**: distribuída e controlada pelo anfitrião — o anfitrião liga a Nota AI, TODOS os clientes transcrevem o próprio microfone (STT on-device) e difundem via `transcript`. Motor: Web Speech quando a Google é alcançável, senão **Whisper WASM local** (privado, self-hosted). Preferir local para soberania.

## 5. Workflow

```bash
make dev                       # infra + backend + frontend
bash deploy/run-dev-server.sh  # só backend (ALLOW_INSECURE)
cd server && cargo build --release   # SEMPRE rebuild após migração nova (touch src/main.rs força re-embed)
bash deploy/publish-web.sh     # compila o frontend e publica em /var/www/delonix (nginx)
```

Portas dev: backend `8180`, frontend `5173`, Postgres `5435`, Redis `6379`, coturn `3478`. **HTTPS obrigatório** fora de localhost (câmara/mic/WebRTC exigem contexto seguro). Emails de teste `@teste.local`, limpar no fim.

**K8s**: `deploy/build-images.sh [--push] [REGISTRY] [TAG]` constrói `delonix-{server,web}`. Em kind: `kind load docker-image <img> --name <cluster>` + `kubectl rollout restart`. Ingress precisa da afinidade por sala (ver §4).

## 6. Padrões de código

**Rust**: `AppError` para todos os erros de handler — nunca `unwrap()` em produção. `sqlx::query!`/`query_as!` (verificação compile-time). Migrações `server/migrations/NNNN_*.sql` sequenciais. Novo módulo → declarar em `main.rs` + registar rotas.

**TS/React**: componentes funcionais + hooks; estado global via Context; mensagens WS tipadas (discriminant union); tokens CSS via custom properties (nunca hardcode de cor); i18n `useTranslation()`. **Nunca `var()` para dimensões de tiles** (transições congelam em background) — dimensões inline por tile.

## 7. Painel de revisores (invocar como persona)

Ver [`docs/ai-reviewers.md`](docs/ai-reviewers.md) para perfis completos. Resumo:

| Persona | Domínio | Invocar para |
|---|---|---|
| **Graydon Hoare** (criador do Rust) | Safety, async Tokio, zero-cost | `sfu.rs`, `recorder.rs`, hot path RTP |
| **Brendan Burns** (co-criador do K8s) | HA, scaling, operações, afinidade por sala | `deploy/`, `deploy/k8s/`, rollout |
| **Justin Uberti** (co-criador do WebRTC, Google Meet) | ICE, simulcast, codecs, E2EE | `sfu.rs`, `webrtc.ts` |
| **Adam Langley** (BoringSSL, Google) | E2EE, TLS/DTLS, JWT, SSRF, cookies | `auth.rs`, `e2ee.ts`, `webhooks.rs` |
| **Lars Bak** (V8, Google) | WASM/Worker perf | `whisperWorker.ts`, `matte.ts` |
| **Skype/Teams Media Architect** (composta, MS) | calling stack, resiliência, compliance | media, retenção, audit |
| **Zoom Reliability Architect** (composta) | redes degradadas, fallback, escala | congestion, TURN, reconnect |

Para compute/performance, combinar **Graydon Hoare** (Rust, alocações) + **Brendan Burns** (recursos/limites K8s) + **Lars Bak** (WASM cliente).

## 8. Gotchas conhecidos

- **Vite proxy** `/ws` e `/rtc` precisam de `ws: true` — reiniciar Vite após mudanças. `vite.config.ts` só lê certos de dev no `serve` (nunca no `build`, senão a imagem Docker falha).
- **Rebuild release** após migração; **não** atualizar reqwest para 0.13.
- **Web Speech** só Chrome/Edge e envia áudio à Google — fallback automático para Whisper WASM local.
- **K8s multi-réplica** exige afinidade por sala (ver §4) — foi a causa de media num só sentido.
- **mkcert em Firefox**: o Firefox não confia na store do SO — `security.enterprise_roots.enabled=true` ou importar a rootCA.

## 9. Próximas prioridades

SSO OIDC genérico · App mobile Flutter (CallKit/ConnectionService) · PSTN dial-in (FreeSWITCH+Kamailio) · i18n completo · SCIM · Whisper server-side (diarização por track) · MLS para E2EE em grupo · SDK público.
