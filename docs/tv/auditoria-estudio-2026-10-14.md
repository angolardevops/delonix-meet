# Auditoria do Estúdio — mesa de som, emissão e paridade (2026-10-14)

**Base:** `origin/develop` `30ada1b6`. **Método:** leitura do código com `git show origin/develop:<path>`
e `git grep`; nenhum ecrã foi aberto no browser e nenhuma emissão foi feita nesta auditoria — o §9 diz
o que isso deixa por provar. **Antecedentes que esta auditoria NÃO repete:**
[auditoria-2026-10-03](auditoria-2026-10-03.md) (achados B1–B8), [RFC-0001](rfc-0001-estudio-e-estacao-de-tv.md),
[ADR-0014](../adr/0014-estudio-de-tv-num-pc.md), [ADR-0015](../adr/0015-channel-engine-ingest-e-distribuicao-de-tv.md)
e a onda 4 (TV1–TV13) do [plano de lacunas](../plano-lacunas-2026-10-04.md). O que aqui é **novo** está
marcado com `[NOVO]`; o resto confirma (ou desmente) o que já estava escrito, com a linha medida hoje.

## 1. Resposta curta

A mesa de som **não está desligada do estúdio — está ligada a metade dele**. Mistura o microfone local
e os convidados da sala; não vê as câmaras, o ecrã partilhado, a música, os clips do editor nem as
fontes do estúdio do servidor. E só entra no caminho do ar quando alguém abre o ecrã «Mesa de som» ou
«Cena completa» — uma emissão feita sem passar por lá vai ao ar pelo microfone cru do compositor.

Dos oito defeitos abaixo, **dois cortam o serviço** (EST-01 impede a emissão com destinos guardados;
EST-02 é SSRF, já conhecido como B2 e ainda aberto) e os restantes degradam o som ou mentem no número
que mostram. Nenhum deles precisa de arquitectura nova: são da ordem das dezenas de linhas, à excepção
do EST-07 (contrapressão) e do EST-08 (faixas ISO).

## 2. Porque é que a mesa parece desligada do estúdio

O caminho do som do estúdio é este (`studio/compositor.ts`), e a mesa é **uma fonte dentro dele**, não o
dono dele:

```
mesa de som ─┐                       (só quando ligada)
microfone ───┼─► ganhoPalco ─┐
convidados ──┘               │
música ──────► ganhoMusica ──┼─► limitadorMestre ─► destino ─► gravação e directo
clips/vídeo ─► ganhoVideo ───┘
ecrã (áudio do separador) ───┘
```

| # | Achado | Evidência (medida hoje) |
|---|---|---|
| EST-A | **As câmaras entram sem som.** `ligarCamara` pede `audio: false`; uma capturadora HDMI, uma câmara USB com microfone ou uma mesa física ligada por USB-áudio **nunca** dá um canal pela via das Fontes — só se a pessoa a adicionar outra vez, à mão, pela lista de microfones da mesa. | `web/src/studio/tv/fontes.ts:275` |
| EST-B | **O ecrã partilhado não é canal.** `TipoDeCanal` prevê `'ecra'` e **nada o cria**: o único `adicionarFluxo` em uso é o dos participantes. O som de um vídeo partilhado vai ao ar pelo barramento do compositor, sem fader, sem medidor, sem EQ e sem porta. | `mesaDeSom.ts:33`; `useSessaoTv.ts:288` (único chamador) |
| EST-C | **A música e os clips também não.** Vivem em `ganhoMusica`/`ganhoVideo` do compositor e controlam-se noutro ecrã (a mistura do palco). A mesa não os vê, logo não os pode baixar sob a voz nem levá-los no «guardar cena». | `compositor.ts` (barramentos), `usePalco.ts` |
| EST-D | **A mesa só existe se alguém abrir o ecrã dela.** `ligarSom()` é chamado no `useEffect` de montagem da «Mesa de som» e da «Cena completa» — mais nenhum sítio. Ir ao ar pelo Estúdio clássico, ou pela mesa de corte, emite com o microfone do compositor (com `echoCancellation` e `noiseSuppression` ligados, que é o certo para uma reunião e errado para uma emissão). | `MesaDeSomEcra.tsx:31`; `CenaCompleta.tsx:78`; `compositor.ts:890-893` |
| EST-E | **A ligação canal↔fonte é decorativa.** O selector «Fonte do canal» só serve o corte por voz. Não há *audio-follow-video* (abrir o canal quando a fonte entra no ar), não há ducking, e a tira não mostra tally. | `MesaDeSomEcra.tsx:316-318`; `useSessaoTv.ts:313-324` (único consumidor de `canal.fonte`) |
| EST-F | **As fontes do estúdio do servidor não chegam cá.** O backend tem estúdios, códigos de emparelhamento, fontes, tally e comandos (`lib.rs:1496-1520`, domínio em `studio/`), e o web **não consome nenhuma dessas rotas** (`git grep -i pairing web/src` = só o gateway de SMS). O próprio texto da interface assume-o: «Emparelhar o telefone por código precisa do servidor do estúdio». É o TV1, e continua aberto. | `lib.rs:1496`; `locales/pt/tv.ts:259` |

## 3. Defeitos medidos

Severidade: **C** corta o serviço · **G** degrada · **M** mente no que mostra.

| # | Sev | Defeito | Evidência | Correcção |
|---|---|---|---|---|
| EST-01 `[NOVO]` | C | **Um destino guardado nunca vai ao ar.** O botão habilita-se com `temChave` (chave escrita **ou** `temChaveGuardada`), e o arranque filtra por `dest.chave.trim()`. Um destino da organização, cuja chave vive cifrada no servidor e nunca volta ao browser — o caminho SEGURO, o do B8/TV2 — é descartado antes de sair. Com todos os destinos guardados, o pedido vai sem destinos e o servidor recusa com `SemDestino`: a pessoa vê «sem destino» num ecrã que lista destinos com chave. Mesma linha, mesmo defeito, no directo multicâmara da sala. | `Studio.tsx:585` vs `:682,686`; `destinosGuardados.ts:98`; `broadcast.rs:233`; `room/useMulticam.ts:98` | trocar o filtro por `temChaveDoDestino` nos dois sítios |
| EST-02 (=B2) | C | **SSRF por RTMP continua aberto.** `rtmp_url_is_valid` olha o esquema e o host não vazio; `net_guard` não aparece no `broadcast.rs` nem no `stream_destinations.rs`. O `ffmpeg` do servidor liga a qualquer host que o browser escrever — rede interna incluída. | `broadcast.rs:194`; `git grep net_guard -- server/src/broadcast.rs` = vazio | passar o URL pelo `net_guard` (existe e é usado nos webhooks) |
| EST-03 `[NOVO]` | M | **O LUFS mostrado não é o que sai.** A captura de sonoridade está no mestre da mesa; depois dela o som ainda passa pelo `ganhoPalco`, pela música, pelo vídeo e pelo **limitador mestre do compositor**. O número do cabeçalho («−14.0 · alvo −16») descreve um sinal que não é o emitido. | `mesaDeSom.ts:150-184`; `compositor.ts:816` | medir no fim da cadeia (TV5) |
| EST-04 `[NOVO]` | G | **Tudo em mono.** As restrições de canal pedem `channelCount: { ideal: 1 }`. Uma mesa física, um leitor de música ou um retorno de programa estéreo são dobrados para mono à entrada. | `mesaDeSom.ts:325` | `ideal: 2` por canal, com *pan* na tira |
| EST-05 (=TV5) | G | **Sem atraso por canal.** `git grep createDelay -- web/src` = 0. Não há como alinhar um som que chega antes da imagem (o caso normal: a câmara atrasa, o microfone não), nem compensar os ~51 ms do canal com redução de ruído ligada contra um canal cru. | `git grep createDelay` = vazio | `DelayNode` por canal, ±200 ms |
| EST-06 (=TV5) | G | **Sem limitador no mestre da mesa.** `pgm → mestre → destino`, sem dinâmica. O limitador que existe é o do compositor, a jusante de tudo — o que torna impossível medir o que a mesa entrega. | `mesaDeSom.ts:150-155` | limitador no mestre + medição depois dele |
| EST-07 (=TV3) | G | **Nada adapta o débito à banda.** `comecar()` é chamado sem opções: 4,5 Mbps fixos, fatias de 500 ms, `audioBitsPerSecond` por declarar; `git grep bufferedAmount -- web/src` = 0, portanto nada trava o envio quando o uplink não dá vazão — os pedaços acumulam na fila do WebSocket, o atraso cresce e a memória com ele. O perfil de qualidade escolhido no estúdio (até 2160p50) **não** chega ao codificador do directo. | `Studio.tsx:585`; `directo.ts:308`; `compositor.ts:419` | §5 |
| EST-08 (=TV7) | M | **«Uma faixa por canal» diz «à espera do servidor».** A bandeira está no ecrã e não há gravação ISO por canal em lado nenhum. | `MesaDeSomEcra.tsx:159`; `locales/pt/tv.ts:274` | retirar a bandeira ou implementá-la |

**O que FUNCIONA e não se mexe:** a cadeia por canal (ganho → EQ 4 bandas → porta → compressor →
limitador → fader → mudo → medidor), o solo como pré-escuta no AUX (não corta o ar — está certo e está
comentado), a porta por worklet, o RNNoise por canal, o guardar cena, o corte por voz, e os
participantes da sala a entrarem e saírem como canais sozinhos.

## 4. Mesa profissional: o que falta

### 4.1 Efeitos sonoros e sonoplastia
Não existe nada: `git grep -i "jingle|soundboard|sampler|sonoplastia"` = 0 (o «stinger» que aparece é a
transição de vídeo, não um som). Falta o óbvio de qualquer estúdio: um painel de teclas com ficheiros
(genérico, vinheta, aplauso, buzina), cada tecla a disparar um canal próprio da mesa, com fader, saída
para o ar e para o retorno, e *ducking* automático da música sob a voz.

### 4.2 Mesa física ligada por USB
Hoje: **uma mesa física já entra como ÁUDIO** — aparece na lista de microfones e vira canal, mono
(EST-04). O que **não** existe é o controlo: `git grep -i "requestMIDI|navigator.hid|OSC"` = 0 em todo o
repo. Não há como detectar uma superfície de controlo (X-Touch, Stream Deck, APC, qualquer coisa
MIDI/HID), nem mapear os faders dela aos faders da mesa, nem os botões ao corte, nem mandar de volta o
tally e a posição dos faders (feedback motorizado/LED). É a diferença entre «o PC tem uma mesa ligada» e
«a mesa comanda o PC».

### 4.3 O que uma mesa de emissão tem e esta não
Barramentos de grupo (sub-mixes), envios AUX extra (hoje há um, o dos auscultadores), *mix-minus* por
convidado e intercomunicação com os bastidores (`git grep -i "talkback|intercom"` = só a ponte
telefónica — é o TV6), *pan*, inversão de fase, filtro passa-alto por canal, e memórias de cena por
programa (há «guardar cena», mas numa só memória).

## 5. Qualidade e adaptação à banda — o que fazer

O caminho é `canvas.captureStream(fps)` → `MediaRecorder` H.264/Opus → WebSocket → `ffmpeg -c:v copy`
(ADR-0003). O `-c:v copy` é um contrato: quem controla a qualidade do ar é o **browser**, não o
servidor. Dentro desse contrato há três degraus, por ordem de valor:

1. **Contrapressão e débito adaptativo** (o pedido literal do dono). Ler `socket.bufferedAmount` a cada
   fatia; acima de um tecto (p. ex. 2 s de débito nominal), baixar o débito com
   `MediaRecorder` recriado ou — melhor — trocar o `MediaRecorder` por `VideoEncoder`
   (WebCodecs), que aceita `configure({bitrate})` a quente e dá controlo de keyframes. Mostrar no ecrã
   o que está a acontecer («a emitir a 2,8 Mbps — a rede não dá mais»), como o OBS mostra.
   Prova: emissão num uplink limitado com `tc`, com o débito a descer e a fila a não crescer.
2. **O perfil escolhido chega ao directo**: débito e fps derivados de `compositor.perfil`, não 4,5 Mbps
   fixos para tudo; keyframes a 2 s (o que as plataformas pedem) e `audioBitsPerSecond` declarado.
3. **Saída pelo servidor** (ADR-0015/TV11-12): com o Channel Engine, a adaptação deixa de depender da
   ligação do realizador — ele manda comandos, o servidor emite. É o único caminho que resolve também o
   B4 (a emissão morre com o separador) e o que permite entrada de OBS/SRT.

Na imagem, o que já existe e vale usar: correcção de imagem por fonte com GL, nitidez, medidas reais de
resolução/fps/latência por fonte. O que falta: escolher a resolução de captura por fonte (hoje
`1920×1080 ideal` para todas), e o *scaling* de qualidade no desenho do canvas.

## 6. Convidados externos no directo

Existe mais do que parece: a sala do estúdio cria a sala com **sala de espera ligada**, tem fila de
convidados, «Pôr no palco», chat, perguntas e sondagens (`SalaDoEstudio.tsx:1-14,63`). Um convidado
externo entra por código de sala e, com áudio, torna-se canal da mesa sozinho.

Falta, por ordem: **bastidores** (um convidado admitido que ainda não está no ar, a ser ouvido e
dirigido — `git grep -i bastidores|backstage` = 0), **intercomunicação** (falar com ele sem ir ao ar,
TV6), **mix-minus por convidado** (ouvir o programa sem se ouvir a si; só a ponte telefónica o tem),
**convite por ligação com papel** (hoje é o código de sala, o mesmo de uma reunião) e **tally para quem
está remoto** (saber que está no ar).

## 7. OBS: o que tem de melhor, e o que vale trazer

A coluna «OBS» é conhecimento público do produto, **não medido neste repo**; a coluna «nós» é medida.

| Capacidade do OBS | Nós, hoje | Vale trazer? |
|---|---|---|
| Débito dinâmico por congestão + reconexão automática | nada (EST-07) | **Sim, primeiro** |
| Painel de estatísticas (frames perdidos, atraso do codificador, congestão) | débito enviado, e por destino o que o servidor reporta | **Sim**, barato |
| Atraso de áudio por fonte (lip-sync) | nada (EST-05) | **Sim**, barato |
| Monitorização: desligada / só monitor / monitor e ar | AUX + solo PFL | já lá está, falta o terceiro modo |
| Filtros de áudio por fonte (gate, compressor, limitador, ganho, EQ) | **temos, e com RNNoise** | — |
| Multi-faixa de áudio na gravação (até 6) | bandeira a dizer «à espera do servidor» (EST-08) | **Sim**, é o ISO |
| Fonte de média (ficheiro/clip) e fonte de browser (HTML) | clips só no editor; sem fonte de browser | **Sim** — resolve §4.1 e os grafismos |
| Stinger de transição com som | transição existe, o som não | **Sim**, com §4.1 |
| Modo estúdio com T-bar | **temos** | — |
| Projector/multiview em segundo ecrã | nada (TV9) | Sim, depois |
| Atalhos globais e controlo externo (obs-websocket → Stream Deck/MIDI) | atalhos no teclado da página; sem API de controlo nem MIDI | **Sim** (§4.2) — e a API de controlo é a mesma que o Channel Engine precisa |
| Buffer de repetição (instant replay) | nada | só com cliente que o peça |
| Câmara virtual | nada | só com cliente que o peça |
| Chroma key / LUT por fonte | correcção de cor por fonte, sem chroma nem LUT | Sim, médio |
| Saída SRT/RIST, saída personalizada | só RTMP(S) (TV12) | Sim, com o ADR-0015 |
| Entrada WHIP/RTMP/SRT (contribuição) | nada (TV11) | **Sim** — é como um OBS entra COMO fonte |

**Onde já ganhamos ao OBS** e vale dizer alto: convidados remotos por WebRTC sem plugins (o OBS precisa
de um StreamYard ao lado), legendas automáticas, gravação na nuvem com transcrição e acta, editor não
destrutivo no mesmo produto, e multi-destino com uma só codificação. O OBS é um codificador local; o
Delonix Meet é um estúdio com sala, nuvem e arquivo. A paridade que interessa é a da **operação** (as
cinco primeiras linhas da tabela), não a do catálogo de plugins.

## 8. Plano, por ondas, com prova

**Onda A — a mesa fica ligada ao estúdio (dias, não semanas)**
| # | Trabalho | Prova |
|---|---|---|
| A1 | EST-01: o filtro de destinos passa a `temChave`, nos dois sítios | teste que vai ao ar só com destino guardado e vê o `id` no pedido |
| A2 | EST-A/B/C: câmara com áudio opcional, ecrã como canal, música e clips como canais | abrir a mesa com câmara USB, ecrã e música: 4 canais, cada um com medidor |
| A3 | EST-D: a mesa liga-se com o estúdio, não com o ecrã | ir ao ar sem abrir a mesa e ver os canais no `MediaStream` emitido |
| A4 | EST-04/05/06: estéreo, atraso por canal, limitador no mestre, LUFS medido no fim | ficheiro emitido medido com `ffmpeg -af ebur128` igual ao número do ecrã (±1 LU) |
| A5 | EST-E: tally na tira e audio-follow-video opcional | fonte que entra no ar abre o canal; prova no e2e do estúdio |

**Onda B — a emissão não mente e não entope**
B1 EST-07 (contrapressão + débito adaptativo + perfil, com aviso no ecrã) · B2 painel de estatísticas ·
B3 EST-02 (`net_guard`) · B4 reconexão automática de destino.

**Onda C — estúdio a sério**
C1 painel de efeitos com ducking · C2 bastidores + intercom + mix-minus por convidado (TV6) ·
C3 fonte de média e fonte de browser · C4 API de controlo do estúdio (a mesma que serve o Channel
Engine) e, por cima dela, MIDI/HID para mesa física.

**Onda D — o que já está decidido e escrito:** TV1 (ligar o backend do estúdio), TV4/ADR-0015 (emissão
que não morre com o separador), TV11/TV12 (entrada de OBS/SRT, saída HLS). Não se recomeça: está na
RFC-0001 e no ADR-0015, à espera de decisão.

## 9. O que NÃO foi validado

- **Nada foi aberto no browser** nesta auditoria: os defeitos são de leitura de código. EST-01 é o
  único com cadeia completa lida de ponta a ponta (ecrã → filtro → servidor) e merece um teste antes
  da correcção, para não se corrigir o sintoma errado.
- **Não se mediu som nenhum**: os ~51 ms do canal com redução de ruído vêm do cabeçalho do
  `mesaDeSom.ts`, escritos a 2026-10-13, e lá diz que «nenhuma das duas foi ouvida numa emissão».
- **Não se correu o `make fitness`** nem a bateria: este documento não muda código.
- **A coluna OBS é conhecimento público**, não medição; antes de a citar num texto de marketing,
  confirma-se versão a versão.
- **Não se auditou** o editor (`studio/edit/`), as legendas, nem a estação de TV do servidor
  (`tv_channels.rs`, `tv_broadcasts.rs`) — essa está na auditoria de 2026-10-03 e no TV13.
