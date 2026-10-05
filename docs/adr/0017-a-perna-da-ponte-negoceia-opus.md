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
   manda já é Opus; a ponte decifra o SRTP, só deixa passar o que cabe num tecto
   de 600 bytes e o seu descodificador consegue ler, mede-lhe o nível para o
   selector de oradores, e publica o MESMO payload. **O relógio de saída segue
   o da origem**: perda, silêncio suprimido e um intervalo em que a perna esteve
   calada ficam no relógio com o tamanho que tiveram. É o relógio, e só ele,
   que chega aos browsers como marca do tempo — o SFU renumera a sequência de
   todo o áudio que lhes entrega. A sequência da origem (com os buracos da
   perda) chega à gravação e aos misturadores das outras pernas.
3. **Sala → telefone, em Opus: mistura a 16 kHz.** O misturador descodifica cada
   microfone a 16 kHz em vez de 8, soma todos menos a própria chamada, limita, e
   codifica **um** fluxo Opus de banda larga (mono, taxa constante de 32 kbps).
   Taxa constante porque o `opus-rs` ignora o alvo em taxa variável (medido:
   84 kbps com 32 pedidos; a 8 kHz, 50 com 16 pedidos).
4. **Quem liga a banda larga é o servidor**, na dial string que devolve ao IVR:
   `absolute_codec_string=OPUS,PCMA` em vez de `PCMA`. `PHONE_BRIDGE_WIDEBAND=0`
   repõe `PCMA` e, com ele, o caminho de sempre — sem reconstruir nada. O valor
   tem uma vírgula, e o `dialin_ivr.lua` passa a escapá-la: sem isso o
   FreeSWITCH partia a lista de variáveis, oferecia só Opus, e o G.711 de
   recurso não existia (apanhado na revisão, antes de qualquer chamada).
5. **A ponte não pede FEC e não anuncia a taxa a que captura.** A resposta SDP
   leva `useinbandfec=0; stereo=0; sprop-stereo=0`, e mais nada. Medido contra
   o FreeSWITCH 1.11.3 (libopus 1.3.1): com `useinbandfec=1` ele codificava para
   a ponte em **banda média** (6 kHz) para o FEC caber — e o FEC não servia a
   ninguém, porque o SFU renumera a sequência; com `sprop-maxcapturerate=16000`
   abria o codec a 16 kHz nos dois sentidos, recodificando e limitando a 8 kHz
   o que vinha do softphone.
6. **O misturador não toca banda média.** O `opus-rs` (0.1.33 e 0.1.34)
   descodifica mal o SILK de banda média a qualquer taxa de saída: contra a
   libopus, um tom sai 8 dB abaixo e fala sai 15 dB acima, distorcida. Esses
   pacotes ficam de fora da mistura (silêncio, e um contador no fecho da perna)
   em vez de chegarem assim ao telefone. Para a sala passam: quem os toca é a
   libopus do browser. A libopus só escolhe banda média quando lhe limitam a
   banda, por isso o caso deixa de acontecer com o ponto 5 — a guarda fica para
   quando acontecer por outra via.
7. **O misturador descodifica sempre a 16 kHz**, e é a ponte que desce a soma
   para 8 kHz numa perna G.711, com um passa-baixo antes (FIR de 47 coeficientes,
   plano até 3,4 kHz, 48 dB ou mais abaixo a partir de 4,6 kHz; 1,4 ms de
   atraso). Um filtro por perna, sobre a soma, e não um por microfone.
8. **No resto, o caminho G.711 fica como estava.** Um tronco ou uma prova que
   ofereça só PCMA/PCMU percorre o código que a R221 e a R222 mediram, com a
   mesma negociação, o mesmo `Ingress` e o mesmo nível.

## Segurança: o que muda em quem controla os bytes

**Antes desta decisão nenhum byte de quem liga chegava a um browser.** Tudo o
que entrava pela perna era G.711, descodificado e recodificado pela ponte. Com a
perna em Opus, o payload passa intacto para três sítios: os browsers da sala, o
ficheiro de gravação, e os misturadores das outras pernas de telefone da sala.

**Quem escolhe esses bytes.** O FreeSWITCH só transcodifica quando os codecs das
duas pernas diferem. Um ramal (softphone), a central de uma organização ou
qualquer par SIP que negoceie Opus com o FreeSWITCH pode, por isso, pôr na sala
bytes que ele escolhe. Um chamador da rede pública não: aí quem gera o Opus é a
libopus do FreeSWITCH.

**O que isto alarga.** Esses três consumidores já recebiam Opus arbitrário de
qualquer participante de browser. A população passa a incluir quem só tem um
ramal ou o PIN da sala. O isolamento entre organizações não muda: a autoridade
continua a ser o PIN ou a organização do ramal.

**O validador não é uma fronteira de segurança.** Garante que o pacote é
estruturalmente Opus e que cabe no tecto; não garante o que ele contém. As três
barras de sempre continuam à frente dele e não foram tocadas: a allowlist de
IP, o SRTP por chamada (`488` sem `a=crypto`), e só o tipo de payload negociado.

**O descodificador lê bytes da rede.** O `opus-rs` tem código `unsafe` e já
corrigiu fora-de-limites. O que este repo tem: uma amostra determinista de
20 000 pacotes aleatórios e mutados sem um pânico (300 000 fora do repo), e a
perna passa a largar a sala mesmo que a sua tarefa rebente — antes, um pânico
deixava a publicação na sala para sempre. Não é uma auditoria do crate.

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
  oferta); o relógio e a numeração da origem ao longo de um minuto com perda e
  troca de ordem nas fronteiras dos dez segundos (onde a primeira versão as
  apagava); o silêncio imposto que fica no relógio e não na sequência; o pacote
  corrompido e o que passa do tecto, que não entram; seis pacotes reais da
  libopus, um de cada forma, que entram; a resposta do filtro da descida,
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
- **Contra o FreeSWITCH real** (laboratório compose, 2026-10-05, dois softphones
  de linha de comandos — `scripts/softphone-prova.sh par` — em dois ramais,
  na mesma sala, com SRTP obrigatório):
  - a oferta do FreeSWITCH à ponte traz `opus/48000/2` e `PCMA`, por essa ordem
    (`absolute_codec_string=OPUS\,PCMA` no log dele: a vírgula chega inteira), e
    a ponte responde Opus;
  - com `PHONE_BRIDGE_WIDEBAND=0` a perna volta a PCMA e cada softphone ouve o
    tom do outro a 0,2504 e 0,2506 (enviado a 0,25) e o próprio a 0,001 — o
    caminho G.711, com o filtro novo, é exacto;
  - com a perna em Opus e a primeira resposta SDP (`useinbandfec=1`), o tom de
    1 kHz chegava ao outro telefone a 0,092 (−8,6 dB), em quatro corridas; o
    registo de depuração do `mod_opus` mostrou porquê: 2 583 blocos codificados
    em `MEDIUMBAND` para a ponte. É o que os pontos 5 e 6 corrigem.
- **Por medir, e é o que fecha isto:** uma chamada real de softphone com a perna
  em Opus — a oferta do FreeSWITCH com os DOIS codecs, a gravação das duas
  pernas — e a mesma chamada ouvida num browser. A prova contra o FreeSWITCH
  real (R222) continua a marcar `PCMA`: o caminho que passa a ser o por omissão
  é uma variante por medir do que ela mediu. Também sem prova: a gravação de
  uma perna em Opus, e o palco (R225) nesse codec.

## Para quem actualiza

A banda larga vem **ligada**. Numa instalação existente, sem ninguém mexer na
configuração: a perna para a ponte passa a oferecer Opus antes de PCMA (o
FreeSWITCH tem de ter o `mod_opus`, ou cai-se no PCMA); os bytes de um softphone
chegam à sala e à gravação sem recodificação; e cada perna passa a descodificar
a sala a 16 kHz. `PHONE_BRIDGE_WIDEBAND=0` repõe o que havia nos dois primeiros.
