# Delonix Meet — Deploy zero-touch (12-factor)

> **Um ficheiro, um comando.** Descreves o que queres em `deploy/config.yml` e o
> Ansible trata do resto — **IPs, DNS público, certificados TLS e segredos são
> gerados sem intervenção humana**, de forma idempotente (correr de novo não
> estraga nada). Serve single-host e multi-host com a mesma config.
>
> Complementa o [runbook de operação](platform-engineering.md) (dimensionamento,
> rede/media, SLO, incidentes). Aqui trata-se de **como fazer o deploy**.

---

## 1. Princípio (12-factor)

| Fator | Como o Delonix aplica |
|---|---|
| **III. Config** | Toda a config vive no ambiente. O servidor lê env e faz *fail-closed* sem segredos fortes (`server/src/config.rs`). Nada de config em código. |
| **Config declarativa** | `deploy/config.yml` é a **única** fonte que um humano edita. Tudo o resto deriva daí. |
| **Build/Release/Run** | O Ansible constrói (release), gera o *release* (env + certs + manifests) e corre — separadamente e de forma repetível. |
| **Descartável / idempotente** | Re-deploy converge para o mesmo estado. Segredos persistidos → sessões sobrevivem. |
| **Paridade dev/prod** | O mesmo `config.yml` descreve single-host e multi-host; muda só `deploy_mode`. |

O que é **gerado automaticamente** (nunca à mão):

- **Segredos** (`JWT_SECRET`, `TURN_SECRET`, password do Postgres, segredo de voz) — fortes,
  criados **uma vez** e persistidos em `deploy/ansible/.secrets/<escopo>/` (gitignored). Re-deploy
  reutiliza-os (trocar o JWT deslogava todos, por isso **não** se regenera).
- **IP** — detetado (single-host: IP de saída público; multi-host: lido do LoadBalancer MetalLB/cloud).
- **DNS público** — registo A criado/atualizado via Cloudflare/Route53 (ou `/etc/hosts` em dev).
- **TLS** — self-signed (interno) **ou** Let's Encrypt (público), escolhido automaticamente,
  com **auto-renovação**.

---

## 2. Arranque rápido

```bash
# 1. Ferramentas (uma vez): Ansible + coleções
ansible-galaxy collection install community.crypto community.general amazon.aws ansible.posix

# 2. Criar a config
make deploy-config            # copia deploy/config.example.yml → deploy/config.yml

# 3. Editar deploy/config.yml (só o essencial: deploy_mode, domain, tls, dns)

# 4. Deploy
make deploy                   # lê config.yml, decide single/multi, faz tudo
make deploy-check             # (opcional) dry-run sem alterar nada
```

`deploy/config.yml` está no `.gitignore` — nunca comitar valores reais.

---

## 3. Referência de `config.yml`

```yaml
deploy_mode: single-host      # single-host | multi-host
domain: "meet.example.com"    # vazio "" ⇒ serve por IP com self-signed
public_ip: auto               # auto (deteta) | <ip explícito>

tls:
  mode: auto                  # auto | selfsigned | letsencrypt
  acme_email: "ops@example.com"
  acme_staging: false         # true = ambiente de teste do LE

dns:
  provider: cloudflare        # none | hosts | cloudflare | route53
  cloudflare_zone: "example.com"
  route53_zone: "example.com."
  proxied: false              # cloudflare proxy (media UDP ⇒ manter false)

turn: { realm: "", min_port: 49152, max_port: 59152 }
secrets: { auto_generate: true }

kubernetes:                   # só multi-host
  metallb_ip_range: "192.168.1.200-192.168.1.250"
  coturn_ip: ""               # vazio ⇒ MetalLB atribui o VIP dinamicamente
  postgres_ha: true
  redis_sentinel: true

paths:                        # single-host (defaults sensatos)
  env_file: /etc/delonix/delonix.env
  web_root: /var/www/delonix
  recordings_dir: /var/lib/delonix/recordings
  tls_dir: /etc/ssl/delonix
  # repo_root: /opt/delonix-meet   # override em deploy remoto (repo no host)
```

---

## 4. TLS — como o modo é escolhido (`tls.mode: auto`)

| Situação | Resultado |
|---|---|
| `domain` vazio | **self-signed** (SAN = IP + localhost). Interno/dev. |
| `domain` + `acme_email` + DNS resolúvel | **Let's Encrypt** (auto-renova). |
| `mode: selfsigned` | força self-signed (nunca contacta o LE). |
| `mode: letsencrypt` | força ACME (exige domínio público + email). |

**Mecanismo por modo de deploy:**
- **single-host** → self-signed via `openssl` (community.crypto), ou **certbot**: HTTP-01 (webroot,
  o nginx serve o challenge) ou **DNS-01** se `dns.provider` for cloudflare/route53 (não precisa de
  porta 80 aberta — ideal atrás de firewall). Renovação pelo timer do certbot + reload do nginx.
- **multi-host** → **cert-manager** com `ClusterIssuer` Let's Encrypt (HTTP-01 via ingress),
  gerado com o teu email/staging. Emissão e renovação automáticas no cluster. Sem domínio →
  Secret TLS self-signed criado e referenciado pelo ingress.

---

## 5. DNS público — sem tocar no painel

| provider | O que faz | Requisitos |
|---|---|---|
| `none` | não mexe no DNS (geres externamente) | o registo A tem de já existir para o LE HTTP-01 |
| `hosts` | escreve `/etc/hosts` (dev/stage numa máquina) | — |
| `cloudflare` | cria/atualiza o registo A `domain → IP` | `dns.cloudflare_zone` + env `CLOUDFLARE_API_TOKEN` |
| `route53` | idem via Route53 | `dns.route53_zone` + `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY` |

Os tokens vivem no **ambiente** de quem corre `make deploy` (12-factor — nunca em `config.yml`):

```bash
export CLOUDFLARE_API_TOKEN=...     # scope: Zone.DNS Edit
make deploy
```

---

## 6. Fluxos por modo

### 6.1 Single-host (1 servidor)
`make deploy` (deploy_mode: single-host) faz, no host alvo:
1. **preflight** — valida config, deteta IP, decide TLS.
2. **secrets** — gera/reutiliza segredos (idempotente).
3. **dns** — cria o registo A (se provider).
4. **tls** — self-signed agora, ou prepara certbot.
5. **single_host** — infra (docker compose prod), render do `delonix.env` (12-factor), `make build`
   (backend release + SPA), publica a SPA, unit systemd, **certbot** (se LE), nginx com TLS, smoke `/health`.

Resultado: `https://<domain-ou-IP>` a servir, backend em systemd, infra em Docker.

### 6.2 Multi-host (Kubernetes HA)
Preenche `[kube_control_plane]`/`[kube_node]` em `deploy/ansible/inventory.ini` e `deploy_mode: multi-host`.
`make deploy`:
1. **preflight + secrets** (no controlador).
2. **k8s_infra** — kubeadm (todos os nós), Calico, join dos workers, ingress-nginx (LoadBalancer),
   MetalLB (pool do `config.yml`).
3. **dns** — resolve o IP do LoadBalancer e cria o registo A.
4. **k8s_app** — Helm Postgres(-HA)/Redis com a password gerada, coturn (Service → **resolve o VIP**
   → Deployment), Secret+ConfigMap gerados (`TURN_HOST`=VIP), **cert-manager** + ClusterIssuer (LE)
   ou Secret TLS self-signed, manifests server/web/dados e o **ingress dedicado `/ws`** (afinidade R3).

Resultado: `https://<domain>` no cluster, media via coturn LoadBalancer (R4), TLS automático.

---

## 7. Idempotência, re-deploy e rotação

- **Re-correr `make deploy`** é seguro: converge para o estado desejado. Certbot só emite se faltar
  (`--keep-until-expiring`); segredos são reutilizados; manifests re-aplicados.
- **Rodar um segredo:** apaga o ficheiro respetivo em `deploy/ansible/.secrets/<escopo>/` e re-deploy.
  (Apagar `jwt_secret` invalida todas as sessões → todos re-login.)
- **Trocar de domínio:** muda `domain` e re-deploy — novo escopo de segredos e novo certificado.

---

## 8. Pré-requisitos

- **Controlador** (onde corres `make deploy`): Ansible 2.15+ e as coleções `community.crypto`,
  `community.general`, `amazon.aws` (Route53), `ansible.posix`.
- **Host(s) alvo:** acesso `sudo`/root (`make deploy ANSIBLE_ARGS=--ask-become-pass` se pedir
  password). Single-host: Docker + Rust 1.80+ + Node 20+ (o build corre no host). Multi-host:
  Ubuntu/Debian nos nós (kubeadm), portas de rede abertas (ver [runbook §3.1](platform-engineering.md#31-matriz-de-portas--firewall)).
- **DNS/TLS público:** token do provider no ambiente + porta 80 aberta (HTTP-01) **ou** DNS-01.

---

## 9. Troubleshooting

| Sintoma | Causa provável | Ação |
|---|---|---|
| `deploy/config.yml não existe` | primeira vez | `make deploy-config`, edita, re-deploy |
| Let's Encrypt falha (rate limit) | testes repetidos | `tls.acme_staging: true` até validar, depois `false` |
| LE HTTP-01 falha | DNS ainda não aponta / porta 80 fechada | usar DNS-01 (`dns.provider`) ou abrir 80 + esperar propagação |
| Cloudflare/Route53 "unauthorized" | token/creds ausentes no ambiente | exportar `CLOUDFLARE_API_TOKEN` / `AWS_*` antes de `make deploy` |
| coturn VIP fica `<pending>` (multi) | `coturn_ip` fora do pool MetalLB | deixar `coturn_ip: ""` (dinâmico) ou pôr um IP do pool |
| Media preta em K8s | ver [regressions.md R4](../reference/regressions.md) | coturn in-cluster+LB, `FORCE_TURN_RELAY=1` |
| `sudo` pede password | become interativo | `make deploy ANSIBLE_ARGS=--ask-become-pass` |

Diagnóstico geral e runbooks de incidente: [platform-engineering.md §9](platform-engineering.md#9-runbooks-de-incidente-top-ocorrências).

---

## 10. Estrutura da automação

```
deploy/
├── config.example.yml         # o único ficheiro humano (copiar → config.yml)
└── ansible/
    ├── site.yml               # entrypoint (despacha por deploy_mode)
    ├── ansible.cfg · inventory.ini
    ├── group_vars/all.yml     # carrega config.yml + segredos idempotentes (lookup password)
    └── roles/
        ├── preflight/         # valida, deteta IP, decide TLS
        ├── secrets/           # perms + reporte (geração via lookup persistente)
        ├── tls/               # self-signed (crypto) | prepara certbot
        ├── dns/               # cloudflare | route53 | hosts
        ├── single_host/       # docker infra + env + build + systemd + nginx + certbot
        ├── k8s_infra/         # kubeadm + Calico + ingress + MetalLB (+ join workers)
        └── k8s_app/           # Helm + secrets/config + coturn VIP + cert-manager + manifests
```

*Ver também: [platform-engineering.md](platform-engineering.md) (operação), [DEPLOYMENT.md](../../DEPLOYMENT.md)
(single-host manual detalhado), [deploy/k8s/README.md](../../deploy/k8s/README.md) (manifestos).*
