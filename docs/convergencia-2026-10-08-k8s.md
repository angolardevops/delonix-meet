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
| **`ServiceMonitor` + `PrometheusRule`** | ✓ (`k8s/observabilidade/`) | — (correcto) | **eu estava errado:** `k8s/observabilidade/` não é o lado do laboratório, é a fase 4 do ADR-0020 — um instalador de produção, fora do kustomize, que põe os objectos no namespace `observabilidade`. Ver o passo 2 |
| `priorityClassName` | — | — | **falta nos dois.** Sob pressão de nó, o SFU e o Postgres são desalojados como qualquer coisa |
| `preStop` | — | — | **falta nos dois**, mas o servidor dren a por SIGTERM (`drenar()`), logo é menos grave do que parece |
| `ResourceQuota` / `LimitRange` no namespace | — | **✓ (2026-10-08)** | **feito no chart**, ligado só em produção. Continua a faltar no `k8s/`, onde é um tecto inventado num cluster de um nó |
| Imagens por digest (`@sha256:`) | — | — | falta nos dois; há `make pin` para as versões, não para digests |

## 3. O que proponho, por raio de dano

**Nada disto é YAML novo sem um porquê medido.** Por ordem:

1. ~~**`ResourceQuota` + `LimitRange` no namespace do chart.**~~ **FEITO a
   2026-10-08.** O ADR-0021 pôs a produção num cluster **partilhado**
   (`ngolacloud-meet` no `delonix-lda`). Sem quota, um pico do Meet come o que é
   dos vizinhos — e é o caso em que o dano não é nosso, é de terceiros.

   Duas correcções ao que eu tinha escrito aqui:

   - **O portão é o `check-helm.sh`, não o `check-k8s-render.sh`.** Este lê o
     `kubectl kustomize` de `deploy/k8s`; o chart é outro caminho para o mesmo
     cluster e tem portão próprio. Escrevi o nome errado.
   - **A conta à mão ficou 10% curta** (22 CPU / 18 Gi estimados contra 24,35 /
     19,81 medidos no render): esqueci o perfil de voz e o Job de migração. E
     faltava-lhe o surge: o pico real durante um rollout é **26,6 CPU / 22,06
     Gi, 17 pods**.

   A quota leva ~20% sobre esse pico (32 CPU, 27 Gi, 40 pods) e vai **desligada
   por omissão** — num laboratório de um nó seria um tecto inventado; é o
   `values-production.yaml` que a liga.

   **Prova:** o `check-helm.sh` **recalcula a conta a partir do render** e falha
   se a quota não cobrir o pico com rollout, se o `max` por contentor ficar
   abaixo do maior contentor do chart (a admissão rejeitaria o nosso próprio
   coturn), ou se o `default`/`defaultRequest` não for uma quantidade > 0. Os
   cinco controlos negativos medidos: quota de memória a 20Gi → falha a 22,06;
   `max` a 2 CPU → falha contra o coturn de 4; `defaultRequest: {}` → falha;
   `maxReplicas` 8→20 sem refazer a conta → falha a 50,6 CPU; template apagado →
   falha. O laboratório é verificado ao contrário: **não** pode levar quota.
2. ~~**O `ServiceMonitor` e as regras mudam-se para o chart.**~~ **MEDIDO a
   2026-10-08, e a proposta estava errada em ambas as pontas.** Fiz outra coisa.

   O que eu afirmei: «a observabilidade de produção vive no lado do
   laboratório». O que a medição mostrou:

   - `deploy/k8s/observabilidade/` **não está no kustomization** e não é o lado
     do laboratório. É a **fase 4 do ADR-0020**: um instalador de produção
     (`instalar.sh`) que monta o kube-prometheus-stack, o Loki, o Alloy, o Tempo
     e o OTel Collector no namespace `observabilidade`, e só no fim aplica o
     `servicemonitor.yaml` e o `alertas.yaml`.
   - O `ServiceMonitor` **vive no namespace `observabilidade`**, com a etiqueta
     `release: kube-prometheus-stack` que o `serviceMonitorSelector` do operador
     exige. Pertence à pilha de monitorização, não ao chart do Meet: metê-lo no
     chart punha o Helm a possuir um objecto fora do namespace do seu release, e
     a depender de CRDs que o cluster pode não ter.
   - E o cruzamento que eu queria **já existe**: o `check-observabilidade.sh`
     valida, desde 2026-10-06, o selector e a porta do `ServiceMonitor` contra o
     que o **chart** renderiza. O risco de deriva que motivava a mudança já
     estava coberto.

   **O defeito real, esse sim medido:** o namespace do Meet está escrito em
   **três** sítios independentes — o objecto `Namespace` em
   `deploy/k8s/00-namespace.yaml`, o `namespaceSelector.matchNames` do
   `ServiceMonitor`, e o `-n …` do comando documentado no
   `values-production.yaml` — e **nada os mantinha em passo**. A #269 teve de os
   encontrar e editar à mão, um a um. Esquecer o do `ServiceMonitor` não dá erro:
   dá **zero alvos**, com o painel verde e o produto em baixo. É o mesmo sintoma
   da R307, por outra porta.

   **O que fiz:** a verificação 4 do `check-observabilidade.sh` cruza as três
   declarações, com o leitor em `scripts/ns-declarado.py` (sem PyYAML, que o CI
   não instala). **Controlos negativos medidos:** rename feito a meio →
   «zero alvos»; comando documentado desactualizado → «um dos dois está
   desactualizado»; `matchNames` com um namespace a mais → «os namespaces a mais
   trazem alvos de outra instalação». E o rename completo da #269 passa.
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
