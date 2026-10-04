# ADR-0016 — A central de uma organização autentica-se no bordo com a conta SIP dela

**Estado:** Proposto · **Data:** 2026-10-04 ·
**Contexto:** o I4b do `kind: PbxService` (delonix-paas, ADR 0063) — o Meet tem de
saber de que organização é a central (PBX) que lhe liga, sem um passo manual do
operador ·
**Assenta em:** [ADR-0009](0009-telefonia-troncos-encaminhamento-e-custo.md) §5 (a
conta SIP da organização) e [ADR-0010](0010-ponte-telefone-sala.md) (a ponte
telefone↔sala).

## Contexto

Medido na `develop` a 2026-10-04, antes de desenhar:

1. **O bordo só filtrava por IP.** O Kamailio aceitava `INVITE` de quem estivesse
   no `address_file` (`allow_source_address("1")`) e recusava os outros com `403`.
   O ficheiro é mantido à mão pelo operador e não vai para o git. Ligar a central
   de um inquilino exigia, portanto, um passo manual do operador — um bloqueio.
2. **O bordo não sabia de quem era a chamada.** Entrada a chamada, a sala
   procurava-se por `(número marcado, PIN)`. A central de uma organização que
   soubesse o PIN de uma sala de outra entrava nela.
3. **O IP não identifica um inquilino.** As centrais que a plataforma entrega
   (`kind: PbxService`) correm numa célula e saem para fora pelo endereço da
   célula: duas centrais de dois inquilinos chegam ao bordo com o MESMO IP.
4. **A conta SIP da organização já existia, e nada se autenticava com ela.** O
   «Registo SIP» (ADR-0009 §5, `telephony_sip_settings`) guarda por organização um
   domínio SIP **único na base**, um utilizador e uma password cifrada em repouso,
   com «ver credenciais» sob reautenticação e auditoria (R214). O único consumidor
   era `telephony_fs_xml.rs`, que lê o domínio do pedido **sem o verificar**.
5. **A imagem do Kamailio traz o que é preciso** (`auth`, `http_client`, `htable`),
   e o `kamailio.cfg` diz, no topo, que dar-lhe uma base de dados é decisão para
   um ADR.

## Decisão

**Quem não é um tronco contratado só entra como central de uma organização: por
TLS, e autenticada por digest com a conta SIP dessa organização. Quem autentica é
o bordo; quem diz ao resto do sistema de quem é a chamada é o bordo, e só ele.**

1. **O bordo autentica.** Uma origem fora da allowlist que chegue por TLS é
   desafiada (`407`), com o domínio que apresenta no `From` como realm. Com as
   credenciais, o Kamailio pede ao servidor o HA1 de `(domínio, utilizador)` —
   `POST /internal/v1/telephony/edge/sip-account`, no listener interno, com o
   segredo da media — e verifica ele próprio a resposta
   (`pv_proxy_authenticate`). **O Kamailio continua sem base de dados.**
2. **Só o bordo fala em nome da central.** Autenticada, a chamada segue com
   `X-Delonix-Central: <domínio>`. O bordo tira esse cabeçalho a tudo o que vem de
   fora, e o IVR só acredita nele se a chamada veio de um endereço do bordo
   (`DELONIX_EDGE_CIDRS`, lista `delonix_bordo` do FreeSWITCH). Sem essa lista,
   nenhuma chamada entra como central: fecha por omissão.
3. **A sala procura-se na organização da central.** O IVR, ao ver o cabeçalho,
   valida o PIN em `POST /internal/v1/voice/ivr/validate-central`: a organização
   sai do domínio autenticado, nunca do pedido, e a sala é a do PIN **dentro
   dela** — a mesma regra do ramal (R273), pelo mesmo código. O número marcado
   não conta. Não há CDR de dial-in: não é uma chamada PSTN.
4. **Daí em diante é a ponte do ADR-0010**, sem uma linha nova: `room_bridge`, a
   perna SIP para o UA do servidor, e o recuo para a conferência local.
5. **Sem TLS não há central.** As chaves do SRTP (SDES) viajam no SDP; por UDP ou
   TCP em claro, uma origem fora da allowlist leva `403` sem desafio.
6. **Um travão por origem no bordo.** À décima falha de autenticação em cinco
   minutos, a origem leva `403` sem mais perguntas ao servidor. O travão de PINs
   do servidor conta por central.
7. **Desligado por omissão.** Sem `DELONIX_CONTROL_URL` no Kamailio, nada disto
   se carrega e o bordo comporta-se como antes.

**O que o inquilino faz, e o operador não:** grava o «Registo SIP» da organização
(domínio, utilizador, password) e põe os mesmos três valores na sua central. Não
há IP a declarar nem ficheiro a editar.

## Alternativas rejeitadas

- **Identificar pelo IP de origem** (uma etiqueta por linha da allowlist). Não
  distingue duas centrais atrás do mesmo endereço (ponto 3 do contexto), e mantém
  o passo manual, ou obriga um inquilino a declarar IPs — que pode declarar o de
  outro.
- **Uma base de dados no Kamailio** (`auth_db`, `permissions` com `db_url`). Liga
  o bordo à base do control plane e ao seu esquema; o próprio `kamailio.cfg`
  avisa contra isso. O pedido HTTP dá o mesmo resultado sem essa dependência.
- **Autenticar no FreeSWITCH**, num perfil novo com `auth-calls`. Mantinha o
  bordo sem saber quem deixa passar, e abria mais um perfil público num FreeSWITCH
  cujo perfil dos ramais não tem TLS.
- **A central como «ramal da empresa»** (R276) no perfil dos ramais. Não precisava
  de código novo, mas esse perfil não tem TLS — as chaves do SRTP viajavam em
  claro — e a skill `delonix-meet-voip` proíbe-o de ser porta de troncos.
- **Um segredo num cabeçalho** (portador). Mais simples que o digest, mas viaja
  em claro entre o bordo e o FreeSWITCH e não é o que a interface de um PBX
  oferece: utilizador e password de um tronco, qualquer central sabe configurar.

## O que se ganha, e o que se perde

**Ganha-se:** a central de uma organização entra sem intervenção do operador; a
organização de quem liga passa a ser um facto autenticado; uma central não entra
em salas de outra organização; a conta SIP guardada passa a ter um consumidor.

**Perde-se, e fica escrito:**

- **A porta TLS do bordo passa a responder a desconhecidos** com um desafio, em
  vez de `403`. A regra de casa «nunca só credenciais num porto aberto» foi
  escrita para troncos de operadora, onde uma credencial roubada dá chamadas
  pagas por nós. Aqui dá o direito de adivinhar PINs de seis dígitos das salas
  dessa organização, com dois travões. Restringir uma conta a redes de origem é
  o passo seguinte, e não está feito.
- **Uma central por organização.** A conta SIP é uma por organização
  (`telephony_sip_settings`, chave `org_id`).
- **O control plane entra no caminho do `INVITE`.** Se não responder em 2 s, a
  central leva `503`. O dial-in de um tronco da allowlist não depende dele no
  bordo.
- **MD5.** O digest SIP é MD5 (RFC 2617); a password é a que o administrador
  escolheu. Só viaja dentro de TLS.
- **O HA1 atravessa a rede interna** entre o servidor e o bordo, em HTTP, como o
  do directório dos ramais já atravessa entre o servidor e o FreeSWITCH.

## Prova

Medida a 2026-10-04, numa réplica isolada do bordo (`voice/pbx-tronco-prova/`: Kamailio
5.8.6, o FreeSWITCH 1.11.3 da imagem do repo com o arranque de `voice/cluster/`, o
servidor desta árvore). As duas organizações semeiam-se **pela API**, como um inquilino
faria: conta, sala, número, sala de voz e «Registo SIP».

**Com dois telefones a fazer de central** (`bash scripts/pbx-tronco-prova.sh central`),
de endereços fora da allowlist:

| O que a decisão promete | O que se mediu |
|---|---|
| a central entra autenticada, por TLS | duas chamadas estabelecidas, com SRTP |
| entra na sala da SUA organização, pela ponte | as duas foram para `room-<sala de A>` (ADR-0010); nenhuma caiu na conferência local; o servidor abriu duas pernas no SFU |
| ouve e é ouvida | cada telefone ouviu o tom do outro (0,2499 e 0,2500) e não o seu (0,0008 e 0,0012) |
| sem TLS não há central | UDP, com a conta certa: `403` |
| sem credenciais não entra | `407`, e a chamada não se estabelece |
| conta errada não entra | password errada, e a conta de B no domínio de A: recusadas, falha contada no bordo |
| a central de A não entra numa sala de B | com o PIN da sala de B, o IVR não a deixa entrar em sala nenhuma; **o mesmo PIN, pela central de B, abre a sala** |
| só o bordo fala em nome da central | um `X-Delonix-Central` forjado pela porta do bordo chega ao FreeSWITCH sem o cabeçalho; direito ao FreeSWITCH, o IVR rejeita (`603`) |
| o travão por origem | à décima falha, `403` mesmo com a password certa; outra origem continua a entrar |
| o PIN não fica no log | zero ocorrências no `freeswitch.log` |

**Com a central real** — a appliance FreePBX 17.0.33 (Asterisk 22.11.0) em QEMU, com a
configuração que o `kind: PbxService` do delonix-paas gera (`meet_trunk.account`) e **a
allowlist do bordo vazia** (`freepbx --seed … --central`): o guião do Kind acaba em
`exit=0`, o tronco fica `Avail` por TLS com o certificado do bordo verificado, as duas
chamadas entram autenticadas, a do PIN certo entra na sala da organização pela ponte do
SFU e a do PIN errado é recusada, com SRTP nas duas e na perna para a ponte.

A regra do servidor tem portão no CI (`server/tests/central_entra_na_sala.rs`, seis
casos contra Postgres real), e a ordem das três garantias do cabeçalho tem um portão
estático (`scripts/check-bordo-central.sh`, com quatro controlos negativos corridos).

**Não medido:** um browser na sala — quem ouve a central é outro telefone, pela mesma
ponte, e a media entre a ponte e um participante WebRTC é da R221/R222; o chart Helm
(`voice.centrais`), que só foi lido; (o `compose.yaml` e o cluster local mediram-se
depois, com o PBX de laboratório como central — R280); o bordo atrás de NAT; uma central que não seja FreePBX.

## O que este ADR não decide

- Chamadas **do Meet para a central** (o contexto `from-meet` da central desliga
  tudo, de propósito).
- A entrada em claro de um tronco `srtp=off` (skill `delonix-meet-voip`).
- O caminho `delonix-outbound` (`telephony_fs_xml.rs`), que continua a decidir a
  organização pelo domínio do pedido sem o autenticar. Só existe no perfil `pbx`
  da prova da telefonia, que nenhum ambiente carrega; **antes de o carregar, tem
  de passar a ler o que o bordo autenticou.**
