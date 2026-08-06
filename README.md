<div align="center">

# Delonix Meet

**Plataforma de videoconferência corporativa — self-hosted, soberana, backend 100% Rust.**

A alternativa séria ao Google Meet / Zoom / Teams para quem tem requisitos de **soberania de dados,
conformidade (BNA/LGPD) e self-hosting**. E2EE real, SFU próprio em Rust, multi-tenant com
isolamento por organização, e deploy de binário único sem runtime externo.

</div>

---

## Por onde começar

| Se és… | Começa aqui |
|---|---|
| **Programador novo no projeto** | [§1 Arrancar em 5 minutos](#1-arrancar-em-5-minutos) → [Onboarding](docs/onboarding.md) (C4, UML, receitas) |
| **Quem vai pôr isto em produção** | [Guia de Deployment](docs/deployment.md) — do portátil ao cluster |
| **DevOps / SRE em serviço** | [Runbook](docs/ops/platform-engineering.md) — dimensionamento, SLO, incidentes |
| **Quem integra outro sistema** | [§5 Integrações](#5-integrações) → [Contrato de API](docs/reference/api-contract.md) |
| **Quem decide** | [§2 O que faz](#2-o-que-faz) → [Posicionamento competitivo](docs/competitive-positioning.md) |
| **Agente de IA** | [HARNESS.md](HARNESS.md) — contexto completo, invariantes, revisores |

---

## 1. Arrancar em 5 minutos

**Precisas de:** Rust 1.80+, Node 20+, Docker + Compose.

```bash
make dev        # infra + backend + frontend, e imprime as URLs
make help       # todos os alvos
make logs       # segue backend + frontend
make stop       # pára o dev (mantém a infra)   ·   make down: pára tudo
```

Abre **http://localhost:5173**. Em `localhost` a câmara e o microfone funcionam sem HTTPS —
é o único host onde isso acontece.

<details>
<summary>Passo a passo, sem o Makefile</summary>

```bash
docker compose up -d                  # Postgres 5435, Redis 6379, coturn 3478
bash deploy/run-dev-server.sh         # backend 8180 (define DELONIX_ALLOW_INSECURE=1)
cd web && npm install && npm run dev  # frontend 5173, com proxy para o backend
```

</details>

> **Fail-closed:** o servidor faz **panic no arranque** sem segredos fortes. O modo de dev
> define `DELONIX_ALLOW_INSECURE=1` — que **nunca** deve existir em produção.

### Provar que uma chamada funciona

Duas abas não chegam para testar media a sério; usa **dois dispositivos** ou dois browsers
diferentes.

1. Regista dois utilizadores (o registo cria org + admin). Usa emails `@teste.local`.
2. Num: **Criar sala** → copia o código (`abc-defg-hij`) ou o link `#/r/abc-defg-hij`.
3. No outro: **Entrar com código**. Vídeo e áudio ligam pelo SFU.
4. Testa chat, mute, partilha de ecrã, quadro branco, gravação.

Limpar depois: `DELETE FROM users WHERE email ~ '@teste\.local$'` e apagar os `.webm` que
esses testes deixaram. **Nunca** um `rm *.webm` em bloco — já destruiu uma gravação real.

### Portas (dev)

| Serviço | Porta | | Serviço | Porta |
|---|---|---|---|---|
| Frontend (Vite) | 5173 | | Postgres | 5435 → 5432 |
| Backend | 8180 | | Redis | 6379 |
| coturn STUN/TURN | 3478 | | HTTPS local (nginx) | 443 |

---

## 2. O que faz

Videoconferência enterprise construída de raiz para mercados que **não podem** (ou não querem)
pôr comunicação crítica em cloud de terceiros. O nome vem da *Delonix regia* — a flamboyant.

**Princípios:** self-hosted first (funciona sem Internet externa) · security by design (sem
atalhos) · enterprise sem lock-in (API keys, webhooks, open core) · performance sem GC.

| Área | Capacidades |
|---|---|
| **Chamada** | SFU Rust (simulcast q/h/f), grelha estilo Meet, palco + speaker detection, partilha de ecrã como track separada, reações, mão levantada, breakouts |
| **Segurança** | E2EE por sala (AES-256-GCM Insertable Streams), SRTP/DTLS, JWT + refresh rotativo, room tokens de 5 min, multi-tenant isolado + **RLS backstop** |
| **Produtividade** | Gravação server-side (VP9+Opus, E2EE via key delegation), transcrição (Web Speech + Whisper WASM local), atas por AI, quadro branco, sondagens, Q&A, temporizador |
| **Enterprise** | Organizações → filiais → grupos, calendário + conflitos, webhooks (Slack/Teams/Mattermost/HMAC), API keys, analytics, retenção, i18n PT/EN |
| **Integração** | **Login com conta Odoo** (org e directório nascem do primeiro login), API de calendário `/api/v1/meetings`, módulo Odoo `nk_delonix_meet` |
| **Media effects** | Blur / fundos virtuais (RVM ONNX), paralaxe 3D por head-tracking, supressão de ruído |

---

## 3. Arquitetura

```
┌──────────────┐   HTTPS (REST) + WSS (sinalização) + SRTP/DTLS (media UDP)
│  WebApp      │─────────────────────────────┐
│  React + TS  │                             ▼
└──────────────┘        ┌────────────────────────────────────────┐
                        │   delonix-server (Rust, 1 binário)     │
   Browser ──media──►   │   API REST · Auth · Signaling · SFU    │
                        └──────┬──────────┬──────────┬───────────┘
                               ▼          ▼          ▼
                          Postgres     Redis      coturn
                          (dados)   (presença/   (STUN/TURN
                                     pub-sub)     relay media)
```

- **`server/`** — axum, webrtc-rs (SFU DTLS/SRTP + fan-out RTP), sqlx (migrações automáticas
  no arranque), tokio. Um executável: API + auth + sinalização + SFU + gravação.
- **`web/`** — React + TypeScript + Vite. `SfuCall` (RTCPeerConnection/simulcast), cliente WS
  tipado, E2EE em worker, efeitos de media, PWA.
- **Infra** — PostgreSQL, Redis (presença/pub-sub cross-nó), coturn (NAT traversal / relay).

> **Porquê SFU próprio, e não LiveKit/mediasoup?** Zero dependência externa, deploy de binário
> único, e controlo total do pipeline RTP para **E2EE com gravação side-car**. O custo é fazer
> simulcast e congestion control à mão. Ver `HARNESS.md` §4.

### Estrutura do repositório

```
delonix-meet/
├── server/            # backend Rust
│   ├── migrations/    # sqlx (0001–00XX), aplicadas no arranque
│   └── src/           # mapa de módulos em HARNESS.md §2
├── web/src/           # React + TS (pages/Room.tsx, webrtc.ts, e2ee.ts, styles/)
├── deploy/            # nginx, systemd, compose.prod, ansible, k8s/
├── docs/              # deployment, arquitetura, ADRs, regressões, ops
├── scripts/           # fitness functions de arquitetura
├── Makefile           # dev · stage · prod · build · test · migrate
└── HARNESS.md          # harness de desenvolvimento AI
```

---

## 4. Build, testes e fitness functions

```bash
make build      # backend (release) + frontend (produção)
make test       # fitness + cargo test + tsc + vitest
make migrate    # cargo sqlx migrate run
```

`make test` inclui **fitness functions de arquitetura** que falham o build se a plataforma
driftar: coerência entre docs e código, afinidade `/ws` por sala, e RLS multi-tenant.
Regressões codificadas em teste: rate-limit token-bucket, ciclo de vida do SFU, glare.

Antes de dar uma feature por pronta, lê [regressions.md](docs/reference/regressions.md) — é a
lista do que já partiu uma vez e não deve voltar a partir.

---

## 5. Integrações

### Login com conta Odoo

Quem tem conta no Odoo entra com as **mesmas credenciais**. No primeiro login a organização é
criada a partir da empresa do utilizador e **todo o directório de utilizadores activos** é
sincronizado. Duas variáveis e um restart:

```bash
PLATFORM_ODOO_URL=https://erp.empresa.com
PLATFORM_ODOO_DB=empresa_prod
```

Vazias = desligado, sem alterar o comportamento existente. Detalhe, limites e implicações de
segurança em [deployment §7](docs/deployment.md#7-integração-odoo).

### API pública `/api/v1`

Contrato estável para integrações, SDK e bots, autenticado por chave de organização
(`dlx_...`). Recursos: organização, salas, **reuniões** (criar/reagendar/cancelar, idempotente),
gravações, atas.

> Para integrar um calendário, usar `POST /api/v1/meetings` — **não** `/api/v1/rooms`. Uma sala
> criada sem anfitrião fica com o utilizador de serviço como dono e, como esse nunca faz login,
> ninguém consegue ser admitido lá dentro.

Fronteira entre a BFF interna e o contrato público: [api-contract.md](docs/reference/api-contract.md).

---

## 6. Segurança

- Passwords **Argon2id**; login com comparação em tempo constante (anti-enumeração).
- JWT de acesso (15 min) + refresh rotativo revogável em cookie **HttpOnly `Secure`** (30 dias).
- O WebSocket de sinalização só aceita **room tokens** (JWT de âmbito-de-sala, 5 min).
- Credenciais TURN efémeras por HMAC, nunca estáticas no cliente.
- **Multi-tenant isolado** em todos os endpoints, com **RLS fail-closed** no Postgres como
  backstop: uma query que esqueça o filtro devolve zero linhas em vez de vazar.
- Rate-limit token-bucket (anti-flood no WS), SSRF bloqueada nos webhooks, limites de corpo por rota.
- **E2EE opcional por sala** — AES-256-GCM por frame; a chave nunca vai ao servidor, exceto por
  *key delegation* explícita e confirmada pelo utilizador para gravação server-side.

Invariantes que não se quebram: `HARNESS.md` §6. Checklist operacional:
[deployment §8](docs/deployment.md#8-verificação-e-go-live).

---

## 7. Documentação

**Operar**
| Documento | Para quê |
|---|---|
| [docs/deployment.md](docs/deployment.md) | **Guia de deployment** — cenários, configuração, Odoo, go-live, avarias |
| [docs/ops/platform-engineering.md](docs/ops/platform-engineering.md) | Runbook DevOps/SRE — hosts, media, escala, incidentes |
| [docs/ops/zero-touch-deploy.md](docs/ops/zero-touch-deploy.md) | Deploy automático (`make deploy`) |
| [deploy/k8s/README.md](deploy/k8s/README.md) | Manifestos Kubernetes, HA de estado |

**Construir**
| Documento | Para quê |
|---|---|
| [docs/onboarding.md](docs/onboarding.md) | Onboarding de engenharia — C4, UML, flowchart, receitas |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Desenho do sistema, fases, monorepo |
| [docs/reference/architecture.md](docs/reference/architecture.md) | Referência estável de arquitetura |
| [docs/reference/api-contract.md](docs/reference/api-contract.md) | Fronteira da API pública `/api/v1` |
| [docs/reference/design-system.md](docs/reference/design-system.md) | Tokens, controlos, temas |
| [docs/reference/regressions.md](docs/reference/regressions.md) | **O que já partiu e não deve voltar a partir** |
| [docs/adr/](docs/adr/) | Decisões de arquitetura (afinidade por sala, RLS) |
| [HARNESS.md](HARNESS.md) | Harness AI — contexto, convenções, revisores |

**Decidir**
| Documento | Para quê |
|---|---|
| [docs/competitive-positioning.md](docs/competitive-positioning.md) | Delonix vs Zoom/Teams/Meet |
| [docs/ux-review.md](docs/ux-review.md) | Revisão de UI/UX enterprise |
| [docs/multi-region-scaling.md](docs/multi-region-scaling.md) | Escala multi-região |

---

<div align="center">
<sub>Delonix Meet · self-hosted · soberano · Rust</sub>
</div>
