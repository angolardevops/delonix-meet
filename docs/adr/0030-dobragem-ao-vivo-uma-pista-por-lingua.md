# ADR-0030 — Dobragem ao vivo: uma pista por LÍNGUA, voz sintética de licença permissiva, e o nome certo para o que é

**Estado:** Proposto — **falta UMA decisão do dono** (o tecto de inferência por
sala). A voz ficou decidida a 2026-10-14: `pt_PT-tugão`, anunciada como
português de Portugal. ·
**Revisto a 2026-10-13** depois de medir as licenças: o Piper **não é MIT**, e
não existe voz pt-AO. ·
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

> **Correcção de 2026-10-13, medida.** A primeira versão deste ADR dizia «Piper
> (MIT)». **Já não é verdade**, e é a diferença entre uma decisão e uma
> suposição — por isso a tabela mudou e a §Decisões por dar também.

| Opção | Veredicto |
|---|---|
| **Piper** — `OHF-Voice/piper1-gpl` | **GPL-3.0**, não MIT. Desde a v1.3.0 o motor **embute o espeak-ng** (GPLv3) para fonemização, e o mantenedor concluiu que isso torna o Piper GPLv3. O repositório MIT antigo (`rhasspy/piper`) está **arquivado e só de leitura desde 2025-10-06**, e aponta para o novo |
| **Kokoro** — pesos Apache-2.0 | O mesmo alçapão: os pesos são Apache-2.0 e o código de inferência MIT, mas o caminho normal puxa o **espeak-ng (GPLv3)** para o G2P |
| Um G2P **não-GPL** (ex.: o `TugaPhone`, Apache-2.0, de uma afinação pt-PT do Kokoro) | É a única rota medida que evita a GPL de ponta a ponta. **Projecto de uma pessoa, por verificar**, e o lect é o de Lisboa |
| **Coqui XTTS** | **recusada.** O modelo sai sob CPML, que **proíbe uso comercial**. Vendemos este produto |
| TTS de cloud (Google, Azure, ElevenLabs) | **recusada.** A soberania é o pilar: o texto de uma reunião não vai a uma cloud externa, e esta seria a primeira vez |

**O que a GPL aqui obriga, e o que NÃO obriga.** A GPL-3.0 é sobre
**distribuição**: correr o Piper num serviço nosso não contamina o resto do
produto, mas **distribuir** a imagem obriga a oferecer o código-fonte
correspondente e a respeitar a GPL naquele binário. É exactamente a situação
que a decisão **D3** já resolveu para o ffmpeg: o servidor fica LGPL e a GPL
vive **confinada a uma imagem própria** (o Channel Engine). A dobragem segue o
mesmo molde — `tts-server` é a sua própria imagem, com a sua licença e a sua
oferta de código —, e por isso a GPL do motor **não é um impedimento**, é uma
obrigação de empacotamento que esta casa já sabe cumprir.

Duas regras que vêm da D3, e por isso não se discutem outra vez:

- **cada voz é um ficheiro fixado por versão e por SHA-256**, com a sua licença
  registada ao lado — como o código-fonte do ffmpeg na imagem do servidor. Uma
  voz é um artefacto de terceiros, não um detalhe de configuração. (Medido a
  2026-10-13: as cinco portuguesas são CC0, mas isso vale para as de HOJE —
  a regra existe para a que entrar amanhã);
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

1. **A licença do MOTOR, não das vozes.** As vozes estavam a ser o problema
   errado: medidas a 2026-10-13, as cinco vozes portuguesas do catálogo
   (`pt_BR`: cadu, edresson, faber, jeff · `pt_PT`: tugão) declaram todas,
   no seu `MODEL_CARD`, conjunto de dados **CC0** e afinação a partir da voz
   inglesa `lessac` média. Nenhuma pede atribuição. **O que é copyleft é o
   motor** (ver a tabela acima). A decisão é: aceitar a GPL-3.0 confinada a uma
   imagem própria, como a D3 fez com o ffmpeg, ou pagar o custo de uma rota de
   G2P não-GPL ainda por verificar.

   **E não existe voz pt-AO.** O `pt/` do catálogo oficial só tem `pt_BR` e
   `pt_PT` — nenhuma voz angolana, em lado nenhum que se tenha encontrado.

   ### ✅ DECIDIDO a 2026-10-14 pelo dono: `pt_PT-tugão`, e o produto diz que é de Portugal

   Das três opções que estavam em cima da mesa — usar a de Portugal e dizê-lo,
   encomendar a gravação de um locutor angolano, ou não oferecer português —
   fica a **primeira**. Três coisas passam a estar decididas com ela, e duas
   não são óbvias:

   **a) Escolher o `tugão` é escolher o Piper, e portanto a GPL-3.0.** O
   `tugão` é uma voz do catálogo do Piper; não existe fora dele. Logo a
   dobragem herda o motor da tabela acima e **o `tts-server` nasce como imagem
   própria com a sua oferta de código**, no molde da D3. Isto não é um
   corolário administrativo: é a razão pela qual a decisão da licença deixa de
   estar em aberto para o caso do português.

   **b) O rótulo é parte da decisão, não um detalhe de interface.** Onde a
   pessoa escolhe a língua lê-se **«Português (Portugal)»** — nunca
   «Português» sozinho. Num produto angolano, «Português» sem qualificação é
   lido como o daqui, e a voz não é daqui: deixar o rótulo nu seria vender
   sotaque de Lisboa como se fosse de Luanda. O crachá da sala diz as **duas**
   coisas: que a voz é sintética (§4) e que é de Portugal.

   **c) pt-AO fica por oferecer, e isso diz-se.** Não aparece na lista como se
   estivesse a caminho, nem se substitui por pt-BR. O desenho já deixa a porta
   aberta — a pista é **por língua** (§2), e `pt-AO` seria mais uma entrada no
   dia em que houver voz. A encomenda de uma voz angolana (2–3 h de gravação
   limpa, cessão de direitos escrita, afinação a partir de um modelo existente)
   continua a ser a via para lá chegar, e a cotação é de um **estúdio em
   Luanda**, não de um fornecedor de TTS.

   O `pt_PT-tugão` entra como qualquer artefacto de terceiros: **fixado por
   versão e por SHA-256**, com o `MODEL_CARD` (CC0, afinada da `lessac` média)
   registado ao lado.
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

## Onde isto foi medido (2026-10-13)

Não de memória — as páginas, para quem quiser refazer a conta:

- o Piper antigo, **arquivado e MIT**: <https://github.com/rhasspy/piper>
  («This repository was archived by the owner on Oct 6, 2025», «Development has
  moved»);
- o Piper actual, **GPL-3.0**: <https://github.com/OHF-Voice/piper1-gpl>
  («embeds espeak-ng for phonemization»), e a explicação do mantenedor na
  discussão #57 do mesmo repositório;
- o catálogo de vozes: <https://huggingface.co/rhasspy/piper-voices> —
  `voices.json` e as pastas `pt/pt_BR` (cadu, edresson, faber, jeff) e
  `pt/pt_PT` (tugão). **Não há `pt/pt_AO`**;
- as licenças de cada voz: o `MODEL_CARD` de cada uma (`pt_PT/tugão/medium`,
  `pt_BR/faber/medium`, `pt_BR/cadu/medium`, `pt_BR/jeff/medium`) — todas
  «License: CC0» e «Finetuned from U.S. English lessac voice (medium
  quality)»;
- o Kokoro e o mesmo alçapão do espeak-ng, e a afinação pt-PT com G2P
  Apache-2.0: <https://huggingface.co/logus2k/kokoro_tts_eu_pt>.
