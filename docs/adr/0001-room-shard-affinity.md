# ADR-0001 — A sala é a unidade de shard do SFU (afinidade por sala)

**Estado:** Aceite · **Data:** 2026-07-11 · **Contexto:** avaliação de arquitetura (Martin Fowler, ponto #5)

## Contexto

O SFU do Delonix Meet (`server/src/sfu.rs`) mantém o estado de media **em memória, por
pod**: `Hub → Room → Publication`, os `RTCPeerConnection`, as tracks e o fan-out RTP
vivem no processo. O Redis (`pubsub.rs`, `redis_state.rs`) propaga **eventos de
sinalização/presença e estado colaborativo** (chat, whiteboard, sondagens) entre pods —
mas **NÃO** transporta RTP.

Consequência inevitável: **todos os pares de uma mesma sala têm de ser servidos pelo
MESMO pod**, senão publisher e subscriber ficam em processos diferentes e a media não
flui (split-brain de pod). Isto faz da **sala a unidade natural de sharding** — o sistema
é, de facto, um *sharded actor system* fragmentado por sala, mesmo que essa abstração
nunca tenha tido um nome.

## Decisão

**A sala (`room` code) é a chave de shard.** A afinidade é imposta na borda:

1. **Ingress** (`deploy/k8s/04-ingress.yaml`): o caminho `/ws` usa
   `nginx.ingress.kubernetes.io/upstream-hash-by: "$arg_room"` → consistent-hash pelo
   argumento `room` da query string.
2. **Service DEDICADO** (`deploy/k8s/02-server.yaml`, `delonix-server-ws`): o `/ws` NÃO
   pode partilhar Service com `/api`/`/rtc`. Se partilhar, o ingress-nginx funde os
   backends num só e **DESCARTA** o `upstream-hash-by` (round-robin ganha) → a afinidade
   deixa de se aplicar. Esta é a regressão **R3**.
3. **Cliente** (`web/src/signaling.ts`): envia `/ws?...&room=CODE` para que o hash tenha
   por onde pegar.
4. **Presença (`/rtc`)** é fan-out por Redis → **não** precisa de afinidade.

A invariante em uma frase: **`hash(room) → sempre o mesmo pod`**. É guardada pela fitness
function `scripts/check-room-affinity.sh`.

## Consequências

- A afinidade **vaza** para quatro camadas (ingress, Service, cliente, e o modelo mental
  do operador de deploy). É complexidade acidental aceite conscientemente — o preço de um
  SFU próprio in-memory (ver [[architecture]] e `docs/reference/regressions.md` R3/R4).
- Escalar horizontalmente **não** se faz aumentando réplicas ingenuamente: cada sala
  continua ligada a um pod. O HPA (`21-server-hpa.yaml`) só distribui salas *novas* por
  pods; não reequilibra salas existentes.
- **Ponto de estrangulamento (Strangler Fig):** se um dia se migrar para um SFU
  horizontalmente escalável (LiveKit/mediasoup/SFU em cascata), a fachada `/ws` mantém-se
  estável enquanto se estrangula o SFU próprio por trás. A fronteira desta ADR é
  exatamente o *seam* dessa migração.
- Media K8s exige ainda coturn alcançável em relay-only (ver `regressions.md` R4) — é uma
  preocupação ortogonal à afinidade (a afinidade resolve o *split-brain*; o relay resolve
  a *alcançabilidade*).

## Alternativas consideradas

- **SFU stateless + media state em Redis:** inviável — RTP é um fluxo em tempo real, não
  cabe num store key-value; a latência mataria a experiência.
- **`sessionAffinity: ClientIP` no Service:** não resolve — afina por IP de cliente, e os
  dois lados de uma sala (SFU e browser) têm IPs diferentes; podiam cair em pods
  diferentes na mesma sala.
- **SFU externo (LiveKit) já:** decisão de produto é evoluir o SFU próprio (binário único,
  controlo do pipeline para E2EE + gravação side-car). Reavaliar quando a escala o exigir
  — esta ADR deixa o *seam* pronto.
