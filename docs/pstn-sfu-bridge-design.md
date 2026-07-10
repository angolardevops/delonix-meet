# Delonix Meet — Ponte FreeSWITCH ↔ SFU (sub-fase 2b) · Design

> Como o áudio **PSTN** (conferência do FreeSWITCH) e o áudio **WebRTC** (SFU
> `webrtc-rs` existente) se misturam na MESMA reunião. Requer teste de media real
> (host próprio) — este documento define a arquitetura para decidir e implementar.

## O problema central: SFU (forwarding) vs conferência (mixing)

O nosso SFU **encaminha** tracks individuais (N participantes → cada um recebe N-1
tracks e mistura no browser). O FreeSWITCH **mistura** (um único stream de áudio). O
telefone PSTN só sabe receber **um** stream. Logo, alguém tem de produzir, para o
lado PSTN, **um mix de todos os participantes WebRTC** — coisa que um SFU puro não faz.

## Abordagem A — FreeSWITCH como participante WebRTC do SFU
O FreeSWITCH junta-se à sala do SFU como **um** participante:
- **Uplink**: publica 1 track Opus = mix dos chamadores PSTN (o `mod_conference` já
  mistura). Os WebRTC recebem-no como "o participante Telefone".
- **Downlink**: subscreve os N tracks WebRTC e mistura-os no `mod_conference` para os
  PSTN ouvirem.
- **Custo**: o FreeSWITCH tem de falar a **sinalização do nosso SFU** (`/rtc`/signaling
  WS) — precisa de um adaptador/shim (o `mod_verto` fala Verto, não o nosso protocolo).
  É um cliente WebRTC completo do lado do FreeSWITCH. Maior acoplamento à sinalização.

## Abordagem B — Bridge RTP com mixer de stream único no SFU (RECOMENDADA)
O SFU ganha, **por sala com PSTN ativo**, um par de pontas RTP/SRTP para o FreeSWITCH:
- **Uplink PSTN → SFU**: o FreeSWITCH envia o mix da conferência (1 stream SRTP) para
  uma **ingress RTP** do SFU; o SFU encaminha-o aos WebRTC como o track "Telefone"
  (o `webrtc-rs` já trata do empacotamento RTP interno).
- **Downlink SFU → PSTN**: o SFU adiciona um **mixer de egress único** (decode Opus dos
  participantes WebRTC → soma → encode Opus) e envia **1** stream SRTP ao FreeSWITCH.
- **Vantagem**: o nosso SFU continua a ser a autoridade WebRTC (sem shim de sinalização
  externo); a complexidade nova fica confinada a **um** mixer de egress bem definido.
- **Custo**: adiciona mixing de áudio ao SFU (CPU por sala com PSTN; jitter buffer +
  Opus). Bounded, mas é código de media novo em `sfu.rs`.

## Recomendação: **B**
Menos acoplamento (sem cliente WebRTC no FreeSWITCH a falar o nosso protocolo), a nova
complexidade é 1 mixer isolado, e o control plane já sabe encaminhar por `media_backend`.

### Mudanças concretas (Abordagem B)
1. **`server/src/sfu.rs`** — por sala, opcional "PSTN bridge":
   - `pstn_ingress(room, ssrc)`: aceita RTP/SRTP de fora e injeta como track de áudio
     "Telefone" na sala (reusa o pipeline de forwarding existente).
   - `pstn_egress_mixer(room)`: subscreve os tracks de áudio dos peers, decodifica
     (Opus), mistura (soma com AGC/limiter simples), reencoda e envia 1 stream SRTP.
   - Ativado só quando existe uma `voice_room` ativa para a sala.
2. **Control plane (já pronto)** — na validação de PIN, além de `room_code`, devolver o
   **endpoint de bridge** do SFU para a sala (host:porta SRTP + chaves). O IVR do
   FreeSWITCH usa-o para `bridge`/`sofia` em vez de `conference` local, OU liga a sua
   conferência a esse endpoint (`conference ... +flags`), consoante a integração.
3. **FreeSWITCH** — o dialplan passa a fazer a ponte da conferência PSTN para o endpoint
   RTP do SFU (SRTP obrigatório), em vez de conferência puramente local.
4. **Segurança** — SRTP nas duas pontas; chaves efémeras por sala emitidas pelo control
   plane; a ingress do SFU só aceita do IP do(s) FreeSWITCH.

## Plano de teste (host próprio, fora do sandbox)
1. Softphone → Kamailio → FreeSWITCH → IVR → ponte para o SFU.
2. Um participante **WebRTC** na mesma sala (`room_code`) via browser.
3. Confirmar áudio bidirecional PSTN↔WebRTC; medir latência/jitter; validar SRTP nas
   duas pontas (sem media em claro).

## Esforço / risco
Médio-alto: mixer de áudio + SRTP + timing são a parte sensível. Recomenda-se um
protótipo de 1 sala (1 PSTN + 1 WebRTC) antes de generalizar e antes de ligar o trunk
real (sub-fase 3). **Não é validável neste ambiente** — precisa de FreeSWITCH + media.
