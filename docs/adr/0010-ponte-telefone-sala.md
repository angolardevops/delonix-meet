# ADR-0010 — A ponte telefone↔sala é um UA SIP no lado do SFU

**Estado:** Aceite · **Data:** 2026-09-30 ·
**Contexto:** fechar o dial-in PSTN — quem liga por telefone tem de ouvir e ser
ouvido pelos participantes WebRTC da MESMA sala ·
**Sucede a:** [Abordagem B](../pstn-sfu-bridge-design.md) (`pstn_bridge.rs`),
que fica **superseded** ·
**Assenta em:** [ADR-0004](0004-organizacao-alvo-do-backend.md).

> O número 0009 fica reservado ao ADR da telefonia (troncos, plano de marcação,
> CDR), que vem noutra entrega.

## Contexto

A Abordagem B punha o SFU a receber RTP/SRTP **cru** do FreeSWITCH num par
UDP, com chaves efémeras entregues por fora, no JSON do IVR. Evitava um
segundo diálogo SIP de propósito: o SFU não teria de falar SDP.

Esse desenho foi implementado e testado do lado do SFU (`pstn_bridge.rs`, 777
linhas, mistura Opus real). **Nunca foi ligado.** O `dialin_ivr.lua` ficou com
uma pergunta escrita no topo, por não ter havido um FreeSWITCH real para a
responder: *qual é o mecanismo do FreeSWITCH para uma chamada activa mandar e
receber RTP cifrado com uma chave dada por fora, para um endereço UDP
arbitrário, sem abrir um segundo diálogo SIP?*

**A resposta, medida contra um FreeSWITCH 1.11.3 real: não há.** A imagem de
stock não traz um módulo que o faça. O `mod_audio_fork` manda áudio por
WebSocket para STT, não RTP bidireccional; `uuid_deflect`, `snoop` e `unicast`
não fazem o que o desenho precisava. O que o FreeSWITCH faz bem, e é o seu
caminho mais batido, é **originar uma segunda perna SIP**.

> Esta leitura da imagem 1.11.3 vem do trabalho que construiu a ponte, não foi
> repetida ao trazê-la para a `main`. O que ESTA entrega volta a medir é o
> caminho da media (ver **Prova**).

Enquanto isso, o dial-in continuava a cair na conferência local: quem ligava
por telefone ouvia os outros chamadores, mas nunca a sala.

## Decisão

O lado do SFU ganha o **shim SIP que a própria pergunta antecipava**:
`server/src/phone_bridge/`, um UA SIP mínimo que

1. atende o `INVITE` da segunda perna que o FreeSWITCH origina;
2. negoceia **SDES-SRTP no SDP** — chaves por chamada, no diálogo, nunca por
   fora e nunca em variáveis de canal. Uma oferta sem `a=crypto` leva `488`;
3. transcodifica **G.711 (PCMA/PCMU) ↔ Opus**, porque é isso que o telefone
   fala e a sala não;
4. publica o chamador na sala como um publicador normal do SFU, e devolve-lhe
   a mistura **menos a própria voz** (mix-minus);
5. só aceita `INVITE` de IPs numa allowlist — **fail-closed**: lista vazia,
   ponte desligada.

O `voice.rs` deixa de devolver `pstn_bridge` (host, porta e chaves) e passa a
devolver `room_bridge`: para onde fazer `bridge` e que variáveis de canal pôr
antes. O `dialin_ivr.lua` executa esse `bridge` e, se ele falhar por qualquer
razão, cai na conferência local como sempre fez.

## O que se ganha, e o que se perde

**Ganha-se** um caminho que o FreeSWITCH de stock sabe percorrer, chaves SRTP
negociadas por chamada em vez de distribuídas por JSON, e o chamador como
participante de primeira classe da sala (entra no censo, no selector de
oradores e na gravação).

**Perde-se** a promessa de «nenhuma sinalização nova no SFU»: o servidor passa
a ter um UA SIP. É pequeno (`sip.rs`, ~886 linhas) e só atende — não regista,
não origina, não faz SIP para fora. A superfície de rede nova é um socket UDP
com allowlist.

**Custo em dependências:** sai o `audiopus` (ligava-se à `libopus` do sistema e
obrigava a `libopus-dev` no host de build e na imagem) e entra `opus-rs`, em
Rust puro.

## Prova

- `sfu_e2e::ponte_telefone_sala_tom_nos_dois_sentidos` — a perna publica na sala e ouve a mistura
  sem a própria voz, contra um SFU real (R221).
- `sfu_e2e::ponte_com_freeswitch_real_tom_nos_dois_sentidos` — cadeia completa
  contra um **FreeSWITCH 1.11.3 real**: `originate` → «telefone» que atende,
  grava e toca 1 kHz → `bridge` SIP → UA da ponte → SFU → participante
  webrtc-rs a publicar 440 Hz. Mede-se o 1 kHz na sala E os 440 Hz da sala na
  gravação do telefone (R222). Fora do CI: precisa da imagem
  `delonix-dev/freeswitch:1.11.3`, que o CI não alcança
  (`scripts/e2e-fora-do-ci.txt`).
- 38 testes unitários em `phone_bridge/` (G.711, SRTP, SDP, mistura, jitter).
