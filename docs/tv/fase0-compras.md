# Fase 0 da RFC-0002 — o que se compra, e o que se mede com isso

**Data:** 2026-10-14 · **Contexto:** [RFC-0002](rfc-0002-tv-por-captura-hdmi.md) §13 (Fase 0) e
[ADR-0031](../adr/0031-a-captura-de-tv-entra-por-um-agente-na-caixa.md).

A Fase 0 existe porque **três decisões da RFC dependem de factos que só o hardware dá**, e nenhuma
linha de Rust os antecipa: o descodificador protege a saída HDMI (HDCP) ou não; responde a comandos
de sintonia ou não; e quantas entradas o barramento USB aguenta. Este documento é a lista de compras
mínima para responder às três, e os comandos que as respondem.

**Os preços são instantâneos de listagens consultadas a 2026-10-14**, não tabelas oficiais, e
**nenhum inclui frete nem alfândega**. As fontes estão no fim. Verifica o preço vivo antes de pagar:
este é o tipo de número que envelhece em semanas.

## 0. O que já existe e não se compra

A caixa de captura da Fase 0 é **a máquina de desenvolvimento**. Medido a 2026-10-14 com `lsusb -t`:
um controlador a **20000M** (bus 002), dois a **10000M** (bus 004 e 006), todos com portas livres —
melhor barramento do que um mini-PC N100 traria. O que **falta** nela é só software livre:

```bash
sudo apt install v4l-utils            # o v4l2-ctl NÃO está instalado (medido)
sudo apt install cec-utils            # só se o caminho de sintonia for CEC
# ffmpeg já está no sistema; python3-broadlink (ou pip) só para o caminho IR
```

## 1. Lista mínima (Cenário A) — responder às três perguntas

| Item | Qt. | Preço unit. | Porquê exactamente este |
|---|---|---|---|
| Capturador HDMI→USB 3.0 com chip **MS2130** (ou MS2130S/MS2131S) | **2** | $8–16 (AliExpress, Walmart) · ~$19 entregue (eBay) | dois **iguais**, de propósito: é o que prova o problema de identidade do ADR-0031 §2 (sem número de série, dois dongles do mesmo modelo são indistinguíveis) e o emparelhamento errado de áudio |
| Cabo HDMI 1 m | 2 | $3–5 | — |
| **Sintonia: escolher UM caminho** | | | |
| ↳ Emissor IR **Broadlink RM4 mini** | 1 | **$23–26** (Walmart ~$23,39; loja própria $25,99, esgotada à data) | **a aposta segura**: imita o telecomando que já se sabe que funciona; controla-se na LAN sem passar pela cloud. Só Wi-Fi 2,4 GHz; fonte 5 V não incluída |
| ↳ Adaptador **USB-CEC Pulse-Eight** (P8-USBCECv1) | 1 | **$48** novo (loja própria) · £42,49 (Amazon UK) · **$30** usado (eBay) · **Amazon US indisponível** | o único caminho CEC sério num PC — e o que pode simplesmente não dar: responder a CEC é **opcional** no aparelho, e o dongle UVC não expõe a linha CEC |

**Total: ~$45 pelo caminho IR · ~$70 pelo caminho CEC · ~$95 com os dois.**

**Recomendação:** IR primeiro. O CEC é a solução mais limpa e tem probabilidade real de falhar com um
descodificador de operador; o IR funciona em qualquer aparelho que responda ao seu próprio
telecomando. Se o CEC entrar, entra depois, com o IR já a servir de controlo.

**Antes de pagar:** confirmar **MS2130/MS2131, não MS2109** (esse é USB 2.0), e lembrar que o «4K»
dos anúncios é a **entrada** HDMI — o lado USB fecha em **1080p60**.

## 2. Como se mede (e o que cada medida decide)

```bash
# (a) O aparelho aparece, e com que modos REAIS — decide a resolução/fps do canal
v4l2-ctl --list-devices
v4l2-ctl -d /dev/videoN --list-formats-ext

# (b) A identidade estável — decide a chave de tv_capture_inputs (ADR-0031 §2)
ls -l /dev/v4l/by-path/        # com os DOIS capturadores ligados, e depois
                               # trocados de porta: o by-path acompanha a PORTA
udevadm info -q property -n /dev/videoN | grep -E 'ID_PATH|ID_SERIAL|ID_VENDOR_ID|ID_MODEL_ID'

# (c) O áudio do MESMO capturador — decide se o par vídeo+áudio se forma
arecord -l                     # o capturador aparece como placa USB própria
ls -l /dev/snd/by-path/        # emparelhar POR CAMINHO, não por nome

# (d) HDCP: imagem ou preto — decide se este descodificador serve
ffmpeg -f v4l2 -i /dev/videoN -f alsa -i hw:CARD=<capturador> -t 60 \
       -c:v libx264 -preset veryfast -c:a aac /tmp/prova.mp4
# ver o ficheiro. Preto com áudio = HDCP (estado `protected`), não «sem sinal».

# (e) O tecto do barramento — decide quantas entradas por caixa
#     Dois (depois três, quatro) capturadores a gravar AO MESMO TEMPO, no mesmo
#     controlador e depois espalhados. Contar frames perdidos no stderr do ffmpeg.
lsusb -t                       # ver em que controlador está cada um

# (f) Sintonia, caminho CEC
cec-client -l                  # o adaptador aparece?
echo 'scan' | cec-client -s -d 1          # o descodificador responde?
# canal acima = user control 0x30, dígitos = 0x20..0x29 (Table 27 da spec HDMI)

# (g) Sintonia, caminho IR: aprender o código do telecomando e reproduzi-lo
#     (python-broadlink: `broadlink_cli --learn`, depois `--send`)
```

**Resultado da Fase 0 a escrever aqui mesmo, em `docs/tv/`:** marca e modelo do descodificador, se
dá imagem, se responde à sintonia e por que caminho, os modos reais do capturador, e quantas
entradas simultâneas antes de perder frames. Sem esses cinco números, as Fases 1–4 não arrancam.

## 3. Quatro canais numa caixa (Cenário B, Fases 1–2)

| Item | Qt. | Preço unit. | Total |
|---|---|---|---|
| Capturador MS2130 | 4 | $8–16 | $32–64 |
| **Hub USB 3.0 com fonte própria** (Sabrent HB-UMP3 $19,99, adaptador 5 V/2,5 A incluído; Cable Sales N-U3-4 $28,95) | 1 | $20–29 | $20–29 |
| Cabos HDMI | 4 | $3–5 | $12–20 |
| Mini-PC fanless N100 16 GB (MeLE Quieter 4C ~£200; Zotac CI337 Nano ~$221, RAM por confirmar; ASUS PN42 **barebone** $299,99) | 1 | $200–300 | $200–300 |
| **Total** | | | **$264–413** |

Dois avisos que custam dinheiro: um hub anunciado como *powered* **muitas vezes não traz o
adaptador** na caixa — confirmar; e não pôr as quatro entradas no mesmo hub por omissão. O tecto é a
banda do **controlador**, não o número de portas, e a máquina de desenvolvimento tem três.

## 4. Alternativa sem caixa nem agente (Cenário C)

Encoder HDMI→IP autónomo, um por entrada, que publica SRT/RTMP directamente no ingest:

| Gama | Exemplos e preço | Nota |
|---|---|---|
| Económica | DDMALL AVC-2K **$99** (regular $149); genérico eBay AU-HCP2 **$86,99** | 1080p30 no AVC-2K |
| Média | J-Tech/DDMALL **$180–340** (DDMALL HEV-10 $339, 4K60 com loopout) | confirmar SRT **no modelo exacto** |
| Marca, mas **sem SRT** | SIIG H.264/H.265 IPTV **$252–280** | protocolos listados sem SRT |

Quatro canais: **~$350–600**, sem PC e sem agente. O que se perde: inventário automático, sintonia e
gestão central — cada aparelho passa a ser configuração à mão. É por isso que o ADR-0031 o guarda
como **opção de implantação** e não como arquitectura.

**Comparação honesta:** com caixa, 4 canais custam $264–413 **mais** o agente por escrever; com
encoders, $350–600 e **nada** por escrever — mas também nada do produto (sem canal ligado a
`tv_channels` por inventário, sem zapping pelo Meet).

## 5. O que não se compra aqui

Se a medida (d) der **preto**, o aparelho protege a saída. As saídas legítimas são a entrada IP ou a
app do operador quando existem, um feed contratado, ou um descodificador que não aplique HDCP nesses
canais. **O contorno por *splitter* é decisão legal do dono, por jurisdição, e não entra nesta lista
nem no produto** — o Meet reporta `protected` e fica assim (ADR-0031 §7).

## Fontes dos preços (consultadas a 2026-10-14)

- MS2130: [cnx-software](https://www.cnx-software.com/2022/11/07/ms2130-based-4k-hdmi-to-usb-3-0-video-capture-dongle-sells-for-19/) ·
  [AliExpress](https://www.aliexpress.com/item/1005004883158574.html) ·
  [Walmart](https://www.walmart.com/ip/MS2130-Chip-Type-C-USB3-0-Capture-Card-4K-HDMI-compatible-Video-Grabber-1080P-Game-Recording-Live-Streaming-for-PS4-Xbox/18194422485)
- USB-CEC: [Pulse-Eight](https://www.pulse-eight.com/p/104/usb-hdmi-cec-adapter) ·
  [Amazon UK](https://www.amazon.co.uk/Pulse-Eight-na-USB-CEC-Adapter/dp/B005JU6LWM)
- IR: [Broadlink RM4 mini (Walmart)](https://www.walmart.com/ip/BroadLink-Smart-IR-Remote-Control-Hub-WiFi-IR-Blaster-for-Smart-Home-Automation-TV-Remote-Compatible-with-Google-Assistant-IFTTT-RM4-mini/355898472)
- Hub: [Sabrent HB-UMP3](https://sabrent.com/products/hb-ump3) ·
  [Cable Sales USA N-U3-4](https://www.cablesalesusa.com/product/usb-3-0-4-port-hub-powered/)
- Mini-PC: [PriceRunner, N100](https://www.pricerunner.com/sp/n100-mini-pc.html) ·
  [MITXPC MES-N100DC](https://mitxpc.com/products/mes-n100dc)
- Encoders: [DDMALL AVC-2K](https://www.ddmalltech.com/products/mini-srt-hdmi-video-encoder) ·
  [DDMALL HEV-10](https://www.ddmalltech.com/products/h-265-hdmi-encoder-4k60-loopout) ·
  [SIIG](https://www.siig.com/hdmi-video-h-264-h-265-iptv-encoder-with-loopout.html)

Os códigos de sintonia CEC (`0x30` canal acima, `0x31` canal abaixo, `0x20`–`0x29` dígitos) e a sua
origem na «Table 27: User Control Codes» da especificação HDMI estão na [RFC-0002 §3](rfc-0002-tv-por-captura-hdmi.md).
