# Teste de carga — uma só reunião, 2026-10-05

Medido a 2026-10-05, das 23:21 à 23:27, contra a `develop` em **`803f4d46`** (o merge do #233),
no mesmo portátil de sempre: **AMD Ryzen 9 8940HX, 16 núcleos / 32 threads, 30 GB**. Servidor
fixado a 16 threads (`taskset -c 0-7,16-23`), gerador nos outros 16, base e portas UDP próprias.
Clientes WebRTC reais do `server/examples/loadgen.rs`, **720p sem simulcast** (`--no-simulcast`),
45 s por degrau, janelas de 5 s.

**Isto é a repetição que o [documento de 2026-09-17](teste-de-carga-2026-09-17.md) pedia** para a
reunião grande. Lá, uma só reunião era estável até 30 pessoas e **colapsava às 40, com 13 a 46 % de
perda** — o que esse documento atribuiu aos defeitos 1 e 2 do SFU (troca de camada simulcast e
subscrições em falta em entradas concorrentes). Aqui, 40 pessoas dão **1560 de 1560 fluxos
activos**.

**Não é um número de produção.** O gerador correu na mesma máquina, em loopback, sem TURN e sem
rede real; os clientes não são o Chrome. Ver «O que isto não prova».

## Resultados

Uma sala, todos com câmara 720p. Cada linha é a agregação das nove janelas do degrau.

| Pessoas | Fluxos de vídeo | Débito | Perda vídeo (média / pior janela) | Jitter p95 | CPU servidor | RSS | Entrada | Máquina |
|---|---|---|---|---|---|---|---|---|
| 8 | 56/56 | 95 Mbps | 0 % / 0 % | 0,26 ms | 0,46 núcleos | 201 MB | 0,3 s | 27 % |
| 16 | 240/240 | 407 Mbps | 0 % / 0 % | 1,08 ms | 1,91 | 577 MB | 0,7 s | 48 % |
| 24 | 552/552 | 941 Mbps | 0 % / 0 % | 1,16 ms | 3,28 | 1100 MB | 1,0 s | 34 % |
| 30 | 870/870 | 1467 Mbps | 0,01 % / 0,02 % | 9,04 ms | 7,01 | 1807 MB | 1,2 s | **90 %** |
| 36 | 1260/1260 | 2162 Mbps | 0 % / 0 % | 2,95 ms | 7,83 | 2524 MB | 1,5 s | 59 % |
| **40** | **1560/1560** | **2834 Mbps** | **0,03 % / 1,25 %** | 6,45 ms | 11,17 | 2929 MB | 1,7 s | 81 % |

Nenhuma PeerConnection falhada, nenhum cliente com erro e **zero erros do servidor** em todos os
degraus. Nenhum degrau foi reprovado pelo critério da rampa (fluxos ≥ 98 %, perda < 2 %, jitter
p95 < 30 ms, nenhuma PC falhada), por isso a rampa acabou por esgotar os degraus pedidos — **não
por encontrar o tecto**.

### O que as janelas dizem e a média esconde

- **Às 40 pessoas, a perda está toda na primeira janela** (1,25 %, com a máquina a 92 % e o
  servidor a 13,2 núcleos): é o custo de as 40 publicações nascerem ao mesmo tempo. Da janela 3 em
  diante, cinco das sete janelas dão **0,00 %**.
- **O degrau de 30 é o mais ruidoso dos seis** — jitter p95 de 9 ms e débito a oscilar entre 1194 e
  1938 Mbps, com a máquina entre 68 % e 96 %. O de 36, com mais pessoas, é mais estável (2,9 ms,
  débito plano a 2,1 Gbps). A diferença não é do servidor: é **carga alheia na máquina**, o que
  também se vê no degrau de 8, onde a ocupação salta de 13 % para 58 % sem o número de fluxos mudar.
- **O gerador nunca saturou**: ticks atrasados a 0,0 % em cinco degraus e 0,03 % no de 30. Nenhum
  degrau é inconclusivo por esse motivo.
- **Custo marginal do servidor**: ~0,06 núcleos por participante até às 24 pessoas, ~0,28 entre as
  30 e as 40. O RSS cresce de forma linear, ~73 MB por participante.

## Método

O arnês está em `.carga/corre.sh` do worktree da medição e não no repo: espera pelo fim do build,
**confere pelo `sha256` que o binário é da árvore medida** (a armadilha de um `target` copiado de
outro worktree, que já deu um binário de outra branch), espera por uma janela de `loadavg ≤ 10` em
três leituras seguidas, e só então arranca. A espera demorou doze minutos; a carga ao arrancar a
rampa era `8.48 9.59 7.43` e no fim `26.16 18.97 11.99`.

```bash
taskset -c 0-7,16-23 ./target/release/delonix-server    # DATABASE_URL/REDIS_URL próprios, SFU_UDP 50600-50999
OUT=resultados/grande.jsonl bash server/examples/loadgen-rampa.sh grande 0 "8 16 24 30 36 40" --no-simulcast --duration 45
```

Os dados crus (`grande.jsonl`, nove janelas por degrau) ficaram no worktree da medição. Não foram
versionados: são 200 kB de JSON que esta página resume.

## O que isto não prova

- **Não é o tecto.** A rampa acabou os degraus sem falhar nenhum. Onde a sala quebra, hoje, não se
  sabe — só que não é às 40, e que às 40 o servidor já usa 11 dos 16 threads que lhe foram dados.
- **Máquina partilhada, e isso contaminou os degraus de 8 e de 30.** A ocupação da máquina varia
  dentro do mesmo degrau sem o número de fluxos mudar. Os números são do servidor + gerador +
  trabalho alheio, não só do servidor.
- **Sem simulcast**, como em Setembro: é o pior caso de débito por fluxo e não é o comportamento
  normal de um browser.
- **Os clientes não são o Chrome.** O webrtc-rs não troca de papel ICE ao responder a uma oferta do
  SFU; o que um browser sofre a mais não aparece aqui.
- **Sem TURN e sem rede real**: loopback, sem perda nem latência de rede. 2834 Mbps em loopback não
  diz nada sobre 2834 Mbps numa placa de rede.
- **Sem gravação a decorrer.** O achado mais duro de Setembro — a composição a tornar as chamadas
  inutilizáveis quando partilha CPU com o SFU — **não foi reavaliado aqui**. A medição correu com
  `gravacao_salas: 0` em todos os degraus.
- **Uma só corrida por degrau.** Nada foi repetido, e com uma máquina que varia assim, uma corrida
  não separa o servidor do ruído.
- **Não se mediu mais de uma sala**, nem o cenário de muitas chamadas pequenas que a tabela de
  Setembro cobria.
