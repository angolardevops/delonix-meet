# Teste de carga — onde uma sala quebra, 2026-10-21

Medido a 2026-10-21, das 12:22 às 12:37, contra a `develop` em **`19263ba1`**, na máquina `kaeso-sys-01` (32 threads, 30 GB). O servidor estava fixado em `taskset -c 0-7,16-23` e o gerador nos outros 16 threads. **A máquina estava partilhada**, com carga alheia entre 15,8 e 35,5. Clientes WebRTC reais do `server/examples/loadgen.rs`, 45 s por degrau, `MAX_WS_PER_USER=0` e `NODE_PEER_CAPACITY=0`.

**Isto é a medição que o `A-04` pedia: continuar a rampa acima dos 40 até a sala reprovar.** O dono decidiu correr tudo nesta máquina, sabendo que o resultado ficava contaminado. É isso que esta página diz: onde **esta máquina**, com gerador e servidor juntos, deixa de aguentar. Não diz onde o servidor quebra.

## O resultado, numa linha

**Com clientes como os browsers (simulcast), a sala reprova aos 40 participantes, e não por capacidade.** Usa 2 núcleos e a máquina fica a 22 %, mas faltam 8 a 15 % das subscrições de vídeo: há pessoas que não vêem algumas das outras. Sem simulcast, que é o pior caso de débito, a sala aguenta os 40 e reprova aos 48 por falta de CPU na máquina. O primeiro limite real do produto é um **defeito de correcção**, que passa a ser o `B-12` da fila de paridade.

## Resultados

Critério do repositório (`server/examples/loadgen-avaliar.py`): fluxos de vídeo ≥ 98 % dos esperados (≥ 90 % com `CRITERIO=media`), perda < 2 %, jitter p95 < 30 ms, nenhuma PeerConnection falhada. Um degrau reprovado repete-se uma vez antes de parar.

### Sem simulcast (720p em todos os fluxos, como em 10-05)

| Pessoas | Fluxos | Perda | Jitter p95 | Servidor | RSS | Gerador (atrasos) | Máquina | Veredicto |
|---|---|---|---|---|---|---|---|---|
| 40 | 1560/1560 | 0,01 % | 10,0 ms | 13,97 núcleos | 2635 MB | 11,3 (0,0 %) | 91 % | OK |
| 48 | 2238/2256 | 34,5 % | 158 ms | 12,10 | 3445 MB | 15,6 (6,1 %) | 97 % | inconclusivo (gerador saturado) |
| 48 | 2252/2256 | 38,8 % | 205 ms | 12,08 | 3734 MB | 15,3 (3,3 %) | 94 % | **reprova** |

**O recurso que satura é a CPU da máquina, não a do servidor.** Aos 48, o servidor teve **menos** núcleos do que aos 40 (12,1 contra 14,0), com a máquina entre 94 e 97 % e a carga alheia entre 30 e 35. A memória (3,7 GB) e o débito em loopback não estão perto do limite. O custo aos 40 é de **0,35 núcleos por participante** em média (13,97 / 40), contra os 0,28 marginais que se inferiram em 10-05. São máquinas e cargas diferentes, por isso os dois números não se comparam.

### Com simulcast (q/h/f, como um browser)

| Pessoas | Entrada | Fluxos | Perda | Servidor | Máquina | Veredicto |
|---|---|---|---|---|---|---|
| 40 | 40 ms | 1324/1560 (85 %) | 0,0 % | 1,93 núcleos | 22 % | **reprova** |
| 40 | 40 ms | 1441/1560 (92 %) | 0,19 % | 2,27 | 25 % | **reprova** |
| 48 (`CRITERIO=media`) | 40 ms | 2020/2256 (90 %) | 0,0 % | 3,79 | 38 % | **reprova** |
| 48 (`CRITERIO=media`) | 40 ms | 2015/2256 (89 %) | 0,0 % | 3,96 | 46 % | **reprova** |
| 48 | 300 ms | 1927/2256 (85 %) | 0,0 % | 3,00 | 31 % | **reprova** |
| 48 | 300 ms | 1644/2256 (73 %) | 0,0 % | 2,52 | 27 % | **reprova** |

O que **não** falha é a media: zero perda, jitter de 1 a 16 ms, e um custo de servidor **seis a sete vezes menor** do que sem simulcast (1,9 contra 14,0 núcleos aos 40). O que falha é a **presença** dos fluxos.

## O defeito que apareceu

- **São subscrições em falta.** Nos clientes incompletos, a última oferta do servidor já trazia menos vídeos do que publicadores. Quem não está ali é o SFU, que não subscreveu o cliente, e não o cliente, que não tem erros de negociação (`erros_nego` vazio). Cada cliente incompleto fica a ver 32 a 38 dos 39 vídeos possíveis aos 40, e a falta **mantém-se** durante a corrida (`video_ativos_min` igual à média na primeira volta).
- **Não é uma corrida na entrada.** Com 300 ms entre entradas, em vez de 40, fica **pior** (73–85 %). É o «Defeito 2» do [relatório de 2026-09-17](teste-de-carga-2026-09-17.md), que lá «não reproduz». Aqui reproduz, em seis corridas seguidas.
- **Só aparece com simulcast.** Nos logs do servidor (`RUST_LOG=warn`), as corridas com simulcast têm `sfu layer switch failed` (10 a 25) e `sfu subscribe failed` (2 a 28), com `error=connection closed`. As corridas sem simulcast não têm nenhum. As 25 falhas de troca da corrida de 300 ms caem dentro da primeira volta, com a sala cheia, e não no fim, quando os clientes saem.
- **Hipótese, NÃO provada:** o recurso do `switch_layer` (`server/src/sfu.rs`). Quando o `replace_track` recusa, a pista é retirada e subscrita de novo; se essa subscrição falha, a falha conta-se e regista-se, e a subscrição fica perdida até à próxima reavaliação daquele peer. Explicaria porque só com simulcast, porque piora com mais trocas, e porque uma entrada mais lenta (mais trocas de camada a cada entrada) não ajuda. A causa e a correcção são o prompt [`B-12`](../../.claude/paridade/B-12-subscricoes-em-falta-com-simulcast.md).

## As três saídas do `A-04`, contra estes números

| Saída | O que resolve | O que parte, ou o que falta saber |
|---|---|---|
| (a) SFU em cascata entre pods | salas acima do que um pod aguenta | Move o RTP entre processos, contra o ADR-0001. **Não há número que o justifique:** com simulcast, um pod usa 2–4 núcleos aos 40–48 e o tecto de capacidade não se mediu, porque a correcção falha primeiro |
| (b) Audiência passiva (HLS) | 500 ouvintes num evento | Não resolve uma reunião de 120 activos. É o caminho do `A-09` (webinar), não deste |
| (c) Aceitar o tecto e vendê-lo | a honestidade agora | O tecto que se pode afirmar hoje é **40, e só sem o defeito**: com browsers reais, uma sala de 40 já perde vídeos |

**Recomendação: (c) agora, e nenhuma decisão de arquitectura antes do `B-12`.**
1. O material comercial diz **40 por sala**, sem prometer mais.
2. O `B-12` corrige as subscrições em falta. Enquanto ele estiver aberto, nem os 40 estão garantidos com browsers reais.
3. Depois de corrigido, repete-se esta rampa **com simulcast** e com o gerador noutra máquina. Com 2–4 núcleos aos 40–48, o tecto de capacidade com simulcast deve estar bem acima dos 48, mas isso é inferência. É esse número que decide se a (a) alguma vez é precisa.
4. Os eventos de centenas de pessoas vão pelo `A-09` e pela (b).

## O que isto não prova

- **Não é o tecto do servidor.** O gerador e mais 15–35 de carga alheia partilham a máquina. Aos 48 sem simulcast, o gerador estava a 15,3–15,6 dos 16 núcleos que lhe foram dados.
- **Os clientes não são o Chrome.** O simulcast do loadgen publica as três camadas sempre, e um browser adapta. O defeito pode aparecer de outra maneira, ou com outra frequência, com browsers.
- **Sem TURN e sem rede real**: loopback.
- **A causa do defeito não foi encontrada.** A hipótese do `switch_layer` assenta só na correlação com os avisos.

## Como repetir

O script está na pasta da medição, e os dados crus (JSONL por degrau, logs comprimidos) ficaram em `~/.cache/meet-medicoes/2026-10-21-a04-sala-grande/` nesta máquina:

```bash
# servidor: MAX_WS_PER_USER=0 e NODE_PEER_CAPACITY=0 são obrigatórios (ver o cabeçalho do loadgen.rs)
taskset -c 0-7,16-23 ./target/release/delonix-server
# um degrau com simulcast (sem --no-simulcast) e outro sem:
taskset -c 8-15,24-31 ./target/release/examples/loadgen --api http://127.0.0.1:8580 --rooms 1 --per-room 40 \
  --server-pid <pid> --media .carga/media --duration 45
taskset -c 8-15,24-31 ./target/release/examples/loadgen … --no-simulcast
```
