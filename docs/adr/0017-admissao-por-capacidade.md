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
  `ORG_MAX_PARTICIPANTS` por omissão (sem ele, uma org sem tecto próprio é ilimitada). **Só o operador
  o fixa** (`PUT /api/operator/v1/organizations/{org_id}/concurrency`): é um limite do plano, e uma org
  que o pudesse subir não estaria limitada. Conta os participantes das salas da organização
  (inquilino, como acima), incluindo lugares em graça.

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
