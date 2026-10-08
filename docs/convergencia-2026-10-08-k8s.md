# Tabela de convergência — `deploy/k8s/` contra o chart de produção

**Data:** 2026-10-08. **Medido contra** a `develop` `7e7284b7`.
**Frente 3** das três de 2026-10-07, primeiro passo: «cada recurso do
`deploy/k8s/` contra o que o chart de produção já faz, com a lista do que falta.
**Não escrever YAML antes desta tabela.**

## 0. O que a medição corrige no levantamento

O levantamento de 2026-10-07 dizia que o `deploy/k8s/` «é de laboratório» e que
«falta `resources`, `PodDisruptionBudget`, `NetworkPolicy` e `securityContext`
na maior parte». **Medido hoje, isso já não é verdade** — o #262 (ADR-0020) e o
#266 fecharam-no. Três afirmações desse levantamento estão desactualizadas, e
vale dizê-lo antes de propor trabalho:

| Afirmação de 2026-10-07 | Medido a 2026-10-08 |
|---|---|
| «falta `resources` na maior parte» | **está em 10 de 10** workloads |
| «falta `securityContext`» | **em 8 de 10** (faltam o `52-data-plain` e o HPA, que não é workload) |
| «falta `PodDisruptionBudget`» | existe (no `02-server`, que é o que tem réplicas) |
| «falta `NetworkPolicy`» | existe (`10-contas-e-rede.yaml`, do #266) |
| «o `delonix-config` nasce de `--from-literal` num script» | **falso**: é um `ConfigMap` declarado em `01-config.yaml`. O `--from-literal` é para **segredos**, no README do chart — e isso é correcto, segredos não se commitam |
| «o `ingress-nginx` vem de um `deploy.yaml` do GitHub por URL» | **verdade**, mas em `scripts/cluster.sh` — o script do **laboratório**, não um artefacto de produção |

## 1. Os dois lados não são alternativas

Antes da tabela, a distinção que a torna legível:

| | `deploy/k8s/` (kustomize) | `deploy/helm/delonix-meet` |
|---|---|---|
| Para quê | o **laboratório** e o cluster local (`scripts/cluster.sh`, `make cluster`) | **produção** (ADR-0020, ADR-0021) |
| Dados | Postgres/Redis por Helm bitnami, ou `50-data.yaml` em manifesto plano | CloudNativePG, Redis com Sentinel, MinIO |
| Quem aplica | `kubectl apply -k deploy/k8s` | `helm upgrade` com `values-production.yaml` |

Portanto **não se trata de os fazer iguais.** Trata-se de nenhum dos dois ficar
sem uma prática que lhe faça falta no seu papel.

## 2. A tabela

| Prática | `k8s/` | chart | Veredicto |
|---|---|---|---|
| `resources` em todo o workload | ✓ | ✓ | — |
| probes (`readiness`/`liveness`) | 8/10 | ✓ | **falta** no `60-ai-gpu-worker` |
| `securityContext` | 8/10 | ✓ | **falta** no `52-data-plain` |
| `runAsNonRoot`, `readOnlyRootFilesystem`, `drop: ["ALL"]`, `seccompProfile` | ✓ | ✓ | — |
| `ServiceAccount` + `automountServiceAccountToken: false` | ✓ | ✓ | — (#266) |
| `NetworkPolicy` | ✓ | ✓ | — (#266) |
| `PodDisruptionBudget` | ✓ | ✓ | — |
| `HorizontalPodAutoscaler` | ✓ (opt-in) | ✓ | — |
| `terminationGracePeriodSeconds` | ✓ | ✓ | — |
| Afinidade por sala no ingress | ✓ | ✓ | — (R3) |
| **`topologySpreadConstraints`** | **—** | ✓ | **o chart espalha por nó e zona; o `k8s/` não.** Num cluster de um nó (kind) é indiferente; na produção partilhada do ADR-0021 **não é** |
| **`ServiceMonitor` + `PrometheusRule`** | ✓ (`k8s/observabilidade/`) | **—** | **invertido:** a observabilidade está no lado do laboratório e **não no chart de produção**. É o lado errado, e a R307 já mostrou o que custa um `ServiceMonitor` sem alvos |
| `priorityClassName` | — | — | **falta nos dois.** Sob pressão de nó, o SFU e o Postgres são desalojados como qualquer coisa |
| `preStop` | — | — | **falta nos dois**, mas o servidor dren a por SIGTERM (`drenar()`), logo é menos grave do que parece |
| `ResourceQuota` / `LimitRange` no namespace | — | — | **falta nos dois.** No cluster PARTILHADO do ADR-0021 é o que impede o Meet de comer o que é dos outros |
| Imagens por digest (`@sha256:`) | — | — | falta nos dois; há `make pin` para as versões, não para digests |

## 3. O que proponho, por raio de dano

**Nada disto é YAML novo sem um porquê medido.** Por ordem:

1. **`ResourceQuota` + `LimitRange` no namespace do chart.** O ADR-0021 pôs a
   produção num cluster **partilhado** (`ngolacloud-meet` no `delonix-lda`).
   Sem quota, um pico do Meet come o que é dos vizinhos — e é o caso em que o
   dano não é nosso, é de terceiros. **Prova:** `helm template` a render os dois
   recursos, e o `check-k8s-render.sh` verde.
2. **O `ServiceMonitor` e as regras mudam-se para o chart.** Hoje a
   observabilidade de produção vive no lado do laboratório. **Prova:** o
   `helm template` a produzi-los com os selectores do chart, e um controlo
   negativo que mostre o portão a ver um selector errado (foi exactamente o
   defeito da R307).
3. **`topologySpreadConstraints` no `k8s/`** — ou a decisão escrita de que o
   laboratório não os quer, porque num nó só são ruído. **Prefiro a decisão
   escrita:** pôr constraints que nunca se satisfazem num cluster de um nó deixa
   pods `Pending` e ninguém percebe porquê.
4. **`priorityClassName`** para o SFU e os dados, nos dois lados. Precisa de uma
   decisão: que classes existem no cluster partilhado, o que é do operador do
   `delonix-lda` e não meu.
5. Os dois buracos pequenos: probes no `60-ai-gpu-worker` e `securityContext`
   no `52-data-plain`.

## 4. O que fica de fora, e porquê

- **Imagens por digest.** Muda o `make pin` e o fluxo de release inteiro; é um
  trabalho de cadeia de fornecimento, não de manifesto.
- **O `ingress-nginx` por URL no `scripts/cluster.sh`.** É o laboratório a
  instalar uma dependência de laboratório. Fixá-lo num chart versionado é
  melhoria real, mas não é «artefactos do cluster segundo as boas práticas» — é
  reprodutibilidade do ambiente de desenvolvimento.
- **A PR #269** está aberta e toca a produção partilhada. Qualquer YAML que eu
  escreva no chart deve esperar por ela ou ser medido contra ela.
