---
name: delonix-meet-telefonia
description: Telefone e sala no Delonix Meet — a ponte telefone↔sala (UA SIP no SFU, SDES-SRTP, G.711↔Opus, mix-minus), o dial-in PSTN e o IVR do FreeSWITCH, e a telefonia por portar (troncos, plano de marcação, CDR, canais da sala). Usa-a quando o pedido falar em «telefone», «PSTN», «dial-in», «SIP», «FreeSWITCH», «SRTP», «G.711», «tronco», «DID», «IVR», «ramal», «CDR», ou quando o diff tocar em `server/src/phone_bridge/`, `voice.rs` ou `voice/freeswitch/`. NÃO a uses para a media do browser dentro do SFU (`delonix-meet-webrtc`) nem para o desenho das rotas (`delonix-meet-api`) — embora sinalizes ambos quando os vires.
---

# Telefone e sala — o que está ligado e o que não está

**Autoridade:** [ADR-0010](../../../docs/adr/0010-ponte-telefone-sala.md) (Aceite).
**Catálogo:** R221 e R222 em [`regressions.md`](../../../docs/reference/regressions.md).
**Histórico da decisão:** [design da Abordagem B](../../../docs/pstn-sfu-bridge-design.md),
marcado **superseded** — lê-o para não repetir o erro, não para o seguir.

## Fronteira

- **`delonix-meet-webrtc`** revê a media do lado do browser: negociação, ICE, simulcast,
  gravação. A perna do telefone entra no SFU como publicador normal — a partir daí é
  território dele.
- **`delonix-meet-security`** revê a superfície nova de rede: o socket SIP, a allowlist e
  o material de chave. Chama-o em qualquer diff que mexa em `phone_bridge/srtp.rs` ou na
  allowlist.
- **`delonix-meet-api`** revê o contrato `room_bridge` se ele mudar de forma.
- Esta skill é a única que sabe **o que o FreeSWITCH consegue fazer e o que não consegue**.

## A pergunta que fazes a tudo

**O chamador ouve a sala e a sala ouve-o, ou só o código existe?** Este domínio já teve
777 linhas escritas, testadas e **nunca ligadas** durante onze dias (R222). «Compila» e
«os testes passam» não distinguem os dois casos.

## O que está ligado (2026-09-30, `main` 1fd4750)

Quem entra por telefone é um **participante da sala**: fala e ouve os participantes
WebRTC. O caminho, de ponta a ponta:

1. o IVR (`voice/freeswitch/scripts/dialin_ivr.lua`) pede o PIN e valida-o em
   `/internal/v1/voice/ivr/validate`;
2. o control plane devolve `room_bridge` — `sip_uri`, `channel_vars`, `srtp_profile`
   (`voice::room_bridge_for`);
3. o Lua faz `bridge` para esse URI, com as variáveis no **prefixo `[k=v,…]` da dial
   string** — no canal A elas não chegam à perna B;
4. o UA SIP da ponte atende (`phone_bridge::sip`), negoceia SDES-SRTP **no SDP, por
   chamada**, e transcodifica G.711↔Opus;
5. a perna publica no SFU (`sfu::PubSource::Bridge`) e recebe a mistura **menos a
   própria voz**.

**Fail-closed em três sítios**, e é assim que fica: sem `PHONE_BRIDGE_SIP_BIND` o UA não
arranca; com `PHONE_BRIDGE_FREESWITCH_IPS` vazio também não; uma oferta sem `a=crypto`
leva `488`. Em qualquer falha o IVR **cai na conferência local** — um chamador nunca fica
de fora por causa da ponte.

### O que o FreeSWITCH 1.11.3 de stock NÃO faz

Mandar e receber RTP cifrado com uma chave dada **por fora**, para um par UDP arbitrário,
sem um segundo diálogo SIP. Não há módulo para isso: o `mod_audio_fork` manda áudio por
WebSocket para STT, e `uuid_deflect`/`snoop`/`unicast` fazem outra coisa. Foi esta
premissa por confirmar que deixou a Abordagem B onze dias no papel. **O que ele faz bem é
originar uma segunda perna SIP** — e é por isso que o shim vive do nosso lado.

## Portões

| O que mexeste | Portão |
|---|---|
| Qualquer coisa em `phone_bridge/` | `cargo test --lib phone_bridge::` (38 unitários: G.711, SRTP, SDP, mistura, jitter) |
| O caminho da media | `cargo test --lib ponte_telefone_sala -- --nocapture` (R221 — imprime atraso por sentido, mix-minus e CPU por chamada) |
| A cadeia toda | a prova real, abaixo (R222) — **fora do CI** |
| O contrato com o IVR | não há portão automático: ver o aviso do Lua, abaixo |
| Qualquer mudança | `make fitness` |

### A prova real, e como a correr

```bash
bash scripts/fs-canais.sh up        # imagem local delonix-dev/freeswitch:1.11.3
FS_ESL_ADDR=127.0.0.1:8221 FS_ESL_PASSWORD=$(cat .fs-canais/esl-password.txt) \
  FS_CANAIS_GW=dlx-0c0a1500-0000-4000-8000-00000000d0d0 \
  FS_CANAIS_RECORDINGS=$PWD/.fs-canais/recordings \
  cargo test --release --lib ponte_com_freeswitch_real -- --nocapture --test-threads=1
bash scripts/fs-canais.sh down
```

**Sem as quatro variáveis o teste diz «NÃO CORREU» e passa** — não deixa o CI vermelho, e
também não prova lá nada ([`e2e-fora-do-ci.txt`](../../../scripts/e2e-fora-do-ci.txt)).
O `FS_BASE_CONF` por omissão (`../../freeswitch-build/conf`) só está certo a partir de um
worktree em `.worktrees/<repo>/<tarefa>`.

Números da corrida de 2026-09-30, para teres com que comparar: `originate`→atendida
328 ms · `200 OK` do UA 330 ms · 1 kHz do telefone na sala 0,1548 · 440 Hz da sala na
gravação do telefone 0,2495 · o próprio tom do telefone nesse canal 0,0001.

**Um detalhe que já enganou o teste:** o FreeSWITCH só fecha o campo de tamanho do chunk
`data` do WAV quando a chamada termina. Ler antes disso dá um ficheiro de 0,0 s com
160 KiB de áudio dentro, e a prova conclui que a sala não chegou ao telefone quando
chegou.

## O aviso que este domínio tem de carregar

**O `dialin_ivr.lua` está no caminho do cliente e ninguém lhe verifica a sintaxe.** Não há
interpretador de Lua na máquina de desenvolvimento nem na imagem do FreeSWITCH, e o CI não
o lê. Um erro de sintaxe ali não aparece em portão nenhum — aparece a quem liga. Se
mexeres nesse ficheiro, di-lo no relatório em vez de o dar por verificado.

## O que NÃO está portado, e onde está

Tudo isto vive em `origin/delonix-meet-backend/v3-canais` e mede-se com
`git diff --stat origin/main...origin/delonix-meet-backend/v3-canais -- <caminho>`:

| Frente | Tamanho | O que traz |
|---|---|---|
| Telefonia (C) | 9 772 linhas, 18 ficheiros | troncos, plano de marcação, registo SIP, CDR, consumo, ESL, `xml_curl`, ADR-0009, 17 rotas, `tests/telephony.rs` |
| Canais da sala | 1 197 linhas, 8 ficheiros | canal e crachás por participante no WebSocket, porta do WhatsApp Business, migração `0095_room_channels` |
| e2e | 300 linhas | `telefonia-freeswitch.mjs` e casos no `isolamento.mjs` |

**A frente dos canais é quem traz o consumidor** do silenciar e do pôr a palco uma perna.
Esses métodos foram **retirados** da ponte no #130, de propósito: API inalcançável conta
avisos e não se pode provar. Voltam com quem os chama.

## Ao fechar uma tarefa

Propõe um a três pedidos seguintes, cada um com o alvo, a prova a medir e o que fica de
fora. Por ordem de valor, hoje:

1. «Porta a frente dos canais (1 197 linhas) sobre a `main`, com o silenciar e o pôr a
   palco uma perna a serem chamados de facto. Prova: um caso em `sfu_e2e` que silencia
   uma perna e mede que o tom deixa de chegar à sala. Fora: a telefonia (frente C).»
2. «Porta a frente C da telefonia (9 772 linhas) com o ADR-0009 e as 17 rotas. Prova:
   `tests/telephony.rs` contra Postgres real e o `telefonia-freeswitch.mjs` contra a
   imagem local. Fora: o WhatsApp Business.»
3. «Põe um verificador de sintaxe de Lua no `make fitness` e no CI, e mete o
   `dialin_ivr.lua` debaixo dele. Prova: um controlo negativo — partir o ficheiro de
   propósito e ver o portão falhar. Fora: testar o comportamento do IVR.»
