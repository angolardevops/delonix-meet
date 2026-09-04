# ADR-0003 — Emissão em directo para plataformas externas (RTMP)

**Estado:** Aceite (2026-08-26) · **Data:** 2026-08-26 · **Contexto:** pedido de produto — «o Estúdio deve fazer directo para as principais ferramentas do mercado, com convidados, e o anfitrião a controlar a sala do evento»

## Contexto

O pedido é o do StreamYard: um anfitrião abre uma sessão, convida gente, controla
quem está no ar, e a composição vai para o YouTube / Twitch / Facebook / LinkedIn.

### O facto que decide o desenho

**Nenhum browser fala RTMP.** Não existe API para abrir um socket RTMP a partir de
uma página. O StreamYard também não o faz no browser: manda WebRTC para os
servidores dele, e são esses que republicam com ffmpeg.

Consequência: por muito trabalho que se faça no frontend, **a emissão é
obrigatoriamente uma capacidade de servidor.** Isto não é uma preferência de
arquitectura — é o que a plataforma web permite.

### O que já existe neste repo, medido

Contra `origin/feat/estudio-offline`, a 2026-08-26:

| Peça | Estado |
|---|---|
| SFU em Rust, ingere RTP por participante | existe (`server/src/sfu.rs`) |
| Gravação server-side conduzida por ffmpeg | existe (`server/src/recorder.rs`) |
| Composição em grelha + mistura de áudio | existe (ffmpeg, `-filter_complex`) |
| Travões de CPU para o ffmpeg | existem: `FFMPEG_THREADS` (2), `FFMPEG_TIMEOUT_SECS` |
| Compositor de vídeo no browser (ecrã + câmara + recorte) | existe (`web/src/studio/compositor.ts`) |
| Sala fixada a UM pod | **ADR-0001** — afinidade por `hash(room)` |

E as duas medições que mudam a conclusão:

1. **O pod do servidor tem `limits.cpu: 1000m`** — um core
   (`deploy/k8s/02-server.yaml`). Uma codificação H.264 contínua a 1080p30 gasta,
   sozinha, entre um e dois cores.
2. **A gravação server-side só sabe VP8 e Opus** (`recordable_codec`, `sfu.rs:1480`).
   Qualquer outro codec faz o depacketizer de VP8 devolver lixo — mas isso **já
   está guardado**: a track é EXCLUÍDA da gravação e o facto sai em
   `tracing::error!`, em dois sítios (`sfu.rs:915` e `:1364`). Não é um buraco
   aberto; é uma fronteira declarada, e é essa fronteira que a emissão vai ter
   de alargar.

### A tensão real

Uma gravação é trabalho **diferível**: os ficheiros por participante são escritos
durante a chamada e o ffmpeg compõe no fim, com `-threads 2` a travá-lo. Um
directo é trabalho **contínuo e não diferível** — se o encoder não acompanhar, o
directo entrecorta à vista de toda a gente.

E, por causa do ADR-0001, a sala que está em directo vive no **mesmo pod** que a
serve. Um encoder que sature o core degrada exactamente a chamada que está a ser
emitida — o pior raio de dano possível.

## Opções consideradas

### A — ffmpeg contínuo no pod do SFU

O `recorder.rs` mantém um ffmpeg vivo com `-f flv rtmp://…` em vez de compor no
fim. É o caminho mais curto em código.

- **A favor:** reutiliza o pipeline inteiro; zero peças novas; a media já está no
  processo.
- **Contra:** exige subir `limits.cpu` de 1 para 3+ cores em **todos** os pods do
  SFU, para uma capacidade que a maioria das salas nunca usa. E não há isolamento:
  o encoder e as chamadas partilham cgroup, e a saturação atinge primeiro a sala
  emitida.
- **Veredicto:** rejeitada por raio de dano, não por dificuldade.

### B — Serviço de emissão separado, que se subscreve como par

Um `delonix-broadcast` que entra na sala como participante de servidor, recebe as
tracks, compõe e emite. Escala e falha à parte do SFU.

- **A favor:** isolamento de CPU real; escala independente; um directo que rebenta
  não leva a chamada atrás.
- **Contra:** peça nova com ciclo de vida próprio; tem de ser agendado no mesmo
  nó ou pagar uma travessia de rede pela media; e continua a transcodificar
  (VP8 → H.264), que é o grosso do custo.
- **Veredicto:** correcta, mas cara — e só se justifica se a composição TIVER de
  ser feita no servidor.

### C — O browser compõe e codifica; o servidor só remultiplexa

O compositor do Estúdio **já compõe** ecrã + câmara + recorte de fundo num canvas.
Estendê-lo para incluir os convidados é trabalho de frontend que se paga a si
próprio (é a mesma superfície que o anfitrião já usa). O browser codifica em
**H.264** — que o WebRTC suporta — e envia UM fluxo. O servidor recebe-o e faz
`-c:v copy` para RTMP.

- **A favor:** o custo de CPU no servidor cai para **transcodificar só o áudio**
  (Opus → AAC), que é aritmética de brinquedo ao lado de um encode de vídeo. Cabe
  no core que o pod já tem. E o encode fica onde o interesse está: na máquina de
  quem apresenta, não na factura da plataforma, multiplicada por sessão.
- **Contra:** obriga o caminho de servidor a aceitar H.264, que hoje **não
  aceita** (`recordable_codec`, `sfu.rs:1480`). Depende da máquina do anfitrião — um portátil fraco
  emite pior. E o anfitrião passa a ser ponto único: se o browser dele engasga, o
  directo engasga.
- **Veredicto:** é a que muda a ordem de grandeza do custo, e a única que cabe no
  envelope de recursos actual.

### D — Media server de prateleira (MediaMTX, LiveKit Egress)

Delegar a emissão a um componente existente.

- **A favor:** não se escreve RTMP nem se mantém ffmpeg.
- **Contra:** é uma segunda pilha de media a operar, com o seu próprio modelo de
  autenticação e de inquilino — e a `ngolacloud-arch` é explícita em que o
  isolamento entre inquilinos não se delega a um filtro de configuração. Para uma
  plataforma que se vende como soberana e self-hosted, trocar o SFU próprio por
  uma caixa preta na parte mais visível do produto é um recuo de posicionamento.
- **Veredicto:** rejeitada por posicionamento, não por técnica.

## Decisão

**Adopta-se a opção C — composição e codificação no cliente, remultiplexação no
servidor — com a opção B como caminho de evolução declarado.**

Em concreto:

1. **O compositor do Estúdio ganha convidados.** Os fluxos dos convidados chegam
   pelo SFU que já existe e entram no mesmo canvas que hoje compõe ecrã e câmara.
   O anfitrião escolhe quem está no ar; quem não está, não é desenhado.
2. **O browser codifica em H.264 + Opus** e publica esse fluxo único.
3. **O servidor ganha um `broadcast.rs`** que recebe esse fluxo e mantém um ffmpeg
   por sessão com `-c:v copy` e `-c:a aac`, para um ou mais destinos RTMP.
4. **A chave RTMP é um segredo** e segue a regra do R43: nunca num tipo que derive
   `Debug`, nunca num log. Usa-se `signaling::Secret`, que já existe.
5. **O controlo do evento** (quem entra, quem vai ao ar, silenciar, expulsar) é
   função da sala e do papel de anfitrião que o SFU já modela — não se inventa um
   segundo modelo de permissões.

### Porque é que a opção C não é a batota que parece

O argumento contra é «estás a empurrar o custo para o cliente». É verdade, e é
deliberado: num directo há **um** anfitrião e N espectadores na plataforma
externa. Codificar uma vez, na máquina de quem já está a apresentar, é o sítio
onde essa codificação custa menos ao sistema inteiro. Fazê-la no servidor é
pagá-la por sessão, em infraestrutura que se aluga.

Se a premissa mudar — anfitriões em máquinas fracas, ou emissão sem ninguém a
apresentar — a opção B fica disponível **sem deitar fora** o trabalho de C: o
compositor de cliente e o `broadcast.rs` continuam a servir; só muda quem
alimenta o encoder.

## Consequências

**O que fica pior:**

- O caminho de media do servidor passa a ter de aceitar H.264. Hoje o
  `recordable_codec` **recusa-o** — correctamente, porque a gravação não o sabe
  escrever. Alargar essa fronteira é a primeira coisa a fazer, e tem de manter a
  propriedade que ela já tem: **um codec não suportado é recusado com erro
  escrito, nunca aceite para produzir lixo.**
- O anfitrião torna-se ponto único de falha do directo. Tem de haver um indicador
  honesto de saúde da emissão no ecrã dele — bitrate a cair, frames largados —,
  porque só ele pode agir.
- O E2EE e o directo são **mutuamente exclusivos**: emitir para o YouTube é, por
  definição, entregar a media a um terceiro. A interface tem de o dizer, não
  desligar o E2EE em silêncio.

**O que fica melhor:**

- O custo de servidor por sessão em directo passa de um encode de vídeo para um
  transcode de áudio, e cabe no `limits.cpu: 1000m` actual.
- A composição do directo é a MESMA superfície do Estúdio, que já existe e já
  está testada por pixéis.

**O que este ADR não decide:**

- Quantos destinos RTMP em simultâneo, e se são um ffmpeg por destino ou um `tee`.
- Se o directo grava em simultâneo (provavelmente sim, e de graça: o mesmo fluxo).
- Latência e recuperação: o que acontece quando o RTMP do YouTube cai a meio.

Cada um é um ADR próprio ou uma decisão de implementação com portão medido.

## Portão de aceitação

Estado a 2026-08-26, com a primeira camada (servidor) feita:

| # | Linha | Estado |
|---|---|---|
| 1 | Codec não suportado recusado com erro escrito | **feito** — `pode_emitir`, 2 testes |
| 2 | Sessão de 30 min a 1080p não passa de `limits.cpu: 1000m` | **por medir** — precisa de emissão real |
| 3 | A chave RTMP não aparece em nenhum log | **feito** — `Debug` explícito, 2 testes |
| 4 | Sala com E2EE recusa emitir, com razão | **feito** — `Recusa::E2ee`, 2 testes |
| 5 | Perder o destino não derruba a chamada | **parcial** — `escrever` devolve erro em vez de pendurar; falta o caminho de cima |

Nenhuma destas linhas se dá por cumprida sem medida:

1. Um codec não suportado no caminho de gravação/emissão continua a ser
   **recusado com erro escrito**, nunca aceite. O `recordable_codec` já garante
   isto para a gravação; a emissão herda a mesma regra, e há um teste que a
   guarda. (Verificado a 2026-08-26: a guarda existe e regista em
   `tracing::error!` — a primeira versão deste ADR dizia, erradamente, que
   falhava em silêncio.)
2. Uma sessão em directo de 30 minutos a 1080p30 não faz o pod passar de
   `limits.cpu: 1000m`, medido com `kubectl top` durante a emissão.
3. A chave RTMP não aparece em nenhum log, verificado com o portão que já existe
   para segredos (R43).
4. Uma sala com E2EE ligado **recusa** iniciar directo, com razão escrita.
5. Perder o destino RTMP a meio não derruba a chamada — a sala continua, o directo
   reporta que caiu.
