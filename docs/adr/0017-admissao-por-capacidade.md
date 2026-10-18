# ADR-0017 — Um nó cheio não abre salas novas (admissão por capacidade)

**Estado:** Aceite · **Data:** 2026-10-05 · **Complementa** o [ADR-0001](0001-room-shard-affinity.md); não o revoga.

## Contexto

O ADR-0001 fixa que a sala é a unidade de shard: `hash(sala) → sempre o mesmo pod`, imposto no
ingress. O ingress não sabe quão carregado está um pod, e o servidor, até aqui, também não
agia sobre isso: `NODE_PEER_CAPACITY` (a capacidade que o operador declara a partir dos seus
testes de carga) só alimentava o inventário (`load`, `GET /api/operator/v1/nodes`). Nenhum
caminho de entrada a impunha.

O teste de carga de 2026-09-17 (`docs/ops/teste-de-carga-2026-09-17.md`) mediu o que acontece
a um nó sem esse travão: com 50 chamadas de 4 (200 pessoas) a perda foi de 29–48% **para
todas as salas do nó**, e com 40 pessoas numa só reunião de 13–46%. Não houve degradação
graciosa: o nó continuou a aceitar salas novas até colapsar.

## Decisão

Com `NODE_PEER_CAPACITY` declarada, o `/ws` decide as salas que **ainda não existem no nó**
(`domain::operations::media_node::admit_new_room`), **por inquilino**:

- **Abaixo de 85% da capacidade** (`NEW_ROOM_LOAD_PERCENT`): aceita.
- **Na zona de margem (85% até à capacidade): recusa só o inquilino que já usa a sua parte
  justa**, `capacidade ÷ inquilinos activos`, com **503**. Um inquilino sozinho no nó tem a
  capacidade toda e não é penalizado; com dois, cada um tem metade, e quem tem menos continua a
  poder abrir salas.
- **Na capacidade**: recusa toda a gente.

**O inquilino** de uma sala é a organização do dono (a primeira, de forma estável, se o dono
pertencer a várias) ou o próprio dono se não tiver organização (um utilizador individual). Deriva-se
**no servidor** de `rooms.owner_id`, nunca do pedido, e não passa pelos claims do token. Marca-se na
sala depois do `join`. Se a base não responder, a justiça falha **aberta** (a sala entra como
inquilino desconhecido); a recusa por capacidade total continua a valer.

Porquê por inquilino (achado de uma revisão de isolamento, 2026-10-05): a primeira versão contava
os participantes de **todos** os inquilinos, e uma conta com muitas ligações levava o nó aos 85% e
fazia com que as salas novas das outras organizações fossem recusadas. O Meet é um SaaS: um
inquilino não pode degradar a disponibilidade dos outros.

- **Só salas novas.** Uma sala que já está no nó admite sempre: não pode mudar de nó
  (ADR-0001) e expulsá-la dava o mesmo colapso que isto evita. É o molde da recusa por *drain*.
- **Sem capacidade declarada, nada se recusa.** Não se inventa um limite que o operador não deu.
- **Observável:** `delonix_node_new_rooms_refused_total` (todas) e
  `delonix_node_new_rooms_refused_fair_share_total` (as recusadas por parte justa; a diferença são as
  recusadas por o nó estar na capacidade). `accepting_new_rooms` no inventário do operador diz se o
  nó aceita salas novas de **qualquer** inquilino (abaixo do limite mole).
- **A ocupação conta os lugares em graça** (R91, 45 s): conservador de propósito.
- **A mensagem de recusa não diz quanto ocupam os outros inquilinos.**

## Tectos por conta e por organização (acrescentado a 2026-10-05)

A admissão por capacidade só decide salas **novas**; nada impedia uma conta, ou uma organização, de
encher uma sala que já existia. Dois tectos, ambos **por nó**, ambos com **429** (e não 503: a
recusa não é «este nó não serve», é «tu já tens o teu»):

- **Por conta** — `MAX_WS_PER_USER` (por omissão 16; `0` desliga): sockets `/ws` que uma conta pode
  ter ligados, em todas as salas do nó, **incluindo a sala de espera**. Não conta lugares em graça,
  bots, a ponte telefónica nem fontes de estúdio (identidades de serviço emitidas pela organização).
- **Por organização** — `organizations.max_concurrent_participants` (migração 0097), ou
  `ORG_MAX_PARTICIPANTS` por omissão. **Só o operador
  o fixa** (`PUT /api/operator/v1/organizations/{org_id}/concurrency`): é um limite do plano, e uma org
  que o pudesse subir não estaria limitada. Conta os participantes das salas da organização
  (inquilino, como acima), incluindo lugares em graça.

### A ausência deixou de significar «sem tecto» (2026-10-18)

Os dois tectos do nó eram lidos com `opt()`: **ausente valia `None`, e `None` não recusa ninguém**. O
`compose.yaml` não declarava nenhum dos dois, por isso essa forma de implantar aceitava a 41.ª pessoa
e degradava a sala para todos sem um sinal — contra um máximo **provado** de 40 em bancada
(`docs/ops/teste-de-carga-2026-10-05.md:29`: 11,2 de 16 núcleos, 0,03 % de perda).

Passaram a ter **três estados declarados**, em `config.rs` (`tecto()`):

| ambiente | significado |
|---|---|
| ausente | a omissão do código — **nunca «sem tecto»** |
| `0` | sem tecto, **de propósito**: é o que uma rampa de carga põe |
| outro inteiro | o valor; um não-inteiro continua a dar pânico no arranque |

A omissão do nó é `DEFAULT_NODE_PEER_CAPACITY` = 50, a capacidade do deploy de **referência** — não um
número novo. A da organização **deriva** dela: 80 % (`ORG_SHARE_OF_NODE`), que é a regra que os três
`values` já aplicavam à mão (25→20, 50→40, 50→40). Com o tecto do nó **desligado** (`0`) não se deriva
nada: não há capacidade de onde tirar uma parte, e inventar uma contradizia o `0` que o operador pôs.

Três consequências que custaram correcções na mesma sessão:

- o ConfigMap do chart **emite `0` tal e qual**. Antes omitia a variável quando o valor era `0` (porque
  o servidor entrava em pânico com um inteiro não positivo) — e com a ausência a valer 50, `0` passaria
  a significar o oposto do que diz, com a rampa a medir contra um tecto que o operador julgava ter
  desligado;
- as guardas de capacidade do `_helpers.tpl` viviam **dentro** do `if gateway.enabled` e por isso nunca
  corriam no caminho por omissão do chart (ingress). Saíram de lá: o tecto não tem que ver com a forma
  como o tráfego entra;
- os tectos em vigor são **ditos no arranque**, com `declarado = true|false`. O inventário do operador
  publica o número mas não de onde veio, e depois de gravado um tecto por omissão é indistinguível de
  um declarado.

O `scripts/check-tectos-de-admissao.sh` recusa uma forma de implantar que não declare os dois.

**Quem volta ao seu lugar não é uma entrada nova.** Uma entrada com `?reconnect=` só fica isenta se o
segredo reclama de facto um lugar em graça desta sala (`Hub::seat_reclaimable`, comparação em tempo
constante); um segredo inventado não isenta. Sem isto, uma quebra de rede com a org no tecto custava o
lugar a quem já estava.

**Cada recusa conta-se** (`delonix_ws_refused_user_cap_total`, `delonix_ws_refused_org_quota_total`) e
ambos os tectos falham **abertos** se a base não responder, como o resto das regras por inquilino.

**Limites conhecidos, ditos de propósito:**

- A contagem é **por nó**: com N nós uma organização pode chegar a N vezes o seu tecto. Um tecto
  global exige um contador partilhado (Redis) que ainda não existe.
- A verificação é feita **antes** do upgrade e a entrada acontece depois: um rebentamento de ligações
  simultâneas pode ultrapassar o tecto por tantas quantas as que estavam em curso. O tecto é mole por
  esse número, não por mais.
- Um utilizador individual sem organização só existe como caminho defensivo: toda a conta pertence a
  uma organização (a edição pessoal cria uma de tipo `personal`).

## O que esta decisão NÃO resolve

**A sala recusada não é realojada noutro pod.** Com a afinidade por hash, um novo pedido para
a mesma sala cai no mesmo nó e é recusado outra vez; por isso a mensagem do 503 não promete
«tentar noutro nó» (ao contrário da do *drain*, em que o pod já saiu dos endpoints). O que
esta decisão faz é proteger as salas que já estão no nó, não colocar a recusada.

### Desenho do realojamento (por fazer)

Uma **época de colocação** por sala, partilhada pelos pods (Redis), entrada na chave do hash:
`upstream-hash-by: "$arg_room$arg_epoch"`. Um pod cheio que recusa uma sala nova incrementa a
época (atómico) e devolve-a; o cliente volta a ligar com a época nova e cai noutro pod; os
restantes participantes obtêm a época no `POST /api/rooms/{code}/join`, para toda a gente
convergir no mesmo pod.

Pré-requisitos antes de o fazer, e porque não entrou agora:

1. Muda o ingress, o `Service` dedicado e a fitness function `check-room-affinity.sh`
   (as quatro camadas do ADR-0001), e o cliente web (`signaling.ts`).
2. **Não se prova sem um ingress real** a fazer consistent hash com a época na chave: o que se
   consegue provar no processo é a decisão do servidor, não o encaminhamento.
3. Corrida: dois pods a recusar a mesma sala ao mesmo tempo têm de acabar na mesma época.

### O que a justiça por inquilino também não resolve

**Não impede que as salas que um inquilino já tem cresçam até à capacidade**, e quando o nó lá
chega ninguém abre salas novas. Isso pede um tecto por inquilino (uma quota de participantes
concorrentes por organização em `org_quotas`, e um tecto de ligações `/ws` por conta, que hoje não
existem): é uma decisão de produto (números por plano, por nó ou global) e fica por fazer. A
justiça só garante que, **enquanto o nó está na zona de margem**, um inquilino não fecha o nó às
salas novas dos outros.

## Alternativas consideradas

- **Recusar também participantes de salas existentes acima de um tecto duro.** Rejeitada
  por agora: sem realojamento a sala não tem para onde ir e a recusa só tira gente a uma
  reunião que está a decorrer. Um tecto duro pode justificar-se depois de medir onde começa
  a degradação.
- **Autoescalar pela ocupação em vez do CPU** (HPA com `peers / capacidade`). Complementar,
  não alternativa: escala o número de pods mas não protege um pod já cheio.

## Prova e limites

- **Domínio** (10 testes): limites com capacidade 100, 10, 7 e 200, parte justa exacta
  (49 passa, 50 não), inquilino sozinho, capacidade total, sem capacidade.
- **Integração por `/ws` a sério** (4 testes), incluindo **duas organizações**: capacidade 7, a
  organização A usa 5 e a B usa 1 (nó a 6, na margem): a sala nova da A é recusada por parte justa
  e a da B é aceite; uma sala existente continua a admitir. E uma organização sozinha só é recusada
  na capacidade total.
- **Controlos negativos:** (1) com a zona de margem a recusar toda a gente (a regra global) falham
  os dois testes de margem; (2) com todas as salas a contar como o mesmo inquilino falha o teste das
  duas organizações. As mutações foram revertidas.
- **Os 85% são uma escolha, não uma medida.** O teste de 17/09 mediu o colapso mas não onde
  começa a degradação.
- **Não está provado que isto evita o colapso medido.** Isso pede o `loadgen` no mesmo nó, com e
  sem a regra, num host livre.
- **Não testado:** o utilizador individual (sem organização) como inquilino, e um dono em várias
  organizações; está no código mas não num teste.

## Ligado no deploy — 2026-10-13

Esta decisão esteve **oito dias no código e desligada na instalação**: o levantamento de
2026-10-13 mediu que `NODE_PEER_CAPACITY` não aparecia em nenhum ficheiro de `deploy/`, logo
`config.node_peer_capacity` era `None`, nada se recusava, e o alerta
`MeetSalasRecusadasPorCapacidade` não podia disparar. Um mecanismo testado que nunca corre é
uma capacidade afirmada e não provada — a mesma falha que o `check-capability-claims.sh`
persegue na documentação.

O que mudou:

- **Produção declara 50 participantes por nó** (`values-production.yaml`), derivados do limite
  de memória do pod — ~73 MB de RSS por participante, medido em
  [`docs/ops/teste-de-carga-2026-10-05.md`](../ops/teste-de-carga-2026-10-05.md) — e **40** de
  tecto por organização (80% da capacidade) para quem não tem tecto próprio.
- **O chart recusa `nodePeerCapacity: 0` em produção** (`_helpers.tpl`), com controlo negativo
  em `scripts/check-helm.sh`: não declarar capacidade deixou de ser uma omissão silenciosa.
  O laboratório fica em 25, que é o tecto do pod pequeno.
- **Os limites do pod subiram para 4 CPU / 4Gi**, porque a capacidade declarada tem de caber
  no pod: com os 2Gi anteriores o OOM chegava aos ~27 participantes, e com 1Gi (o que o chart
  tinha por omissão e o `deploy/k8s` ainda tinha) aos ~11.

**Continua sem prova, como antes:** que a regra evita o colapso medido — isso pede o `loadgen`
no mesmo nó, com e sem a regra. E agora também: que 50 é o número certo em Kubernetes, porque
a medição que o derivou foi feita em metal, em loopback, sem TURN e sem browsers.
