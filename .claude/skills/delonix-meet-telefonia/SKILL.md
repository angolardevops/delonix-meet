---
name: delonix-meet-telefonia
description: >-
  Telefone no Delonix Meet — a ponte telefone↔sala (UA SIP no SFU, SDES-SRTP,
  G.711↔Opus, mix-minus), o telefone no censo da sala (crachás, `ForceMute`), o
  dial-in PSTN e o IVR do FreeSWITCH, a telefonia de troncos, plano de marcação, CDR
  e custo (ADR-0009), e o que ainda não tem consumidor.
when_to_use: >-
  Quando o pedido falar em «telefone», «PSTN», «dial-in», «SIP», «FreeSWITCH»,
  «SRTP», «G.711», «tronco», «DID», «IVR», «ramal», «CDR», «plano de marcação», ou
  quando o diff tocar em `server/src/phone_bridge/`, `voice.rs`, `ramais.rs`,
  `telephony_*.rs`, `server/crates/delonix-meet-domain/src/telephony/` ou
  `voice/freeswitch/`. NÃO a uses para a media do browser dentro do SFU (revisor
  `delonix-meet-webrtc`) nem para a forma das rotas (`delonix-meet-api`) — embora
  sinalizes ambos quando os vires.
---

# Telefone e sala — o que está ligado e o que não está

**Autoridade:** [ADR-0010](../../../docs/adr/0010-ponte-telefone-sala.md) (Aceite) para a
ponte; [ADR-0009](../../../docs/adr/0009-telefonia-troncos-encaminhamento-e-custo.md)
para troncos, encaminhamento e custo — **ainda «Proposto»** (`:3`) com o código já na
`main` desde o #136: di-lo no relatório, não o trates como aceite.
**Catálogo:** R210–R214 (telefonia) e R221–R225 (ponte, imagem, censo, palco) em
[`regressions.md`](../../../docs/reference/regressions.md).
**Histórico da decisão:** [design da Abordagem B](../../../docs/pstn-sfu-bridge-design.md),
marcado **superseded** — lê-o para não repetir o erro, não para o seguir.

## Fronteira

- **`delonix-meet-api`** (skill e revisor) — a forma das rotas `/api/orgs/{org_id}/telephony/*`
  e `/internal/v1/telephony/*`, e o contrato `room_bridge` se mudar de forma. Aqui
  está o que elas fazem.
- **`delonix-meet-backend`** — camadas, catraca e o estado geral da segurança. As
  regressões de segurança da telefonia (R213, R214) são descritas aqui e listadas lá.
- **`delonix-meet-voip`** — como o outro lado se liga a nós ANTES de a
  chamada entrar: o PBX de um cliente (Issabel, FreePBX), a operadora (tronco, GSM, eSIM),
  e as boas práticas de SIP/VoIP de um tronco. Aqui está o que acontece depois.
- **`delonix-meet`** — encaminhamento e portões das outras áreas.
- **Revisor `delonix-meet-webrtc`** — a media do lado do browser: negociação, ICE,
  simulcast, gravação. A perna do telefone entra no SFU como publicador normal — a
  partir daí é território dele.
- **Revisor `delonix-meet-security`** — a superfície de rede e de segredos: o socket SIP,
  a allowlist, o material de chave SRTP, as credenciais SIP e de tronco. Chama-o em
  qualquer diff que mexa em `phone_bridge/srtp.rs`, na allowlist, em
  `telephony_trunks.rs` ou em `telephony_sip.rs`.
- Não há revisor só de telefonia. Esta skill é a única que sabe **o que o FreeSWITCH
  consegue fazer e o que não consegue**.

## A pergunta que fazes a tudo

**O chamador ouve a sala e a sala ouve-o, ou só o código existe?** Este domínio já teve
777 linhas escritas, testadas e **nunca ligadas** durante onze dias (R222), e um modelo
de canais cujos `join_external`/`update_external` só eram chamados por testes (R224).
«Compila» e «os testes passam» não distinguem os dois casos.

## O que está ligado (2026-10-03, `main` `024583a`)

### A ponte telefone↔sala (#130, ADR-0010, R221/R222)

Quem entra por telefone é um **participante da sala**: fala e ouve os participantes
WebRTC. O caminho, de ponta a ponta:

1. o IVR (`voice/freeswitch/scripts/dialin_ivr.lua`) pede o PIN e valida-o em
   `/internal/v1/voice/ivr/validate`;
2. o control plane devolve `room_bridge` — `sip_uri`, `channel_vars`, `srtp_profile`
   (`voice::room_bridge_for`, `voice.rs:868`);
3. o Lua faz `bridge` para esse URI, com as variáveis no **prefixo `[k=v,…]` da dial
   string** — no canal A elas não chegam à perna B;
4. o UA SIP da ponte atende (`phone_bridge::sip`), negoceia SDES-SRTP **no SDP, por
   chamada**, e transcodifica G.711↔Opus;
5. a perna publica no SFU (`sfu::PubSource::Bridge`) e recebe a mistura **menos a
   própria voz**.

**Fail-closed em três sítios**, e é assim que fica: sem `PHONE_BRIDGE_SIP_BIND` o UA não
arranca; com `PHONE_BRIDGE_FREESWITCH_IPS` vazio também não; uma oferta sem `a=crypto`
leva `488` (`phone_bridge/sip.rs:547`). Em qualquer falha o IVR **cai na conferência
local** — um chamador nunca fica de fora por causa da ponte. As chaves SRTP são por
chamada e negoceiam-se no SDP: **nunca** em JSON, em variáveis de canal ou em log.

### O telefone no censo da sala (#135, R224)

- Quem vem de fora da app **aparece na lista de participantes** (`signaling::join_external`),
  com o canal, o número mascarado e os crachás «sem nome», «vídeo indisponível» e
  «ligação fraca» — este último **medido** no jitter e na perda do RTP da perna
  (`phone_bridge/quality.rs`).
- O `ForceMute` de um anfitrião **impõe-se no servidor**: um telefone não tem cliente que
  honre a mensagem, por isso o comando acciona o interruptor da perna pela porta
  `signaling::PhoneControl`, que a ponte implementa (`phone_bridge/sip.rs:366`). A
  dependência corre no sentido certo — o `signaling` não conhece o `phone_bridge`.
- O pacote silenciado **conta na mesma** para a estatística e para o RTP simétrico.
- **Destacar um telefone vale de facto** (R225): o `Spotlight` fixa o áudio da perna no SFU
  pela porta `signaling::StageControl` (`SfuState::set_audio_pinned`), o selector de
  oradores deixa de a poder suprimir, e o lugar leva o crachá `on_stage`. Trocar ou limpar
  o destaque liberta o anterior.

### A telefonia (#136, ADR-0009, R210–R214)

- **Onde está:** regras sem IO em `server/crates/delonix-meet-domain/src/telephony/`
  (`cost`, `dial_plan`, `money`, `number`, `ports`, `trunk`); adaptadores e handlers em
  `server/src/telephony_{trunks,dial_plan,sip,calls,cdr,esl,fs_xml,service}.rs`;
  migrações `0069`–`0072`.
- **Rotas:** dezassete registos — quinze em `lib.rs:845-902`, sob
  `/api/orgs/{org_id}/telephony/` (`trunks`, `trunks/{id}`, `trunks/{id}/prices`,
  `trunk-order`, `exchange-rates`, `dial-plan`, `dial-plan/test`, `sip-settings`,
  `sip-settings/reveal-credentials`, `sip-registration`, `sip-registration/restart`,
  `test-calls`, `test-calls/{id}`, `call-records`, `usage`) e dois no listener interno,
  `lib.rs:213-218` (`/internal/v1/telephony/call-records`, onde o `mod_json_cdr` entrega os CDR, e
  `/internal/v1/telephony/freeswitch-config`, que serve o `mod_xml_curl`).
- **As cinco regras que custaram**, e não se reabrem:
  - **R210** — a emergência (112) nunca é gravada, bloqueada nem travada pelo limite de
    canais, por muito que o plano de marcação do cliente diga o contrário;
  - **R211** — um CDR reenviado não cobra duas vezes (`200 duplicate`);
  - **R212** — o custo é o do preço em vigor **quando a chamada aconteceu**;
  - **R213** — o host de um tronco passa por `net_guard::check_tenant_config_url`
    (SSRF por SIP); SRTP diferente de `off` exige TLS; o domínio SIP é único na base;
  - **R214** — passwords de tronco e de SIP cifradas em repouso, nunca devolvidas em
    `GET`; a única saída é `POST …/sip-settings/reveal-credentials`, com reautenticação,
    bloqueio às cinco falhas e auditoria.
- **Os ramais** (`ramais.rs`, migrações `0066`/`0067`) são anteriores e vivem em
  `/api/orgs/{org_id}/voice/…`; o FreeSWITCH chama-os por `/api/voice/ivr/*`
  (`lib.rs:834-841`) — três rotas de máquina na árvore pública, dívida nomeada em
  `delonix-meet-api`.

### O que o FreeSWITCH 1.11.3 de stock NÃO faz

Mandar e receber RTP cifrado com uma chave dada **por fora**, para um par UDP arbitrário,
sem um segundo diálogo SIP. Não há módulo para isso: o `mod_audio_fork` manda áudio por
WebSocket para STT, e `uuid_deflect`/`snoop`/`unicast` fazem outra coisa; o `mod_rtp` não
existe na 1.11.3 (`voice/freeswitch/image/Containerfile:11`). Foi esta premissa por
confirmar que deixou a Abordagem B onze dias no papel. **O que ele faz bem é originar uma
segunda perna SIP** — e é por isso que o shim vive do nosso lado.

## Portões

| O que mexeste | Portão |
|---|---|
| Qualquer coisa em `phone_bridge/` | `cargo test --lib phone_bridge::` (38 unitários: G.711, SRTP, SDP, mistura, jitter, qualidade) |
| O caminho da media | `cargo test --lib ponte_telefone_sala -- --nocapture` (R221 — imprime atraso por sentido, mix-minus e CPU por chamada) |
| O censo e o `ForceMute` | `cargo test --lib force_mute_cala_o_telefone_na_perna` (R224 — o tom desaparece e **volta**) |
| O palco (`Spotlight`, `StageControl`, `pinned`) | `cargo test --lib destacar_fixa_o_audio_no_sfu` e `cargo test --lib palco_impede_o_selector -- --nocapture` (R225 — suprimido, e fixado **volta**) |
| Troncos, plano de marcação, CDR, custo, credenciais | `cargo test --release --test telephony -- --test-threads=4` contra Postgres real (14 casos; precisa de `DATABASE_URL`) + os unitários do domínio |
| Uma rota `/telephony` | os portões de `delonix-meet-api`, com o caso negativo em `web/e2e/isolamento.mjs` |
| A cadeia toda da ponte | a prova real da R222, abaixo — **fora do CI** |
| Originar e controlar SIP (`telephony_esl.rs`) | `cargo test --release --test telephony_freeswitch` + `node web/e2e/telefonia-freeswitch.mjs` contra um FreeSWITCH real — **fora do CI** |
| Os `*.lua` do FreeSWITCH | `bash scripts/check-lua-sintaxe.sh` (R223 — só sintaxe, com o `luac5.2`) |
| Os `*.xml` e `*.xml.inc` do FreeSWITCH | `bash scripts/check-fs-xml.sh` (R226 — bem formado, sem directivas `X-PRE-PROCESS` em comentários, sem `$${AMBIENTE}`); o comportamento é do `scripts/softphone-prova.sh srtp-real`, fora do CI |
| A imagem (`voice/freeswitch/image/`) | `make freeswitch-image` — build + prova de fumo; depois a R222 com `FS_IMAGE` |
| O contrato com o IVR | não há portão automático: ver o aviso do Lua, abaixo |
| Qualquer mudança | `make fitness` |

### As provas reais, e como as correr

**A ponte (R222):**

```bash
make freeswitch-image              # a imagem de voice/freeswitch/image/ (R223)
bash scripts/fs-canais.sh up        # FS_IMAGE=<outra> para correr contra a publicada
FS_ESL_ADDR=127.0.0.1:8221 FS_ESL_PASSWORD=$(cat .fs-canais/esl-password.txt) \
  FS_CANAIS_GW=dlx-0c0a1500-0000-4000-8000-00000000d0d0 \
  FS_CANAIS_RECORDINGS=$PWD/.fs-canais/recordings \
  cargo test --release --lib ponte_com_freeswitch_real -- --nocapture --test-threads=1
bash scripts/fs-canais.sh down
```

**A telefonia (ADR-0009):** o comando está em
[`voice/freeswitch/telefonia-prova/README.md`](../../../voice/freeswitch/telefonia-prova/README.md)
e no cabeçalho de `server/tests/telephony_freeswitch.rs` (`FS_ESL_ADDR`, `FS_ESL_PASSWORD`,
`FS_GW_DOWN`, `FS_GW_UP`; o servidor com `TELEPHONY_ESL_ADDR`/`TELEPHONY_ESL_PASSWORD` e
`VOICE_INTERNAL_SECRET`). **Esse README ainda manda usar a imagem antiga**
`delonix-dev/freeswitch:1.11.3` (`:4`, `:25`), que não tem `mod_lua` nem `mod_curl` e não
está no repo — a imagem do repo é `delonix-meet/freeswitch:1.11.3` (`Makefile:349`). A
prova contra a imagem nova **não está registada**.

**Sem as variáveis, estes testes dizem «NÃO CORREU» e passam** — não deixam o CI vermelho,
e também não provam lá nada ([`e2e-fora-do-ci.txt`](../../../scripts/e2e-fora-do-ci.txt),
linhas 30 e 31). O `FS_BASE_CONF` por omissão (`../../freeswitch-build/conf`,
`scripts/fs-canais.sh:9`) só está certo a partir de um worktree em
`.worktrees/<repo>/<tarefa>`.

Números da R222 para teres com que comparar. Corrida de 2026-09-30: `originate`→atendida
328 ms · `200 OK` do UA 330 ms · 1 kHz do telefone na sala 0,1548 · 440 Hz da sala na
gravação do telefone 0,2495 · o próprio tom do telefone nesse canal 0,0001. Repetida
contra a imagem do repo (R223): 0,1547 · 0,2496 · 0,0001, e `originate`→atendida 564 ms
com o host a carga ~19.

**Um detalhe que já enganou o teste:** o FreeSWITCH só fecha o campo de tamanho do chunk
`data` do WAV quando a chamada termina. Ler antes disso dá um ficheiro de 0,0 s com
160 KiB de áudio dentro, e a prova conclui que a sala não chegou ao telefone quando
chegou.

## O aviso que este domínio tem de carregar

**O `dialin_ivr.lua` está no caminho do cliente, e o portão só lhe vê a sintaxe.**
`scripts/check-lua-sintaxe.sh` (R223) compila-o com o `luac5.2` no `make fitness` e no CI;
o **comportamento** do IVR — PIN, `room_bridge`, recuo para a conferência local — continua
sem portão automático, e **nunca correu de ponta a ponta na imagem do repo** (R223, «por
medir»). Se mexeres no fluxo, di-lo no relatório em vez de o dar por verificado.

**A imagem** vive em `voice/freeswitch/image/` (três fontes fixadas por commit, base por
digest, `mod_lua` e `mod_curl`) e publica-se a partir da `main`
(`.github/workflows/freeswitch-image.yml`), com tag imutável `1.11.3-<sha8>`. Duas pontas
soltas, medidas a 2026-10-03:

- a configuração segura que o `fs-canais.sh` e a prova da telefonia montam por cima
  **ainda não está no repo** (`.worktrees/freeswitch-build/conf/`);
- o `voice/docker-compose.voice.yml:26` continua em `safarov/freeswitch:latest`, e não
  na imagem do repo.

## O que NÃO está na `main`, e porquê

Medido por `grep` em `server/src` e `server/migrations` a 2026-10-03 — os símbolos não
existem:

| Em falta | Porquê ficou de fora |
|---|---|
| `DialOutUpdated`, `SessionCost` (mensagens do WebSocket) e os tipos `DialOutView`/`SessionCostView` | descrevem chamadas de saída e custo por sessão; nada na `main` os produz |
| A porta do WhatsApp Business | sem consumidor |
| A migração `room_channels` | o censo de canais vive em memória (`signaling::Seat`) |
| Os consumidores «frente D» de cinco adaptadores da telefonia | `#[allow(dead_code)]` em `telephony_service.rs:287,301,630,658,682` — custo antes de convidar, `RoomInvite`, canais na sala, SMS com PIN |

A origem é `origin/delonix-meet-backend/v3-canais` e `…/v3-telecom`. **O
`git diff --stat origin/main...<branch>` já não mede o que falta**: as branches estão a
centenas de commits da `main` e o diff inclui o que já foi portado. Mede por símbolo.

**A regra que decidiu tudo isto:** uma mensagem no protocolo que ninguém produz, ou um
método que ninguém chama, é uma capacidade anunciada sem código por trás. Entra com quem
a usa, e com a prova de que o efeito acontece.

O [`docs/reference/contrato-telefonia.md`](../../../docs/reference/contrato-telefonia.md)
é o contrato interno frente C → frente D. **O cabeçalho dele ainda diz que a frente C não
está portada** (`:6-7`) — está, desde o #136; lê o corpo, desconfia do cabeçalho.

## Ao fechar uma tarefa

Propõe um a três pedidos seguintes, cada um com o alvo, a prova a medir e o que fica de
fora. Por ordem de valor, hoje:

1. «Traz a configuração segura do FreeSWITCH (`.worktrees/freeswitch-build/conf/`) para o
   repo, com o `mod_curl` carregado, troca o `safarov/freeswitch:latest` do
   `voice/docker-compose.voice.yml` pela imagem de `voice/freeswitch/image/`, e põe o
   `telefonia-prova/README.md` a usar essa imagem. Prova: a R222 e o
   `telefonia-freeswitch.mjs` a correr só a partir do repo, sem nada fora dele. Fora: o
   PBX de cliente.»
2. «Um portão de comportamento para o `dialin_ivr.lua`: PIN certo → `bridge` para o
   `room_bridge`; ponte em baixo → conferência local. Prova: contra a imagem do repo, com
   controlo negativo (partir o recuo e ver falhar). Fora: a qualidade da media, que é a
   R222.»
3. «Liga um consumidor «frente D» de cada vez — começa pelo custo antes de convidar
   (`telephony_service.rs:287`) — e tira o `#[allow(dead_code)]` com ele (revisores
   `delonix-meet-api` e `delonix-meet-security`). Prova: `tests/telephony.rs` contra
   Postgres real e o efeito visível na sala. Fora: o WhatsApp Business.»
