# RFC-0002 — TV digital por captura HDMI de descodificadores

**Estado:** Rascunho para decisão · **Data:** 2026-10-14 · **Base:** `origin/develop` `30ada1b6`
**Antecessora:** [RFC-0001](rfc-0001-estudio-e-estacao-de-tv.md) — canais que **nós produzimos**.
Esta RFC trata do caso simétrico: canais que **entram já produzidos** por um sinal HDMI externo.
**Decisões do dono do produto (2026-10-14):** agente sem interface na caixa de captura ·
distribuição em duas fases (sala pelo SFU primeiro, HLS depois) · zapping comandado a partir do
Meet · esta volta é só desenho, sem código.

Como na RFC-0001, **[a verificar]** marca tudo o que depende de hardware, versão ou desempenho que
ainda não foi medido. Nada marcado assim deve ser copiado como facto — e nesta RFC há mais disso do
que o habitual, por uma razão medida: **não existe nenhum capturador HDMI ligado a esta máquina**
(`lsusb` a 2026-10-14 mostra só a webcam interna `3277:0010`; os `/dev/video10` e `/dev/video11`
são o `v4l2loopback` do `dxcam`). O percurso de media desta RFC nunca foi corrido ponta a ponta.

## 1. Problema

Há descodificadores de TV (um por operador, ou vários do mesmo) com saída HDMI. O pedido é vê-los
**a partir do Delonix Meet**: ligar um descodificador por HDMI a um capturador USB, vários
capturadores a uma *dock station*, e ter os canais disponíveis no produto — para ver só, ou para ver
em conjunto numa sala.

O que isto **não** é: não é o estúdio a realizar um programa (RFC-0001), não é terrestre/cabo/
satélite, não é DRM, e não é um PVR (gravar o canal para ver depois) — a gravação do canal cai no
que a RFC-0001 já desenha para o Program.

Objectivos, por ordem:

1. **O sinal chega.** Uma entrada HDMI viva produz vídeo+áudio no Meet, identificada de forma
   estável entre reinícios e entre recriações da *dock*.
2. **Vários ao mesmo tempo.** N entradas numa caixa, com o tecto a ser medido e dito, não assumido.
3. **O espectador escolhe.** Uma lista de canais no Meet, e um canal escolhido toca.
4. **Sintonizar à distância.** Mudar o canal do descodificador a partir do Meet.
5. **Sobrevive ao operador.** Fechar o browser não interrompe a captura.

## 2. Estado medido na `origin/develop` `30ada1b6`

| Peça que esta RFC precisa | Existe? | Onde |
|---|---|---|
| Fonte de vídeo local na mesa de corte, com resolução/fps/latência medidos | **sim** | `web/src/studio/tv/fontes.ts` — `TipoDeFonte = 'camara' \| 'ecra' \| 'quadro' \| 'participante'`; um capturador UVC entra hoje como `camara` |
| Canal como identidade da organização (nome, slug, fuso, visibilidade, retenção) | **sim** | `tv_channels` (migr. `0088`), `server/src/tv_channels.rs`, rotas `/api/orgs/{org_id}/tv/channels` |
| Plano de controlo de emissões, com *lease* e *fencing token* | **sim** | `tv_broadcast_sessions` (`0089`), `server/src/tv_broadcasts.rs` |
| Registo observado de uma emissão, ligado a um canal | **esquema só** | `live_sessions` (`0074`, `0094`) — **nenhum código Rust a usa** (ADR-0015) |
| Máquina de estados de um destino (queda, backoff, degradado, perdido) | **sim** | `server/crates/delonix-meet-domain/src/content/live_output.rs` |
| Remux para RTMP sem reencodificar vídeo | **sim** | `server/src/broadcast.rs` (`-c:v copy`, um processo por destino) |
| Ingest (WHIP/RTMP/SRT) e origem HLS | **decidido, não construído** | [ADR-0015](../adr/0015-channel-engine-ingest-e-distribuicao-de-tv.md) — MediaMTX, **Proposto**; *spike* parcial |
| Channel Engine (executor) | **contrato só** | `docs/tv/executor-contrato.md` — não há código |
| Agente externo que se enrola, reporta inventário USB e reivindica trabalho | **sim, para SMS** | `sms_gateway` + `sms_device` (`0039`); token `dlxg_` em `server/src/sms.rs:364` (`crypto::random_token`, `crypto::sha256_hex`) |
| Peça comum de fila com reivindicação | **sim** | `delonix_meet_core::jobs::Queue` + `delonix_meet_store::jobs::claim`; portão `scripts/check-filas-reivindicacao.sh` (fasquia 1) |
| Qualquer noção de *dispositivo de captura*, *descodificador* ou *sintonia* | **não** | — |

**A leitura que importa:** o que falta não é o difícil. O molde do agente (enrolamento por token que
sai uma vez, inventário USB reportado e substituído a cada relatório, trabalho reivindicado só por
quem o reclamou) **já está construído e em produção para o gateway de SMS**. Esta RFC copia-lhe a
*forma* e chama-lhe os helpers — não os reescreve.

## 3. Factos externos verificados (2026-10-14)

| Facto | Fonte | O que fica de fora |
|---|---|---|
| Os capturadores HDMI→USB comuns (MS2109, MS2130/MS2130S) são **UVC**: funcionam no Linux sem driver do fabricante | cnx-software, análises do MS2130 | não foi medida nenhuma unidade **nossa** |
| O **áudio não vem pelo nó V4L2**: aparece como dispositivo USB de áudio separado (ALSA), e exige uma segunda entrada no ffmpeg | lista do Cinelerra-GG, Maio 2025 | — |
| O «4K» dos anúncios é a **entrada** HDMI; o lado USB fecha tipicamente em 1080p60 | cnx-software | o limite real da unidade mede-se com `v4l2-ctl --list-formats-ext` — **que esta máquina não tem instalado** |
| Relatos no Linux de fps negociado diferente do pedido (25 → 30) e de buffers vazios/corrompidos | lista do Cinelerra-GG | sem resolução conhecida; é risco, não impedimento |
| **CEC:** os códigos de controlo de utilizador incluem `0x30` (canal acima), `0x31` (canal abaixo) e `0x20`–`0x29` (dígitos 0 a 9) | `drivers/media/rc/keymaps/rc-cec.c` do Linux, que cita a especificação HDMI, «Table 27: User Control Codes p. 95» | o ficheiro **não** diz que dispositivos enviam ou respondem a estes códigos |
| Mudar de canal pode também ir por **Tuner Control** (incremento/decremento de passo), que é um mecanismo distinto das teclas | referência de CEC consultada | os *opcodes* exactos **não** foram lidos na especificação |
| Há relatos de aparelhos em que os dígitos funcionam e o canal acima/abaixo não | fórum de utilizadores | confirma que **responder a CEC é opcional** no aparelho |
| **HDCP:** um capturador não grava nem emite conteúdo protegido — dá preto; a saída de alguns aparelhos não permite desligar HDCP | suporte da Plugable (cartão NIX), fóruns de OBS | não foi testado com **nenhum** descodificador nosso |
| *Splitters* que removem HDCP são zona legal cinzenta: a imprensa técnica alemã diz que contornar HDCP viola direitos de autor e que esses aparelhos não devem ser vendidos na Alemanha; nos EUA a acção da Warner/DCP contra a LegendSky (2016) **terminou em acordo, sem decisão de mérito** | heise, Sound & Vision, documentos do processo | nada específico de Angola; **não é parecer jurídico** |

**O que não foi verificado e condiciona tudo:** se os descodificadores concretos (a) emitem HDMI sem
HDCP nos canais em questão, (b) respondem a CEC, e (c) que largura de banda USB a *dock* aguenta com
N capturadores. Estes três não se decidem por desenho: medem-se com o hardware na mão (§13, Fase 0).

## 4. Arquitectura

```
descodificador ──HDMI──> capturador UVC ─┐
descodificador ──HDMI──> capturador UVC ─┼─ dock USB ─> CAIXA DE CAPTURA
descodificador ──HDMI──> capturador UVC ─┘                 │
      ^                                                    │ delonix-tv-agent
      └── IR / CEC (sintonia) ────────────────────────────┘ │
                                                            │ (1) enrola-se e reporta inventário
                                                            │ (2) reivindica comandos de sintonia
                                                            │ (3) publica cada entrada
                                                            v
  delonix-server (CONTROLO)  <── REST/token ──>  INGEST (MediaMTX, ADR-0015)
   tv_capture_agents                                  │
   tv_capture_inputs  ── liga uma entrada a ──> tv_channels
   tv_tuner_commands  (fila comum)                    │
                                           ┌──────────┴──────────┐
                                     Fase A: SFU            Fase B: HLS
                                 (canal entra na sala)   (grelha + player)
```

Quatro peças, e a fronteira de cada uma:

- **Caixa de captura.** O PC/mini-PC onde os capturadores estão ligados. Corre o `delonix-tv-agent`
  e **nada mais do nosso**. Não tem interface, não precisa de browser aberto, e o que sabe fazer é
  olhar para o barramento, abrir uma entrada, publicá-la, e executar comandos de sintonia.
- **Controlo.** No `delonix-server`: o inventário, a ligação entrada↔canal, a fila de sintonia, as
  permissões e o estado desejado vs observado. Postgres é a fonte de verdade.
- **Ingest e distribuição.** O que o ADR-0015 já decidiu (MediaMTX atrás do nosso plano de
  controlo), reutilizado sem alteração: um canal por captura é, para o ingest, igual a um canal
  produzido pelo estúdio.
- **Consumo no Meet.** Fase A: o canal entra numa sala pelo SFU. Fase B: grelha de canais e player
  HLS.

**Princípio:** a caixa de captura é substituível e desconfia-se dela. Ela não é dona de nada — o
canal, o nome, a visibilidade e a autorização vivem no servidor. Desligar a caixa apaga sinal, não
configuração.

## 5. O agente de captura (`delonix-tv-agent`)

### 5.1 Enrolamento e autenticação

Igual ao gateway de SMS, de propósito: o operador cria o agente na consola da organização, recebe um
token **uma vez** (`crypto::random_token` com prefixo próprio, guarda-se o `sha256_hex`), e o agente
autentica-se com `Authorization: Bearer <token>` nas rotas `/api/tv/agent/*`. Revogar na base nega o
pedido seguinte. **Um agente pertence a uma e uma só organização** — o inventário dele nunca é
visível noutra (§10).

### 5.2 Identidade de uma entrada: o caminho físico, não o nó

Esta é a decisão com mais consequências práticas, e vem de uma lição já paga neste repo: os
identificadores de dispositivo de media **rodam**. No telemóvel, o `deviceId` da câmara mudou entre
sessões e as etiquetas vinham vazias; a correcção foi deixar de confiar no id (#328). Do lado do
Linux o problema é o mesmo com outro nome: `/dev/video0` passa a `/dev/video2` quando se liga a
*dock* noutra ordem, e dois capturadores do mesmo modelo têm o mesmo `vendor:product` e muitas vezes
**nenhum número de série**.

Logo, a chave estável de uma entrada é o **caminho físico no barramento** — a porta onde está
ligada, não o aparelho (por exemplo `usb-0000:00:14.0-3.2`, como o `by-path` do udev o escreve).
Consequências ditas à frente, não descobertas depois:

- Trocar o capturador de porta **é** uma entrada nova. É o comportamento certo: o que define o canal
  é a porta onde o descodificador do canal X está ligado, não o adaptador de 15 dólares.
- O agente reporta, por entrada: caminho físico, `vendor:product`, série (quando existe), nó V4L2
  actual, nó ALSA do **mesmo** capturador, formatos/resoluções/fps que o aparelho declara, e se tem
  sinal.
- O inventário é **substituído a cada relatório**, como o `sms_device` já faz. O que não aparece
  deixa de ter sinal; a linha não se apaga, para o canal não perder a configuração por um cabo solto.

### 5.3 O par vídeo+áudio

Um capturador HDMI é **dois dispositivos** no SO. Emparelhá-los é trabalho do agente, por caminho
físico comum, e o par fica explícito no modelo (`video_path` + `audio_path`). Um par incompleto é um
estado reportado («sem áudio»), não um silêncio por adivinhar. Nada no repo faz isto hoje: as fontes
do Estúdio tratam a câmara como vídeo e o microfone como escolha separada do utilizador.

### 5.4 Publicação

O agente publica cada entrada activa no ingest com um processo por entrada (a mesma forma do
`broadcast.rs`: um processo por destino, supervisor que drena o `stderr`, backoff limitado). Codec e
transporte: **[a verificar]** no *spike* da Fase 1 — a escolha entre SRT e WHIP e entre encode no
agente (H.264 por CPU/VAAPI) ou passagem do MJPEG para transcodificação no Channel Engine muda o
custo por canal, e não há medida nenhuma. O que **não** se discute: a máquina de estados por destino
é a do `live_output.rs`, que já existe e está testada linha a linha.

## 6. Sintonizar à distância (zapping)

Decisão do dono: o canal muda-se a partir do Meet. O que isso exige, dito com precisão:

**O que o padrão dá.** A especificação HDMI tem códigos para «canal acima» (`0x30`), «canal abaixo»
(`0x31`) e para os dígitos (`0x20`–`0x29`), e um mecanismo separado de controlo de sintonizador.
Um Linux com acesso CEC envia-os pelo subsistema CEC (`/dev/cecN`).

**O que o padrão não dá, e é o risco:**

1. **Responder é opcional.** Nada obriga um descodificador a implementar o papel de sintonizador nem
   a aceitar comandos de terceiros. Há relatos de aparelhos que obedecem aos dígitos e ignoram o
   canal acima/abaixo. Isto mede-se por aparelho, não se desenha.
2. **O capturador barato não tem CEC.** A linha CEC é do HDMI, e um dongle UVC de 20 dólares não a
   expõe. CEC exige hardware próprio (um adaptador USB-CEC, ou a saída HDMI de um SBC). **[a verificar]**
3. **Não há confirmação.** Mesmo que o comando chegue, nada no caminho diz «estou agora no canal 42».
   O estado do descodificador não é observável por CEC de forma garantida.

Por isso o desenho é este, e é deliberadamente modesto:

- Um comando de sintonia é uma **linha numa fila**, reivindicada pelo agente — e a fila usa a **peça
  comum** (`delonix_meet_core::jobs::Queue`, `delonix_meet_store::jobs::claim`). Não se escreve um
  oitavo `FOR UPDATE … SKIP LOCKED`: o portão `check-filas-reivindicacao.sh` existe exactamente para
  isso e a sua fasquia é 1.
- O ciclo de vida do comando é `queued → claimed → sent → acked | failed`. **`sent` significa «o
  agente enviou», e nada mais.** A interface diz «canal pedido», não «canal mudado»; só se diz
  mudado quando houver prova, e prova não existe na Fase 1.
- Dois executores por trás da mesma fila, escolhidos por entrada: **CEC** (`tuner_kind = 'cec'`) e
  **IR** (`tuner_kind = 'ir'`, emissor infravermelho com perfil de códigos por modelo). `'none'` é o
  valor por omissão e um estado legítimo: a entrada está fixa num canal e ninguém a sintoniza.
- Confirmação do canal (por impressão digital de imagem, OCR do mosaico, ou leitura do EPG do
  operador) fica **fora** desta RFC. Nomeá-la agora é honesto; prometê-la não seria.

## 7. Modelo de dados

Três tabelas novas, todas com `org_id` e RLS desde o início (ADR-0002), identificadores em inglês:

**`tv_capture_agents`** — a caixa. `id`, `org_id`, `name`, `location`, `token_prefix`,
`token_hash` (único), `agent_version`, `created_by`, `created_at`, `last_seen_at`, `revoked_at`.
Molde: `sms_gateway`.

**`tv_capture_inputs`** — a entrada física. `id`, `org_id`, `agent_id`, `device_path` (o caminho
físico; `UNIQUE (agent_id, device_path)`), `label`, `vendor_id`, `product_id`, `serial`,
`video_node`, `audio_node`, `capabilities` (JSONB: o que o aparelho declara), `signal` (`live |
no_signal | protected | error` — `protected` é o caso HDCP, dito pelo nome), `tuner_kind`
(`none | cec | ir`), `tuner_config` (JSONB: endereço lógico CEC ou perfil IR), `channel_id`
(FK `tv_channels`, nulo), `last_seen_at`, `last_signal_at`. Molde: `sms_device`.

**`tv_tuner_commands`** — a fila de sintonia. `id`, `org_id`, `input_id`, `requested_by`, `kind`
(`channel_number | channel_up | channel_down | power | key`), `payload` (JSONB), `status`
(`queued | claimed | sent | acked | failed`), `claimed_by` (FK `tv_capture_agents` — só quem
reclamou reporta, como no `sms_message`), `attempts`, `error`, `created_at`, `claimed_at`,
`sent_at`. Declarada em `delonix_meet_core::jobs` como as outras.

**O que não se cria:** nenhuma tabela de canal nova. Uma entrada liga-se a um `tv_channels` que já
existe, e é por aí que herda nome, slug, visibilidade e retenção. Uma emissão contínua de captura é
uma `tv_broadcast_sessions` com `live_session_id` — o mesmo plano de controlo e o mesmo *lease* com
*fencing* que o estúdio usa. Esta RFC **não** fecha o ponto aberto do ADR-0015 (`live_sessions.
started_by` é `NOT NULL` e uma captura 24/7 não tem pessoa por trás): precisa do actor de sistema, e
é a mesma decisão que o agendador de playout espera.

## 8. API

**Consola (BFF, sessão + capacidade):**

- `GET|POST /api/orgs/{org_id}/tv/capture-agents` · `GET|PATCH|DELETE .../{agent_id}` —
  o `POST` devolve o token uma vez.
- `GET /api/orgs/{org_id}/tv/capture-agents/{agent_id}/inputs` — inventário observado.
- `PATCH /api/orgs/{org_id}/tv/capture-inputs/{input_id}` — etiqueta, `tuner_kind`, `tuner_config`,
  e a ligação ao canal.
- `POST /api/orgs/{org_id}/tv/capture-inputs/{input_id}/tune` — enfileira um comando; devolve o `id`
  e `status=queued`. **Não** devolve «canal mudado».
- `GET /api/orgs/{org_id}/tv/capture-inputs/{input_id}/commands` — histórico.

**Agente (token `Bearer`, sem sessão):**

- `POST /api/tv/agent/inventory` — relatório completo; substitui o inventário do agente.
- `POST /api/tv/agent/commands/claim` — reivindica um lote.
- `POST /api/tv/agent/commands/{id}/result` — só o `claimed_by`.
- `POST /api/tv/agent/inputs/{id}/state` — sinal, débito, erros.

Checklist de rota nova (`delonix-meet-api`), e as duas que só correm no CI: cada rota precisa de
secção em `web/e2e/isolamento.mjs` com o *path* literal (`check-isolamento-cobertura`) e de
`BLESS=1 scripts/check-openapi.sh`. Um teste 404 em Rust **não** satisfaz esses portões.

## 9. Distribuição, nas duas fases decididas

**Fase A — o canal entra na sala (SFU).** O agente publica para o SFU como um participante de
sistema; os presentes vêem-no como mais um vídeo, com o chat e o «ver em conjunto» que a sala já tem.
Vantagem: usa o que existe, atraso sub-segundo, prova o percurso completo com pouco código novo.
Limites: uma sala, um *publisher* por canal, e o tecto de produção de hoje é o coturn (~150
participantes, medido na frente de escala).

**Fase B — grelha e player (HLS).** O canal vai ao ingest e sai em HLS/LL-HLS; o Meet ganha um ecrã
de TV com a lista de canais e um player. Escala por CDN, custa 2–6 s de atraso **[a verificar]** e
depende do que o ADR-0015 ainda não fechou: Channel Engine, MediaMTX a correr, e o *spike* completo
(WHIP, leitores reais, ≥ 30 min, encoder real).

O modelo é o mesmo nas duas: um canal é um `tv_channels`. A fase não muda o que o espectador escolhe.

## 10. Segurança e isolamento

- **Um agente é de uma organização.** O token é dela; o inventário, os comandos e os canais filtram
  por `org_id` com RLS. Duas organizações com capturadores na mesma sala física não se vêem — e a
  prova é um teste com duas orgs, não uma leitura do código.
- **O token sai uma vez** e guarda-se o `sha256_hex`; revogar nega o pedido seguinte. Chamam-se os
  helpers (`crypto::random_token`, `crypto::sha256_hex`), não se recopiam.
- **Só quem reclamou reporta.** O resultado de um comando exige o `claimed_by` — a regra que o
  `sms_message` já impõe.
- **O agente não escolhe o que publica.** O servidor diz qual entrada está ligada a que canal; um
  agente comprometido não ganha um canal novo nem muda a visibilidade de um existente.
- **Canal `restricted`** continua a autorizar por espectador (RF-12 da RFC-0001). Uma captura não
  abre uma porta nova de leitura.
- **A caixa de captura é uma máquina na rede do cliente.** Ela liga-se **para fora** (REST + ingest);
  o servidor nunca lhe liga. É o que evita abrir porta na casa de quem tem o descodificador.

## 11. Capacidade e custo

- **USB é o primeiro tecto.** 1080p60 em MJPEG anda na ordem das centenas de Mbit/s e a *dock*
  partilha um controlador. A regra a medir, não a assumir: quantas entradas a 1080p30/60 numa *dock*
  antes de aparecerem frames perdidos. **[a verificar]** — Fase 0.
- **CPU.** Se o agente encodificar, é ~1 core por canal a 1080p30 em x264 *veryfast* **[a verificar]**;
  se passar o MJPEG, o custo muda de sítio (Channel Engine) e sobe a banda da rede.
- **Canais 24/7 custam continuamente**, ao contrário de uma reunião. Uma quota por organização
  (entradas activas e canais no ar) entra com a Fase 2, antes de haver mais de uma organização a usar.

## 12. Legal — três coisas distintas

Ditas uma vez, para a decisão ser do dono e não ficar implícita:

1. **HDCP.** Se o descodificador protege a saída, o capturador dá preto. O contorno é hardware, e é
   zona cinzenta: há jurisdições onde contornar HDCP é ilegal, e o caso americano mais citado acabou
   em acordo sem decisão de mérito. **O Meet não remove HDCP e não traz nada para o remover**: o
   estado `protected` numa entrada é um relatório, não um problema a resolver em software.
2. **Direitos de retransmissão.** Distribuir canais de terceiros a espectadores é redistribuição e
   precisa de acordo com o canal ou o operador. Monitorização interna (uma equipa a ver o que está no
   ar) é outro caso. A Fase A, numa sala fechada da organização, é o caso mais defensável; a Fase B,
   com grelha pública, é o que exige contrato. **Decisão do dono, por mercado.**
3. **Licenças de código.** Nada aqui muda o que o ADR-0015 §D3 fixou: ffmpeg GPL com libx264 só na
   imagem do Channel Engine, `nonfree` nunca, e as patentes de H.264 continuam por validar.

## 13. Plano por incrementos verificáveis

| Fase | O que entra | Prova que a fecha | Fora |
|---|---|---|---|
| **0 — hardware na mesa** (sem código) | um capturador + um descodificador | `v4l2-ctl --list-formats-ext` com os modos reais; `ffmpeg -f v4l2` a gravar 60 s com áudio ALSA do mesmo aparelho; HDCP: vê-se imagem ou preto; CEC: o descodificador responde ou não | tudo o resto |
| **1 — o sinal chega** | `tv_capture_agents`, `tv_capture_inputs`, rotas de consola e de agente, agente mínimo (inventário + publicar 1 entrada), Fase A na sala | duas orgs isoladas (teste), e uma sala com o canal visível ≥ 30 min sem queda | zapping, HLS, quotas |
| **2 — vários e com dono** | N entradas, par vídeo+áudio reportado, quotas, estado `protected` na interface | tecto de entradas por *dock* medido e escrito; `make fitness` + os dois portões do CI | grelha |
| **3 — sintonia** | `tv_tuner_commands` na peça comum, executor CEC e executor IR | um comando pedido na consola muda o canal **no aparelho**, filmado ou medido; e o estado diz «pedido», não «mudado» | confirmação automática do canal |
| **4 — grelha e player** | Channel Engine + MediaMTX (fecha o ADR-0015), ecrã de TV, HLS | latência p50/p95 com amostra ≥ 30 min, CPU por canal, leitores reais | anúncios, DRM |

A Fase 0 é mesmo primeiro. **Três das decisões desta RFC dependem de factos que só o hardware dá**, e
nenhuma linha de Rust os antecipa.

A lista de compras da Fase 0, com preços datados e os comandos que respondem às três perguntas,
está em [fase0-compras.md](fase0-compras.md).

## 14. Critérios de aceitação

1. Uma entrada identificada pelo caminho físico sobrevive a reinício da caixa e a religar a *dock*
   noutra ordem — o canal continua ligado à mesma porta.
2. O áudio do canal é o áudio **daquele** capturador, provado com dois capturadores ligados ao mesmo
   tempo e sinais diferentes.
3. Fechar o browser do operador não interrompe a captura.
4. Um agente de uma organização não vê nem comanda nada de outra (teste com duas orgs).
5. Uma entrada com HDMI protegido reporta `protected` e não finge preto como «sem sinal».
6. Um comando de sintonia nunca aparece como «canal mudado» sem prova.

## 15. Decisões pendentes (do dono do produto)

1. **Que hardware se compra** para a Fase 0 — orçamentado em
   [fase0-compras.md](fase0-compras.md): ~45 USD pelo caminho IR, ~70 USD pelo caminho CEC. Falta
   o «compra» dele, e a escolha do caminho de sintonia (a recomendação é IR primeiro).
2. **Encode no agente ou no Channel Engine** — decide-se com o número da Fase 0, não antes.
3. **Âmbito de distribuição por mercado** (§12.2): sala interna, ou grelha para espectadores.
4. **O actor de sistema** para uma emissão sem pessoa (`live_sessions.started_by`), que esta RFC
   partilha com o agendador de playout da RFC-0001.
