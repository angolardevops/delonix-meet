# Chart Helm do Delonix Meet

Instala o que é do Meet num cluster Kubernetes: servidor (API, sinalização e
SFU), web, Ingress com afinidade por sala, coturn relay-only e a voz (Kamailio
e FreeSWITCH). É um segundo caminho para o mesmo cluster que `deploy/k8s/` e
`deploy/k8s-overlays/` — não os substitui, e `make stage`/`make prod`
continuam a aplicar os manifestos.

Este documento separa o que o chart **faz**, o que fica **fora** dele e os
**achados** que impedem ou condicionam a produção. O que foi medido e o que
não foi está em [§Prova](#prova).

## Uso

```bash
# Laboratório (cluster do `make cluster`), num namespace próprio
helm upgrade --install meet deploy/helm/delonix-meet -n meet-lab --create-namespace \
  -f deploy/helm/delonix-meet/values-local.yaml --set host=meet-lab.test

# Produção — meet.ngolacloud.com
helm upgrade --install meet deploy/helm/delonix-meet -n delonix-meet \
  -f deploy/helm/delonix-meet/values-production.yaml \
  --set image.tag=<git describe> \
  --set secrets.existingSecret=<nome do Secret> \
  --set coturn.externalIP=<IP público do relay> \
  --set server.image.repository=<registo>/delonix-server \
  --set web.image.repository=<registo>/delonix-web
```

| Ficheiro | Para quê |
|---|---|
| `values.yaml` | Omissões **fechadas**: sozinho, o `helm template` falha e lista o que falta |
| `values-production.yaml` | Topologia de `meet.ngolacloud.com`: 3 réplicas + HPA, Job de migração, base e Redis externos, cert-manager, NetworkPolicy, gravações RWX. Sem segredos, sem tag, sem IP |
| `values-local.yaml` | Um nó, uma réplica, Postgres e Redis do chart, segredos gerados, voz com PBX de laboratório |
| `ci/*.yaml` | Entradas de prova do portão (`scripts/check-helm.sh`); não vão no pacote |

`make helm-lint` corre o portão; `make helm-package` produz o `.tgz`.

### Nomes fixos, uma release por namespace

Os recursos não levam o nome da release (`delonix-server`, `delonix-server-ws`,
`delonix-server-internal`, `delonix-web`, `coturn`, `kamailio`, `freeswitch`).
A imagem web faz `proxy_pass http://delonix-server:8180`
(`deploy/k8s/nginx.conf:74`), o dispatcher do Kamailio resolve `freeswitch`, e
os portões do repo procuram `delonix-server-ws`. Duas releases no mesmo
namespace colidem.

## O que o chart recusa

`production: true` é a omissão. O `helm template` falha, com todas as razões de
uma vez, quando:

- falta `host`, a tag de uma imagem, ou a tag é `latest` (em produção);
- em produção falta `secrets.existingSecret`, ou se pede `secrets.create`;
- em produção se liga o Postgres/Redis do chart ou o PBX de laboratório;
- em produção falta TLS no Ingress (`clusterIssuer` ou `existingSecret`), o
  `coturn.externalIP`, ou o certificado do bordo SIP;
- há mais de uma réplica do servidor (ou HPA) sem Redis, ou com gravações
  `ReadWriteOnce`;
- se liga a ponte telefone↔sala sem IPs do FreeSWITCH, ou com mais de uma réplica;
- se liga o gRPC sem mTLS, ou a migração por Job sem Secret externo.

O chart nunca define `DELONIX_ALLOW_INSECURE`.

## Segredos

O chart não traz nenhum valor. Em produção lê **um** Secret criado fora dele
(`secrets.existingSecret`), chave a chave (`secretKeyRef`, nunca `envFrom`):

| Chave | Obrigatória | Para que serve | O que o servidor recusa |
|---|---|---|---|
| `DATABASE_URL` | sim | Ligação ao Postgres (`postgres://user:pass@host:5432/db`) | Ausente, ou o valor de desenvolvimento → não arranca |
| `JWT_SECRET` | sim | Assina os tokens de sessão e de sala | Ausente, valor de desenvolvimento, ou menos de 32 caracteres → não arranca |
| `TURN_SECRET` | sim | Assina as credenciais TURN; o coturn do chart lê a MESMA chave | Ausente, valor de desenvolvimento, ou menos de 16 caracteres → não arranca |
| `DATA_ENCRYPTION_KEYS` | sim | Cifra de segredos em repouso: `kid:base64` de 32 bytes, vários separados por vírgula | Malformada → não arranca. (Sem ela o servidor arrancaria e recusaria com 422 o que guarda segredos; o chart exige-a) |
| `VOICE_INTERNAL_SECRET` | com `voice.enabled` | Cabeçalho `X-Voice-Secret` entre o FreeSWITCH e o servidor | Vazio, menos de 32 caracteres, ou um valor já publicado no repo → as rotas de IVR respondem 503 |
| `PROVISIONING_SECRET` | não | Autoriza a provisão de organizações (`/api/operator/v1/organizations`) | Vazio → endpoint desligado |
| `REDIS_URL` | com `externalRedis.fromSecret` | Redis com password (`redis://:pass@host:6379`) | — |
| `TELEPHONY_ESL_PASSWORD` | com `server.telephony.eslAddr` | Password do ESL do FreeSWITCH (ADR-0009) | `TELEPHONY_ESL_ADDR` sem ela → não arranca |

Outros Secrets, por nome:

| Secret | Quando | Conteúdo |
|---|---|---|
| `ingress.tls.existingSecret` | sem cert-manager | `kubernetes.io/tls` do host público |
| `voice.kamailio.tls.existingSecret` | voz em produção | `kubernetes.io/tls` do bordo SIP (5061) |
| `server.grpc.tlsSecretName` | gRPC sem cert-manager | `tls.crt`, `tls.key`, `ca.crt` |
| `imagePullSecrets` | registo privado | credenciais do registo |

```bash
# Exemplo de criação — os valores nunca passam pelo git nem pelo chart
kubectl -n delonix-meet create secret generic delonix-meet-secrets \
  --from-literal=DATABASE_URL="postgres://…" \
  --from-literal=JWT_SECRET="$(openssl rand -hex 32)" \
  --from-literal=TURN_SECRET="$(openssl rand -hex 24)" \
  --from-literal=DATA_ENCRYPTION_KEYS="k1:$(openssl rand -base64 32)" \
  --from-literal=VOICE_INTERNAL_SECRET="$(openssl rand -hex 32)"
```

Em laboratório (`production: false`, `secrets.create: true`) o chart gera o
Secret `delonix-secrets` com valores aleatórios e, nos `helm upgrade`
seguintes, preserva os que já lá estão (`lookup`). O `helm template` não fala
com o cluster e mostra valores novos a cada execução. Rodar um segredo é
apagar a chave do Secret e fazer `helm upgrade`. Os literais de
`deploy/k8s/01-config.yaml` não entram aqui: `scripts/check-helm.sh` lê-os da
fonte e falha se algum aparecer no chart ou no que ele renderiza.

## Postgres e Redis

**Produção: externos.** O chart recebe um `DATABASE_URL` e um `REDIS_URL` e
mais nada. Não cria a base nem o utilizador, não faz backup, não dá réplica
nem failover. HA, backup, PITR e a rotação da password são de quem opera a base.

**Laboratório:** `postgresql.enabled` e `redis.enabled` criam um pod de cada
(`postgres:17-alpine` com um PVC, `redis:7-alpine` sem persistência nem
password). Não é HA e o chart recusa-os com `production: true`. O PVC do
Postgres não é apagado pelo `helm uninstall`.

**Migrações.** `server.migrations.mode`:

- `startup` — o servidor migra ao arrancar (`DELONIX_MIGRATE=1`);
- `job` — um Job `pre-install,pre-upgrade` corre `delonix-server migrate` com a
  mesma imagem, e o Deployment arranca com `DELONIX_MIGRATE=0`. Só lê o Secret
  do operador, que já existe antes de o chart criar o que quer que seja.

## Componentes

| Componente | Recursos | Notas |
|---|---|---|
| Servidor | Deployment, Services `delonix-server` / `delonix-server-ws` / `delonix-server-internal`, PDB, HPA, PVC | distroless uid 65532, rootfs só de leitura, drop ALL; `/ready` com `failureThreshold: 1`; `terminationGracePeriodSeconds: 60`; espalhado por nó e zona |
| Web | Deployment, Service, PDB | nginx sem privilégios (uid 101), rootfs só de leitura |
| Ingress | `delonix-ingress` (`/api`, `/rtc`, `/`) e `delonix-ws` (`/ws`) | `/ws` num Service dedicado com `upstream-hash-by: $arg_room` (ADR-0001) |
| coturn | Deployment de 1 réplica, Service | relay-only; `FORCE_TURN_RELAY=1` no servidor |
| Voz | Kamailio, FreeSWITCH, ConfigMaps, Services `kamailio`, `kamailio-border`, `freeswitch` | desligada por omissão |
| NetworkPolicy | `delonix-server` | opcional; `:8181`, `:9180` e a ponte só para o FreeSWITCH |

Não são instalados: whisper, ollama, open-webui, o worker de IA, o gateway de
SMS e o `ClusterIssuer` (é do cluster, não de uma release).

### Portas internas

O listener interno (`:8181` — `/internal/v1/*`, `/metrics`) e o gRPC (`:9180`)
só existem no Service `delonix-server-internal`, ClusterIP. Nenhum Ingress o
refere; o portão falha se referir.

### Voz

A configuração sai dos ficheiros de `voice/`: `files/voice/` são **ligações
simbólicas** para lá, e o `helm package` resolve-as (o `.tgz` leva o conteúdo).
Não há cópia que possa divergir; o portão falha se uma ligação virar ficheiro
ou se o pacote diferir do repo. Num sistema de ficheiros sem ligações
simbólicas (checkout em Windows sem `core.symlinks`) o chart não renderiza a voz.

Só dois ficheiros são gerados, porque dependem dos valores:

- `address` — a allowlist do bordo, de `voice.trunks[]` (grupo, IP, máscara,
  porta, etiqueta);
- `dispatcher.list` — `1 sip:freeswitch:5080 0 0 weight=100`.

O FreeSWITCH arranca pelo `voice/cluster/freeswitch-entrypoint.sh`, que copia a
configuração vanilla da imagem, fecha o que ela deixa aberto e põe por cima a
do Meet. O Kamailio leva `command: ["kamailio"]` e um `initContainer` que
espera que o nome `freeswitch` resolva (ordem FreeSWITCH → Kamailio). O Service
do FreeSWITCH é headless com `publishNotReadyAddresses`.

### O PBX do cliente (Issabel, FreePBX)

Não é instalado: não corre em contentor e em produção é do cliente, fora do
cluster. O chart expõe o que a interligação precisa:

| Valor | O que é |
|---|---|
| `voice.trunks[]` | Origens aceites pelo bordo. Vazia = `403` a todas |
| `voice.kamailio.service.{type,loadBalancerIP,annotations}` | O Service `kamailio-border`: SIP 5060 UDP/TCP e 5061 TLS. `externalTrafficPolicy: Local` para a allowlist ver o IP real |
| `voice.kamailio.tls.existingSecret` | Certificado do 5061 |
| `voice.centrais.enabled` | A central de uma organização entra **sem estar em `voice.trunks[]`**: por TLS (5061), autenticada com a conta SIP dessa organização (ADR-0016). O bordo passa a falar com `delonix-server-internal:8181` |
| `voice.centrais.edgeCidrs[]` | De onde o bordo fala para o FreeSWITCH. O IVR só acredita em «esta chamada é da central de X» vindo daqui; vazia com as centrais ligadas, o chart falha |
| `voice.ramais.acl` | `DELONIX_RAMAIS_ACL` — redes de onde um ramal se regista |
| `voice.freeswitch.{externalIP,rtp,ramaisService}` | Ramais registados de fora do cluster |

`voice.labPbx.enabled` instala o Asterisk de `voice/pbx-cliente/` para o
laboratório e é recusado em produção.

## Fora do chart

O que a produção em `meet.ngolacloud.com` precisa e o chart não cria:

1. **DNS** de `meet.ngolacloud.com` para o IP do ingress-nginx.
2. **ingress-nginx**. A afinidade por sala é a anotação `upstream-hash-by`
   dele; outro controlador ignora-a e o SFU fica em split-brain.
3. **cert-manager** e o `ClusterIssuer` de `ingress.tls.clusterIssuer`, ou um
   Secret TLS já emitido.
4. **IP público para o coturn**: um LoadBalancer que aceite UDP e um IP fixo,
   posto em `coturn.externalIP` (e em `coturn.service.loadBalancerIP`, ou nas
   anotações do fornecedor). Portas: **UDP 3478**. A gama de relay
   (49152–59152) não precisa de ser exposta — em relay-only dos dois lados o
   relay-a-relay é interno ao coturn. O IP tem de ser alcançável pelos browsers
   **e** pelos pods do servidor.
5. **Postgres e Redis** (acima).
6. **StorageClass ReadWriteMany** para as gravações (abaixo).
7. **Registo de imagens** com `delonix-server`, `delonix-web` e a imagem do
   FreeSWITCH, por tag imutável.
8. Para a voz: um IP para o bordo SIP alcançável pelo PBX do cliente, e os IPs
   de origem do tronco.
9. Um CNI que aplique NetworkPolicy, se `networkPolicy.enabled`.

### Gravações: ReadWriteOnce com várias réplicas

Medido nos manifestos: `deploy/k8s/02-server.yaml:9` pede `replicas: 3` e
`:117` um PVC `ReadWriteOnce` montado em todas (o comentário de `:99-102` já o
avisa). Com um nó só — o `kind` de stage — funciona, porque `ReadWriteOnce` é
por **nó**. Num cluster de vários nós é um bloqueio real: o volume prende-se a
um nó, as réplicas que o scheduler puser noutro ficam em `ContainerCreating`
(volume em uso) ou `Pending` (afinidade do volume), e a anti-afinidade deixa de
poder espalhá-las. O `maxSurge: 1` de um rollout bate no mesmo.

Por isso o chart recusa `ReadWriteOnce` com mais de uma réplica, e
`values-production.yaml` pede `ReadWriteMany`. A saída só para um nó é
`recordings.allowReadWriteOnceWithReplicas=true`. A StorageClass RWX
(NFS, CephFS, Longhorn RWX) é da infra-estrutura. O `helm uninstall` não apaga
o PVC das gravações (`helm.sh/resource-policy: keep`).

## Ponte telefone↔sala (ADR-0010)

Valores: `server.phoneBridge.{enabled,sipPort,advertise,freeswitchIPs,rtp.min,rtp.max}`
→ `PHONE_BRIDGE_SIP_BIND`, `PHONE_BRIDGE_SIP_ADVERTISE`,
`PHONE_BRIDGE_FREESWITCH_IPS`, `PHONE_BRIDGE_RTP_MIN/MAX`. Desligada por omissão.

**Achado — não está resolvido pelo chart.** O servidor só aceita SIP e RTP do
FreeSWITCH por **IP exacto**: `server/src/config.rs:584` e `:663` (lista de
IPs, sem CIDR) e `server/src/phone_bridge/sip.rs:405` (`contains(&from.ip())`;
quem não está na lista nem recebe resposta). Vazia, a ponte não arranca. Um
pod do FreeSWITCH muda de IP a cada reinício, portanto não há valor estável
para pôr na lista. O chart falha se a ponte for ligada com a lista vazia, em
vez de a fingir a funcionar.

Saídas possíveis, nenhuma medida aqui:

| Saída | O que muda | Custo |
|---|---|---|
| FreeSWITCH em `hostNetwork` num nó fixo (`voice.freeswitch.hostNetwork`, `nodeSelector`) | A origem passa a ser o IP do nó | O IP que o servidor vê depende do CNI (do mesmo nó pode ser o da bridge); portas do nó ocupadas; um só nó |
| IP de pod fixo pelo CNI (Calico/Cilium IPAM por anotação) | O pod guarda o IP | Depende do CNI do cluster |
| O servidor aceitar CIDR | `PHONE_BRIDGE_FREESWITCH_IPS=10.244.0.0/16` | Mudança em `config.rs`/`sip.rs`; alarga a confiança a toda a rede de pods, a compensar com a NetworkPolicy |

**Segundo achado — várias réplicas.** O IVR pede o destino da ponte ao
listener interno, que é um Service: responde uma réplica qualquer
(`server/src/voice.rs:1026-1060` não sabe em que pod vive a sala), e o SFU é em
memória por pod. Com mais de uma réplica o telefone pode entrar numa sala
vazia noutro pod. O chart recusa a ponte com mais de uma réplica. Isto foi
lido no código, não reproduzido.

## Achados

Encontrados ao construir e ao instalar o chart. Cada um diz onde está e de quem é.

| # | Onde | O quê | De quem |
|---|---|---|---|
| 1 | `voice/cluster/freeswitch-entrypoint.sh:16` (origin/develop) | `rm -rf "$CONF"` falha quando `/conf` é um volume montado: o FreeSWITCH entra em CrashLoop (visto: «rm: cannot remove '/conf': Device or resource busy»). A correcção existe em `origin/fix/voz-arranque-no-cluster` (b210f2fe), por fundir. O chart contorna-o não montando volume em `/conf` | voz |
| 2 | `voice/kamailio/kamailio.cfg:39-41` | `listen` sem `advertise`: atrás de um LoadBalancer o Kamailio põe o IP do pod no Via/Record-Route, e os pedidos dentro do diálogo (ACK, BYE) de um PBX externo vão para um IP inalcançável. Não medido — o laboratório só tem PBX dentro do cluster | voz |
| 3 | `voice/cluster/freeswitch-entrypoint.sh:27` e `:86-93` | O perfil do dial-in anuncia o IP do pod no SDP; o Kamailio não faz relay de media. O áudio de um PBX externo não tem por onde chegar. `DELONIX_EXTERNAL_IP` só cobre o perfil dos ramais. Saídas: `hostNetwork`, ou um relay de media no bordo | voz |
| 4 | `voice/cluster/freeswitch-entrypoint.sh` (passo 4) | **Fechado no arranque e no chart (R300), por aplicar num cluster.** Com `server.telephony.eslAddr` o FreeSWITCH recebe a `TELEPHONY_ESL_PASSWORD` do Secret e abre o ESL (8021); `voice.freeswitch.eslCidrs` estreita a lista de acesso, e com `networkPolicy.enabled` o 8021 fica só para os pods do servidor. Em `hostNetwork` a política não se aplica: o chart exige `eslCidrs`. O ESL não tem TLS — a password vai em claro na rede de pods. Sem `eslAddr` continua em `127.0.0.1` com password aleatória | voz |
| 5 | `voice/freeswitch/image/Containerfile` (sem `USER`) e a imagem do Kamailio | Correm como root. O chart tira-lhes a escalada e todas as capacidades (e o rootfs de escrita, no Kamailio), mas non-root exige mudar as imagens | voz |
| 6 | `deploy/k8s/cluster/voice.yaml:69-71` | O Kamailio resolve o dispatcher ao arrancar. Um FreeSWITCH que reinicie com outro IP fica fora do pool até o Kamailio reiniciar. Lido no comentário do repo, não reproduzido | voz |
| 7 | `voice/cluster/tls.cfg:5-6` | O único `tls.cfg` com os caminhos de um Secret `kubernetes.io/tls` é o do laboratório, com `verify_certificate = no`. O chart usa-o também em produção | voz |
| 8 | `voice/kamailio/kamailio.cfg:129-136` | Com `WITH_XHTTP_RPC`, o `/rpc` responde na mesma porta TCP 5060 do bordo, sem filtro de origem. `voice.kamailio.xhttpRpc` fica desligado | voz |
| 9 | `deploy/k8s/51-coturn.yaml:29-44` | Uma réplica do coturn, por desenho (hairpin). É ponto único de falha da media e não escala sem redesenho | servidor / dono |
| 10 | `deploy/k8s/51-coturn.yaml:91` | `--no-tcp-relay --no-tls`: só TURN/UDP 3478. Uma rede que só deixe sair 443/TCP fica sem media | dono |
| 11 | `Makefile:47-48`, `.github/workflows/ci.yml` | As imagens do servidor e da web não têm registo: o CI não as publica e o Makefile carrega-as nos nós. Só a do FreeSWITCH é publicada (`freeswitch-image.yml`) | CI / infra |
| 12 | `deploy/k8s-overlays/components/edition-common/networkpolicy.yaml:49` | A NetworkPolicy dos overlays deixa entrar `app: delonix-freeswitch`; o FreeSWITCH de `deploy/k8s/cluster/voice.yaml:23` tem `app: freeswitch`. Nos overlays o IVR ficaria barrado. O chart usa a sua própria etiqueta | devops |

## Prova

O que foi medido e o que não foi está no relatório da entrega; os comandos são:

```bash
make helm-lint                       # scripts/check-helm.sh
DRYRUN=1 bash scripts/check-helm.sh  # + dry-run de servidor (precisa de cluster e do namespace meet-helm)
make helm-package
```

Não validado por este chart em lado nenhum: media por coturn com um browser,
cert-manager, LoadBalancer, várias réplicas em vários nós, HPA, base externa
real, um PBX fora do cluster e a ponte telefone↔sala.
