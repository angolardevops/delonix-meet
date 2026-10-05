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

Com `NODE_PEER_CAPACITY` declarada, o `/ws` recusa com **503** as salas que **ainda não
existem no nó** quando a ocupação chega a **85%** da capacidade (`NEW_ROOM_LOAD_PERCENT`;
regra pura em `domain::operations::media_node::accepts_new_rooms`).

- **Só salas novas.** Uma sala que já está no nó admite sempre: não pode mudar de nó
  (ADR-0001) e expulsá-la dava o mesmo colapso que isto evita. É exactamente o molde da
  recusa por *drain*, que já distingue «sala nova» de «sala existente».
- **Os 15% que sobram** são a margem para as salas existentes continuarem a crescer.
- **Sem capacidade declarada, nada se recusa.** Não se inventa um limite que o operador não
  deu (a mesma regra de `load_ratio`).
- **Observável:** `delonix_node_new_rooms_refused_total` e `accepting_new_rooms` no inventário
  do operador. Um contador a subir é o sinal para acrescentar nós.
- **A ocupação conta os lugares em graça** (R91, 45 s): quem acabou de cair ainda ocupa
  capacidade. É conservador de propósito.

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

## Alternativas consideradas

- **Recusar também participantes de salas existentes acima de um tecto duro.** Rejeitada
  por agora: sem realojamento a sala não tem para onde ir e a recusa só tira gente a uma
  reunião que está a decorrer. Um tecto duro pode justificar-se depois de medir onde começa
  a degradação.
- **Autoescalar pela ocupação em vez do CPU** (HPA com `peers / capacidade`). Complementar,
  não alternativa: escala o número de pods mas não protege um pod já cheio.

## Prova e limites

- 4 testes de domínio (limites com capacidade 10 e 200, sem capacidade, capacidade 0,
  contagem negativa) e 2 de integração por `/ws` a sério (capacidade 4): sala nova recusada
  com 503 e contada; sala existente continua a admitir; com o nó vazio a sala nova entra;
  sem capacidade nada é recusado. **Controlo negativo:** com a recusa desligada falha
  `a_full_node_refuses_new_rooms…` e o outro teste passa.
- **Os 85% são uma escolha, não uma medida.** O teste de 17/09 mediu o colapso mas não onde
  começa a degradação.
- **Não está provado que isto evita o colapso medido.** Isso pede o `loadgen` no mesmo nó, com
  e sem a regra, num host livre (as corridas em que se tentou tinham a carga a 56–65).
