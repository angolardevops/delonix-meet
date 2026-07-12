<div align="center">

# Delonix Meet

**Plataforma de videoconferência corporativa — self-hosted, soberana, backend 100% Rust.**

A alternativa séria ao Google Meet / Zoom / Teams para quem tem requisitos de **soberania de dados,
conformidade (BNA/LGPD) e self-hosting**. E2EE real, SFU próprio em Rust, multi-tenant com
isolamento por organização, e deploy de binário único sem runtime externo.

[Arquitetura](ARCHITECTURE.md) · [Deploy (single-host)](DEPLOYMENT.md) · [Runbook DevOps/SRE](docs/ops/platform-engineering.md) · [Regressões](docs/reference/regressions.md)

</div>

---

## O que é

Videoconferência enterprise construída de raiz para mercados que **não podem** (ou não querem) pôr
comunicação crítica em cloud de terceiros. O nome vem da *Delonix regia* — a flamboyant.

**Princípios:** self-hosted first (funciona sem Internet externa) · security by design (sem atalhos)
· enterprise sem lock-in (API keys, webhooks, open core) · performance sem GC (backend Rust).

### Destaques

| Área | Capacidades |
|---|---|
| **Chamada** | SFU Rust (simulcast q/h/f), grelha estilo Meet, palco + speaker detection, screen-share como track separada, reações, mão levantada, breakouts completos |
| **Segurança** | E2EE por sala (AES-256-GCM Insertable Streams), SRTP/DTLS, JWT+refresh rotativo, room tokens de 5 min, multi-tenant isolado + **RLS backstop** |
| **Produtividade** | Gravação server-side (VP9+Opus, E2EE via key delegation), transcrição (Web Speech + Whisper WASM local), Atas por AI (MoM), quadro branco, sondagens, Q&A, temporizador |
| **Enterprise** | Organizações → filiais → grupos, calendário + conflitos, webhooks (Slack/Teams/Mattermost/HMAC), API keys, analytics admin, retenção, i18n PT/EN |
| **Media effects** | Blur / fundos virtuais (RVM ONNX), paralaxe 3D por head-tracking, supressão de ruído |

Inventário completo e roadmap em [ARCHITECTURE.md](ARCHITECTURE.md) e no `HARNESS.md` (§3).

---

## Arquitetura num relance

```
┌──────────────┐   HTTPS (REST) + WSS (sinalização) + SRTP/DTLS (media UDP)
│  WebApp      │─────────────────────────────┐
│  React + TS  │                             ▼
└──────────────┘        ┌───────────────────────────────────────┐
                        │        delonix-server (Rust, 1 binário) │
   Browser ──media──►   │   API REST · Auth · Signaling · SFU     │
                        └──────┬──────────┬──────────┬───────────┘
                               ▼          ▼          ▼
                          Postgres     Redis      coturn
                          (dados)   (presença/   (STUN/TURN
                                     pubsub)      relay media)
```

- **Backend** `server/` — axum 0.7, webrtc-rs (SFU DTLS/SRTP + RTP fan-out), sqlx (migrações
  automáticas no arranque), tokio. Um só executável: API + auth + sinalização + SFU + gravação.
- **Frontend** `web/` — React + TypeScript + Vite. `SfuCall` (RTCPeerConnection/simulcast),
  cliente WS tipado, E2EE em worker, efeitos de media, PWA.
- **Infra** — PostgreSQL, Redis (presença/pub-sub cross-nó), coturn (NAT traversal / relay).

> Porquê SFU próprio (não LiveKit/mediasoup)? Zero dependência externa, deploy de binário único, e
> controlo total do pipeline RTP para **E2EE + gravação side-car**. Ver `HARNESS.md` §4.

---

## Arrancar em desenvolvimento

**Pré-requisitos:** Rust 1.80+, Node 20+, Docker + Compose. (Detalhe de hosts: [runbook §2](docs/ops/platform-engineering.md#2-pré-requisitos-de-host-single--multi).)

**Um comando** — sobe infra + backend + frontend e imprime as URLs:

```bash
make dev        # ambiente de dev pronto a usar
make help       # lista todos os alvos
make logs       # segue os logs (backend + frontend)
make stop       # para dev (mantém a infra)   ·   make down: para tudo
```

Abre **http://localhost:5173** (em localhost a câmara/mic funcionam sem HTTPS). Para acesso por
IP na rede, a câmara exige contexto seguro → usa `make web-https` ou o nginx HTTPS.

Ou manualmente:

```bash
docker compose up -d                 # 1. Infra (Postgres 5435, Redis 6379, coturn 3478)
bash deploy/run-dev-server.sh        # 2. Backend (8180) — define DELONIX_ALLOW_INSECURE=1 (fail-closed)
cd web && npm install && npm run dev # 3. Frontend (5173, proxy → backend)
```

> **Fail-closed:** o servidor faz **panic no arranque** sem segredos fortes. `make dev` e
> `run-dev-server.sh` já definem `DELONIX_ALLOW_INSECURE=1` — **nunca** usar isto em produção.

### Portas (dev)

| Serviço | Porta | | Serviço | Porta |
|---|---|---|---|---|
| Frontend (Vite) | 5173 | | Postgres | 5435 (host) → 5432 |
| Backend | 8180 | | Redis | 6379 |
| coturn STUN/TURN | 3478 | | HTTPS local (nginx) | 443 → `meet.delonix.local` |

### Variáveis de ambiente (backend)

| Variável | Default (dev) | Descrição |
|---|---|---|
| `DATABASE_URL` | `postgres://delonix:delonix_dev@localhost:5435/delonix_meet` | Postgres |
| `BIND_ADDR` | `0.0.0.0:8180` | endereço do servidor |
| `JWT_SECRET` | — | **obrigatório em produção** (≥32 bytes) |
| `TURN_HOST` / `TURN_SECRET` | `localhost:3478` / — | coturn (segredo == `--static-auth-secret`) |
| `FORCE_TURN_RELAY` | — | `1` força relay-only (K8s/media atrás de NAT) |
| `CORS_ORIGINS` | vazio | allowlist de origens (vazio = same-origin) |
| `RECORDINGS_DIR` | `recordings` | disco das gravações |
| `COOKIE_INSECURE` | — | `1` só em HTTP puro (cookie sem `Secure`) |
| `DELONIX_ALLOW_INSECURE` | — | `1` aceita defaults de dev — **nunca em produção** |

---

## Estrutura do repositório

```
delonix-meet/
├── server/            # backend Rust (main.rs, auth, org, rooms, sfu, signaling, recorder, …)
│   ├── migrations/    # migrações sqlx (0001–00XX), aplicadas no arranque
│   └── src/           # ver HARNESS.md §2 para o mapa de módulos
├── web/src/           # React + TS (pages/Room.tsx, webrtc.ts, presence.ts, e2ee.ts, styles/)
├── deploy/            # produção: nginx, systemd, docker-compose.prod, ansible, k8s/
│   └── k8s/           # manifestos Kubernetes (00-namespace … 51-coturn) + helm-values
├── docs/              # arquitetura, ADRs, regressões, ops, PSTN, posicionamento
├── scripts/           # fitness functions (check-docs-drift / room-affinity / tenant-rls)
├── Makefile           # dev · stage (kind) · prod (k8s) · build · test · migrate
└── HARNESS.md          # harness de desenvolvimento AI (contexto completo, invariantes)
```

---

## Testar uma chamada manualmente

1. Abre **http://localhost:5173** em duas janelas (uma normal, uma anónima).
2. Regista dois utilizadores (o registo cria org + admin; usa emails `@teste.local`).
3. Na primeira: **Criar sala** → copia o código (ex.: `abc-defg-hij`) ou o link `#/r/abc-defg-hij`.
4. Na segunda: **Entrar com código**. Vídeo/áudio ligam automaticamente (SFU).
5. Testa chat 💬, mute 🎙, screen-share 🖥, quadro branco, gravação.

Limpar no fim: `DELETE FROM users WHERE email ~ '@teste\.local$'` + apagar `.webm` órfãos em `recordings/`.

---

## Build, testes & fitness functions

```bash
make build      # backend (release) + frontend (produção)
make test       # fitness + cargo test + tsc + vitest
make migrate    # cargo sqlx migrate run
```

`make test` inclui as **fitness functions de arquitetura** (Fowler) que falham o build se a
plataforma driftar: coerência de docs, afinidade `/ws` por sala (R3), e RLS multi-tenant (ADR-0002).
Testes de regressão codificados: `rate_limit` (R6, token bucket) e `sfuLifecycle.test.ts` (R1/R2).

---

## Deploy em produção

**Zero-touch (recomendado)** — um só ficheiro (`deploy/config.yml`), um só comando. IPs, DNS
público, TLS (self-signed ou Let's Encrypt) e segredos gerados **sem intervenção humana**,
single **ou** multi-host:

```bash
make deploy-config     # cria deploy/config.yml a partir do exemplo
# edita deploy/config.yml (deploy_mode, domain, tls, dns)
make deploy            # faz tudo (Ansible + 12-factor)
```

Guia completo: [docs/ops/zero-touch-deploy.md](docs/ops/zero-touch-deploy.md).

Caminhos alternativos / manuais:

| Modelo | Quando | Como | Guia |
|---|---|---|---|
| **Single-host** (systemd + nginx) | 1 servidor, IP público | `bash deploy/deploy.sh` / `make prod-legacy` | [DEPLOYMENT.md](DEPLOYMENT.md) |
| **Single-host** (docker-compose) | tudo em Docker | `docker compose -f ... -f deploy/docker-compose.prod.yml up -d` | [runbook §1.2](docs/ops/platform-engineering.md) |
| **Stage local** (kind + k8s) | demo/QA | `make image-push && make stage` | [runbook §5.2](docs/ops/platform-engineering.md) |
| **Multi-host HA** (Kubernetes) | zero-downtime, escala | `make prod DOMAIN=meet.example.com` | [runbook §5.3](docs/ops/platform-engineering.md) |

👉 **Antes de subir, lê o [Runbook DevOps · SRE · Platform Engineering](docs/ops/platform-engineering.md)** —
cobre pré-requisitos de host (single/multi), dimensionamento, a **rede de media** (portas/firewall/NAT
que mais falham), afinidade por sala, observabilidade/SLO, backup/restore e runbooks de incidente.

---

## Segurança (resumo)

- Passwords **Argon2id**; login com comparação de tempo constante (anti-enumeração).
- JWT de acesso (15 min) + refresh rotativo revogável em cookie **HttpOnly `Secure`** (30 dias).
- WebSocket de sinalização só aceita **room tokens** (JWT âmbito-de-sala, 5 min).
- Credenciais TURN efémeras (HMAC), nunca estáticas no cliente.
- **Multi-tenant isolado** em todos os endpoints + **RLS backstop** fail-closed no Postgres.
- Rate-limit token-bucket (WS anti-flood), SSRF bloqueada nos webhooks, limites de corpo por rota.
- **E2EE opcional por sala** — AES-256-GCM por frame (Insertable Streams); a chave nunca vai ao
  servidor, exceto por *key delegation* explícita para gravação server-side.

Modelo completo em [DEPLOYMENT.md](DEPLOYMENT.md) (checklist) e `HARNESS.md` §6 (invariantes).

---

## Documentação

| Documento | Para quê |
|---|---|
| [ARCHITECTURE.md](ARCHITECTURE.md) | Desenho do sistema, fases, monorepo |
| [docs/ops/platform-engineering.md](docs/ops/platform-engineering.md) | **Runbook DevOps/SRE/Platform** — produção, hosts, media, scaling, incidentes |
| [docs/ops/zero-touch-deploy.md](docs/ops/zero-touch-deploy.md) | **Deploy zero-touch** — `make deploy`, 12-factor, IP/DNS/TLS/segredos automáticos |
| [DEPLOYMENT.md](DEPLOYMENT.md) | Deploy single-host bare-metal, go-live |
| [deploy/k8s/README.md](deploy/k8s/README.md) | Manifestos Kubernetes, HA de estado |
| [docs/reference/architecture.md](docs/reference/architecture.md) | Referência estável de arquitetura |
| [docs/reference/regressions.md](docs/reference/regressions.md) | R1–R12 — regressões a **não** reintroduzir |
| [docs/reference/api-contract.md](docs/reference/api-contract.md) | Fronteira de API pública (`/api/v1`) |
| [docs/adr/](docs/adr/) | Decisões de arquitetura (afinidade por sala, RLS) |
| [docs/competitive-positioning.md](docs/competitive-positioning.md) | Delonix vs Zoom/Teams/Meet |
| [HARNESS.md](HARNESS.md) | Harness AI — contexto completo, convenções, revisores |

---

<div align="center">
<sub>Delonix Meet · self-hosted · soberano · Rust</sub>
</div>
