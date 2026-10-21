# Teste de carga — o tempo de entrada numa sala, 2026-10-21

Medido a 2026-10-21, das 10:00 às 10:16, contra a `develop` em **`2099024a`**, na máquina `kaeso-sys-01` (32 threads).
O servidor estava fixado em `taskset -c 0-7,16-23` e o gerador nos outros 16 threads. **A máquina estava partilhada**: a carga média andou entre 17,8 e 25,2 durante toda a medição. Os números absolutos valem para esta máquina e esta hora. O que se compara é base contra variante, intercaladas, nas mesmas condições.

**Pergunta:** o `reevaluate_room_except` percorre os peers **em série**, com `.await` (`server/src/sfu.rs:1826-1837`). É isso que faz a entrada piorar com o tamanho da sala (lacuna `B-06` da fila de paridade)?

**Resposta: não, nesta medição.** Tornar o laço concorrente (`join_all`) não baixou o tempo até ao primeiro vídeo em nenhum degrau, e aos 40 piorou-o. A subida acompanha a ocupação da máquina, não o número de reavaliações em série. A alteração **não foi entregue**: o diff ficou no worktree da medição como registo, e a razão está aqui.

## Duas coisas que se julgavam medidas e não estavam

1. **O «0,3 s com 8 / 1,7 s com 40» não é tempo de entrada de ninguém.** É o `entrada_s` do loadgen, o tempo que o **gerador** leva a lançar os clientes, com `--join-gap-ms 40` entre cada um (`server/examples/loadgen.rs:861-864`). A conta bate com o relatório de [2026-10-05](teste-de-carga-2026-10-05.md) em todos os degraus: 8 × 40 ms = 0,32 s, 40 × 40 ms = 1,6 s. O [levantamento de 2026-10-13](../levantamento-2026-10-13-concorrencia-escala-stack.md) (linha 239) atribuiu esse número ao laço em série, e o `B-06` herdou a atribuição. O tempo que uma pessoa espera até ver alguém é o `primeiro_video_ms`, que o loadgen já media e o relatório de 10-05 não publicou.
2. **Desde 2026-10-05 a rampa corta aos 16 clientes.** O `MAX_WS_PER_USER` (omissão 16, `925bd1eb`) recusa o 17.º `/ws` com **429**, e o loadgen entra com uma só conta. A primeira passagem desta medição, sem a variável, deu 240 fluxos de vídeo em todos os degraus de 24 a 40, e 24 clientes com `HTTP error: 429` no log do gerador. Lido à pressa, parecia que a sala colapsava às 24. Ficou escrito no cabeçalho do `loadgen.rs` e do `loadgen-rampa.sh`: o servidor de uma rampa arranca com `MAX_WS_PER_USER=0`.

## Resultados

Uma sala, 720p **sem simulcast** (como em 10-05), 20 s por degrau, `--join-gap-ms 40`. Duas voltas, com a ordem base, variante, base, variante. Os binários foram conferidos por `sha256`: base `d3d561971ca06d51`, variante `1c410a6624e17dc0`.

`pv50`/`pv95` são o percentil 50 e 95 do `primeiro_video_ms`, o tempo desde que o cliente arranca até receber o primeiro pacote de vídeo de alguém. «Máquina» é a ocupação total do anfitrião, servidor + gerador + trabalho alheio.

| Pessoas | base v1 pv50 / pv95 | base v2 pv50 / pv95 | variante v1 pv50 / pv95 | variante v2 pv50 / pv95 | Máquina |
|---|---|---|---|---|---|
| 8 | 67 / 117 ms | 68 / 121 ms | 76 / 124 ms | 99 / 150 ms | 8–33 % |
| 16 | 77 / 124 ms | 76 / 136 ms | 87 / 151 ms | 91 / 205 ms | 15–39 % |
| 24 | 86 / 418 ms | 133 / 504 ms | 114 / 294 ms | 193 / 734 ms | 36–53 % |
| 32 | 268 / 2033 ms | 352 / 1494 ms | 114 / 1951 ms | 449 / 3252 ms | 53–75 % |
| 40 | 826 / 3979 ms | 906 / 3046 ms | 868 / 5361 ms | 1279 / 6016 ms | 92–98 % |

Em todos os degraus: todos os fluxos de vídeo esperados activos (56, 240, 552, 992 e 1560), zero PeerConnections falhadas e zero clientes com erro.

### O que a tabela diz

- **Até 16 pessoas, a entrada é plana:** primeiro vídeo em ~70–90 ms, nas duas variantes. O laço em série com 16 peers não se vê.
- **A subida começa aos 24 e explode aos 40**, e é aos 40 que a máquina passa os 90 %. Entre as duas voltas da base, o p95 aos 40 varia de 3,0 s a 4,0 s, com a mesma máquina e o mesmo binário. Esse é o tamanho do ruído.
- **A variante concorrente não ganha em nenhum degrau** com uma margem acima desse ruído, e aos 40 perde nas duas voltas. Uma leitura plausível: concorrência dentro da mesma tarefa não dá CPU a ninguém, só faz as 40 renegociações competirem no mesmo instante. Isto é inferência, não está medido.

## O que isto não prova

- **Não é uma máquina limpa.** A carga alheia (17–25) contamina sobretudo os degraus de 32 e 40. A medição de 10-05 esperou por `loadavg ≤ 10`; esta não, porque o A/B intercalado responde à pergunta «a variante ajuda?», e não a «quanto custa a entrada».
- **Não separa o servidor do gerador.** Aos 40 a máquina está saturada pelos dois. Um gerador noutra máquina, que é o que o `A-04` precisa, diria quanto da subida é do servidor.
- **Sem simulcast e em loopback**, como em 10-05: é o pior caso de débito, e os clientes não são o Chrome.
- **Um só desenho de concorrência** foi experimentado (`join_all` na mesma tarefa). Reavaliar só os peers afectados por uma entrada, em vez da sala inteira, é outro desenho, e não foi medido.

## Como repetir

```bash
# servidor (MAX_WS_PER_USER=0 é obrigatório: o gerador usa uma só conta)
DATABASE_URL=… REDIS_URL=… BIND_ADDR=127.0.0.1:8280 DELONIX_ALLOW_INSECURE=1 COOKIE_INSECURE=1 \
AUTH_RATE_PER_MIN=5000 SFU_UDP_MIN=50600 SFU_UDP_MAX=50999 NODE_PEER_CAPACITY=0 MAX_WS_PER_USER=0 \
  taskset -c 0-7,16-23 ./target/release/delonix-server
# gerador, um degrau
taskset -c 8-15,24-31 ./target/release/examples/loadgen --api http://127.0.0.1:8280 --rooms 1 \
  --per-room 40 --server-pid <pid> --media .carga/media --no-simulcast --duration 20
```
