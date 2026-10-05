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
**Catálogo:** R210–R214 (telefonia), R221–R225 (ponte, imagem, censo, palco), R273
(ramal entra na sala), R276 (PIN do ramal) e R279 (o IVR identifica quem liga) em
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
- **Rotas:** dezoito registos — quinze em `lib.rs:845-902`, sob
  `/api/orgs/{org_id}/telephony/` (`trunks`, `trunks/{id}`, `trunks/{id}/prices`,
  `trunk-order`, `exchange-rates`, `dial-plan`, `dial-plan/test`, `sip-settings`,
  `sip-settings/reveal-credentials`, `sip-registration`, `sip-registration/restart`,
  `test-calls`, `test-calls/{id}`, `call-records`, `usage`) e três no listener interno,
  em `internal_routes()` (`/internal/v1/telephony/call-records`, onde o `mod_json_cdr` entrega os CDR,
  `/internal/v1/telephony/freeswitch-config`, que serve o `mod_xml_curl`, e
  `/internal/v1/telephony/edge/sip-account`, onde o bordo pede o HA1 da conta SIP de uma
  organização — ADR-0016).
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

### O ramal entra na sala (R273) — a regra está medida, a chamada não

Um ramal marca o **número de acesso às reuniões** (`VOICE_MEETING_ACCESS_NUMBER`, `8000`
por omissão) e entra pela MESMA ponte. Não há ponte nem IVR novos:

1. o `ramais_dial.lua` pergunta o número em `/internal/v1/voice/ivr/resolve-extension`; a resposta
   `{"meeting_access": true}` manda-o chamar `dialin_ivr.lua ramal`;
2. o IVR, em modo `ramal`, lê `sip_auth_username`/`sip_auth_realm` (o que o digest
   autenticou — **nunca o `From`**) e valida o PIN em
   `/internal/v1/voice/ivr/validate-extension` (`voice::validate_pin_for_extension`);
3. a sala procura-se **na organização do ramal** — é essa a fronteira, porque o PIN só é
   único por DID. Ramal inactivo, membro arquivado, domínio de outra org, PIN de outra org
   ou PIN em duas salas: o mesmo `404`;
4. daí em diante é o `room_bridge` e o recuo do dial-in, linha por linha o mesmo código.

O número é **só do servidor** (o dialplan não o tem escrito), nenhum ramal o pode ter
(`409 ramais.extension_reserved`) e as leituras dos ramais trazem-no em
`meeting_access_number`. **Sem CDR** neste caminho. Quem entra leva o nome da pessoa do
ramal (ou a etiqueta do ramal da empresa) para o censo — ver «O IVR identifica quem
liga», abaixo.

**Por medir, e não o dês por feito:** uma chamada real. O modo `ramal` do Lua nunca
correu; `sip_auth_*` num INVITE do perfil `internal` e o `session:execute("lua", …)` de
um script para o outro são pressupostos por confirmar contra um FreeSWITCH real.

### A central de uma organização entra na sala (ADR-0016, R280)

A central (PBX) de uma organização liga-se ao bordo por TLS e autentica-se com a **conta
SIP dessa organização** — o «Registo SIP» do ADR-0009 §5, que até aqui se guardava e não
tinha consumidor. O que acontece antes de a chamada entrar (o desafio, o HA1, o
cabeçalho) é da `delonix-meet-voip`. Aqui:

1. o `dialin_ivr.lua` vê `X-Delonix-Central: <domínio>` — e só o aceita se a chamada veio
   de um endereço do bordo (lista `delonix_bordo`); de outro lado, desliga;
2. valida o PIN em `/internal/v1/voice/ivr/validate-central`
   (`voice::validate_pin_for_central`): a organização sai do domínio autenticado, e a sala
   é a do PIN **dentro dela** — o mesmo `room_by_pin_in_org` do ramal;
3. daí em diante é o `room_bridge` e o recuo do dial-in, o mesmo código.

**É um terceiro modo do mesmo IVR**, que não se pede: reconhece-se pelo cabeçalho. Sem
CDR (não é PSTN), o número marcado não conta, e quem entra é «Telefone» anónimo no censo.
Uma conta SIP sem password não autentica nem abre salas.

### O PIN do ramal, os ramais da empresa e a numeração automática (R276)

Número do ramal, password SIP e PIN são **três coisas separadas** (decisão de 2026-10-04,
item 3.8 do plano). O PIN (6 dígitos, só o hash, mostrado uma vez) vive em
`server/src/extension_pin.rs`; as recusas e o sorteio em
`crates/delonix-meet-domain/src/telephony/extension_pin.rs`. O PIN de um ramal de pessoa é
dela (`/my-extension`); o administrador só o limpa. Um ramal sem pessoa (`member_id` nulo,
etiqueta obrigatória) é da empresa, e o PIN dele é do administrador. Os números
automáticos saem do intervalo da organização (`extension-range`, por omissão 1000–1999)
com `POST …/extensions/assign-missing`.

### O IVR identifica quem liga (R279) — o ramal registado está medido numa chamada, o PIN de fora não

**A verificação** (`extension_pin::verify_from_call`, rota
`/internal/v1/voice/ivr/verify-extension-pin`) tem agora consumidor, e foi endurecida
antes de o ter:

- **travão por ORIGEM** — o pedido exige `origin` (`caller_number`, `network_ip`); a
  origem é cobrada ANTES de se verificar e a falha devolvida num acerto; trava à terceira
  falha em 15 minutos (primeiro bloqueio: 20, mais que a janela do ramal), e travada nem
  o ramal é lido (`origin_locked`). O estado vive em
  Postgres (`voice_pin_origins`): o servidor tem várias réplicas e o `RateLimiter` é por
  processo. Como trava à terceira e o ramal à quinta, **uma origem não bloqueia o ramal
  de ninguém**;
- **janela e duração crescente** — falhas contadas numa janela estrita de 15 minutos; bloqueio de 15, 30,
  60 min… até 24 h, no ramal e na origem (`telephony::extension_pin::Throttle`);
- a origem vai no alvo da auditoria (`ramal.pin_falhado`, `ramal.pin_bloqueado`,
  `ramal.origem_travada`); o actor é o de sistema.

**Quem entra com nome:**

1. **ramal registado** (`dialin_ivr.lua ramal`) — sem PIN pessoal: o aparelho já está
   autenticado. `validate_pin_for_extension` resolve a pessoa (ou a etiqueta);
2. **de fora** — depois do PIN da sala, o IVR pede ramal e PIN pessoal e chama a
   verificação com a origem e o `voice_room_id` (o domínio vem do `validate`,
   `org_sip_domain`). **Uma só frase de recusa** para todas as razões, duas tentativas, e
   quem falha entra na mesma, anónimo;
3. nos dois casos o servidor emite um **bilhete** (`voice_caller.rs`: opaco, uso único,
   45 segundos, uma só sala, só o hash guardado; invalidado se a ponte recusar a perna) que vai nas `channel_vars` como
   `sip_h_X-Delonix-Caller-Ticket`. O UA da ponte leva-o no `BridgeEvent::Started` e
   `voice::seat_phone_caller` troca-o pelo nome no censo. **O Lua não é a fonte do nome** (a resposta da
   verificação traz o nome até ao FreeSWITCH, mas o Lua não o usa nem o regista).
4. os cabeçalhos `X-Delonix-*` só nascem dentro: o Kamailio tira-os no bordo do tronco e
   o Lua tira-os da perna que recebe antes do `bridge`.

**Medido no laboratório (compose, 2026-10-04):** o caminho 1 correu em duas chamadas
reais de Linphone para a mesma sala — o servidor emitiu o bilhete de quem liga
(`voice_caller_tickets`) para o ramal 101 e para o ramal 1000, cada um com o nome da
pessoa, e cada um foi gasto 2 segundos depois; o dono viu o telefone no censo com o
crachá e o nome. **O áudio nos dois sentidos é relato do dono, não medição.** Os registos
dos contentores perderam-se num `compose-down`: a prova que fica é a das tabelas.

**Por medir, e não o dês por feito:** o caminho 2 — ramal e PIN pessoal pedidos a quem
liga de fora — nunca correu numa chamada, nem a recusa, nem o travão por origem. Nesse
caminho o Lua só tem a sintaxe verificada: `session:read(0, …)` e as variáveis de origem
na perna são pressupostos. **Os direitos de anfitrião por telefone não existem:**
quem entra por telefone já passa à frente da sala de espera (identificado ou não), a
sala não tem um estado «à espera do anfitrião», e dar `is_host` a uma perna sem cliente
ficou por desenhar. O número de quem liga pode ser forjado: contra quem o rode a cada
chamada só o contador do ramal trava, e quatro falhas por janela nunca bloqueiam (384
palpites por dia por ramal). O travão por origem pode negar a IDENTIFICAÇÃO a terceiros
(número forjado, PBX com um só número de tronco, chamadas sem número). A lista completa está na R279.

### O Linphone por QR e o ramal automático a quem entra (R278)

**A password SIP não se digita.** `server/src/extension_provisioning.rs` emite um bilhete de
uso único (256 bits, só o SHA-256 em `voice_extension_provisioning_tickets`, 10 minutos, um
vivo por ramal) — a pessoa para o seu ramal (`POST …/my-extension/provisioning-ticket`), o
administrador para qualquer um da organização (`POST …/extensions/{id}/provisioning-ticket`).
O URL sai da primeira origem de `CORS_ORIGINS`, só se for `https` e pública; sem ela ou sem
`VOICE_RAMAIS_PUBLIC_HOST` a emissão é `422`. O resgate é a rota PÚBLICA
`GET /api/public/extension-provisioning/{token}`: gasta o bilhete num só `UPDATE`, grava uma
password SIP **nova** (`ramais::SipSecret`, o mesmo da regeneração — o Argon2 calcula-se antes
da transacção) e devolve o `lpconfig` do Linphone (regras e XML em
`crates/delonix-meet-domain/src/telephony/extension_provisioning.rs`). Toda a recusa é o
mesmo `404 ramais.provisioning_invalid`. O bilhete de um ramal inactivo gasta-se na tentativa,
e regenerar a password apaga os bilhetes do ramal. O que protege a rota são os 256 bits e o
uso único — o limite por IP contorna-se (R278 §Aberto). O token não entra na auditoria (que
guarda o IP de quem resgatou) nem no span HTTP; nos nginx do repositório a rota tem
`access_log off`; no cluster, onde o `/api` vai do ingress directo ao servidor, a rota tem um
`Ingress` próprio (`delonix-provisioning`, `enable-access-log: "false"`) no chart e em
`deploy/k8s/04-ingress.yaml`, guardado pelos dois portões de render e medido no cluster local (0 linhas no registo do
controlador; só vale para o ingress-nginx).
**Ler o QR troca a password: o aparelho antigo deixa de registar.**

**Quem entra recebe ramal** se a organização tiver `auto_assign_on_join` ligado (desligado por
omissão; vive na linha e na rota do `extension-range`; num `PUT`, ausente = manter). O ÚNICO
ponto é `ramais::assign_on_join`, chamado depois do commit; nunca devolve erro — intervalo
esgotado fica na auditoria e o membro entra sem ramal. Recebe quem ocupa lugar
(`org::seat_holder_username`): convidados externos e o utilizador de serviço não.

**Só o chama um caminho em que houve um acto de um administrador ou de um IdP:** juntar um
colaborador, convite aceite, reactivação, SSO OIDC, Odoo. **O auto-registo (`auth::register`)
e o convidado de reunião da API v1 NÃO o chamam, de propósito:** o registo não verifica o
email, e numa instalação `single` + `open` um desconhecido ficava com uma conta SIP. Um
caminho novo que insira em `org_members` faz esta pergunta antes de chamar a função — e
chama-a, não copia a regra.

**Medido no laboratório (compose, 2026-10-04):** o QR foi resgatado três vezes
(`ramal.provisionado` na auditoria: ramal 101 duas vezes, ramal 1000 uma), e o Linphone
do ramal 1000 ligou para a sala 95 segundos depois do seu resgate, identificado como
ramal registado — ou seja, o `lpconfig` serviu a um Linphone real, um só resgate bastou, e
o aparelho autenticou-se com um realm que não é o host do proxy. O dono confirma que
configurou pelo QR, sem digitar a password. **Continua por medir:** a versão do Linphone e do Android
não ficaram registadas; se `media_encryption_mandatory` é respeitado; o diálogo a 375 px.
**Aberto:** quem
sai da organização continua a registar com a password antiga (arquivar não desactiva o
ramal). A lista completa está na R278.

**Como fazer essa prova no laboratório (2026-10-04).** O QR é um URL https que o TELEMÓVEL
abre, por isso só serve com `make compose-up LAN_IP=<ip desta máquina>` (`scripts/compose-lan.sh`):
a borda fica também em `<ip>:8443`, com um certificado assinado por uma raiz de laboratório
(`deploy/compose/generated/lan-tls/`, fora do git) que cobre o nome e o IP, e o `CORS_ORIGINS`
leva o IP em primeiro lugar — é da primeira origem que sai o URL do QR, e uma origem `.local`
é recusada (mDNS não resolve no telemóvel). No telemóvel: instalar a raiz a partir de
`http://<ip>:8080/lab-ca.crt` como certificado de CA, confirmar que `https://<ip>:8443` abre
sem aviso, e só então ler o QR no Linphone («obter configuração remota»). Medido até aqui: a
raiz descarrega-se, o https pelo IP valida contra ela e o bilhete sai com o IP. **Continua
não medido:** se o Linphone confia numa raiz instalada pelo utilizador ao descarregar a
configuração. A 2026-10-04 houve um resgate vindo de um endereço da rede local (ramal 101),
mas a auditoria não diz que cliente o fez — a câmara ou o browser do telemóvel gastam o
bilhete da mesma maneira — e os dois resgates que se sabe terem servido um Linphone
chegaram pelo túnel público, com um certificado de uma AC pública. Se o browser do telemóvel
abrir e o Linphone recusar o certificado, é isso. O cluster local não serve para esta prova:
não está exposto à rede local.

### Os troncos na configuração que corre (R291) — a saída está ligada, a entrada não

Até 2026-10-05 a telefonia de troncos só existia em `voice/freeswitch/telefonia-prova/`.
Agora o `voice/cluster/freeswitch-entrypoint.sh` liga-a no compose, no cluster e no chart:

- o binding `delonix_telefonia` do `xml_curl.conf.xml` (gateways e o contexto
  `delonix-outbound`), depois dos dois dos ramais;
- os troncos como gateways do perfil `external`, pelo domínio `delonix-trunks` — que
  SUBSTITUI o `all` da vanilla (com os dois a lista lia-se duas vezes);
- o `mod_json_cdr` (`json_cdr.conf.xml`): **só a perna de um tronco deixa registo** — um
  registo leva todas as variáveis do canal, chaves SRTP incluídas. `log-b-leg` desligado,
  `force_process_cdr=true` na perna do tronco (`telephony_fs_xml.rs`), `process_cdr=false`
  nos contextos `public` e `delonix_ramais`; o que sobra o servidor aceita e ignora (`204`);
- **texto de inquilino nunca chega ao FreeSWITCH com um `$`** (`esc`, `xml_escape` →
  `&#36;`): o FreeSWITCH pré-processa a resposta do `mod_xml_curl` e `$${nome}` lia uma
  variável global — o segredo de voz. E nunca em `data` de acção nem em dial string;
- no máximo 20 troncos por organização (`MAX_TRUNKS_PER_ORG`): vão todos num só documento;
- um ciclo de `sofia profile external rescan` (`DELONIX_TRUNKS_RESCAN_SECS`, 60 s): um
  tronco novo regista-se sozinho, e um arranque com o servidor em baixo recupera.

**Não o dês por feito além disto:** nenhum perfil leva um ramal ou uma central ao
`delonix-outbound` — um cliente ainda não faz uma chamada para a rede pública (T2); um
tronco alterado ou apagado só se actualiza reiniciando o FreeSWITCH, porque o `killgw` vai
pelo ESL, fechado em loopback (T11) — e por isso a consola não mostra o estado do registo
nem faz a «chamada de teste»; nenhuma operadora de verdade, e nenhuma chamada por tronco
num cluster. O host de um tronco só é verificado ao gravar: um nome que depois aponte para
dentro leva o FreeSWITCH a um endereço interno, e não há política de rede que o trave.

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
| Os troncos no arranque do FreeSWITCH (`freeswitch-entrypoint.sh`, `xml_curl.conf.xml`, `json_cdr.conf.xml`, a ingestão de CDR) | `bash scripts/troncos-prova.sh` — **fora do CI** (R291: tronco registado numa operadora de ensaio, chamada com custo e MOS, reinícios, e o controlo do `204`); mais `softphone-prova.sh srtp-real` |
| Troncos, plano de marcação, CDR, custo, credenciais | `cargo test --release --test telephony -- --test-threads=4` contra Postgres real (14 casos; precisa de `DATABASE_URL`) + os unitários do domínio |
| Uma rota `/telephony` | os portões de `delonix-meet-api`, com o caso negativo em `web/e2e/isolamento.mjs` |
| A cadeia toda da ponte | a prova real da R222, abaixo — **fora do CI** |
| Originar e controlar SIP (`telephony_esl.rs`) | `cargo test --release --test telephony_freeswitch` + `node web/e2e/telefonia-freeswitch.mjs` contra um FreeSWITCH real — **fora do CI** |
| O ramal a entrar na sala (`validate_pin_for_extension`, número reservado) | `cargo test --test ramal_entra_na_sala` contra Postgres real (R273 — 7 casos; o isolamento por org tem controlo negativo) |
| A central de uma organização a entrar na sala (`validate_pin_for_central`, `ha1_for_edge`) | `cargo test --release --test central_entra_na_sala` contra Postgres real (ADR-0016 — 6 casos) + `bash scripts/check-bordo-central.sh`; a cadeia toda é `bash scripts/pbx-tronco-prova.sh central`, **fora do CI** |
| O PIN do ramal, os ramais da empresa e a atribuição em massa | `cargo test --release --test ramal_pin` contra Postgres real (R276 — 9 casos, dois de concorrência) + `telephony::extension_pin::tests` |
| O QR de provisionamento do Linphone e o ramal automático a quem entra | `cargo test --release --test ramal_provisionamento --test ramal_ao_entrar` contra Postgres real (R278 — 7 + 5 casos, um de concorrência) + `telephony::extension_provisioning::tests` |
| A verificação do PIN (origem, janela, bloqueio crescente) e quem liga identificado (bilhete, nome no censo) | `cargo test --release --test ramal_pin_origem --test ivr_identifica_quem_liga` contra Postgres real (R279 — 9 + 4 casos; nenhum FreeSWITCH) + `cargo test --lib um_invite_recusado` |
| O `kamailio.cfg` | `kamailio -c -f` na imagem `ghcr.io/kamailio/kamailio:5.8.6-bookworm` (só sintaxe; **não há portão no repo**) |
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

**Os troncos na configuração distribuída (R291):** `bash scripts/troncos-prova.sh`, com
`SERVER_IMAGE` (a do `make image`) ou `SERVER_BIN=<binário da árvore>`. Ergue uma réplica
própria, mede e desmonta; não toca no laboratório.

**A telefonia com o ESL (ADR-0009):** o comando está em
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
medir»). Se mexeres no fluxo, di-lo no relatório em vez de o dar por verificado. O mesmo
ficheiro serve agora **dois modos** (dial-in por DID e `ramal`, R273): uma mudança no
caminho comum — PIN, `bridge`, recuo — mexe nos dois. A identificação por ramal e PIN
(R279) corre só no dial-in e só com a ponte; **nunca correu numa chamada** — o que correu,
a 2026-10-04, foi a do ramal registado, à mão e sem portão.

**A imagem** vive em `voice/freeswitch/image/` (três fontes fixadas por commit, base por
digest, `mod_lua` e `mod_curl`) e publica-se a partir da `main`
(`.github/workflows/freeswitch-image.yml`), com tag imutável `1.11.3-<sha8>`. Uma ponta
solta, medida a 2026-10-03:

- a configuração segura que o `fs-canais.sh` e a prova da telefonia montam por cima
  **ainda não está no repo** (`.worktrees/freeswitch-build/conf/`).

O compose de voz antigo (`voice/docker-compose.voice.yml`, sobre `safarov/freeswitch:latest`)
foi retirado a 2026-10-04: nunca correu. A voz sobe pelo `compose.yaml` e pelo cluster
local, os dois com a imagem do repo e o mesmo arranque
(`voice/cluster/freeswitch-entrypoint.sh`).

## O que NÃO tem consumidor, e porquê

Re-medido por `grep` a 2026-10-04 sobre a `develop` (`974edaae`). A tabela de 2026-10-03
dizia que estes símbolos e a migração não existiam: **existem na `develop`** (entraram com
os ramos antigos), mas continuam sem quem os produza.

| O que existe | O que lhe falta |
|---|---|
| `DialOutUpdated`, `SessionCost` (mensagens do WebSocket) e os tipos `DialOutView`/`SessionCostView` (`signaling.rs`, `conferencing/channels.rs`) | um produtor: só os testes os emitem, e o web não os lê. A única chamada originada de facto é o tom de teste de 3 s (`telephony_calls.rs`) |
| A migração `0086_room_channels.sql` | código que leia ou escreva `room_dial_outs`: zero ocorrências em `server/src` |
| A porta do WhatsApp Business | consumidor |
| Cinco adaptadores da telefonia «frente D» | `#[allow(dead_code)]` em `telephony_service.rs:287,301,630,658,682` — custo antes de convidar, `RoomInvite`, canais na sala, SMS com PIN |

**E a telefonia de troncos não está ligada na configuração distribuída** (medido a
2026-10-04). `voice/freeswitch/autoload_configs/xml_curl.conf.xml` só tem as duas bindings
dos ramais; a binding de `/internal/v1/telephony/freeswitch-config` (gateways e o contexto
de saída) e o `json_cdr.conf.xml` só existem em `voice/freeswitch/telefonia-prova/`, e o
`voice/cluster/freeswitch-entrypoint.sh` não copia nenhum dos dois. No compose, no cluster
e no chart: nenhum gateway é carregado, um ramal não sai para a PSTN, e o único CDR que
chega é o magro do `dialin_ivr.lua`. É o item T1 do
plano de lacunas de 2026-10-04 (`docs/plano-lacunas-2026-10-04.md`).

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
   repo, com o `mod_curl` carregado, e põe o `telefonia-prova/README.md` a usar a
   imagem de `voice/freeswitch/image/`. Prova: a R222 e o
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
