# ADR-0020 — A produção do Meet em `meet.ngolacloud.com`

**Estado:** Aceite · **Data:** 2026-10-06 · **Decisor:** o dono do produto

## Contexto

O chart Helm do Meet já descreve a produção — `deploy/helm/delonix-meet/values-production.yaml`
fixa `meet.ngolacloud.com`, três réplicas do servidor e da web, autoscaling até oito,
migrações por Job, ingress com `letsencrypt-prod` e NetworkPolicy. **O que nunca existiu
foi a infra por baixo dele.** Medido a 2026-10-06 contra a `develop`:

- **zero** ficheiros OpenTofu ou Terraform no repositório;
- **zero** `ServiceMonitor`, **zero** `PrometheusRule`, **zero** ocorrências de
  `opentelemetry` ou `otlp` em todo o `deploy/` e `server/src/`;
- Postgres e Redis são **externos** por desenho do chart (`externalRedis.fromSecret`), e
  nada no repositório os levanta com alta disponibilidade;
- as gravações pedem um volume **ReadWriteMany**, e o servidor só sabe escrever em disco
  local, NFS e WebDAV — **não há cliente S3 nenhum** no `Cargo.toml`.

O plano de lacunas de 2026-10-04 tinha isto como **D1** («chart Helm directo ou pelo
PaaS») e **D2** («volume partilhado ou objectos»), ambos por decidir, e como **O4** — «o
chart nunca foi instalado em perfil de produção».

## Decisão

**O Meet é um produto separado e tem a sua própria infra.** Não entra no `delonix-paas`
nem partilha substrato com o resto do `ngolacloud`: nem cluster, nem base de dados, nem
observabilidade. Isto resolve o **D1** e é explícito — a regra do workspace manda o
substrato vir do `delonix-deploy`, e esta é a excepção escrita para um produto que se
vende por si.

**Onde.** Um cluster Kubernetes próprio, em VMs no Proxmox `192.168.1.10`.

1. **As VMs nascem de OpenTofu** (`deploy/tofu/`), não à mão. Três de control-plane e três
   de trabalho, por omissão, com os tamanhos em variáveis — para que recriar o cluster seja
   um comando e não uma tarde.
2. **O Kubernetes instala-se por Ansible** (`deploy/ansible/`), sobre as VMs que o OpenTofu
   deu. O inventário é gerado pelo OpenTofu: um sítio só a dizer que máquinas existem.
3. **A plataforma do cluster** — ingress-nginx, cert-manager, CloudNativePG, Redis com
   Sentinel, MinIO e a observabilidade — instala-se por Helm, com valores versionados em
   `deploy/k8s/plataforma/`.
4. **A aplicação** continua a instalar-se com o chart que já existe e o
   `values-production.yaml` que já aponta para `meet.ngolacloud.com`.

**Os dados.**

- **Postgres com CloudNativePG**, três instâncias, failover automático e backup contínuo
  para o MinIO. O chart do Meet continua a vê-lo como externo, pelo `DATABASE_URL` do
  Secret — o operador é que decide quem é o primário.
- **Redis com Sentinel**, três nós. O servidor já fala Sentinel onde precisa; o que esta
  decisão acrescenta é **autenticação obrigatória**, que hoje não existe.
- **MinIO para os ficheiros, e o servidor ganha um cliente S3** (resolve o **D2**). As
  gravações deixam de precisar de um volume ReadWriteMany, que era o que amarrava o
  produto a armazenamento de blocos partilhado e o impedia de servir três réplicas em nós
  diferentes.
- **O que hoje só vive no IndexedDB do browser passa a ter servidor**: diagramas, cenas do
  Estúdio, projectos de edição e arquivo de aulas. São quatro bases locais sem rota
  nenhuma — offline-first sem sincronização é só local, e trocar de browser perde tudo. O
  offline-first mantém-se; o que muda é passar a haver com que sincronizar.

**A observabilidade não é um extra.** OpenTelemetry Collector no cluster, Prometheus para
métricas, Loki para registos, Tempo para rastos, Grafana para os ver. O servidor passa a
emitir OTLP. Sem rastos, um pedido que atravessa ingress, servidor, Postgres e Redis
diagnostica-se a adivinhar — e foi exactamente isso que custou tempo nos achados de
Outubro.

## O que isto NÃO decide, e os limites que ficam escritos

- **Um só host Proxmox não dá alta disponibilidade de verdade.** Três control-planes em
  três VMs do MESMO servidor físico protegem contra a morte de uma VM, de um kubelet ou de
  um etcd — **não** contra a morte do host, do disco ou da fonte de alimentação. Quem ler
  «HA» neste ADR tem de ler também esta frase. Um segundo host é a diferença entre
  resiliência de software e resiliência a sério, e não está decidido.
- **Os tamanhos das VMs são palpite.** Não medi o que o `192.168.1.10` tem de CPU, memória
  e disco. Os valores do OpenTofu são um ponto de partida declarado, não um
  dimensionamento.
- **O autoscale do Meet é de pods, não de nós.** O HPA sobe réplicas até oito; se os nós
  não derem, os pods ficam pendentes. Autoscale de nós no Proxmox não entra aqui.
- **A voz fica desligada** (`voice.enabled: false`), como o `values-production.yaml` já
  dizia: o chart sabe instalá-la e o caminho de um PBX externo não foi medido.
- **Nada disto foi corrido.** Este ADR é um desenho; o que o validar é a primeira
  instalação, e é ela que vai mudar números deste documento.

## Consequências

- O repositório passa a ter infra (`deploy/tofu/`), e com ela a responsabilidade de a
  manter a par do cluster real.
- O `deploy/ansible` deste repositório deixa de ser um corpo estranho: passa a ser o que
  instala o Kubernetes do Meet, com um ADR a dizer porquê.
- O servidor ganha uma dependência nova — um cliente S3 — e com ela a obrigação de a
  manter fora do caminho quente da media.
- Passa a haver duas formas de correr o Meet: o cluster local do `make cluster`, para
  desenvolver, e esta, para produção. **Não se misturam**, e o `CLUSTER_NAME` diferente é
  o que as separa.
