# ADR-0017 — A perna da ponte negoceia Opus: banda larga entre o FreeSWITCH e a sala

**Estado:** Aceite (decisão do dono a 2026-10-05) · **Data:** 2026-10-05 ·
**Contexto:** o áudio de um softphone chegava à sala «baixo e sem qualidade».
**Estende:** [ADR-0010](0010-ponte-telefone-sala.md), ponto 3 da decisão
(«transcodifica G.711 ↔ Opus»). Tudo o resto do ADR-0010 fica como está.

---

## Contexto

O ADR-0010 fixou G.711 na perna entre o FreeSWITCH e o UA da ponte, «porque é
isso que o telefone fala». Era verdade para a rede telefónica e deixou de o ser
para os ramais: um softphone fala Opus.

Medido numa chamada real no laboratório (compose, 2026-10-05), com o gravador do
FreeSWITCH nas duas pernas:

- a perna do ramal (Linphone Android 6.2.8) negociou **Opus a 48 kHz**; a perna
  para a ponte, **PCMA a 8 kHz** — porque o servidor manda
  `absolute_codec_string=PCMA` e o UA só aceita PCMA/PCMU;
- a voz chegava à ponte a −20 dBFS em fala, com picos a −3 dBFS: o nível não era
  baixo;
- a mesma voz, passada pelo `Ingress` e pelo `Mixer` da ponte e descodificada com
  a libopus, saiu com o mesmo nível (±0,1 dB) e as mesmas bandas até 3,9 kHz
  (±1,5 dB): **a ponte é transparente dentro da banda que recebe**;
- o que se perdia era tudo o que estava acima de ~3,4 kHz, que o softphone enviava
  e a perna a 8 kHz deitava fora.

O estrangulamento era a perna, não os codecs da ponte.

Ao escrever o controlo negativo da banda apareceu **um defeito que já lá estava**,
no sentido sala → telefone do caminho G.711: o misturador pedia 8 kHz ao
descodificador do `opus-rs`, que desce um pacote SILK de banda larga sem filtro.
Um tom de 6 kHz de um microfone da sala saía inteiro para o telefone (−27,1 dB,
dobrado para dentro da banda), onde a libopus com filtro dá silêncio (−72,6 dB).
Os pacotes CELT e híbridos — o que um browser manda quando tem largura de banda
— não sofrem disto; os SILK de banda larga, que ele manda quando a rede aperta,
sim.

## Decisão

1. **O UA da ponte passa a aceitar Opus** (`opus/48000/2`) além de PCMA/PCMU, e
   responde com o primeiro codec da oferta que souber falar — a ordem é a de
   quem oferece. Uma oferta sem nenhum dos três continua a levar `488`.
2. **Telefone → sala, em Opus: sem recodificação.** O pacote que o FreeSWITCH
   manda já é Opus; a ponte decifra o SRTP, valida-o com o seu descodificador
   (o que ele recusa não entra na sala), mede-lhe o nível para o selector de
   oradores, e publica o MESMO payload. A numeração e o relógio seguem os do
   FreeSWITCH, para a perda chegar ao browser como perda (e o FEC e o PLC dele
   servirem para alguma coisa).
3. **Sala → telefone, em Opus: mistura a 16 kHz.** O misturador descodifica cada
   microfone a 16 kHz em vez de 8, soma todos menos a própria chamada, limita, e
   codifica **um** fluxo Opus de banda larga (mono, taxa constante de 32 kbps).
   Taxa constante porque o `opus-rs` ignora o alvo em taxa variável (medido:
   84 kbps com 32 pedidos; a 8 kHz, 50 com 16 pedidos).
4. **Quem liga a banda larga é o servidor**, na dial string que devolve ao IVR:
   `absolute_codec_string=OPUS,PCMA` em vez de `PCMA`. `PHONE_BRIDGE_WIDEBAND=0`
   repõe `PCMA` e, com ele, o caminho de sempre — sem reconstruir nada.
5. **O misturador descodifica sempre a 16 kHz**, e é a ponte que desce a soma
   para 8 kHz numa perna G.711, com um passa-baixo antes (FIR de 47 coeficientes,
   plano até 3,4 kHz, 48 dB ou mais abaixo a partir de 4,6 kHz; 1,4 ms de
   atraso). Um filtro por perna, sobre a soma, e não um por microfone.
6. **No resto, o caminho G.711 fica como estava.** Um tronco ou uma prova que
   ofereça só PCMA/PCMU percorre o código que a R221 e a R222 mediram, com a
   mesma negociação, o mesmo `Ingress` e o mesmo nível.

## O que se ganha, e o que se perde

**Ganha-se** a voz de um softphone na sala com a banda que ele enviou, sem passar
por dois codecs a mais; e a sala no telefone a 16 kHz em vez de 8.

**Uma chamada da rede pública não ganha nada**: nasce a 8 kHz. Com a banda larga
ligada, quem a converte para Opus passa a ser a libopus do FreeSWITCH em vez do
`opus-rs` da ponte — o custo de CPU muda de lado, a qualidade não piora.

**Perde-se** a simplicidade de um só codec na perna: o UA lê mais uma linha de
`rtpmap`, e a perna tem dois modos. Não há dependência nova — o `opus-rs` já
fazia os dois sentidos.

**Fica por resolver, e não é desta decisão:** o ruído de fundo da origem (medido
a −35 dBFS entre palavras nessa chamada) e o nível — nenhum dos dois é da ponte.
Não há controlo de ganho nem supressão de ruído no caminho do telefone.

## Prova

- `phone_bridge::` — a negociação (Opus à frente, só Opus, só G.711, ordem da
  oferta), a numeração e o relógio da origem a passarem com os buracos da perda
  no sítio, o pacote corrompido que não entra, a resposta do filtro da descida,
  e um tom de 6 kHz da sala que **chega** ao telefone em Opus e **não chega nem
  dobra** em G.711 — o controlo negativo da banda, que antes do filtro falhava
  com o tom dobrado a 0,25 (a amplitude toda).
- `sfu_e2e::ponte_em_opus_leva_banda_larga_nos_dois_sentidos` — contra um SFU
  real: o softphone manda 5 kHz e a sala recebe-os em payloads que são, byte a
  byte, os que ele mandou; a sala manda 6 kHz e ele ouve-os, sem se ouvir a si.
- Sonda fora do repo (2026-10-05), com fala sintética a 16 kHz codificada pelo
  `opus-rs` e descodificada pela libopus: nível igual (−19,6 dB), banda de
  5–7,5 kHz a −45,0 dB contra −44,5 dB do codificador da libopus à mesma taxa;
  0,2 ms de CPU por bloco de 20 ms.
- **Por medir, e é o que fecha isto:** uma chamada real de softphone com a perna
  em Opus, gravada nas duas pernas, e a mesma chamada ouvida num browser.
