# ADR-0021 — A produção do Meet partilha o cluster `ngola-lda`

**Estado:** Aceite · **Data:** 2026-10-07 · **Decisor:** o dono do produto

**Sucede ao [ADR-0020](0020-producao-do-meet-em-meet-ngolacloud-com.md)** na parte do
«onde». Tudo o que o 0020 diz sobre o domínio, a topologia da aplicação, os dados e a
observabilidade continua de pé. O que muda é o substrato: **o Meet deixa de ter cluster
próprio.**

## Contexto

O 0020 decidiu, a 2026-10-06, um cluster Kubernetes próprio em VMs no Proxmox
`192.168.1.10`, e escreveu a razão: «não partilha substrato com o resto do `ngolacloud`:
nem cluster, nem base de dados, nem observabilidade». O OpenTofu para isso foi escrito e
está em `deploy/tofu/`.

A 2026-10-07 mediu-se o que esse plano custava e o que já existia. Os números abaixo são
todos medidos nesse dia, no sítio.

### O que o plano próprio custava, e porque não cabia

O `terraform.tfvars` estava por preencher (igual ao exemplo, com a chave SSH do
placeholder) e tinha dois erros de facto: `node_name = "pve"`, quando o nó se chama
**`ngola`**, e os IPs por omissão — `.20`–`.22` para control-plane e `.30`–`.32` para
trabalho — **colidiam com máquinas vivas**: o `.20` é o próprio `delonix02`, o `.21` e o
`.22` são VMs dele (`delonix-lda-w-4`, `delonix-lda-cp-2`), o `.30` é o `delonix03` e o
`.32`/`.33` são VMs dele. Nenhum respondia a ping porque o `delonix02` estava desligado —
o inventário autoritativo era o `corosync.conf` e a lista de nós do Kubernetes, não o ping.

Seis VMs com os tamanhos por omissão pediam 30 vCPU e **61 440 MiB**. Com 69 632 MiB já
alocados às VMs a correr no `ngola` (128 778 MiB físicos) e `floating = 0` no `main.tf`, o
total passava o físico. Reduzido ao mínimo — 1 control-plane e 2 workers — ainda eram
10 vCPU e 20 GiB que o parque não tem de sobra.

E o cluster novo nascia **sem StorageClass**: o Ansible instala MetalLB e traz o
kubeconfig, mais nada (`k8s_remates`), e o `k8s_base` só põe o pacote `nfs-common`. O
`values-production.yaml` pede `recordings` em **ReadWriteMany, 100Gi**. O PVC ficava
`Pending` e a aplicação nunca arrancava — cluster de pé, produto em baixo.

### O que o cluster existente já tinha

O `delonix-lda`, no cluster PVE `ngola-lda`: **8 nós** (3 control-plane, 5 de trabalho),
Kubernetes **v1.30.14**, Ubuntu 26.04, containerd 2.2.2 — e **praticamente vazio**, com os
workers a 14–18% de CPU pedido e 2–3% de memória. Quatro workers Ready dão ~24 CPU e
~70 GiB alocáveis; o Meet mínimo pede ~3 CPU e ~4 GiB.

E tinha, já instalado, aquilo que faltava ao cluster novo:

| Peça | Estado medido |
|---|---|
| **Longhorn** | StorageClass **por omissão**, e faz RWX — resolve o bloqueio das gravações |
| MetalLB | gama `192.168.1.73-192.168.1.100` (**não** os `.200-.220` do `group_vars` do Meet) |
| cert-manager | ClusterIssuer `delonix-letsencrypt`, `Ready` |

O ferro total do cluster PVE: `ngola` 12 threads/126 GiB, `delonix02` 24 threads/47 GiB,
`delonix03` 8 threads/31 GiB — **44 threads e 204 GiB**, os três na versão 9.2.2 e no
mesmo `repoid`.

## Decisão

**1. A produção do Meet corre no namespace `ngolacloud-meet`, no cluster `delonix-lda`
que já existe.** Não se criam VMs para ela. A razão é medida, não estética: o Longhorn
resolve o RWX que de outro modo deixava o produto em baixo, e o cluster tem o ferro
parado enquanto o parque não tem 20 GiB para dar.

**2. O prefixo dos namespaces da casa é `ngolacloud-*`.** O do Meet passa de
`delonix-meet` a `ngolacloud-meet`.

**3. O OpenTofu de `deploy/tofu/` fica, não se apaga.** É o caminho de volta a um cluster
próprio no dia em que houver ferro, e o plano está validado: `tofu plan` dá `6 to add`
(três VMs mais três ficheiros de cloud-init) com `node_name = "ngola"` e os IPs em
`.40`/`.43`/`.44`, que foram verificados livres em três passagens.

**4. O `vertical` é o SEGUNDO SÍTIO, nunca um quarto membro do cluster.** É um nó Proxmox
independente — `CN = vertical.local`, com **CA de cluster PVE própria** —, em
`197.216.1.11/29` público mais a sua LAN `192.168.8.0/24`, já ligado a Luanda por
WireGuard (`:51820`) e já a alojar o `pbs-vtc-01`.

Não entra no `ngola-lda`, e por três razões medidas:

- **Latência**: 4,5 a 20,6 ms, média 11,9 ms, jitter 6,5 ms, pela internet pública. A
  doutrina do `delonix-deploy` (`docs/COLOCACAO-E-NIVEIS.md:192`) exige um **switch
  gigabit dedicado** ao corosync em `10.99.99.0/29`, e declara o cluster em `t0` até esse
  anel existir. Um nó a 12 ms pela internet é o oposto desse padrão.
- **Modo de falha**: um corosync a oscilar faz o cluster perder quórum e o `/etc/pve`
  ficar **só-leitura em todos os nós** — nenhuma VM arranca, pára ou nasce. (A 2026-10-07
  o `ha-manager config` está vazio e o watchdog em `standby`, logo não haveria fencing;
  o bloqueio de gestão haveria.)
- **Aritmética do quórum**: 3 nós toleram 1 em baixo; **4 nós também toleram 1**; só 5
  toleram 2. Esticar o cluster não compra tolerância nenhuma.

O papel do `vertical` é o que Luanda não se pode dar a si mesma: **domínio de falha
independente** — backup fora do sítio, restauro ensaiado e alvo de recuperação. É também o
que a própria doutrina aponta como bloqueio ao nível T1: «falta backup com restauro
ensaiado (o PBS está fora)».

**5. O etcd fica em três membros, número ímpar, um por chassis.** Já está assim —
`cp-1`→`ngola`, `cp-2`→`delonix02`, `cp-3`→`delonix03`. Quatro membros teriam a mesma
tolerância (um) e pior latência de escrita, porque cada commit passaria a precisar de três
confirmações em vez de duas. Um quarto chassis, se vier, leva **só workers**.

## O que isto NÃO decide, e os limites que ficam escritos

- **O caminho de entrada não existe.** O cluster **não tem IngressClass nenhuma**, e o
  chart do Meet pede `ingress.className: nginx`. Há Envoy Gateway com a GatewayClass
  `delonix` aceite e Gateways em `delonix-system` (`.73`) e `ngolacloud-stage` (`.99`),
  com listeners HTTP:80 e HTTPS:443 sem restrição de hostname — mas **o chart não tem uma
  única linha de Gateway API**: zero ocorrências de `httproute`, `gateway.networking` ou
  `parentRefs`. Escrever os HTTPRoutes é trabalho, não configuração.
- **O TLS público não funciona, e não é por configuração.** Todos os desafios ACME estão
  `pending` há 10 dias e não existe objecto `Certificate`. O `meet.ngolacloud.com` resolve
  para `197.148.40.67` e o self-check do cert-manager diz
  `dial tcp 197.148.40.67:80: connect: no route to host`. Sem entrada de fora o HTTP-01
  nunca fecha. O **DNS-01** é a saída proposta — não decidida aqui. O
  `values-production.yaml` pede ainda `clusterIssuer: letsencrypt-prod`, que **não existe**
  neste cluster (chama-se `delonix-letsencrypt`).
- **O etcd está lento a escrever.** Com tudo saudável, em 120 s: 25 `apply took too long`
  no `cp-1`, 14 no `cp-2`, 8 no `cp-3`, com picos de 309, 338 e 342 ms contra 100 ms
  esperados. É latência de disco e contenção de CPU, e é o verdadeiro limitador de carga
  deste cluster. A correcção é ferro — disco próprio e rápido para os três membros — e
  não cabe num ADR de software.
- **Há um ponto único de falha na porta da API.** O kubeconfig aponta para
  `https://192.168.1.11:6443`, um nó só, sem VIP. Se o `cp-1` cair, o `kubectl`, o Helm e
  o CI perdem o cluster mesmo com os outros dois control-planes vivos e o etcd em quórum.
  O kube-vip que o 0020 desenhou continua a fazer sentido, aqui também.
- **Não há GitOps.** Zero CRDs de Argo ou Flux. O estado do cluster é o que alguém
  aplicou; cada recuperação é manual até deixar de ser.
- **A gama do DHCP nunca foi verificada.** O router `192.168.1.1` é o servidor DHCP e a
  gama não foi lida. Até ser, nem os IPs das VMs nem a gama do MetalLB estão provados —
  só observados desocupados, o que não é o mesmo.
- **Nada disto decide o que é dos outros repos.** Renomear `delonix-system` para
  `ngolacloud-system` mexe em `delonix-paas/deploy/k8s/base/` (12 ficheiros) e em
  `delonix-admin/deploy/k8s/base/httproute.yaml`; remover o `ngolacloud-stage` tem
  teardown escrito no `delonix-deploy`
  (`roles/stage_environment/tasks/teardown.yml`). Fazem-se lá, com um dono por recurso,
  como manda a regra §1 do workspace.

## Consequências

**O vizinho ruidoso muda de sítio.** Até aqui a promessa de isolamento do Meet era entre
organizações; agora há uma camada acima dela — o Meet contra o resto da plataforma —, com
control-plane, Longhorn e nós partilhados com o `delonix-system`. **Um namespace não é uma
fronteira dura.** O mínimo para isto não ser uma promessa vazia é `ResourceQuota`,
`LimitRange` e `NetworkPolicy` no `ngolacloud-meet`. Os três vêm do chart, e a quota é
UMA só (D1, 2026-10-10): conta o chart e a plataforma que vive no mesmo namespace — o
Postgres e o Redis —, porque duas quotas no mesmo namespace aplicam-se as duas e a do chart,
sozinha, recusava o Postgres. O `00-namespace.yaml` fica com o Namespace e o Pod Security.

**O 0020 fica com uma parte morta e uma parte viva.** Morta: «um cluster Kubernetes
próprio, em VMs no Proxmox». Viva: tudo o resto, incluindo a observabilidade própria, que
continua a instalar-se com `deploy/k8s/observabilidade/instalar.sh` — e que depende do
MinIO, porque o Loki e o Tempo escrevem lá. A ordem é MinIO primeiro.

**Fica registado um episódio que vale mais que um alerta.** Entre 28 de Setembro e 7 de
Outubro o `delonix02` esteve desligado. O membro de etcd que lá vivia ficou inalcançável,
o `etcd` do `cp-1` encheu o buffer de envio do Raft e começou a **deixar cair heartbeats**
— incluindo para o peer vivo. O `kube-apiserver` acumulou 231 e 353 reinícios, e o
`envoy-gateway` 123, todos com saída 0 e `stopped leading`: perdia o Lease de eleição por
`DeadlineExceeded`. **O envoy não tinha defeito nenhum.** Quando o anfitrião voltou, as
mensagens perdidas foram a zero e os apiservers estabilizaram.

A lição fica escrita porque é contra-intuitiva: **«quórum 2 de 3» não é «está bem»** — um
membro morto faz dano activo, e três nós com um em baixo são pior que dois nós sãos.
