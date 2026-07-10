# Delonix Meet — Guia de Deploy de Produção

Guia passo-a-passo para pôr o Delonix Meet em produção com o modelo de
segurança endurecido (auditoria Fases 0/1/2). Segue a ordem tal como está.

> **TL;DR** — depois da primeira configuração (segredos + TLS + nginx + systemd),
> cada deploy resume-se a: `bash deploy/deploy.sh`.

---

## Arquitetura de produção

```
                     HTTPS 443                    proxy loopback
   Browser  ───────────────────────►  Nginx  ──────────────────►  delonix-server (Rust)
   (câmara/mic exigem                 (TLS, CSP,   /api  /ws  /rtc      :8180  (systemd)
    contexto seguro)                  headers,           │                 │
                                      /var/www/delonix)   │                 ▼
                                                          │        Postgres · Redis  (docker)
   Media WebRTC (SFU/relay) ─────────────────────────────┴────►  coturn (TURN/STUN 3478)
```

- **Nginx** (serviço de sistema) termina TLS, serve a SPA compilada de
  `/var/www/delonix` e faz reverse-proxy de `/api`, `/ws`, `/rtc` para o backend.
- **delonix-server** (systemd `--user`) — API + sinalização + SFU, escuta em `127.0.0.1:8180`.
- **delonix-infra** (systemd `--user`) — Postgres, Redis, coturn via Docker Compose.
- As **migrações** correm automaticamente no arranque do servidor.

---

## Pré-requisitos

| Componente | Versão | Notas |
|---|---|---|
| Rust | 1.80+ | `cargo build --release` |
| Node | 20+ (aqui v25) | build da SPA |
| Docker + Compose | recente | Postgres/Redis/coturn |
| Nginx | 1.18+ | TLS + proxy |
| Um domínio + DNS | — | ex.: `meet.example.com` a apontar para o servidor |
| Certificado TLS | — | Let's Encrypt (público) ou mkcert (rede interna) |

---

## Passo 1 — Segredos (fail-closed)

O servidor **faz panic no arranque** se os segredos estiverem ausentes ou
forem os defaults de dev. Nunca usar `DELONIX_ALLOW_INSECURE=1` em produção.

```bash
sudo install -d -m 750 /etc/delonix
sudo cp deploy/delonix.env.example /etc/delonix/delonix.env
sudo chmod 600 /etc/delonix/delonix.env

# Gerar segredos fortes:
openssl rand -hex 48   # → JWT_SECRET   (>= 32 bytes)
openssl rand -hex 32   # → TURN_SECRET  (igual ao coturn)
openssl rand -hex 24   # → password do Postgres
```

Editar `/etc/delonix/delonix.env` e preencher `DATABASE_URL`, `JWT_SECRET`,
`TURN_HOST`, `TURN_SECRET`, `RECORDINGS_DIR`. Deixar `CORS_ORIGINS` vazio
(app e API são same-origin via nginx). Não definir `COOKIE_INSECURE`
(produção é HTTPS → o cookie de refresh precisa de `Secure`).

---

## Passo 2 — Infraestrutura (Postgres, Redis, coturn)

Exportar os segredos para o Compose e subir a infra endurecida:

```bash
set -a; source /etc/delonix/delonix.env
export POSTGRES_PASSWORD='<a mesma que puseste no DATABASE_URL>'
export TURN_REALM='meet.example.com'
set +a

docker compose -f docker-compose.yml -f deploy/docker-compose.prod.yml up -d --wait
```

O override de produção ([deploy/docker-compose.prod.yml](deploy/docker-compose.prod.yml)):
- lê a password do Postgres e o segredo do coturn do ambiente;
- publica Postgres/Redis **só no loopback** (`127.0.0.1`).

> **Clientes atrás de firewall corporativa**: ativar TURN sobre TLS (porta 5349)
> no coturn com um certificado válido. Sem isto, alguns participantes em redes
> restritas não conseguem media relay.

**Backups** — agendar `pg_dump` diário e cópia do diretório de gravações:
```bash
pg_dump "$DATABASE_URL" | gzip > /var/backups/delonix-$(date +%F).sql.gz
```

---

## Passo 3 — TLS

**Público (Let's Encrypt):**
```bash
sudo certbot certonly --nginx -d meet.example.com
# certs em /etc/letsencrypt/live/meet.example.com/
```

**Rede interna (mkcert):**
```bash
mkcert -install
IPS=$(hostname -I | tr ' ' '\n' | grep -v ':' | grep -v '^$')
mkcert -cert-file delonix.crt -key-file delonix.key meet.example.com localhost 127.0.0.1 $IPS
sudo install -d /etc/ssl/delonix
sudo cp delonix.crt /etc/ssl/delonix/fullchain.pem
sudo cp delonix.key /etc/ssl/delonix/privkey.pem
# Noutros dispositivos, instalar o CA: $(mkcert -CAROOT)/rootCA.pem
```

---

## Passo 4 — Nginx

O ficheiro [deploy/nginx-delonix.conf](deploy/nginx-delonix.conf) já traz:
cabeçalhos de segurança (**HSTS, CSP com `worker-src blob:`, X-Frame-Options,
nosniff, Referrer-Policy, Permissions-Policy**), `client_max_body_size` por rota
(1 MB global, 512 MB só no upload de gravações), upgrade de WebSocket e
`access_log off` nas rotas `/ws|/rtc` (evita gravar os JWT que vão em `?token=`).

```bash
sudo cp deploy/nginx-delonix.conf /etc/nginx/sites-available/delonix.conf
# Ajustar: server_name, caminhos dos certificados (ssl_certificate*).
sudo ln -sf /etc/nginx/sites-available/delonix.conf /etc/nginx/sites-enabled/delonix.conf
sudo nginx -t && sudo systemctl reload nginx
```

---

## Passo 5 — systemd (backend)

O backend corre como serviço systemd `--user`. Para produção, referenciar o
ficheiro de segredos e **remover** a flag de dev.

Editar `~/.config/systemd/user/delonix-server.service`:
```ini
[Service]
WorkingDirectory=/home/walter/workspaces/delonix-meet/server
EnvironmentFile=/etc/delonix/delonix.env      # ← segredos de produção
# REMOVER esta linha em produção:
# Environment=DELONIX_ALLOW_INSECURE=1
ExecStart=/home/walter/workspaces/delonix-meet/server/target/release/delonix-server
Restart=on-failure
RestartSec=5
```

```bash
systemctl --user daemon-reload
systemctl --user enable --now delonix-infra delonix-server
loginctl enable-linger "$USER"     # arranca no boot sem sessão iniciada
```

---

## Passo 6 — Deploy

Com tudo configurado, o deploy é um comando. O script faz **pré-voo**
(valida segredos não-default), build+test, publica a SPA, reinicia o servidor
e corre smoke tests:

```bash
bash deploy/deploy.sh
# opções: --skip-tests  (build sem testes)   --skip-build (só reinicia/smoke)
```

Passos internos: `0) pré-voo → 1) infra → 2) backend (test+build) →
3) migrações → 4) frontend (build+publish) → 5) restart → 6) smoke`.

---

## Passo 7 — Verificação

```bash
# Cabeçalhos de segurança presentes:
curl -sI https://meet.example.com/ | grep -iE 'strict-transport|content-security|x-frame|x-content-type|referrer'

# Fail-closed dos segredos (deve estar a correr; se paniquiou, faltam segredos):
systemctl --user status delonix-server

# Fluxo E2E manual: abrir https://meet.example.com/ → criar organização →
# iniciar reunião → 2º participante entra pelo link → câmara/mic OK.
```

---

## Checklist de segurança (auditoria Fases 0/1/2)

- [ ] `JWT_SECRET`/`TURN_SECRET`/`DATABASE_URL` fortes; `DELONIX_ALLOW_INSECURE` **ausente**.
- [ ] `TURN_SECRET` do backend == `--static-auth-secret` do coturn.
- [ ] Nginx com CSP/HSTS/X-Frame-Options/nosniff/Referrer-Policy; `nginx -t` OK.
- [ ] Cookie de refresh `Secure` (site em HTTPS); `COOKIE_INSECURE` não definido.
- [ ] Postgres/Redis só no loopback; password do Postgres trocada.
- [ ] `client_max_body_size` grande só na rota de upload de gravações.
- [ ] `access_log off` em `/ws|/rtc` (não gravar tokens).
- [ ] Backups de Postgres + `RECORDINGS_DIR` agendados.
- [ ] Retenção de gravações configurada por org (consola admin), se aplicável.

Já garantido em código: isolamento multi-tenant (join/pesquisa/presença/chamadas/
analytics escopados à org), SSRF bloqueada nos webhooks, rate-limit (por conta no
login, por IP com XFF, WS anti-flood), limites de corpo por-rota, chaves de API 256-bit.

---

## Operação

- **Logs**: `journalctl --user -u delonix-server -f` · Nginx em `/var/log/nginx/`.
- **Após alterar código Rust**: `bash deploy/deploy.sh` (rebuild + restart).
- **Só frontend**: `bash deploy/publish-web.sh`.
- **Rollback**: manter a build anterior (`target/release` + `dist/`); `git checkout <tag>`
  e correr `deploy.sh`. Migrações são aditivas — reverter esquema exige cuidado manual.
- **Rotação de segredos**: trocar `JWT_SECRET` invalida todas as sessões (todos
  re-login). Trocar `TURN_SECRET` exige atualizar o coturn em simultâneo.

---

## Go-live checklist — sexta-feira

**Véspera (quinta):**
1. [ ] DNS de `meet.example.com` a apontar para o servidor.
2. [ ] Certificado TLS emitido e testado (`curl` sem erro de cert).
3. [ ] `/etc/delonix/delonix.env` preenchido com segredos fortes (Passo 1).
4. [ ] Infra a correr e healthy; coturn com o segredo de produção (Passo 2).
5. [ ] `nginx -t` OK e a servir a SPA (Passo 4).
6. [ ] `systemctl --user enable` + `loginctl enable-linger` (arranca no boot).
7. [ ] `bash deploy/deploy.sh` verde de ponta a ponta.
8. [ ] Smoke test manual: criar org, reunião com 2 dispositivos, gravação, quadro.
9. [ ] Backup inicial do Postgres tirado.

**Sexta (go-live):**
10. [ ] `bash deploy/deploy.sh` final na branch/tag de release.
11. [ ] Verificação (Passo 7) + checklist de segurança tudo ✓.
12. [ ] Monitorizar `journalctl --user -u delonix-server -f` na primeira hora.
13. [ ] Plano de rollback à mão (build anterior + tag git).
