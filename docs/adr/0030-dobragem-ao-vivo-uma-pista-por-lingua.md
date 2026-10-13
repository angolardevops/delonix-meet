# ADR-0030 — Dobragem ao vivo: uma pista por LÍNGUA, voz sintética de licença permissiva, e o nome certo para o que é

**Estado:** Proposto — **precisa de duas decisões do dono** (§Decisões por dar) ·
**Numeração:** era 0029 quando se escreveu; a `develop` ganhou entretanto a sua
(«a IA em produção passa pelo chart»), e esta renumerou-se — o número escolhe-se
ao abrir a PR. ·
**Data:** 2026-10-13 ·
**Contexto:** o dono pediu, a 2026-10-13, que a «tradução de áudio em tempo
real» fosse **dobragem com voz sintetizada** e não só legendas traduzidas ·
**Assenta em:** [ADR-0027](0027-superficie-mcp-para-agentes.md) §9 (a tool
configura; a media fica no SFU) e [ADR-0008](0008-papeis-capacidades-e-ambito.md).

## Contexto

Medido na `origin/develop` a 2026-10-13, antes de desenhar:

1. **Não há TTS em lado nenhum.** `piper`, `coqui`, `text-to-speech` e `\btts\b`
   não aparecem em `server/`, `web/src/`, `ai-worker/`, `whisper-server/` nem
   `deploy/`. Esta é a única das seis famílias que o dono pediu sem **nada**
   feito.

2. **A transcrição ao vivo já é soberana e já existe duas vezes.** No browser
   (Web Speech ou whisper WASM) e no `whisper-server/app.py`: `faster-whisper`
   por WebSocket (`/asr?lang=pt`), PCM Int16 mono a 16 kHz, com VAD por energia
   e respostas `interim`/`final`. O áudio nunca sai da infraestrutura.

3. **A tradução ao vivo é POR OUVINTE e vive no browser.** Cada cliente recebe
   a legenda e pede a sua língua a `POST /api/ai/translations`
   (`web/src/room/useTranscription.ts:104`), com *debounce* e um pedido em voo.
   Funciona para texto porque o custo é o do LLM local por frase; **não escala
   para áudio** — sintetizar por ouvinte multiplica a inferência pelo número de
   pessoas na sala.

4. **O SFU leva mais um publicador sem desenho novo.** É o que o
   [ADR-0026](0026-assistencia-remota-o-agente-e-o-consentimento.md) já disse
   do vídeo de quem assiste: simulcast, track separada, áudio nos dois
   sentidos. Uma pista de áudio sintetizada é um publicador como os outros.

5. **A capacidade já existe, e já se chama assim.** No catálogo do ADR-0008,
   `studio.generate_captions` é **«Gerar legendas e dobragem»**, com a dica
   «consome nó de inferência» (`authorization.rs:249`). O que falta é o código
   por trás do nome — e é por isso que este ADR existe em vez de a tool ser
   anunciada.

6. **A casa já sabe decidir uma licença: confina-a a uma imagem.** A decisão D3
   pôs o ffmpeg do servidor em LGPL e empurrou a GPL para a imagem do Channel
   Engine, com `scripts/check-ffmpeg-licenca.sh` a impedir que uma edição
   descuidada a mude sem ninguém decidir. É o molde que a voz sintética vai
   seguir.

## Decisão

### 1. O nome: **dobragem com atraso**, não interpretação simultânea

O caminho é uma soma de etapas, e cada uma tem o seu tempo:

| Etapa | O que custa |
|---|---|
| VAD + ASR até à `final` | a frase tem de acabar — é o maior termo, e não se encurta sem partir frases a meio |
| tradução pelo LLM local | um pedido por frase |
| síntese | proporcional ao texto |
| publicar no SFU e chegar ao ouvido | o de sempre |

Somado, a voz traduzida chega **segundos** depois do original. Chamar-lhe
«tempo real» é vender outra coisa: um intérprete humano sobrepõe-se ao orador,
isto não. O contrato escrito é **dobragem com atraso**, o atraso é **medido** e
mostrado na sala, e nenhum texto de produto diz o contrário.

### 2. Uma pista por LÍNGUA, nunca por ouvinte

O servidor publica **um publicador sintético por língua activa** na sala
(`pt→en`, `pt→fr`, …). Quem quer ouvir inglês subscreve essa pista e abafa o
original no seu cliente; quem não quer nada continua como hoje.

Porque não por ouvinte: dez pessoas a ouvir inglês são **uma** síntese, não
dez. A tradução de texto pode ser por ouvinte (etapa 3 do contexto) porque o
LLM local custa uma frase; a síntese de voz não.

Consequência que se aceita: a pista é a mesma para todos os que escolhem aquela
língua, e por isso **não leva voz de ninguém em particular** (§4).

### 3. A voz é sintética e de licença permissiva — e cada voz é um artefacto fixado

| Opção | Veredicto |
|---|---|
| **Piper** (MIT, ONNX, corre em CPU) | **a proposta.** Licença do motor compatível; muitas línguas; já empacotável numa imagem própria |
| **Kokoro** (Apache-2.0) | alternativa viável; melhor qualidade em inglês, cobertura mais estreita — e o que nos interessa é português (incluindo pt-AO) |
| **Coqui XTTS** | **recusada.** O modelo sai sob CPML, que **proíbe uso comercial**. Vendemos este produto |
| TTS de cloud (Google, Azure, ElevenLabs) | **recusada.** A soberania é o pilar: o texto de uma reunião não vai a uma cloud externa, e esta seria a primeira vez |

Duas regras que vêm da D3, e por isso não se discutem outra vez:

- **cada voz é um ficheiro fixado por versão e por SHA-256**, com a sua licença
  registada ao lado — como o código-fonte do ffmpeg na imagem do servidor. Uma
  voz é um artefacto de terceiros, não um detalhe de configuração;
- a dobragem vive na **sua própria imagem** (`tts-server`), e um portão
  (`check-tts-licenca.sh`) recusa uma voz sem licença escrita.

### 4. O que isto **não** faz: clonar a voz de quem fala

Clonar a voz de um participante é tratar a voz dele como **biometria**: exige
consentimento informado, prazo de retenção, e uma decisão de conformidade que
não é desta frente (BNA/LGPD, e a `ngolacloud-cyber-resilience` tem portão
próprio). A dobragem fala com uma voz **neutra e anunciada como sintética** —
e um crachá na sala di-lo, para ninguém confundir a voz com a pessoa.

### 5. Quem liga, quem paga, e o que a tool faz

- **Liga** o anfitrião da sala, com `studio.generate_captions` (a capacidade que
  já se chama «Gerar legendas e dobragem»). Não é por participante: é uma
  decisão da sessão, como a gravação.
- **Paga-se em inferência**: uma sala com cinco línguas são cinco cadeias
  ASR→LLM→TTS. Passa pelo mesmo tecto por organização do `ai_studio` e pelas
  vagas do `fair_slots` — a dobragem não é um caminho novo para esgotar o
  modelo partilhado de outra organização.
- **A tool MCP `room_live_translation_set` CONFIGURA** (liga, escolhe línguas,
  diz o estado e o atraso medido). Não transporta áudio: MCP leva JSON
  (ADR-0027 §9). E **não entra no catálogo antes do código** — é a segunda
  regra da casa.

## Decisões por dar (são do dono, não de quem escreve a tool)

1. **A licença e as vozes.** O Piper é MIT, mas **cada voz tem a sua**: algumas
   são CC0, outras CC-BY (que obriga a atribuição no produto) e outras herdam a
   licença do corpus. Escolher as línguas é escolher licenças — e para
   português de Angola pode não haver voz aceitável, caso em que a resposta
   honesta é não oferecer pt-AO em vez de usar uma voz de pt-BR a fingir.
2. **O tecto de inferência que se aceita por sala.** Cinco línguas numa sala de
   uma hora é uma conta de CPU (ou de GPU, ADR-0024) que alguém paga. Sem um
   tecto decidido, a primeira sala com oito línguas descobre-o em produção.

Até as duas serem dadas, **nada disto se constrói** — e a família «tradução de
áudio ao vivo» continua a ter, no produto, só o que existe de verdade: legendas
traduzidas por ouvinte.

## Consequências

- **Boas:** o atraso é um número medido e não uma promessa; a soberania
  mantém-se (nada sai do cluster); a licença fica confinada a uma imagem, como
  a GPL do ffmpeg; e a capacidade que o catálogo já anuncia passa a ter código.
- **Más:** é a frente mais cara deste programa — uma imagem nova, um serviço
  novo no caminho de media, e um custo de inferência por sala e por língua. E o
  caminho ASR→LLM→TTS tem três sítios onde uma avaria se vê como «a dobragem
  calou-se».
- **Risco que se nomeia:** uma dobragem que falha a meio é pior do que não a
  haver — quem estava a ouvir inglês fica em silêncio sem saber porquê. O
  desenho obriga a que a pista diga o seu estado (a sintetizar, em atraso,
  parada) e a que o cliente volte ao original automaticamente.
