# Delonix Meet sobre o `delonix-runtime` e o PaaS NgolaCloud

Manifestos das edições que **não** correm em Kubernetes (ADR-0005 §4). Para
Kubernetes: `deploy/k8s/` (base) e `deploy/k8s-overlays/{saas,enterprise}`.

| Ficheiro | Edição | Alvo | Aplica-se com |
|---|---|---|---|
| [`meet-stack.yaml`](meet-stack.yaml) | enterprise | um host com `delonix-runtime` | `delonix apply -f` |
| [`meet-stack-personal.yaml`](meet-stack-personal.yaml) | personal | um host com `delonix-runtime` | `delonix apply -f` |
| [`meet-application.yaml`](meet-application.yaml) | saas | PaaS NgolaCloud | `delonixctl apply -f` |

A regra do workspace (CLAUDE.md, §1): o Meet é uma carga **acima** do PaaS. No
PaaS entra pela API (`delonixctl apply -f`), nunca por Ansible nem por SSH ao
host. O Ansible do `delonix-deploy` só instala o substrato por baixo.

## Um host — `meet-stack.yaml` (enterprise) e `meet-stack-personal.yaml`

### 1. Segredos

Nenhum segredo vive nos manifestos. Cada `Secret` lê um ficheiro `KEY=value`
**da pasta do manifesto** (`fromEnvFile`), e esses ficheiros estão no
[`.gitignore`](.gitignore) desta pasta.

```bash
cd deploy/delonix
for f in meet-db meet-server meet-coturn; do
  cp "$f.secrets.env.example" "$f.secrets.env"; chmod 600 "$f.secrets.env"
done
$EDITOR *.secrets.env          # substituir TODOS os CHANGE_ME
grep -l CHANGE_ME *.secrets.env && echo "ainda há CHANGE_ME" >&2
```

| Ficheiro | Chaves | Tem de bater com |
|---|---|---|
| `meet-db.secrets.env` | `POSTGRES_USER`, `POSTGRES_PASSWORD`, `POSTGRES_DB` | a password do `DATABASE_URL` |
| `meet-server.secrets.env` | `DATABASE_URL`, `JWT_SECRET`, `TURN_SECRET`, `TURN_HOST`, `SFU_EXTERNAL_IP`, `PROVISIONING_SECRET`, `VOICE_INTERNAL_SECRET`, `PLATFORM_ADMIN_USER_IDS` | — |
| `meet-coturn.secrets.env` | `TURN_CONFIG=static-auth-secret=<segredo>` | o `TURN_SECRET` do servidor |

O coturn recebe o segredo como **ficheiro** (`secretFiles: true` →
`/run/secrets/TURN_CONFIG`, lido com `-c`), para não aparecer no `argv` do
processo. `TURN_HOST` e `SFU_EXTERNAL_IP` não são segredos, mas são deste host,
e ficam no mesmo ficheiro para haver um só sítio a preencher.

O servidor é fail-closed: com um `CHANGE_ME` num segredo, não arranca.

### 2. Imagens e aplicar

```bash
delonix build -f Dockerfile.server -t delonix-server:latest .
delonix build -f Dockerfile.web    -t delonix-web:latest    .   # só enterprise

delonix manifest validate -f deploy/delonix/meet-stack.yaml --strict
delonix plan              -f deploy/delonix/meet-stack.yaml
delonix apply             -f deploy/delonix/meet-stack.yaml
```

À frente do `127.0.0.1:8080` (enterprise) ou do `127.0.0.1:8180` (personal)
fica um proxy TLS do host ([docs/deployment.md §5.3](../../docs/deployment.md)):
o WebRTC exige HTTPS. Abrir no firewall 3478 (TCP/UDP), 49160-49200/UDP
(relay do coturn) e 50000-50200/UDP (SFU).

### 3. Diferenças entre as duas edições

| | enterprise | personal |
|---|---|---|
| `DELONIX_EDITION` | `enterprise` | `personal` |
| Registo / inquilinos | `invite` / `single` | defaults da edição |
| Redis | incluído, opcional (uma réplica não precisa) | não |
| UI | contentor `meet-web` (nginx) | o binário, `UI_DIR=/srv/ui` montado de `/srv/delonix-meet/ui` |
| Listener interno `:8181` | publicado em `127.0.0.1` (FreeSWITCH do host) | desligado |
| gRPC `:9180` | desligado | desligado |

**Ligar o gRPC** (ADR-0005 §3, mTLS obrigatório): colocar `tls.crt`, `tls.key`
e `ca.crt` numa pasta do host, montá-la só-de-leitura no `meet-server`
(`"/etc/delonix-meet/grpc-tls:/etc/delonix/grpc-tls:ro"`), e acrescentar ao
`env` `GRPC_BIND_ADDR=0.0.0.0:9180`, `GRPC_TLS_CERT=/etc/delonix/grpc-tls/tls.crt`,
`GRPC_TLS_KEY=/etc/delonix/grpc-tls/tls.key` e
`GRPC_CLIENT_CA=/etc/delonix/grpc-tls/ca.crt`. Publicar a porta só em
`127.0.0.1` ou no IP da rede de gestão. Nunca `DELONIX_ALLOW_INSECURE=1`.

**ADR-0001:** um host tem um servidor, logo um SFU; a afinidade por sala é
trivialmente verdadeira. Mais de um servidor é Kubernetes.

## PaaS — `meet-application.yaml` (saas)

```bash
delonixctl apply -f deploy/delonix/meet-application.yaml
```

Dois `kind: Application`: `delonix-server` (addons `meet-pg` e `meet-redis`,
volume das gravações, UDP do SFU publicado) e `delonix-web` (nginx, com o
domínio e o TLS). O nginx da imagem web faz proxy para `delonix-server:8180`,
que é o nome da primeira Application.

### O que ainda não fecha — ler antes de aplicar

Medido no código do `delonix-paas` em `origin/main` (`000d873`, 2026-09-16):

1. **BLOQUEIO — os segredos não chegam ao servidor.** `env[].from_secret` é
   aceite e validado, mas o expander não o injecta
   (`crates/delonix-orchestrator/src/controllers/expand.rs`: «Diferido …
   `env.from_secret` → Pilar 5»). A expansão medida injecta 13 variáveis e
   deixa de fora `JWT_SECRET`, `TURN_SECRET`, `PROVISIONING_SECRET` e
   `VOICE_INTERNAL_SECRET`. Sem `JWT_SECRET` o servidor não arranca. Pôr os
   valores em `value:` resolveria o arranque e poria os segredos no manifesto:
   não se faz. Fecha quando o PaaS entregar o Pilar 5.
2. **Uma instância do servidor.** A `Route` do PaaS encaminha por backend, sem
   hash por sala. Com duas instâncias, dois participantes da mesma sala podem
   cair em SFUs diferentes (ADR-0001, regressão R3). Por isso
   `instances: { min: 1, max: 1 }`. Escalar o servidor no PaaS precisa de uma
   `Route` com afinidade por parâmetro, que não existe.
3. **Migrações no arranque** (`DELONIX_MIGRATE=1`), e não num Job: a
   Application não expande em `Job`. Com uma instância não há corrida.
4. **`publish` de uma gama UDP** (`50000-50200:50000-50200/udp`) passa na
   validação e chega ao `Stack` expandido; não foi provado que o motor do PaaS
   publique a gama num host partilhado por vários inquilinos, onde duas apps
   com a mesma gama colidem.
5. **`TURN_HOST` e `SFU_EXTERNAL_IP` são `CHANGE_ME`.** Dependem do preset
   `coturn` do inquilino (`delonixctl app create --preset coturn`) e do IP
   público do nó. Preencher é um passo manual no caminho do cliente.
6. **As rotas internas (`:8181`, `:9180`) não estão declaradas** nesta
   Application: o PaaS não tem forma de publicar uma porta só para outra carga
   do mesmo inquilino sem a pôr no host. A voz e o gRPC ficam desligados nesta
   edição até existir.

## O que foi validado, e como

| Manifesto | Comando | Resultado | O que NÃO prova |
|---|---|---|---|
| `meet-stack.yaml` | `delonix manifest validate --strict` (delonix 3.1.0) | OK, 12 documentos, referências resolvidas | a sintaxe de `ports` e os campos dentro de um `Stack` só dão aviso, não erro |
| `meet-stack.yaml` | `delonix plan` e `delonix apply --dry-run`, com os `.example` copiados para `*.secrets.env` numa pasta temporária | 12 a criar; nenhum recurso criado no host | que as imagens arrancam, que o coturn lê `-c /run/secrets/TURN_CONFIG`, que a gama UDP publica |
| `meet-stack-personal.yaml` | os mesmos três | OK, 9 documentos | o `UI_DIR` servido pelo binário (depende do servidor) |
| `meet-application.yaml` | programa Rust contra `delonix-orchestrator` de `origin/main` `000d873`: `parse_bundle` → `ApplicationSpec::validate` → `expand`, mais uma comparação YAML ↔ forma serializada para apanhar campos que o serde ignora em silêncio | parse OK, `validate` OK, zero campos ignorados; expande em 2 `Addon` + 2 `Stack` + 1 `Route` | nada contra uma API viva: nenhum `delonixctl apply` foi corrido. O `delonixctl` instalado não tem modo de validação offline |

Nenhum destes manifestos foi aplicado num host ou no PaaS.
