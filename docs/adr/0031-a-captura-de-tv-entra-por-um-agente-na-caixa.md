# ADR-0031 — A captura de TV entra por um agente na caixa, e a entrada identifica-se pelo caminho físico

**Estado:** Proposto · **Data:** 2026-10-14 ·
**Contexto:** [RFC-0002](../tv/rfc-0002-tv-por-captura-hdmi.md) — ver canais de descodificadores
externos a partir do Meet, por captura HDMI, vários por *dock station* ·
**Assenta em:** [ADR-0015](0015-channel-engine-ingest-e-distribuicao-de-tv.md) (ingest e
distribuição), [ADR-0013](0013-emissao-que-sobrevive-a-queda-da-rede.md) (estado de um destino),
[ADR-0005](0005-gateway-sms.md) (o molde do agente externo) e
[ADR-0002](0002-tenant-isolation-rls.md) (isolamento por organização).

**Não é Aceite, e a razão é hardware:** a 2026-10-14 não existe nenhum capturador HDMI ligado a
nenhuma máquina nossa (`lsusb`: só a webcam interna e o `v4l2loopback` do `dxcam`). O percurso de
media nunca foi corrido. Este ADR fixa a **forma**; os números e a viabilidade de CEC vêm da Fase 0
da RFC.

## Contexto medido (`origin/develop` `30ada1b6`)

1. **A fonte de vídeo local já existe no browser, e não serve para isto.** A mesa de corte do
   Estúdio tem seis fontes e uma delas é `camara` (`web/src/studio/tv/fontes.ts`); um capturador UVC
   entra lá hoje. Mas o Estúdio é efémero por desenho — fechar o browser acaba com a composição — e
   um canal de TV é contínuo.
2. **O plano de controlo de canais e emissões está construído**: `tv_channels` (`0088`),
   `tv_broadcast_sessions` com *lease* e *fencing* (`0089`), `live_sessions` (`0074`/`0094`), e a
   máquina de estados por destino em `content/live_output.rs`.
3. **O executor não existe** (só `docs/tv/executor-contrato.md`), e o ingest do ADR-0015 continua
   **Proposto**, com *spike* parcial.
4. **O molde de um agente externo existe e está em produção**: `sms_gateway` + `sms_device` (`0039`)
   — token `dlxg_` que sai uma vez (`crypto::random_token`, `crypto::sha256_hex`), inventário USB
   reportado e substituído a cada relatório, e trabalho que só o `claimed_by` pode reportar.
5. **A reivindicação de trabalho tem uma peça comum** (`delonix_meet_core::jobs::Queue`,
   `delonix_meet_store::jobs::claim`) e um portão com fasquia 1
   (`scripts/check-filas-reivindicacao.sh`), por sete filas copiadas à mão em 2026-10-07.
6. **Nada no repo conhece dispositivos de captura, descodificadores ou sintonia.**

## Decisão

1. **A captura corre num agente sem interface na caixa (`delonix-tv-agent`), não no browser.** O
   browser do Estúdio continua a servir realização; um canal de captura não depende de nenhuma
   sessão aberta. O agente copia a **forma** do gateway de SMS e **chama** os helpers de token e de
   hash em vez de os repetir.
2. **A chave estável de uma entrada é o caminho físico no barramento USB**, não o `/dev/videoN`, não
   o `deviceId` do browser e não o `vendor:product` (dois dongles iguais sem número de série são
   indistinguíveis). Mudar o capturador de porta é, por definição, uma entrada nova — e o que define
   um canal é a porta onde o seu descodificador está ligado.
3. **O par vídeo+áudio é explícito.** Um capturador HDMI é dois dispositivos no SO (V4L2 + ALSA); o
   agente emparelha-os pelo caminho físico comum e o modelo guarda os dois. Um par incompleto é o
   estado «sem áudio», reportado, nunca um silêncio por adivinhar.
4. **Uma entrada liga-se a um `tv_channels` que já existe.** Não há tabela de canal nova, e uma
   captura no ar é uma `tv_broadcast_sessions` com o mesmo *lease* do estúdio. O ingest e a
   distribuição são os do ADR-0015, sem fork.
5. **O zapping é um comando numa fila, e a fila é a peça comum.** Ciclo
   `queued → claimed → sent → acked | failed`, com dois executores por trás (`cec` e `ir`) e `none`
   como omissão legítima. **`sent` quer dizer «o agente enviou»**; a interface diz «canal pedido» e
   só diria «canal mudado» com prova — que não existe nesta fase. Responder a CEC é opcional no
   aparelho e o dongle UVC comum não expõe a linha CEC.
6. **A distribuição entra em duas fases:** primeiro o canal na sala pelo SFU (prova o percurso com o
   que já existe), depois HLS com grelha e player (depende do Channel Engine e de o ADR-0015 passar
   a Aceite).
7. **O Meet não remove HDCP.** Uma entrada protegida reporta `protected` e fica assim. O contorno é
   hardware do cliente e é uma decisão legal dele, por jurisdição.
8. **A caixa liga-se para fora.** O servidor nunca inicia ligação à caixa de captura: nada de abrir
   porta na casa de quem tem o descodificador.

## Alternativas consideradas

| Alternativa | Porque não |
|---|---|
| **Browser do Estúdio com os capturadores como `camara`** | funciona hoje e é a demonstração mais rápida, mas exige uma máquina com browser aberto por emissão, não dá 24/7, e a correspondência dispositivo↔canal é frágil (os ids rodam — lição do #328) |
| **Capturadoras de rede (encoder HDMI→SRT/RTMP autónomo)** | elimina a caixa e o agente, e é a saída mais robusta se o cliente puder comprar hardware por entrada; mas não há inventário, não há sintonia, e cada aparelho é uma configuração manual. Fica como **opção de implantação**, não como arquitectura |
| **Entrada IP do descodificador (quando existe) em vez de HDMI** | é tecnicamente superior (sem perda de geração, sem HDCP, sem USB) e deve ser preferida **sempre que o aparelho a tenha**. Não substitui esta decisão: a maioria dos descodificadores de operador não a expõe |
| **`/dev/videoN` ou `vendor:product` como chave** | rodam com a ordem de ligação e colidem entre unidades iguais — era garantir que o canal 5 passa a mostrar o canal 2 depois de um reinício |
| **Fila própria com `FOR UPDATE … SKIP LOCKED`** | seria a oitava cópia; o portão `check-filas-reivindicacao.sh` recusa-a, e com razão |

## Consequências

- Mais um binário nosso a operar fora do cluster (imagem, actualização, saúde) — mas no lugar onde o
  cabo HDMI está, que é o único lugar possível.
- O inventário do barramento passa a ser estado persistido, com a confusão habitual entre «não
  aparece» e «apagado»: a linha fica, o sinal cai. Isto é deliberado, para um cabo solto não apagar
  a configuração do canal.
- A sintonia fica honestamente incompleta: pede-se e não se confirma. Confirmar o canal (impressão
  digital de imagem, OCR, EPG do operador) é trabalho futuro, nomeado e não prometido.
- Nada disto fecha o ponto aberto do ADR-0015: `live_sessions.started_by` é `NOT NULL` e uma captura
  24/7 não tem pessoa por trás. Precisa do actor de sistema, com o agendador de playout.
- Quotas por organização passam a ser necessárias mais cedo do que nas reuniões: um canal custa
  continuamente.

## O que fecha este ADR (para passar a Aceite)

1. Fase 0 da RFC-0002 com hardware: modos reais do capturador, 60 s de gravação com áudio
   emparelhado, HDMI protegido ou não, e CEC a responder ou não — em aparelho nomeado.
2. Uma entrada publicada ≥ 30 min sem queda, vista numa sala.
3. Tecto medido de entradas por *dock* alimentada, escrito em `docs/tv/`.
4. Teste de duas organizações a provar que um agente não vê o inventário da outra.
