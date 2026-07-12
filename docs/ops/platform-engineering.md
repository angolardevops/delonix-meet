# Delonix Meet — DevOps · SRE · Platform Engineering

> **Runbook único de operação.** Como subir o Delonix Meet em produção, o que cada
> host precisa (single vs. multi-host), como a rede de media tem de estar aberta,
> como escalar, observar, fazer backup/restore e reagir a incidentes.
>
> Para o **passo-a-passo bare-metal** (systemd + nginx) ver [DEPLOYMENT.md](../../DEPLOYMENT.md).
> Para o **desenho do sistema** ver [ARCHITECTURE.md](../../ARCHITECTURE.md) e
> [docs/reference/architecture.md](../reference/architecture.md).
> Antes de mexer em media/deploy, **lê** [docs/reference/regressions.md](../reference/regressions.md) (R1–R12).

---

## 0. TL;DR — que caminho seguir

| Cenário | Alvo | Comando | Guia |
|---|---|---|---|
| Programar localmente | 1 máquina, HMR | `make dev` | [README](../../README.md#arrancar-em-desenvolvimento) |
| Demo / QA num portátil | kind + k8s local | `make stage` | §5.2 |
| Produção pequena (1 host) | systemd + nginx **ou** docker-compose | `make prod-legacy` / compose | §4 · [DEPLOYMENT.md](../../DEPLOYMENT.md) |
| Produção HA (multi-host) | Kubernetes + Helm + coturn LB | `make prod` | §5.3 |

Regra de ouro: **começa pelo modelo mais simples que serve a tua carga** (§2 dimensiona).
Um único host bem afinado aguenta dezenas de salas pequenas. Só migra para K8s multi-host
quando precisas de HA real (zero-downtime, tolerância a falha de nó) ou de escala horizontal.

---

## 1. Topologias de deploy

O binário `delonix-server` é **um só executável** (API + auth + sinalização + SFU + gravação).
Isso permite três topologias, da mais simples à mais robusta:

### 1.1 Single-host bare-metal (systemd + nginx)
```
Browser ──HTTPS 443──► nginx ──proxy loopback──► delonix-server (systemd :8180)
   │                    (TLS/CSP/SPA)                    │
   └──SRTP/DTLS (UDP)──► coturn (3478) ◄─── media ──────┘
                          Postgres · Redis (docker, só loopback)
```
O caminho de referência, documentado em [DEPLOYMENT.md](../../DEPLOYMENT.md). Ideal para
1 servidor com IP público. Deploy: `bash deploy/deploy.sh` (ou `make prod-legacy`).

### 1.2 Single-host containerizado (docker-compose)
`docker-compose.yml` + [deploy/docker-compose.prod.yml](../../deploy/docker-compose.prod.yml).
Postgres/Redis/coturn em containers (Postgres/Redis só no loopback). Mesmo host, isolamento
de processo. Bom para quem já opera tudo em Docker.

### 1.3 Multi-host Kubernetes (HA)
```
                       ┌── ingress-nginx (LB metallb/cloud, VIP público) ──┐
   Browser ──HTTPS──►  │  /api /rtc → Service delonix-server                │
                       │  /ws?room=X → Service delonix-server-ws (afinidade)│  ← R3
                       └────────────────────┬──────────────────────────────┘
                          3× delonix-server (SFU in-memory por pod)
                          3× delonix-web (nginx SPA)
   Media UDP ──► coturn (Service LoadBalancer, VIP dedicado) ◄── SFU relay  ← R4
                          Postgres-HA (repmgr) · Redis (Sentinel)
```
O SFU é **in-memory por pod** → todos os pares de uma sala têm de aterrar no **mesmo pod**
(afinidade por sala, §6). Redis propaga presença/sinalização entre pods, **não RTP**.

---

## 2. Pré-requisitos de HOST (single / multi)

### 2.1 Software base (qualquer host)

| Ferramenta | Versão mín. | Para quê |
|---|---|---|
| Linux x86-64 | kernel 5.10+ | host |
| Docker + Compose | 24+ | infra (Postgres/Redis/coturn), builds |
| Rust | 1.80+ | `cargo build --release` (se compilar no host) |
| Node | 20+ | build da SPA |
| nginx | 1.18+ | TLS + proxy (topologia 1.1) |
| **K8s multi-host:** kubectl, helm 3, kind (stage) ou kubeadm/k3s (prod), ansible | recente | §5 |

### 2.2 Dimensionamento — SINGLE-HOST

Um host serve **control-plane + media** juntos. O gargalo é quase sempre a **rede de media**
(o SFU faz fan-out RTP: uma sala de N publicadores gera até N×(N-1) fluxos de saída).

| Carga simultânea | vCPU | RAM | Rede (up/down) | Disco | Notas |
|---|---|---|---|---|---|
| Dev / demo (≤5 pessoas) | 2 | 4 GB | 50 Mbps | 20 GB | `make dev` / compose |
| Pequena (≤30 em 5–8 salas) | 4 | 8 GB | 200 Mbps | 100 GB SSD | systemd 1.1 confortável |
| Média (≤100 em 15–20 salas) | 8 | 16 GB | 500 Mbps sim. | 250 GB SSD | ativar TURN-TLS; considerar K8s |
| Gravação server-side ativa | +2 vCPU | +2 GB | — | +ffmpeg I/O | recorder + ffmpeg post-stop |

**Regra de banda:** vídeo 720p ≈ 1.5–2.5 Mbps por publicador. Reserva **upload = downlink dos
subscritores** — numa sala de 10 câmaras, o servidor emite ~9× o bitrate de cada. É a rede,
não a CPU, que satura primeiro.

**Disco:** gravações `.webm` ≈ 30–60 MB por 10 min por sala. Dimensiona `RECORDINGS_DIR` +
retenção (consola admin) e monitoriza. Postgres cresce devagar (metadados, não media).

### 2.3 Dimensionamento — MULTI-HOST (Kubernetes)

Separa **planos de controlo** (stateless, escala horizontal) de **estado** (Postgres/Redis, HA):

| Papel | Nós | Por nó | Observação |
|---|---|---|---|
| Control-plane K8s | 1 (stage) / 3 (prod) | 2 vCPU / 4 GB | etcd quorum ímpar em prod |
| Workers (server+web) | 2+ | 4 vCPU / 8 GB | HPA 2–8 réplicas server (§6) |
| Nó de media (coturn) | 1+ | 4 vCPU / 8 GB, **IP público/1:1 NAT** | isola tráfego RTP; ver §3 |
| Estado (Postgres-HA/Redis) | 2–3 | 4 vCPU / 8 GB, disco rápido | ou serviço gerido |

**Anti-afinidade:** espalha as réplicas `delonix-server` por nós distintos (tolerância a falha
de nó). O manifesto [02-server.yaml](../../deploy/k8s/02-server.yaml) já traz PodDisruptionBudget
(`maxUnavailable:1`) para drenagens sem perda total.

### 2.4 Sistema operativo — afinação

- **`ulimit -n`** (file descriptors): ≥ 65535. Cada peer WebSocket + fluxos RTP consomem FDs.
- **Portas efémeras / UDP buffers:** `net.core.rmem_max`/`wmem_max` ≥ 16 MB para o relay de media.
- **Relógio:** NTP sincronizado (JWT/refresh dependem de tempo correto; skew quebra tokens).
- **conntrack:** em hosts de media com muito UDP, subir `nf_conntrack_max` evita drops silenciosos.

---

## 3. Rede & media WebRTC (o requisito que mais falha)

> **Media é UDP e precisa de caminho aberto.** 90% dos "liga mas fica preto" são firewall/NAT,
> não a app. Trata isto como pré-requisito de host, não como afinação posterior.

### 3.1 Matriz de portas / firewall

| Serviço | Porta | Proto | Exposição | Nota |
|---|---|---|---|---|
| HTTPS (app + API + WSS) | 443 | TCP | **pública** | nginx / ingress |
| HTTP→HTTPS redirect | 80 | TCP | pública | opcional (ACME/redirect) |
| STUN/TURN | 3478 | **UDP** + TCP | **pública** | descoberta + relay |
| TURN sobre TLS (TURNS) | 5349 | TCP/TLS | pública | **obrigatório** atrás de firewall corporativa restritiva |
| Relay RTP (range) | 49152–59152 (k8s) · 49160–49200 (dev) | **UDP** | **pública** | coturn aloca aqui; abrir o range inteiro |
| Backend | 8180 | TCP | **loopback** | nunca exposto direto |
| Postgres | 5432/5435 | TCP | **loopback/interno** | nunca público |
| Redis | 6379 | TCP | **loopback/interno** | nunca público |

### 3.2 IP público / NAT

- O SFU e o coturn têm de anunciar um **endereço alcançável pelo cliente**.
- Bare-metal com IP público direto: coturn com `external-ip=<IP público>`.
- Atrás de NAT 1:1 (cloud): mapear e definir o IP público como `external-ip`.
- **K8s (R4):** coturn corre **in-cluster** com Service `LoadBalancer` (VIP metallb em stage,
  LB de cloud com IP público em prod). `TURN_HOST=<VIP>:3478`, `FORCE_TURN_RELAY=1`,
  `external-ip = allowed-peer-ip = <VIP>` (o `--allowed-peer-ip` autoriza o hairpin relay-a-relay).
  **Nunca** coturn-no-host da mesma máquina do kind (o SNAT parte o relay UDP → 100% perda).
  Detalhe completo e diagnóstico em [regressions.md R4](../reference/regressions.md) e
  [ADR-0001](../adr/0001-room-shard-affinity.md).

### 3.3 Validar o caminho de media (sem 2 browsers)

```bash
# 0% perda = relay OK. Correr de dentro de um pod E de fora (host/cliente):
turnutils_uclient -y -W <TURN_SECRET> -u t -n 6 -m 2 <coturn-ip>
# Sinal de saúde nos logs do coturn: "peer usage" com rb>0 (relayou pacotes de peer).
kubectl -n delonix-meet logs -l app=coturn | grep 'peer usage'
```

---

## 4. Produção single-host (referência rápida)

Passo-a-passo completo em [DEPLOYMENT.md](../../DEPLOYMENT.md). Resumo operacional:

1. **Segredos fail-closed** → `/etc/delonix/delonix.env` (`chmod 600`), gerar com `openssl rand`.
   O servidor faz **panic** sem `JWT_SECRET`/`TURN_SECRET`/`DATABASE_URL` fortes.
   `DELONIX_ALLOW_INSECURE=1` **nunca** em produção.
2. **Infra** → `docker compose -f docker-compose.yml -f deploy/docker-compose.prod.yml up -d --wait`
   (Postgres/Redis só no loopback; coturn com o segredo de produção).
3. **TLS** → Let's Encrypt (`certbot`) público ou `mkcert` em rede interna.
4. **nginx** → [deploy/nginx-delonix.conf](../../deploy/nginx-delonix.conf) (CSP/HSTS, limites de
   corpo por rota, `access_log off` em `/ws|/rtc` para não gravar JWTs).
5. **systemd** → `delonix-server` + `delonix-infra` (`--user`), `loginctl enable-linger`.
6. **Deploy** → `bash deploy/deploy.sh` (pré-voo de segredos → build+test → migrações → publish → smoke).

---

## 5. Produção multi-host (Kubernetes)

### 5.1 Manifestos (fonte única — namespace `delonix-meet`)

| Ficheiro | Papel |
|---|---|
| [00-namespace.yaml](../../deploy/k8s/00-namespace.yaml) | namespace isolado |
| [01-config.yaml](../../deploy/k8s/01-config.yaml) | ConfigMap + Secret (`TURN_HOST`, `FORCE_TURN_RELAY`, `DATABASE_URL`, `JWT_SECRET`…) |
| [02-server.yaml](../../deploy/k8s/02-server.yaml) | Deployment server (3×) + Service + **Service dedicado `-ws`** (R3) + PDB + securityContext |
| [03-web.yaml](../../deploy/k8s/03-web.yaml) | Deployment web (3×) + Service |
| [04-ingress.yaml](../../deploy/k8s/04-ingress.yaml) | Ingress `/api /rtc` + Ingress `/ws` com `upstream-hash-by:$arg_room` (R3) |
| [21-server-hpa.yaml](../../deploy/k8s/21-server-hpa.yaml) | HPA **opt-in** (2–8, CPU 70%) — só com afinidade garantida |
| [50-data.yaml](../../deploy/k8s/50-data.yaml) | PVCs / dados |
| [51-coturn.yaml](../../deploy/k8s/51-coturn.yaml) | **coturn Deployment + Service LoadBalancer** (media, R4) |
| [05-cert-manager.yaml](../../deploy/k8s/05-cert-manager.yaml) | ClusterIssuer Let's Encrypt (prod) |
| helm-values/ | Postgres-HA / Redis (prod) e single/standalone (stage) |

> ⚠️ **Não recriar um "Set B"** de manifestos. Os antigos duplicados (namespace `delonix`) foram
> eliminados por causarem drift (config ia para lá e nunca chegava ao cluster). Editar sempre
> `01-config.yaml` + `51-coturn.yaml` — ver [regressions.md R4 (deploy-path)](../reference/regressions.md).

### 5.2 Stage local (kind) — `make stage`

Idempotente. Cria o cluster kind `delonix-stage`, instala ingress-nginx + MetalLB (auto-deteta a
subnet docker `kind` → pool `.200–.250`), pré-carrega imagens Bitnami no kind (evita
ImagePullBackOff offline), sobe Postgres single + Redis standalone via Helm, aplica os manifestos
da app + coturn, e atualiza `/etc/hosts` com o VIP → `meet.delonix.local`.

```bash
make image-push   # build server+web + kind load (necessário antes)
make stage        # cluster + infra + app + coturn + /etc/hosts
kubectl get po -n delonix-meet
```
Redeploy só das imagens: `make image-push` (faz `rollout restart` automático).

### 5.3 Prod bare-metal / cloud — `make prod`

1. **Ansible** ([deploy/ansible/](../../deploy/ansible/)) provisiona o cluster kubeadm.
   Ajustar [inventory.ini](../../deploy/ansible/inventory.ini): `metallb_ip_range` e hosts worker
   (`[kube_node]`) com IPs reais.
2. **cert-manager** + ClusterIssuer Let's Encrypt (certificados automáticos para o `$DOMAIN`).
3. **Helm**: `postgresql-ha` (repmgr, primário-secundário) + `redis` (Sentinel).
4. Manifestos da app + coturn; ingress com o domínio real (`make prod DOMAIN=meet.example.com`).
5. **Produção real de media:** trocar o VIP metallb pelo **LB de cloud com IP público** em
   `01-config.yaml` (`TURN_HOST`) e `51-coturn.yaml`. Resto igual.

---

## 6. Escalar & afinidade por sala (crítico)

O SFU é in-memory por pod. Escalar horizontalmente **exige** que todos os pares de uma sala
caiam no mesmo pod:

- **Como:** cliente envia `/ws?...&room=CODE`; o Ingress `/ws` usa `upstream-hash-by:$arg_room`
  num **Service DEDICADO** `delonix-server-ws`. Se `/ws` partilhar Service com `/api`/`/rtc`, o
  ingress-nginx funde os backends e **descarta** o hash → round-robin → media num só sentido,
  admissão e screen-share falham. Ver [ADR-0001](../adr/0001-room-shard-affinity.md) + R3.
- **Verificar:** `curl .../ws?room=X` repetido → sempre o mesmo pod.
  Fitness function automática: `bash scripts/check-room-affinity.sh` (corre em `make fitness`/`make test`).
- **HPA** ([21-server-hpa.yaml](../../deploy/k8s/21-server-hpa.yaml)): **opt-in**, só depois da
  afinidade garantida. `stabilizationWindowSeconds:300` no scale-down evita cortar reuniões a meio.
- **`/rtc` (presença)** é fanned por Redis → **não** precisa de afinidade.

---

## 7. Observabilidade (SRE)

- **Métricas Prometheus:** `GET /metrics` (série `delonix_*`): WS de sinalização/presença (gauges),
  PCs SFU ligados, publicações/peers (counters). Módulo [server/src/metrics.rs](../../server/src/metrics.rs).
  Scrape via anotações de pod (já em [02-server.yaml](../../deploy/k8s/02-server.yaml)).
  ```bash
  kubectl -n delonix-meet port-forward deploy/delonix-server 9292:8180
  curl -s localhost:9292/metrics | grep '^delonix_'
  ```
- **Health:** `GET /health` (200 = vivo) e `GET /api/status` (`{"status":"ok"}`, público).
- **Logs:** k8s → `kubectl logs -l app=delonix-server -n delonix-meet -f`.
  Bare-metal → `journalctl --user -u delonix-server -f`.
- **Golden signals a alarmar:**

  | Sinal | Fonte | Sintoma se mau |
  |---|---|---|
  | `delonix_sfu_peer_connections` cai a 0 com salas ativas | /metrics | SFU/afinidade partida |
  | coturn `peer usage rb=0` | logs coturn | media preta (R4) |
  | latência/erros 5xx no ingress | ingress-nginx | backend saturado |
  | disco `RECORDINGS_DIR` > 80% | node exporter | gravações a encher o disco |
  | replicação Postgres em atraso | postgres-ha | risco no failover |

### SLO sugeridos (ponto de partida)
| SLO | Alvo |
|---|---|
| Disponibilidade da app (`/api/status`) | 99.9% mensal |
| Estabelecimento de media (ICE connected < 5s) | 99% das entradas |
| Perda de pacotes no relay (turnutils) | < 1% |

---

## 8. Backup, restore & retenção

- **Postgres** (dados): `pg_dump` diário. K8s: alvo `make destroy` já faz dump antes de destruir;
  `make restore` reconstrói e restaura. Bare-metal:
  ```bash
  pg_dump "$DATABASE_URL" | gzip > /var/backups/delonix-$(date +%F).sql.gz
  ```
- **Redis:** estado efémero (presença/pubsub) — não é fonte de verdade. Backup opcional (`SAVE` + `dump.rdb`).
- **Gravações** (`RECORDINGS_DIR` / PVC): incluir no backup de ficheiros; aplicar **retenção por org**
  (consola admin) — há sweep automático em background.
- **Migrações são aditivas** e correm no arranque do servidor. Reverter esquema exige cuidado manual.

---

## 9. Runbooks de incidente (top ocorrências)

> A causa raiz de cada uma está catalogada em [regressions.md](../reference/regressions.md). Não
> reintroduzir a "correção óbvia" que já quebrou antes.

| Sintoma | Diagnóstico rápido | Ação | Ref |
|---|---|---|---|
| Liga mas **vídeo preto** | `logs -l app=coturn \| grep 'peer usage'` → `rb=0`; `turnutils_uclient` > 0% perda | Confirmar coturn in-cluster+LB, `allowed-peer-ip=VIP`, `FORCE_TURN_RELAY=1`; VIP alcançável do cliente | R4 |
| **Media num só sentido** / share falha (multi-pod) | `curl .../ws?room=X` cai em pods diferentes | Repor Service `-ws` dedicado + `upstream-hash-by:$arg_room` | R3 |
| **Reload em loop** ao admitir convidado | flood WS logo após admitir | Convidado não monta SFU em espera (`makeCallHolderStart`) | R2 |
| **Sem vídeo** ao entrar, sem `track published` | logs sem `pc connected` | Oferta SFU no construtor da `SfuCall`, não gateada por `joined` | R1 |
| Backend **panic no arranque** | `status`/logs: falta segredo | Preencher `JWT_SECRET`/`TURN_SECRET`/`DATABASE_URL` fortes | §4 |
| Host **cai durante rajada ICE** | rate-limit cortou o próprio host | Manter token bucket (600/300 `/ws`) — não voltar a janela fixa | R6 |
| `make stage` falha no build web | falta `web/dist` / lê certos no build | `.dockerignore` não exclui `web/dist`; vite lê certos só no `serve` | R8 |
| Migração nova não aplica | binário velho a correr | `touch server/src/main.rs` + `cargo build --release` antes de restart | R9 |

**Comandos de triagem rápidos:**
```bash
kubectl -n delonix-meet get po,svc,ingress        # estado geral
kubectl -n delonix-meet rollout status deploy/delonix-server
kubectl -n delonix-meet logs -l app=delonix-server --tail=100
bash scripts/check-tenant-rls.sh                  # isolamento RLS intacto?
bash scripts/check-room-affinity.sh               # afinidade /ws intacta?
```

---

## 10. Segurança & conformidade (checklist de operação)

Garantido em código (não desligar): segredos fail-closed, isolamento multi-tenant + **RLS backstop**
([ADR-0002](../adr/0002-tenant-isolation-rls.md)), room tokens de 5 min, SSRF nos webhooks,
rate-limit token-bucket, cookie de refresh `Secure` HttpOnly, E2EE por sala (key delegation p/ gravação).

Antes do go-live, correr a checklist de [DEPLOYMENT.md §Checklist de segurança](../../DEPLOYMENT.md).
Verificação de RLS ativa em qualquer ambiente: `bash scripts/check-tenant-rls.sh`.

Detalhe de compliance (BNA/LGPD, eDiscovery, DLP, SCIM, audit) →
[docs/reference/architecture.md](../reference/architecture.md) e o agente `delonix-security-compliance`.

---

## 11. CI / fitness functions

`make test` corre, além dos testes, as **fitness functions de arquitetura** (Fowler) que falham o
build se a plataforma driftar:

| Script | Garante |
|---|---|
| `scripts/check-docs-drift.sh` | docs de referência coerentes com o código |
| `scripts/check-room-affinity.sh` | Service `-ws` + `upstream-hash-by` intactos (R3) |
| `scripts/check-tenant-rls.sh` | RLS `ENABLE`+`FORCE` nas tabelas cobertas (ADR-0002) |

Wire num pipeline CI: `make build && make test` (inclui `cargo test`, `tsc`, `vitest`, fitness).

---

## 12. Índice de documentação complementar

| Documento | Conteúdo |
|---|---|
| [README.md](../../README.md) | Arranque rápido, estrutura, dev, portas |
| [ARCHITECTURE.md](../../ARCHITECTURE.md) | Desenho do sistema, fases, monorepo |
| [DEPLOYMENT.md](../../DEPLOYMENT.md) | Deploy single-host bare-metal (systemd+nginx), go-live |
| [deploy/k8s/README.md](../../deploy/k8s/README.md) | Manifestos K8s, HA de estado |
| [docs/reference/architecture.md](../reference/architecture.md) | Referência estável de arquitetura |
| [docs/reference/regressions.md](../reference/regressions.md) | R1–R12 — regressões a não reintroduzir |
| [docs/reference/api-contract.md](../reference/api-contract.md) | Fronteira de API (`/api/v1` público) |
| [docs/adr/0001-room-shard-affinity.md](../adr/0001-room-shard-affinity.md) | Afinidade por sala (SFU shard) |
| [docs/adr/0002-tenant-isolation-rls.md](../adr/0002-tenant-isolation-rls.md) | Isolamento multi-tenant via RLS |
| [docs/multi-region-scaling.md](../multi-region-scaling.md) | Escala multi-região |
| [docs/pstn-bridge-architecture.md](../pstn-bridge-architecture.md) | PSTN/dial-in (FreeSWITCH/Kamailio) |
| [docs/competitive-positioning.md](../competitive-positioning.md) | Posicionamento vs Zoom/Teams/Meet |

---

*Mantido em sincronia com o código pelas fitness functions (`make fitness`). Ao mudar deploy/media,
atualiza este runbook E [regressions.md](../reference/regressions.md).*
