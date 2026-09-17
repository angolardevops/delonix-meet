# Teste de carga — quantas chamadas aguenta uma máquina

Medido a 2026-09-17 contra `origin/main` `4ff5249`, num portátil (AMD Ryzen 9
8940HX, 16 núcleos/32 threads, 30 GB). **Não é um número de produção**: o
gerador de carga correu na mesma máquina e havia outras cargas a correr ao mesmo
tempo (ver «O que isto não prova»).

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
