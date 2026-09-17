# ADR-0014 — Estúdio de TV num PC: o que corre no browser, no servidor e num agente local

**Estado:** Proposto · **Data:** 2026-09-17 · **Contexto:** template Navegavel5, seis ecrãs
novos (Sources, Switcher, AudioMixer, Lighting, StudioLive, PhoneCam) — «três telefones,
uma capturadora e o PC bastam: corte, som e luz vivem nesta janela».
**Relaciona-se com:** ADR-0001 (sala fixada a um pod), ADR-0003 (directo: o browser compõe,
o servidor remultiplexa), ADR-0005 (agente USB do SMS: o padrão do agente local),
ADR-0006 (camadas e contrato).
**Contrato:** [`docs/reference/estudio-tv.md`](../reference/estudio-tv.md).

## Contexto

O template desenha uma régie de televisão: seis fontes com tally, mesa de corte com T-bar
e transições, mesa de som de 16 canais com EQ e dinâmica, iluminação DMX e Hue, gravação
por câmara (ISO), directo para 5 destinos, e uma app de telefone que serve de câmara.
A pergunta deste ADR não é «como se faz isto», é **onde corre cada peça**.

### O que já existe, medido (2026-09-17, `origin/main` `03fecb9`)

| Peça | Estado |
|---|---|
| Compositor do Estúdio no browser (`web/src/studio/compositor.ts`) | canvas a 30 fps (`requestAnimationFrame`), `captureStream`, `AudioContext`, três `MediaRecorder` (completo, só vídeo, só áudio). Compõe ecrã + câmara; não tem corte entre N fontes nem EQ — mas a plataforma dá `BiquadFilterNode`, `DynamicsCompressorNode` e `GainNode`, que é tudo o que a mesa de som desenhada precisa |
| Directo (`server/src/broadcast.rs`) | WebSocket `/api/rooms/{room_code}/live`: o browser empurra Matroska H.264+Opus, o servidor faz `-c:v copy -c:a aac` para N destinos RTMP (R230). **Não expõe estado nenhum**: nem bitrate, nem hora de início, nem forma de parar a partir de outro ecrã |
| Gravação no servidor (`recorder.rs` + `sfu.rs`) | o SFU já escreve **um ficheiro por publicação** (VP8→IVF, Opus→OGG) durante a gravação; no fim compõe uma grelha e **apaga os ficheiros por publicação**. Só VP8 e Opus (`recordable_codec`) |
| Identidade sem conta | não existe token de sala sem utilizador; o `sub` do JWT é sempre uma conta |
| Agente local | `sms-gateway/` (ADR-0005): binário à parte, liga-se para fora com token `dlxg_`, reporta inventário, reclama trabalho, devolve resultado |
| Armazenamento de objectos (MinIO/S3) | **não existe** no servidor; as gravações vão para `RECORDINGS_DIR` (volume local/NFS) |

### Duas medições que decidem a gravação ISO

Nesta máquina (32 threads, carga média 7–15 durante a medição), quatro faixas VP8
1280×720 a 30 fps de 60 s (15 MB cada) e uma faixa Opus:

| Operação | Tempo de parede | CPU (user) |
|---|---|---|
| **ISO:** 4 × remux `-c copy` para WebM + 1 × cópia do Opus | **0,54 s** | 0,37 s |
| **Agregado multi-faixa:** 3 vídeos + 1 áudio num só WebM por `-c copy`, com `-itsoffset` por faixa | **0,11 s** | 0,10 s |
| **A composição de hoje:** `xstack` de 4 entradas em VP9 CRF 30, `-threads 2` | **45,8 s** | **275 s** |

Duas conclusões:

1. Guardar cada fonte à parte é **~500× mais barato** do que a composição que o servidor
   já faz hoje, porque não recodifica.
2. **O `-threads 2` do recorder não trava o encoder.** Está posto antes dos `-i`, por isso
   aplica-se aos descodificadores; o `libvpx-vp9` com `-row-mt 1` gastou 275 s de CPU em
   45,8 s de parede — **seis cores**, não dois. A composição actual degrada as chamadas vivas
   do mesmo pod muito mais do que o comentário promete. Fica registado aqui e no relatório;
   não se corrige nesta frente (é o caminho da gravação de toda a gente, não só do estúdio).

## Decisão

### 1. Browser — tudo o que é processamento de sinal em tempo real

Corte PGM/PRÉ, transições (cortar, misturar, limpar, stinger), T-bar, sobreposições
(legenda, logótipo, relógio, sondagem), mistura de som (ganho, EQ de 4 bandas, porta,
compressor, limitador, barramentos, medição LUFS), correcção de imagem por câmara
(exposição, temperatura, contraste, realce de rosto) e a execução das macros correm **no
browser do operador**, sobre o compositor que já existe.

- **Porquê:** é o mesmo argumento do ADR-0003, agora com mais peso. O ADR-0001 fixa a sala
  a um pod de um core. Um corte com transições a 1080p30 é um encode contínuo; uma mesa de
  16 canais com EQ e dinâmica são 16 cadeias de DSP contínuas. Qualquer uma delas no pod
  degrada exactamente a chamada que está a ser produzida. Na máquina do operador custam
  zero à plataforma e a latência de controlo é zero (o T-bar não pode esperar por uma ida
  e volta à rede).
- **O servidor não recodifica vídeo que o browser já compõe.** O programa sai pelo
  `/live` como já sai.
- **O servidor guarda** as cenas de mistura, as macros, as sobreposições, os perfis de
  correcção e o alinhamento (§5) — é persistência, não processamento.

### 2. Servidor — identidade, coordenação, gravação por cópia, estado

#### 2.1 A app Delonix Câmara entra como FONTE, sem conta

- **Código de emparelhamento** gerado pelo operador: `XXXX-XXXX`, Crockford base32
  (40 bits). Os 4 primeiros localizam a linha, os 4 últimos são o segredo, comparado em
  tempo constante contra um SHA-256. Validade 10 min, uso único, **5 tentativas erradas
  queimam o código**. Com localizador + segredo, a tentativa conta contra UM código em vez
  de ser um palpite livre contra todos; a rota pública tem ainda rate-limit por IP.
- **Token de fonte:** JWT com `typ: "source"`, `sub` = id da fonte (não uma conta),
  `room` = sala do estúdio, 12 h. Um `typ` novo e não um token de sala com uma bandeira: todas
  as rotas existentes exigem `typ` `room` ou `access`, por isso **nenhuma o aceita por
  construção** — o âmbito mínimo não depende de alguém se lembrar de verificar uma bandeira.
- **No `/ws`**, uma fonte entra sem sala de espera, nunca é anfitriã, e só pode mandar
  mensagens de media do SFU e as duas do estúdio (estado e resultado de comando). É uma
  lista de **permitidas**, não de proibidas: uma mensagem nova do protocolo da sala nasce
  fechada para fontes.
- **Revogação:** apagar a fonte revoga-a na base (o upgrade confere) e expulsa o socket vivo.

#### 2.2 Tally, comandos e estado do telefone pelo WebSocket da sala

Mensagens novas no protocolo da sala (contrato §4). **Só o anfitrião ACTUAL da sala do
estúdio** (`hub.is_host`, lido do hub e não do token, para sobreviver a um
`transfer-host`) fixa o tally e manda comandos. O servidor valida os intervalos de cada
comando antes de o encaminhar — o telefone é o executor, não o validador. O estado do
telefone (bateria, temperatura, rede) vai para os anfitriões e é persistido com tecto de
uma escrita por segundo (a REST de outro pod lê o último estado conhecido, com a hora).

Não há gRPC nem canal novo: o telefone já tem de estar no `/ws` para negociar a media com
o SFU, e é aí que o tally chega com a latência mínima.

#### 2.3 Gravação ISO: o SFU grava, por cópia

**Rejeitada:** o browser envia cada faixa (multipart/resumível). Obrigaria o PC do operador
a subir N fluxos de vídeo em simultâneo com o directo, e perdia as faixas quando o browser
do operador cai — exactamente o que a gravação ISO existe para evitar («o corte não se
perde»). Os telefones já publicam para o SFU; o SFU já escreve um ficheiro por publicação.

**Adoptada:** numa sala de estúdio com `iso_recording`, ao finalizar:
- cada faixa de vídeo fica num WebM próprio (`-c copy`), e cada faixa de áudio num OGG
  próprio, **antes de qualquer mistura** (é o requisito «faixas separadas na gravação»);
- a gravação agregada (a linha de `recordings`) é **um WebM multi-faixa por cópia**, com
  os títulos das fontes e o desfasamento de cada uma (`-itsoffset`). **Não** se compõe a
  grelha VP9 numa sala de estúdio: o programa é do browser, e a grelha custaria seis cores
  por pod para um produto que ninguém pediu;
- cada ficheiro regista-se em `recording_tracks` (fonte, número, codec, tamanho,
  desfasamento).

Custo aceite: o espaço duplica (as faixas + o agregado). Mitigação possível, não feita:
servir as faixas por extracção a pedido do agregado. Fica por decidir com números de uso.

Fronteira herdada: só VP8 e Opus. Uma app que publique H.264 fica fora da gravação com erro
escrito — igual a hoje.

Destino: o volume de `RECORDINGS_DIR`, com o espaço livre real (`statvfs`). **MinIO não
existe no servidor**; o contrato diz `not_configured` em vez de desenhar um destino falso.

**Ligação ao #93** (gravações no modelo da UI, por fundir): `recording_tracks.recording_id`
referencia `recordings(id)`, e o acesso reutiliza `recordings::load_item`. Quando o #93
entrar, o item da biblioteca ganha a contagem de faixas; não se antecipa aqui.

#### 2.4 Directo: estado e paragem

O `/live` passa a medir o que o servidor consegue medir de facto: bytes recebidos, bitrate
na janela de 5 s e p95 do tempo de escrita no ffmpeg. **O atraso até ao espectador não é
medido** (é da plataforma) e o contrato devolve `null` — o «atraso 8 s» do template não se
inventa. Com Redis, o estado espelha-se com TTL e a paragem é pedida ao pod que emite
(o `/live` não tem afinidade por sala no ingress, por isso o pedido REST pode cair noutro pod).

### 3. Agente local — o que só a LAN do estúdio alcança

**O servidor não vê a LAN do estúdio**, tal como não vê o USB (ADR-0005 §1). DMX por
Art-Net é UDP para um nó na rede do estúdio; a bridge Hue responde em HTTP na LAN. Nenhum
dos dois é alcançável do pod — e abri-los ao pod seria abrir a rede do cliente à plataforma.

Adopta-se o padrão do ADR-0005, sem mudar nada no que ele decidiu:

```
 nó Art-Net ◄─UDP 6454─┐
                       [delonix-studio-agent] ──HTTPS (sai; sem ingress)──► [delonix-server]
 bridge Hue ◄─HTTP LAN─┘   inventário, transições      token dlxs_ da org      fila, cenas, auditoria
```

- **`studio-agent/`** — binário Rust à parte, como `sms-gateway/`. Liga-se para fora,
  reporta os aparelhos declarados/descobertos, reclama comandos, executa-os e devolve o
  resultado.
- **Protocolos abertos e documentados, nada inventado:** Art-Net 4 (`ArtDmx`, OpCode
  `0x5000`) e a API local v1 da bridge Hue. As transições são interpoladas no agente a 40 Hz
  (Art-Net) ou delegadas à bridge (`transitiontime`).
- **DMX não se descobre**: o mapa de canais é configuração do operador. A Hue descobre as
  lâmpadas, mas obter o `username` exige carregar no botão da bridge — um passo manual, uma
  vez por bridge, dito no README do agente.
- **No máximo uma vez**, como no SMS: um comando reclamado e não confirmado em 60 s passa a
  `expired` e não volta à fila. Uma cena de luz aplicada duas vezes é inofensiva, mas um
  «apagar tudo» que volta 5 minutos depois a meio do programa não é.
- **Capturadoras e câmaras que o browser não enumera** (SDI, NDI, câmaras sem UVC) são do
  mesmo agente, **mas não nesta fase**: o browser já enumera UVC (telefones por USB e as
  capturadoras HDMI→USB comuns entram como webcam). Fica EXTERNAL e por decidir com um caso
  real.

### 4. Autorização

| Acção | Quem (hoje, com `org::role_in_org`) | Capacidade proposta |
|---|---|---|
| Ver estúdio, fontes, documentos, estado | membro activo | `studio.view` |
| Operar (códigos, documentos, comandos de luz, fontes) | admin da org ou quem criou o estúdio | `studio.operate` |
| Criar/apagar estúdio e agentes | admin da org | `studio.manage` |
| Tally e comandos ao telefone | anfitrião actual da sala | (sala) |

As capacidades ficam propostas para o catálogo da frente A (`require_capability`, numa
branch local por fundir); a verificação desta frente usa o que está na `main`.

### 5. Persistência: documentos versionados por estúdio

Cenas de mistura, macros, sobreposições, cenas de luz, perfis por câmara e alinhamentos são
**documentos** do estúdio com corpo tipado e validado no domínio
(`delonix-meet-domain::studio`), versão monotónica, histórico, e concorrência optimista
(`409` quando a versão não é a actual — dois operadores a gravar a mesma cena não se apagam
em silêncio). Uma tabela para os seis tipos, porque o ciclo de vida é idêntico; a forma de
cada um é do domínio, não da base.

## O que fica de fora (e é dito)

- Processamento de sinal no servidor (corte, mistura, correcção) — por decisão, §1.
- Atraso da plataforma no directo — não mensurável pelo servidor.
- MinIO/S3 como destino das gravações.
- Capturadoras SDI/NDI e câmaras não enumeráveis pelo browser — EXTERNAL.
- Hardware real de DMX e Hue — os adaptadores são provados contra um receptor Art-Net e uma
  bridge falsos; a prova real é EXTERNAL.
- Correcção do `-threads` do recorder — medida e registada, fora do âmbito.

## Consequências

- Um passo manual no caminho do cliente para a luz: instalar o agente e, para a Hue,
  carregar no botão da bridge. É um bloqueio para produção, não para a demonstração
  (como no ADR-0005).
- O PC do operador passa a ser o ponto único do corte e da mistura. A gravação ISO no SFU é
  precisamente a rede de segurança: se o browser cair, os ângulos não se perdem.
- Numa sala de estúdio com ISO, a gravação agregada deixa de ser uma grelha legível num
  leitor qualquer: é um WebM com N faixas de vídeo (os leitores mostram a primeira). O
  editor do Estúdio é quem escolhe o ângulo.

## Portão de aceitação

| # | Linha | Estado |
|---|---|---|
| 1 | Um código errado 5 vezes fica queimado; o certo depois disso é recusado | por provar |
| 2 | O token de fonte não abre nenhuma rota REST nem o `/live` | por provar |
| 3 | Uma fonte que manda `chat`, `set-role` ou `server-record` é recusada | por provar |
| 4 | Só o anfitrião actual fixa o tally e comanda; outro participante é recusado | por provar |
| 5 | Um telefone (cliente webrtc-rs) emparelha, publica, recebe tally e comando, devolve estado | por provar ao vivo |
| 6 | A faixa ISO dessa fonte fica em ficheiro próprio, legível por `ffprobe` | por provar ao vivo |
| 7 | Nenhum documento, fonte, código ou agente de outra org é alcançável | por provar (`isolamento.mjs`) |
| 8 | O agente produz `ArtDmx` válido e o pedido Hue certo | por provar (falsos) · hardware EXTERNAL |
