# Teste de carga — quantas chamadas aguenta uma máquina

Medido a 2026-09-17 contra `origin/main` `4ff5249`, num portátil (AMD Ryzen 9
8940HX, 16 núcleos/32 threads, 30 GB). **Não é um número de produção**: o
gerador de carga correu na mesma máquina e havia outras cargas a correr ao mesmo
tempo (ver «O que isto não prova»).

> **Os números desta página são de 2026-09-17 e NÃO foram repetidos.** O
> gerador entra na árvore a 2026-09-24, muitos commits depois (`b6b769d`), e
> nessa altura a única coisa portada foi a ferramenta — nenhuma medição foi
> refeita. Em particular, o defeito que explica os 75–87 de 96 fluxos abaixo
> — publicações que morriam poucos milissegundos depois de nascer, com
> entradas concorrentes — está a ser corrigido à parte e ainda não fundiu.
> Enquanto não fundir, uma corrida nova mede a ferramenta E o defeito ao mesmo
> tempo. Volta a correr antes de citar qualquer número daqui, e diz contra que
> commit.
>
> **A reunião grande foi repetida a 2026-10-05** e os números dela estão em
> [teste-de-carga-2026-10-05.md](teste-de-carga-2026-10-05.md). As restantes
> tabelas desta página continuam sem repetição.

## Método

`server/examples/loadgen.rs` abre **clientes WebRTC reais** (webrtc-rs 0.17, o
mesmo do servidor), e cada um faz o caminho do browser: `POST
/api/rooms/{code}/join` → `/ws` → `joined` → oferta SFU com áudio Opus e vídeo
VP8 (simulcast q/h/f, ou só 720p com `--no-simulcast`) → trickle ICE → SRTP
real nos dois sentidos. A media vem de ficheiros pré-codificados
(180p ≈ 240 kbps, 360p ≈ 620 kbps, 720p ≈ 1,8 Mbps), por isso o custo do
gerador fica no DTLS/SRTP/RTP e não num encoder.

Por janela de 5 s mede: fluxos de vídeo activos contra os esperados, perda
(buracos de sequência), jitter RFC 3550, débito, CPU e RSS do servidor, CPU dos
`ffmpeg` filhos, ocupação total da máquina, e a **saturação do próprio
gerador** (ticks de envio atrasados). Sem este último valor, um degrau falhado
pode estar a medir o gerador e não o servidor.

```bash
# servidor fixado a 16 threads, gerador nas outras 16
taskset -c 0-7,16-23 ./target/release/delonix-server   # com DATABASE_URL/REDIS_URL próprios
cargo build --release --example loadgen
# gerar a media (uma vez): comandos ffmpeg no cabeçalho de loadgen.rs
CRITERIO=media OUT=resultados.jsonl \
  server/examples/loadgen-rampa.sh salas 4 "8 16 25 30 40 50" --no-simulcast --join-gap-ms 100
server/examples/loadgen-rampa.sh grande 0 "8 16 20 25 30 40" --no-simulcast
target/release/examples/loadgen --rooms 20 --per-room 4 --record-rooms 20 --duration 60 --post 600 --server-pid <pid>
```

A rampa sobe um degrau de cada vez e pára no primeiro que falhe duas vezes:
fluxos activos ≥ 98% (≥ 90% com `CRITERIO=media`), perda < 2%, jitter
p95 < 30 ms, nenhuma PeerConnection falhada. Se o gerador estiver saturado, o
degrau é marcado INCONCLUSIVO em vez de FALHA.

## Resultados

### Chamadas de 4 pessoas, 720p sem simulcast

| Chamadas | Pessoas | Vídeo encaminhado | Perda | CPU servidor | Fluxos |
|---|---|---|---|---|---|
| 8 | 32 | 180 Mbps | 0% | 1,0 núcleo | 96/96 |
| 16 | 64 | 322 Mbps | 0% | 1,9 | 192/192 |
| 25 | 100 | 480 Mbps | 0,1% | 2,5 | 300/300 |
| 30 | 120 | 560 Mbps | 0,05% | 3,2 | 345/360 |
| **40** | **160** | **811 Mbps** | 0,26% | 4,8 | 471/480 |
| 50 | 200 | colapso | 29–48% | 5,8 | 436/600 |

### Uma só reunião, todos com câmara 720p

Estável até **30 pessoas** (870 fluxos, 1,5 Gbps, 5,6 núcleos, 0% perda).
Com 40 pessoas colapsou (13–46% de perda).

> **Repetido a 2026-10-05** contra a `develop` `803f4d46`, na mesma máquina:
> 40 pessoas dão **1560 de 1560 fluxos** e 0,03% de perda média (2834 Mbps,
> 11,2 núcleos). O colapso das 40 desapareceu. Ver
> [teste-de-carga-2026-10-05.md](teste-de-carga-2026-10-05.md) — que também não
> reavaliou a gravação a decorrer.

### Gravação no servidor com chamadas a decorrer (20 chamadas × 4)

- **Durante a gravação** (escrita RTP → IVF/OGG): nos primeiros ~45 s a perda
  ficou ≤ 1% e não se perdeu nenhum pacote de gravação. Nos últimos ~15 s a
  perda subiu para 5–11%, com a máquina a 95% — não ficou separado se foi a
  gravação ou carga alheia.
- **Na composição** (20 `ffmpeg` xstack + VP9 em simultâneo, `FFMPEG_THREADS=2`,
  ~12 núcleos): as chamadas ficaram **inutilizáveis**, com 64% de perda média
  e só 74 de 240 fluxos vivos no pior momento. Cada gravação de 58 s demorou
  **13–15 min** a compor. As 20 gravações saíram válidas (VP9 1280×720 + Opus).
- Consequência: a composição não pode partilhar CPU com o SFU. Tem de ter um
  tecto global de composições simultâneas, ou correr noutro nó.

## Defeitos do SFU encontrados

1. **Troca de camada simulcast perde o vídeo.** `switch_layer` fazia
   `remove_track` + `add_track` com um id de track determinístico. Ao voltar a
   uma camada já usada, o webrtc-rs reutilizava um sender já esvaziado
   (`new track must have the same envelope as previous`), e o subscritor
   deixava de ver esse participante até sair da sala. Com simulcast,
   aconteceu com 2 chamadas. Há correcção numa branch à parte
   (`sfu-troca-camada`), ainda não fundida à data desta medição.
2. **Subscrições em falta quando várias pessoas entram quase ao mesmo tempo.**
   Com 8 salas × 4 e entradas a 40 ms, o gauge `delonix_sfu_subscriptions`
   ficou em 165 com 192 esperadas. As ofertas SDP do servidor já vinham sem
   essas m-lines. Com entradas espaçadas a 150 ms ficou 96/96. É isto que
   explica os «345/360» e «801/870» das tabelas acima. Em aberto.
3. **Retenção de memória e sockets UDP presos.** O RSS continuava em 219 MB
   180 s depois de 4 ciclos curtos, mas voltou a baixar ~30 min depois:
   retenção, e não uma fuga provada. Depois de corridas que colapsaram por
   contenção de CPU ficaram 359 sockets UDP abertos durante mais de 30 min, sem
   nenhum peer. Em aberto.

## O que isto não prova

- **Máquina partilhada.** Outras sessões compilavam Rust e corriam VMs (load
  até 75). Uma corrida de 30 chamadas foi invalidada e repetida, e o colapso
  das 50 coincidiu com uma compilação alheia e com descartes do kernel (~55 mil
  pacotes UDP/s, `RcvbufErrors`, com `rmem_default` a 212 KB).
- **O gerador partilhava a máquina.** Os limites de 50 chamadas e de 40 pessoas
  são da máquina inteira (servidor + gerador + outros processos), e não só do
  servidor. Não foi medido com clientes noutra máquina.
- **Os clientes não são o Chrome.** O webrtc-rs não troca de papel ICE ao
  responder a uma oferta do SFU, por isso a queda de ligação que o Chrome sofre
  (correcção na branch `sfu-envio-parado`) não aparece aqui. Até essa correcção entrar, os números são optimistas
  para browsers.
- **A capacidade foi medida sem simulcast** (por causa do defeito 1). É o pior
  caso de débito por fluxo, mas não é o comportamento normal de um browser.
- **Sem TURN e sem rede real:** tudo correu em loopback, sem perda nem latência
  de rede.
- **Intervalo UDP sobreposto:** até à reunião grande, o intervalo UDP do
  servidor (40000–49999) apanhava as portas de relay de um coturn local.
  Estes testes não usam TURN, mas o efeito não foi verificado.

## Reavaliação dos defeitos do SFU — 2026-10-04

Repetida contra a `main` em `b4ac7cc7` e contra a `develop` em `70ccd9bf`, na
mesma máquina (Ryzen 9, servidor fixado a 16 threads, gerador nos outros 16,
base e portas UDP dedicadas). Números de `server/examples/loadgen.rs` com
`--no-simulcast`. **As corridas da `main` foram feitas com `RUST_LOG=info`; as
da `develop` com o filtro por omissão** — a diferença conta (ver abaixo).

| Configuração | Execuções | Fluxos de vídeo | Subscrições (pico) | Perda máx. por execução | Carga do host |
|---|---|---|---|---|---|
| `main`: 8 salas × 4, entradas a 40 ms | 1 | 96/96 | 192/192 | 0% | 4 |
| `main`: 8 salas × 4, entradas a 0 ms | 3 | 96/96 | 192/192 | 0% | 4–5 |
| `main`: 8 salas × 4, entradas a 10 ms | 3 | 96/96 | 192/192 | 0% | 8–10 |
| `main`: 16 salas × 4, entradas a 10 ms | 3 | 192/192 | 384/384 | 0,09%, 0%, 9,3% | 15–27 |
| `main`: 16 salas × 4, entradas a 0 ms | 3 | 192/192 | 384/384 | 39%, 0%, 0% | 18–28 |
| `develop`: 8 salas × 4, entradas a 40 ms | 3 | 96/96 | 192/192 | 0% | 3–10 |
| `develop`: 16 salas × 4, entradas a 10 ms | 2 | 192/192 | 384/384 | 0%, 0,16% | 15–19 |

As perdas de 9,3% e 39% da `main` aconteceram com `RUST_LOG=info` e carga do
host entre 18 e 28: com esse filtro o `webrtc_srtp` escreve ~8 milhões de linhas
(`srtp … duplicated`, ~1 GB de log) no mesmo CPU, por isso **não medem o
servidor** e não foram repetidas sem o filtro. Não correr cargas com
`RUST_LOG=info`.

- **Defeito 1 (troca de camada simulcast).** Corrigido no código (R156, com
  `sfu_e2e` a percorrer f→h→f→q→f). **Não foi re-medido aqui**: estas corridas
  foram sem simulcast.
- **Defeito 2 (subscrições em falta em entradas quase simultâneas).** **Não
  reproduz**: 13 execuções na `main` e 5 na `develop`, até 16 salas × 4 sem
  intervalo de entrada, sempre com todas as subscrições. Não se sabe porquê:
  pode ter sido corrigido entre 17/09 e 04/10, ou o original pode ter sido
  efeito da carga de 75 que a máquina tinha na altura. Com esta máquina nunca
  chegou a 75 durante as corridas (máx. 28), por isso **a condição original
  não foi recriada**.
- **Defeito 3 (sockets UDP presos, memória).** Corrigido no código (R158). Com
  a `develop`, 25 s depois do fim: 0 PeerConnections, 0 presas
  (`delonix_sfu_pc_unclosed`), 0 sockets UDP. **Confirmação fraca**: não se
  provocou um colapso, que era a condição em que os 359 sockets ficaram presos.
  RSS 828 MB 25 s depois da carga (1076 MB noutra corrida): retenção, não uma
  fuga provada — não se esperou os ~30 min em que a corrida de 17/09 baixou.
- **Avisos no log.** 221 linhas `create_offer failed: connection closed` e
  `renegotiation timed out` durante as corridas da `develop`, sem efeito nos
  fluxos nem nas subscrições. Não se verificou que só ocorrem quando um cliente
  do gerador sai com uma renegociação pendente.

**Continua sem prova:** capacidade com browsers reais, com TURN e com rede real;
capacidade com simulcast; o limite de 4 CPU do pod (`deploy/k8s/02-server.yaml`)
sob carga; e clientes noutra máquina.

## Fecho da Sprint 2 — 2026-10-05

O critério de aceitação do plano era: fluxos de vídeo ≥98%, perda <2%, jitter
p95 <30 ms, nenhuma PeerConnection falhada e nenhum socket UDP preso 30 minutos
depois. Medido na `develop` (`70ccd9bf`), 5 execuções válidas (8 salas × 4 com
entradas a 40 ms, e 16 × 4 a 10 ms), sem simulcast, com a carga do host entre
3 e 19:

| Critério | Medido | Passa |
|---|---|---|
| Fluxos de vídeo activos | 100% (96/96 e 192/192) | sim |
| Perda de vídeo | máx. 0,16% | sim |
| Jitter p95 | máx. 1,2 ms | sim |
| PeerConnections falhadas | 0 clientes com erro | sim |
| Gerador de carga saturado | máx. 0,2% de ticks atrasados | sim (não invalida) |
| Sockets UDP presos 30 min depois | **não medido** (só 25 s: 0 sockets) | **por provar** |

**Fechada com reservas.** O que continua em aberto, por inteiro:

- **Defeito 2 do teste de carga (17/09): continua sem explicação.** O sintoma era
  `delonix_sfu_subscriptions` a 165 de 192 *no servidor*, com ofertas SDP já sem
  as m-lines. Não reproduz (18 execuções), e a condição original não foi
  recriada. A causa abaixo **não o explica**: ela deixa a subscrição feita no
  servidor.
- **Os 30 minutos do defeito 3** não foram esperados.
- **Simulcast** não foi medido (o defeito 1 foi corrigido no código, R156, e
  está coberto por `sfu_e2e`, mas estas corridas foram sem simulcast).

### Outra falha, de causa diferente: `entradas_concorrentes_todos_recebem_todos`

Este teste do `sfu_e2e` falhava ao acaso (CI da `main` vermelho a 29/09; ~8% das
execuções aqui). A assinatura é outra: as subscrições estão **todas feitas** e um
subscritor não recebe media, com `renegotiation timed out` (3×) e
`renegotiação falhou após 3 tentativas` no SFU.

A causa, medida com o tracing ligado, é do **cliente de teste** (webrtc-rs), não
do SFU: a oferta do servidor e a descrição remota anterior tinham o **mesmo**
`ice-ufrag`, mas o cliente leu a oferta como reinício de ICE («ICE Agent can not
be restarted when gathering») porque chegou antes de o transporte ICE registar as
credenciais remotas, e o PC ficou preso em `have-remote-offer`. Correcção na
PR #209 (o cliente responde com o ICE ligado). Controlo de 100 execuções cada:
8 falhas sem a correcção, 0 com ela (p ≈ 0,003).

O que isto **não** prova: que o SFU não sofra a mesma corrida do lado do
servidor com um browser a enviar uma 2.ª oferta muito cedo (não apareceu nas
falhas analisadas, não foi excluído); nem que o `loadgen` (também webrtc-rs) não
a tenha. Os `renegotiation timed out` das corridas de carga acima têm a mesma
assinatura de log e **não foram atribuídos**: na altura não se verificou se
eram a corrida do cliente ou clientes a sair com uma renegociação pendente.
