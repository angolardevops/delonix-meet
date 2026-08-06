# Delonix Meet — Guia de Deployment

Guia único para pôr o Delonix Meet a correr, do portátil ao cluster. Escolhe o
**cenário**, segue os passos dessa secção, e valida com a [checklist final](#8-verificação-e-go-live).

> **Já em produção e só queres publicar código novo?**
> `bash deploy/deploy.sh` (single-host) ou `make image-push && make pin` (Kubernetes).
> Salta para [§9 Operação](#9-operação).

**Índice**

1. [Escolher o cenário](#1-escolher-o-cenário)
2. [Pré-requisitos](#2-pré-requisitos)
3. [Configuração — todas as variáveis](#3-configuração--todas-as-variáveis)
4. [A rede de media (a parte que mais falha)](#4-a-rede-de-media-a-parte-que-mais-falha)
5. [Cenário A — Single-host](#5-cenário-a--single-host-systemd--nginx)
6. [Cenário B — Kubernetes](#6-cenário-b--kubernetes)
7. [Integração Odoo](#7-integração-odoo)
8. [Verificação e go-live](#8-verificação-e-go-live)
9. [Operação](#9-operação)
10. [Diagnóstico de avarias](#10-diagnóstico-de-avarias)

---

## 1. Escolher o cenário

| Cenário | Quando | Comando de entrada | Detalhe |
|---|---|---|---|
| **Zero-touch** | Primeira vez, 1 ou N hosts. IP/DNS/TLS/segredos automáticos | `make deploy-config` → `make deploy` | [zero-touch-deploy.md](ops/zero-touch-deploy.md) |
| **Single-host** | 1 servidor com IP público, controlo total | `bash deploy/deploy.sh` | [§5](#5-cenário-a--single-host-systemd--nginx) |
| **Docker Compose** | Tudo em contentores, 1 host | `docker compose -f docker-compose.yml -f deploy/docker-compose.prod.yml up -d` | [runbook §1.2](ops/platform-engineering.md) |
| **Kubernetes** | HA, zero-downtime, escala | `make prod DOMAIN=meet.example.com` | [§6](#6-cenário-b--kubernetes) |
| **Stage local (kind)** | Demo/QA antes de produção | `make image-push && make stage` | [runbook §5.2](ops/platform-engineering.md) |
| **Demo com Odoo** | Apresentação da integração | `bash deploy/demo-kaeso.sh up` | [§7](#7-integração-odoo) |

Se hesitas: **zero-touch**. Faz o que as secções 5 e 6 fazem à mão.

### O que é preciso saber sobre a arquitectura

```
                     HTTPS 443                      proxy loopback
   Browser  ───────────────────────►  Nginx  ────────────────────►  delonix-server
   (câmara/mic exigem                 TLS, CSP,   /api  /ws  /rtc        :8180
    contexto seguro)                  SPA estática      │                  │
                                                        │                  ▼
   Media WebRTC (SRTP/DTLS) ───────────────────────────┴──► coturn   Postgres · Redis
```

Um só executável serve API, autenticação, sinalização, SFU e gravação. As
migrações correm sozinhas no arranque. O estado que importa vive em três
sítios: **Postgres** (tudo), **disco de gravações**, e **Redis** (efémero:
presença e pub/sub entre nós).

---

## 2. Pré-requisitos

| Componente | Versão | Para quê |
|---|---|---|
| Rust | 1.80+ | compilar o backend |
| Node | 20+ | compilar a SPA |
| Docker/Podman ou `delonix` | recente | Postgres, Redis, coturn |
| Nginx | 1.18+ | TLS + proxy + SPA |
| Domínio + DNS | — | ex. `meet.example.com` → IP do servidor |
| Certificado TLS | — | Let's Encrypt (público) ou mkcert (interno) |

**Dimensionamento** (ponto de partida, ver [runbook](ops/platform-engineering.md) para
o cálculo a sério): 4 vCPU / 8 GB serve ~50 participantes em simultâneo. O SFU é
CPU-bound no fan-out RTP; a gravação server-side acrescenta ffmpeg por sala.

> **Contexto seguro é obrigatório.** `getUserMedia` só funciona em HTTPS ou em
> `localhost`. Não há como servir isto em HTTP por IP e ter câmara.

---

## 3. Configuração — todas as variáveis

O servidor é **fail-closed**: sem segredos fortes faz *panic* no arranque, em vez
de subir inseguro. `DELONIX_ALLOW_INSECURE=1` aceita os defaults de dev e **nunca**
deve existir em produção.

```bash
sudo install -d -m 750 /etc/delonix
sudo cp deploy/delonix.env.example /etc/delonix/delonix.env
sudo chmod 600 /etc/delonix/delonix.env

openssl rand -hex 48   # → JWT_SECRET  (mínimo 32 bytes)
openssl rand -hex 32   # → TURN_SECRET (igual ao --static-auth-secret do coturn)
openssl rand -hex 24   # → password do Postgres
```

### Obrigatórias

| Variável | Descrição |
|---|---|
| `DATABASE_URL` | `postgres://delonix:<password>@localhost:5435/delonix_meet` |
| `JWT_SECRET` | ≥ 32 bytes. **Trocá-la invalida todas as sessões.** |
| `TURN_HOST` | `turn.meet.example.com:3478` — host **alcançável pelos clientes** |
| `TURN_SECRET` | ≥ 16 bytes, **idêntico** ao `--static-auth-secret` do coturn |

### Rede e serviço

| Variável | Default | Descrição |
|---|---|---|
| `BIND_ADDR` | `0.0.0.0:8180` | em single-host usar `127.0.0.1:8180` (só o nginx entra) |
| `CORS_ORIGINS` | vazio | vazio = same-origin. Só preencher se a SPA vier de outra origem |
| `COOKIE_INSECURE` | — | `1` só em HTTP puro. Em produção **não definir** |
| `RECORDINGS_DIR` | `recordings` | disco com espaço; entra nos backups |
| `REDIS_URL` | — | sem isto = single-node. Obrigatório para multi-réplica |
| `SFU_EXTERNAL_IP` | — | IP que o SFU anuncia nos candidatos ICE (NAT 1:1) |
| `FORCE_TURN_RELAY` | — | `1` força relay-only. **Necessário em Kubernetes** ([§4](#4-a-rede-de-media-a-parte-que-mais-falha)) |

### Integrações

| Variável | Descrição |
|---|---|
| `PROVISIONING_SECRET` | Autoriza `POST /api/v1/admin/orgs`. Vazio = endpoint desligado |
| `PLATFORM_ODOO_URL` / `PLATFORM_ODOO_DB` | Login com conta Odoo ([§7](#7-integração-odoo)). Vazias = desligado |
| `WEBHOOK_ALLOW_HOSTS` | Hosts isentos da guarda anti-SSRF dos webhooks, por nome exacto. Necessário para um Odoo on-prem em rede privada |
| `OLLAMA_URL` | LLM local para atas e legendas. Vazio = MoM por regras (fail-open) |
| `OLLAMA_MODEL_SUMMARY` / `OLLAMA_MODEL_TRANSLATE` | modelos (ex. `qwen2.5:7b` / `qwen2.5:1.5b`) |
| `VOICE_INTERNAL_SECRET` | API interna de IVR (PSTN). Vazio = desligada |

---

## 4. A rede de media (a parte que mais falha)

Quase todas as avarias de "entrou mas não vejo ninguém" são aqui. Duas regras:

**1. O `TURN_HOST` tem de ser alcançável pelo cliente, não pelo servidor.** Um
`localhost:3478` funciona nos testes do próprio host e falha para todos os outros.

**2. Portas UDP abertas.** O relay do coturn usa uma gama (`--min-port`/`--max-port`,
por omissão 49160–49200 no compose de dev). Fechada a gama, o ICE liga e a imagem
fica preta.

| Porta | Protocolo | Para quê |
|---|---|---|
| 443 | TCP | HTTPS + WebSocket |
| 3478 | UDP+TCP | STUN/TURN |
| 5349 | TCP | TURN sobre TLS — **necessário atrás de firewall corporativa** |
| 49160–49200 | UDP | relay de media (ajustar à concorrência esperada) |

**Em Kubernetes, `FORCE_TURN_RELAY=1` não é opcional:** o IP do pod (`10.244.x`) é
inalcançável de fora e os *host candidates* do SFU não transportam media. Sem
relay-only o ICE "liga" por um par que passa o check e fica preto.

**Multi-réplica exige afinidade por sala.** O SFU é in-memory por pod: todos os
participantes de uma sala têm de cair no **mesmo** pod. O cliente envia
`/ws?...&room=CODE` e o ingress faz `upstream-hash-by: $arg_room` — e o `/ws`
precisa de um **Service dedicado**, senão o ingress funde os backends e descarta
o hash. Ver [ADR-0001](adr/0001-room-shard-affinity.md).

---

## 5. Cenário A — Single-host (systemd + nginx)

### 5.1 Infraestrutura

```bash
set -a; source /etc/delonix/delonix.env
export POSTGRES_PASSWORD='<a mesma do DATABASE_URL>'
export TURN_REALM='meet.example.com'
set +a

docker compose -f docker-compose.yml -f deploy/docker-compose.prod.yml up -d --wait
```

O override de produção publica Postgres e Redis **só no loopback** e lê os
segredos do ambiente.

### 5.2 TLS

```bash
# Público
sudo certbot certonly --nginx -d meet.example.com

# Rede interna
mkcert -install
mkcert -cert-file delonix.crt -key-file delonix.key \
       meet.example.com localhost 127.0.0.1 $(hostname -I)
sudo install -d /etc/ssl/delonix
sudo cp delonix.crt /etc/ssl/delonix/fullchain.pem
sudo cp delonix.key /etc/ssl/delonix/privkey.pem
# Nos outros dispositivos, instalar o CA: $(mkcert -CAROOT)/rootCA.pem
```

### 5.3 Nginx

[`deploy/nginx-delonix.conf`](../deploy/nginx-delonix.conf) já traz HSTS, CSP (com
`worker-src blob:`, que o E2EE precisa), `client_max_body_size` por rota (1 MB
global, 512 MB só no upload de gravações), upgrade de WebSocket e **`access_log off`
em `/ws|/rtc`** — sem isso, os JWT que viajam em `?token=` ficavam no log.

```bash
sudo cp deploy/nginx-delonix.conf /etc/nginx/sites-available/delonix.conf
# Ajustar: server_name e ssl_certificate*
sudo ln -sf /etc/nginx/sites-available/delonix.conf /etc/nginx/sites-enabled/
sudo nginx -t && sudo systemctl reload nginx
```

### 5.4 Serviço + deploy

O backend corre em systemd `--user`, com `EnvironmentFile=/etc/delonix/delonix.env`
e **sem** `DELONIX_ALLOW_INSECURE`.

```bash
systemctl --user enable delonix-server
loginctl enable-linger "$USER"    # arranca no boot, sem sessão iniciada
bash deploy/deploy.sh             # build + migrações + restart + smoke
```

Opções úteis: `--skip-tests` (build sem testes), `--skip-build` (só reinicia).

---

## 6. Cenário B — Kubernetes

```bash
make prod DOMAIN=meet.example.com     # Ansible + Helm + manifestos + Let's Encrypt
```

Publicar código novo:

```bash
make image-push                        # build versionado + load + pin da tag
```

> **Nunca confiar no `:latest`.** `make image-push` gera tags versionadas
> (`git describe`) e fixa-as nos Deployments. Um `kubectl apply -f 02-server.yaml`
> avulso repõe `:latest` — que pode estar obsoleto no nó — e reintroduz a versão
> antiga em silêncio. A seguir a um apply manual, correr sempre `make pin`.

Pontos que exigem atenção: `FORCE_TURN_RELAY=1`, o Service dedicado para `/ws`
com afinidade por sala ([§4](#4-a-rede-de-media-a-parte-que-mais-falha)), e
**RWX** para o PVC de gravações em multi-nó (RWO só funciona em nó único).
Detalhe em [deploy/k8s/README.md](../deploy/k8s/README.md) e no
[runbook §5.3](ops/platform-engineering.md).

---

## 7. Integração Odoo

Duas capacidades independentes. Podes ligar uma sem a outra.

### 7.1 Login com conta Odoo

Quem tem conta no Odoo entra no Meet com as **mesmas credenciais**. No primeiro
login: a organização é criada a partir da **empresa** do utilizador, ele entra
como membro (admin, se for admin no Odoo), e **todos os utilizadores internos
activos** dessa empresa são sincronizados em segundo plano.

```bash
PLATFORM_ODOO_URL=https://erp.empresa.com
PLATFORM_ODOO_DB=empresa_prod
```

Reiniciar o servidor. É tudo — não há mais nada a configurar. A configuração
por-organização (URL, BD, `odoo_enabled`) fica preenchida pelo próprio login e é
depois editável em **Análises → Integração Odoo**.

É configuração de **plataforma** e não de organização porque, no primeiro login,
ainda não existe organização nenhuma onde a guardar — é precisamente a org que
está a nascer.

**Limites a conhecer antes de vender isto:**

- **Uma instância Odoo por plataforma.** Várias empresas com Odoos diferentes
  exigiriam resolver o Odoo pelo domínio de email — não implementado.
- **Qualquer conta válida nesse Odoo entra no Meet.** Desactivar o utilizador no
  Odoo corta-lhe o acesso no login seguinte (o Odoo é a fonte de verdade e uma
  password revogada é rejeitada mesmo com hash local válido), mas uma sessão já
  aberta dura até o token expirar.
- Se o Odoo estiver em baixo, quem **já** entrou uma vez continua a entrar pelo
  hash Argon2 em cache; quem nunca entrou, não.

### 7.2 Calendário: reuniões criadas a partir do Odoo

O módulo `nk_delonix_meet` cria a reunião no Delonix quando se gera o link de
videochamada num evento de calendário.

1. **No Delonix:** definir `PROVISIONING_SECRET` e reiniciar.
2. **No Odoo:** Definições → Delonix Meet → URL base + segredo de provisionamento
   → **Provisionar**. Isso cria a organização e guarda a chave `dlx_...`.
3. Se o Odoo estiver em rede privada e quiseres o webhook de aceleração das atas,
   acrescentar o host a `WEBHOOK_ALLOW_HOSTS`.

A API usada é `POST /api/v1/meetings` — que cria uma **reunião** (anfitrião +
convidados), não uma sala solta. A distinção importa: uma sala criada por
`/api/v1/rooms` fica com o utilizador de serviço como dono, e como esse nunca
faz login, **ninguém consegue ser admitido**. Ver
[api-contract.md](reference/api-contract.md).

### 7.3 Demo local

`bash deploy/demo-kaeso.sh up` sobe a stack completa (Postgres + servidor +
nginx) numa rede de contentores partilhada com o Odoo. O script imprime as URLs
e resolve sozinho o IP do upstream do nginx.

---

## 8. Verificação e go-live

```bash
# O serviço subiu? (se paniquiou, faltam segredos fortes)
systemctl --user status delonix-server

# Saúde e cabeçalhos de segurança
curl -sI https://meet.example.com/ | grep -iE 'strict-transport|content-security|x-frame'
curl -s  https://meet.example.com/api/status
```

**Fluxo E2E manual** — é o único que prova a media: abrir o site em **dois
dispositivos diferentes** (não duas abas), criar reunião num, entrar pelo link no
outro, confirmar vídeo **e** áudio nos dois sentidos, partilhar ecrã, gravar.

### Checklist de segurança

- [ ] `JWT_SECRET`/`TURN_SECRET`/`DATABASE_URL` fortes; `DELONIX_ALLOW_INSECURE` **ausente**
- [ ] `TURN_SECRET` do backend == `--static-auth-secret` do coturn
- [ ] `nginx -t` OK; HSTS/CSP/X-Frame-Options/nosniff/Referrer-Policy presentes
- [ ] Cookie de refresh `Secure` (site em HTTPS); `COOKIE_INSECURE` não definido
- [ ] Postgres/Redis só no loopback; password do Postgres trocada
- [ ] `client_max_body_size` grande **só** na rota de upload de gravações
- [ ] `access_log off` em `/ws|/rtc` (não gravar tokens)
- [ ] Backups de Postgres + `RECORDINGS_DIR` agendados **e testados a restaurar**
- [ ] Retenção de gravações configurada por org, se aplicável
- [ ] Se o login por Odoo está ligado: confirmado que só utilizadores esperados entram

Já garantido em código: isolamento multi-tenant em todos os endpoints + RLS
backstop fail-closed, SSRF bloqueada nos webhooks, rate-limit (por conta no login,
por IP com XFF, token-bucket no WS), limites de corpo por rota, chaves de API de
256 bits guardadas como SHA-256.

### Go-live

**Véspera:** DNS a apontar · TLS emitido e testado · `/etc/delonix/delonix.env`
completo · infra healthy · `nginx -t` OK · `enable-linger` activo · `deploy.sh`
verde · smoke test com 2 dispositivos · **backup inicial tirado**.

**No dia:** `deploy.sh` na tag de release · checklist de segurança toda ✓ ·
acompanhar `journalctl --user -u delonix-server -f` na primeira hora.

---

## 9. Operação

| Tarefa | Como |
|---|---|
| Logs | `journalctl --user -u delonix-server -f` · nginx em `/var/log/nginx/` |
| Métricas | `/metrics` (Prometheus). Restringir por NetworkPolicy/ingress interno |
| Publicar código | `bash deploy/deploy.sh` · K8s: `make image-push` |
| Migrações | Automáticas no arranque. Depois de uma migração nova, **rebuild antes do restart** |
| Backup | `pg_dump "$DATABASE_URL" \| gzip > /var/backups/delonix-$(date +%F).sql.gz` + `RECORDINGS_DIR` |
| Rollback | Guardar a build anterior (`target/release` + `dist/`); `git checkout <tag>` + `deploy.sh`. **As migrações são aditivas — reverter esquema é manual** |
| Rotação de `JWT_SECRET` | Invalida **todas** as sessões (toda a gente re-login) |
| Rotação de `TURN_SECRET` | Tem de mudar no backend **e** no coturn ao mesmo tempo |

---

## 10. Diagnóstico de avarias

| Sintoma | Causa provável | Verificação |
|---|---|---|
| Servidor não arranca, *panic* imediato | Segredo fraco/ausente (fail-closed) | `journalctl` diz qual variável |
| Câmara/mic não pedem permissão | Sem contexto seguro | Só HTTPS ou `localhost` |
| Entra na sala, imagem preta | Media não passa | `TURN_HOST` alcançável? UDP aberto? Em K8s, `FORCE_TURN_RELAY=1`? |
| Media só num sentido, multi-réplica | Sem afinidade por sala | `curl .../ws?room=X` repetido → sempre o mesmo pod? Service dedicado para `/ws`? |
| 502 no `/api` | Backend em baixo ou upstream errado | `curl 127.0.0.1:8180/health`; num container, confirmar o IP do upstream |
| Login por Odoo devolve 401 | Odoo inacessível, ou credenciais | Logs: `login por conta Odoo indisponível` distingue os dois casos |
| Botão "SSO empresarial" dá 404 | OIDC não configurado — **não é** o login por Odoo | O login por Odoo é o formulário email+password ([§7.1](#71-login-com-conta-odoo)) |
| Gravação corrompida | Codec não-VP8/Opus | O recorder exclui a track e regista `error!` |
| Estilos "perdidos" após deploy K8s | Imagem `:latest` obsoleta no nó | `make pin IMAGE_TAG=<tag>` |

Regressões conhecidas e o que **não** reintroduzir: [regressions.md](reference/regressions.md).

---

**Ver também:** [Runbook DevOps/SRE](ops/platform-engineering.md) (dimensionamento,
SLO, incidentes) · [Zero-touch](ops/zero-touch-deploy.md) · [Kubernetes](../deploy/k8s/README.md)
