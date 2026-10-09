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
| probes (`readiness`/`liveness`) | **9/10 (2026-10-09)** | ✓ | **feito** no `60-ai-gpu-worker`: `startupProbe` + `livenessProbe` sobre um batimento pulsado por PROGRESSO. Sem `readiness`, de propósito — não tem Service |
| `securityContext` | **10/10 (2026-10-09)** | ✓ | **feito**: `52-data-plain`, `50-data` e `09-whisper`, todos com o uid **medido** na imagem. Ver o passo 5 — `drop: ALL` sozinho quebrava o Postgres e o Redis |
| `runAsNonRoot`, `readOnlyRootFilesystem`, `drop: ["ALL"]`, `seccompProfile` | ✓ | ✓ | — |
| `ServiceAccount` + `automountServiceAccountToken: false` | ✓ | ✓ | — (#266) |
| `NetworkPolicy` | ✓ | ✓ | — (#266) |
| `PodDisruptionBudget` | ✓ | ✓ | — |
| `HorizontalPodAutoscaler` | ✓ (opt-in) | ✓ | — |
| `terminationGracePeriodSeconds` | ✓ | ✓ | — |
| Afinidade por sala no ingress | ✓ | ✓ | — (R3) |
| **`topologySpreadConstraints`** | **✓ (2026-10-08)** | ✓ | **feito**, com o padrão exacto do chart (`ScheduleAnyway`, nó e zona no servidor, nó na web) e um portão a impedir a regressão. A razão não é o laboratório: são os overlays `saas`/`enterprise`, que são artefacto de PRODUTO |
| **`ServiceMonitor` + `PrometheusRule`** | ✓ (`k8s/observabilidade/`) | — (correcto) | **eu estava errado:** `k8s/observabilidade/` não é o lado do laboratório, é a fase 4 do ADR-0020 — um instalador de produção, fora do kustomize, que põe os objectos no namespace `observabilidade`. Ver o passo 2 |
| `priorityClassName` | — (deliberado) | **maçaneta (2026-10-09)** | o chart expõe-o nos 9 workloads, **vazio por omissão**; o kustomize **não** o pode fixar. Falta o dono dizer os NOMES das classes do `delonix-lda` |
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
3. ~~**`topologySpreadConstraints` no `k8s/`** — ou a decisão escrita de que o
   laboratório não os quer. **Prefiro a decisão escrita.**~~ **FEITO a
   2026-10-08: escolhi o YAML, e a minha preferência estava errada por duas
   razões medidas.**

   - **O medo era infundado.** Eu escrevi «constraints que nunca se satisfazem
     num cluster de um nó deixam pods `Pending` e ninguém percebe porquê». Isso
     só é verdade com `whenUnsatisfiable: DoNotSchedule`. O padrão que o **próprio
     chart** já usa é `ScheduleAnyway`, que num nó é um **no-op** — não há pod
     `Pending` nenhum. O portão passou a recusar `DoNotSchedule` por este
     motivo, para que a armadilha não entre por outra mão.
   - **O `k8s/` não é só o laboratório.** A secção 1 deste documento diz
     «`deploy/k8s/` = o laboratório», e isso é verdade para a *base aplicada
     directamente*. Mas o `docs/deployment.md` mostra que os overlays
     `k8s-overlays/saas` e `.../enterprise` são o **artefacto de produto** para
     Kubernetes (ADR-0006 §2, um binário três perfis), que um cliente aplica com
     `kubectl apply -k` no **seu** cluster, de vários nós. E a base declara
     `replicas: 3` no servidor e na web **sem espalhamento nenhum**: as três
     podiam aterrar todas no mesmo nó, com o Deployment `Available` e três pods,
     num único ponto de falha — sem dar erro.

   **Paridade medida** depois da mudança, nos três renders e no chart:

   | | servidor | web |
   |---|---|---|
   | `deploy/k8s` | nó + zona, `ScheduleAnyway`, anti-afinidade `preferred` | nó, `ScheduleAnyway` |
   | overlay `saas` | idem | idem |
   | overlay `enterprise` | idem (1 réplica: indiferente, inofensivo) | idem |
   | chart de produção | idem | idem |

   **Prova:** a verificação 7 do `check-k8s-render.sh` — todo o `Deployment` com
   mais de uma réplica **ou com um HPA a mirá-lo** tem de se espalhar por nó, e
   nenhum pode usar `DoNotSchedule`. **Controlos negativos medidos:** tirar o
   espalhamento da web → falha nos três; `DoNotSchedule` no servidor → falha;
   tirar o do servidor → falha. E o ramo «um HPA a mirá-lo» **não é exercitado
   pelos manifestos de hoje** (nenhum Deployment tem ≤1 réplica com HPA), pelo
   que foi provado sinteticamente: `replicas: 1` no servidor, que no overlay
   `saas` tem HPA → «tem um HPA a mirá-lo e não se espalha por nó».
4. **`priorityClassName`** — **a canalização ficou feita a 2026-10-09; o que
   falta é a resposta do dono, e é só isso.**

   A decisão que eu não podia tomar continua a ser dele: que classes existem no
   cluster partilhado é do operador do `delonix-lda`. Mas isso não impedia a
   parte que **é** minha, e a medição mostrou que ela nem existia: antes disto,
   **`priorityClassName` não aparecia uma única vez em `deploy/`** — não era «o
   nome está em falta», era não haver sítio onde o pôr.

   **O chart expõe-o nos nove workloads** (servidor, web, coturn, FreeSWITCH,
   Kamailio, PBX de laboratório, Postgres, Redis, e o **Job de migração**, que
   bloqueia o release: um Job não agendado deixa a versão nova a meio). Dois
   níveis, para o caso natural de o caminho da chamada valer mais do que o
   resto:

   ```
   --set priorityClassName=meet-normal \
   --set server.priorityClassName=meet-critico \
   --set coturn.priorityClassName=meet-critico
   ```

   **E nunca fixo, nem no chart nem no kustomize.** Não é timidez: um
   `priorityClassName` que não exista no cluster faz o **API server recusar o
   pod** — a instalação falha no `kubectl apply`, não num aviso. Um nome
   inventado por nós quebrava a instalação de quem nos aplica. (As classes
   `system-cluster-critical` e `system-node-critical` são do Kubernetes e, por
   omissão, só se usam no `kube-system`: não servem aqui.)

   **Prova:** por omissão nenhum workload renderiza prioridade; com um valor,
   **todos** a levam; com um override, o servidor e o coturn ficam em
   `meet-critico` e os outros em `meet-normal`. Medido nos perfis de produção,
   de produção com voz, e de laboratório.

   **Controlos negativos** (cruzamento 6d do `check-helm.sh`, e o novo no
   `k8s-optin-higiene.py`):

   | ataque | o portão diz |
   |---|---|
   | tirar a maçaneta de um workload | `com priorityClassName=prova-prioridade …, estes workloads NÃO a levam: Deployment/coturn` |
   | FIXAR uma classe no chart | `sem priorityClassName nos valores, estes workloads renderizam um: Deployment/delonix-web` |
   | FIXAR uma classe num manifesto do `k8s/` | `52-data-plain.yaml StatefulSet/delonix-postgres fixa priorityClassName «meet-inventado»` |
   | FIXAR uma classe num overlay **nosso** | `k8s-overlays/components/edition-common/server-patch.yaml … fixa priorityClassName` |

   A segunda metade do cruzamento é a que apodrece sozinha: **um workload novo
   que não leve a maçaneta** só se vê com um render que a ligue, e é isso que o
   portão passou a fazer.

   ### Os nomes, decididos a 2026-10-09 — o passo fecha

   O dono concordou com a proposta: **duas** classes.

   | classe | valor | quem a leva | porquê |
   |---|---|---|---|
   | `meet-critico` | 10000 | servidor, coturn, Postgres, Redis, e o Job de migração | desalojá-los **derruba reuniões a decorrer** |
   | `meet-normal` | 1000 | web, FreeSWITCH, Kamailio | degradam sem a reunião cair |

   Ficam em `deploy/k8s/plataforma/priorityclasses.yaml` — **objectos de cluster,
   não do release**: num cluster partilhado o `helm uninstall` do Meet não pode
   levar atrás uma classe que outros possam estar a usar. E são o **primeiro
   passo do `instalar.sh`** dessa pasta, porque não têm dependência nenhuma e
   tudo o resto depende delas.

   **A afirmação que repeti toda a sessão, agora medida** num cluster real
   (`kubectl apply --dry-run=server`, que passa pela admissão sem escrever):

   | pod com | o API server |
   |---|---|
   | `priorityClassName: meet-critico` | aceita, e resolve `spec.priority` = **10000** |
   | `priorityClassName: meet-inventado` | `Error from server (Forbidden): pods "…" is forbidden: no PriorityClass with name meet-inventado was found` |

   Não é um aviso nem um pod a ficar `Pending`: é um **403 na criação**. É por
   isso que as classes são o primeiro passo, e que o repo nunca fixa um nome num
   manifesto que vá para o cluster de outra pessoa.

   Três decisões que valem a pena ler no ficheiro:

   - **`globalDefault: false` nas duas.** A `true`, uma classe passa a ser a
     prioridade de **todos** os pods do cluster que não declarem uma — incluindo
     os de outras equipas, noutros namespaces. Seria o mesmo dano que a quota
     evita, pela porta oposta;
   - **`preemptionPolicy: Never` no `meet-normal`.** Essa classe serve para não
     ser desalojada antes de quem não tem classe, **não** para desalojar os
     outros: espera a sua vez em vez de tirar o lugar a um vizinho;
   - **os valores são um ponto de partida.** Um número de prioridade só tem
     significado comparado com as outras classes do mesmo cluster, e as dos
     vizinhos não são nossas para conhecer. Subir a nossa é baixar a de outro —
     fala-se com o operador do `delonix-lda` antes.

   O **Postgres e o Redis não aparecem no `values-production.yaml`**: em produção
   são externos (CloudNativePG e Redis com Sentinel, ADR-0020 fase 3), logo
   configurá-los no chart era config morta. A prioridade deles põe-se onde eles
   vivem, na plataforma.

   **Prova** — cruzamento 6e do `check-helm.sh`, e o 6d corrigido:

   | ataque | o portão diz |
   |---|---|
   | os valores pedem uma classe que ninguém declara | `usam a classe «meet-inventado» e o …/priorityclasses.yaml não a declara` |
   | `globalDefault: true` | `a classe «meet-critico» … passaria a ser a prioridade de TODOS os pods do cluster` |
   | classes declaradas que ninguém usa | `declara ['meet-critico', 'meet-normal'] e nenhum valor do chart as usa` |

   **E o portão apanhou-me a mim:** o cruzamento 6d media a metade «ninguém fixa
   uma classe» contra o render de **produção** — que agora liga as classes, pelo
   que deixou de servir de controlo negativo. Passou a ter dois renders
   dedicados, um com todas as chaves vazias e outro com uma só.
5. ~~Os dois buracos pequenos: probes no `60-ai-gpu-worker` e `securityContext`
   no `52-data-plain`.~~ **MEDIDO a 2026-10-08. Metade feita; e não eram dois,
   nem pequenos.**

   **O que a medição acrescentou ao enunciado:** a kustomization de `deploy/k8s`
   inclui **12 dos 17** ficheiros. Os outros cinco — `06-external-apps`,
   `09-whisper`, `21-server-hpa`, `50-data`, `52-data-plain`, `60-ai-gpu-worker`
   — aplicam-se à mão com `kubectl apply -f` e **nenhum render os cobria**. Não
   era um buraco no `52-data-plain`: era não haver portão nenhum sobre essa
   família. Era por aí que as lacunas entravam.

   ### Feito: o `securityContext`, com a armadilha medida

   `drop: ["ALL"]` sozinho **quebra** as imagens oficiais do Postgres e do
   Redis: elas arrancam como root e trocam de utilizador no entrypoint, que faz
   `chown`. Medido com o motor (`delonix container run --cap-drop ALL`), seis
   execuções:

   | imagem | utilizador | resultado |
   |---|---|---|
   | `postgres:16-alpine` | root | `chown: /var/lib/postgresql/data/pgdata: Operation not permitted` |
   | `postgres:16-alpine` | uid 70:70 | `database system is ready to accept connections` |
   | `redis:7-alpine` | root | `chown: .: Operation not permitted` |
   | `redis:7-alpine` | uid 999:1000 | `Ready to accept connections tcp` |
   | `postgres:16-alpine` | uid 70, rootfs só de leitura + tmpfs `/tmp` e `/var/run/postgresql` | `ready to accept connections` |
   | `redis:7-alpine` | uid 999, rootfs só de leitura + tmpfs `/data` | `Ready to accept connections tcp` |

   Daí o `52-data-plain.yaml` levar os uids do `/etc/passwd` das imagens (70 e
   999, não um palpite), `readOnlyRootFilesystem: true` com os volumes que a
   medição mostrou necessários, e o `50-data.yaml` — que já corria non-root —
   levar só o `drop: ["ALL"]` e o `allowPrivilegeEscalation: false`.

   **Uma coisa que medi e NÃO era defeito:** o `50-data.yaml` corre o
   `postgres:17-alpine` com `runAsUser: 999`, que é o uid do *redis* (o do
   postgres é 70). Parecia um copiar-colar errado. Medido, **arranca**: o
   Postgres corre com um uid arbitrário desde que o diretório seja escrivível,
   e o `fsGroup: 999` garante-o. Não mexi.

   **Não medido:** o `fsGroup` sobre um PVC. As provas acima correram com
   tmpfs, porque não havia cluster de pé (`kubectl cluster-info` → connection
   refused). Está anotado no próprio manifesto.

   ### Feito: o portão que faltava

   A verificação 8 do `check-k8s-render.sh`
   (`scripts/k8s-optin-higiene.py`) exige, nos manifestos fora da
   kustomization: **recursos, `drop: ALL`, sem escalada, non-root**. Uma
   excepção, com a razão escrita (`09-whisper`: a imagem é construída neste
   repo, pôr non-root exige fixar o uid no Dockerfile dela), e o portão **falha
   se a excepção deixar de ser necessária** — uma excepção morta esconde a
   próxima regressão.

   **Não exige `livenessProbe`, de propósito.** Numa base de dados, uma sonda de
   liveness transforma uma consulta lenta num ciclo de reinícios, e a readiness
   já a tira do Service. É também por isso que **não** acrescentei liveness ao
   `52-data-plain`.

   **Controlos negativos medidos:** tirar o `drop: ALL` do Postgres → falha;
   tirar os recursos do `09-whisper` (que tem excepção só para `non-root`) →
   falha; pôr o `09-whisper` non-root → falha a dizer que a excepção já não é
   necessária.

   ### Feito a 2026-10-09: as sondas do `60-ai-gpu-worker`

   Na primeira passagem ficaram de fora, com o desenho escrito. Foi esse desenho
   que se executou, sem mudar de ideias:

   - **um batimento pulsado por PROGRESSO** (`ai-worker/batimento.py`), não por
     relógio. É a decisão que faz a sonda valer algo: um batimento de relógio
     continuaria a tocar o ficheiro com o CUDA travado, e seria uma sonda que
     não prova nada — o mesmo defeito de um `ServiceMonitor` sem alvos. O pulso
     parte de quem avança: **uma volta do ciclo** (`worker.run`) e **cada
     segmento** que o `faster-whisper` produz. O gerador de segmentos em
     `transcriber.py` era uma compreensão de tuplo; passou a ciclo só para poder
     pulsar;
   - **o ficheiro só nasce depois do modelo estar carregado.** É o que deixa o
     `startupProbe` distinguir «ainda a descarregar o `large-v3`» (~3 GB no
     primeiro arranque) de «pendurado»: ele espera que o ficheiro **apareça**,
     com 10 s × 180 = **30 minutos** de folga. Só depois a liveness olha para a
     **idade** (600 s, 3 × 60 s ⇒ reinício após ~12 min sem progresso);
   - **sem `readinessProbe`**, de propósito: o worker não tem Service e não
     recebe tráfego. Uma readiness aqui seria um campo preenchido para o portão
     ver, não para servir de nada;
   - **um batimento que falha a escrever não derruba a transcrição.** Queixa-se
     **uma vez** e continua; quem decide é o Kubernetes, ao não ver o ficheiro
     aparecer. Essa é a decisão certa para ficar no Kubernetes, não no Python.

   **Medido na base da imagem** (`ubuntu:22.04`, a camada de baixo do
   `nvidia/cuda:…-ubuntu22.04`), com o motor: o `startupProbe` vê o ficheiro;
   com um batimento fresco a liveness diz vivo; com 20 minutos diz morto; sem
   ficheiro, o `startupProbe` continua a esperar. Quatro comportamentos, quatro
   medições.

   ### E um achado maior do que a peça: a bateria do worker nunca corria

   Ao escrever os testes descobri que `ai-worker` **não aparecia no Makefile nem
   no `ci.yml`** — os **24 testes do worker nunca correram em sítio nenhum**. Uma
   bateria que ninguém corre não é uma bateria, e sete testes novos sobre um
   batimento não valeriam nada dentro dela.

   Está ligada nos dois sítios: um passo no job `fitness` do CI e um alvo
   `make test-ai-worker` (que o `make test` chama). Instala **só** o
   `grpcio==1.66.2` e o `protobuf==5.27.5`, nas versões do `requirements.txt`: o
   `faster-whisper` e os 3 GB de modelo não entram, porque o `transcriber.py`
   importa-o **tarde**, dentro do construtor, e nenhum teste o constrói. Nesta
   máquina, sem `grpcio`, três testes dão `ModuleNotFoundError` — o alvo **diz
   qual é o comando** em vez de passar por cima. Com a dependência: **24/24**.

   ### Os controlos negativos

   Dos sete testes novos:

   | ataque ao código | o teste diz |
   |---|---|
   | tirar o pulso do ciclo | `0 != 3: um pulso por volta` **e** `0 != 1: nenhum enquanto está pendurado` |
   | pulsar uma vez em vez de por segmento | `1 != 6: um pulso por segmento do gerador` |
   | queixar-se a cada pulso em vez de uma vez | `2 != 1: queixa-se UMA vez` |
   | o ficheiro nascer antes do modelo | falha — o `startupProbe` deixava de distinguir |

   **Um defeito no meu próprio teste, encontrado ao atacá-lo:** a primeira versão
   de `test_cada_volta_do_ciclo_pulsa` usava o **pulso** para parar o ciclo, pelo
   que tirar o pulso fazia o teste **pendurar** em vez de falhar — e um teste que
   pendura é pior do que um que falha. Passou a ser travado pela **fonte**.

   E dois portões a mais em `scripts/k8s-optin-higiene.py`, porque a sonda
   depende de três ficheiros concordarem:

   | ataque | o portão diz |
   |---|---|
   | renomear a variável só no manifesto | `tem uma sonda que refere $HEARTBEAT_FILE e o contentor não declara essa variável — a sonda não dá erro, dá sempre falso` |
   | renomear só no código do worker | `as sondas leem $HEARTBEAT_FILE e o transcribe_worker.py não lê essa variável — a sonda mediria um ficheiro que ninguém toca` |

   ### Fechado a 2026-10-09: o `09-whisper` non-root, e a imagem que ninguém construía

   Era a última excepção do portão nº8, com a razão «exige fixar o uid no
   Dockerfile dela». Ao ir fixá-lo, a medição corrigiu-me duas vezes:

   - **não era difícil**: o modelo vem **embutido na imagem** (`/models`, por um
     `RUN` do build) e o `app.py` **não escreve em disco nenhum**. O manifesto não
     monta volume nenhum. Bastava dar o `/models` a ler ao uid;
   - **mas havia um bloqueio real, e não era o que eu tinha escrito**: nada neste
     repo construía o `delonix-whisper:latest`. O `deploy/build-images.sh`
     constrói o servidor, a web e (opcional) o `ai-worker` — o `whisper-server`
     **não estava lá**. E o motivo é concreto: o `Dockerfile` dele faz
     `COPY requirements.txt .`, e o `build_one` passava a **raiz** por contexto,
     o que falha com `No such file or directory`. O `build_one` passou a aceitar
     um contexto, e o whisper entra com `BUILD_WHISPER=1`.

   **Medido com o motor** (`--user 65532:65532`, que é o que o `runAsUser` do
   Kubernetes faz): `MODELO CARREGADO como uid 65532`, o uvicorn arranca, e o
   `/health` devolve `{"ok":true,"model":"small","device":"cpu"}`. O uid é o 65532,
   o mesmo do `ai-worker/Dockerfile`.

   **Um achado sobre o motor, de passagem:** o `delonix` 4.5.0 **não honra o
   `USER` da imagem** — sem `--user` corre como uid 0, e o `container exec` nem
   consegue trocar de utilizador. A medição acima teve de forçar o uid, que é
   aliás o que o Kubernetes faz.

   O portão nº8 fica com **zero excepções**. A regra que as acompanha mantém-se:
   uma excepção traz a razão pela qual não é só uma linha de YAML, e o portão
   **falha quando ela deixa de ser necessária**.

   ### Continua por fazer

   Nada. **Os cinco passos estão fechados** (o 4 a 2026-10-09, com os nomes
   decididos pelo dono).

## 4. O que fica de fora, e porquê

- **Imagens por digest.** Muda o `make pin` e o fluxo de release inteiro; é um
  trabalho de cadeia de fornecimento, não de manifesto.
- **O `ingress-nginx` por URL no `scripts/cluster.sh`.** É o laboratório a
  instalar uma dependência de laboratório. Fixá-lo num chart versionado é
  melhoria real, mas não é «artefactos do cluster segundo as boas práticas» — é
  reprodutibilidade do ambiente de desenvolvimento.
- **A PR #269** está aberta e toca a produção partilhada. Qualquer YAML que eu
  escreva no chart deve esperar por ela ou ser medido contra ela.
