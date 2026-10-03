---
name: delonix-meet-voip
description: >-
  Interligação SIP do Delonix Meet com o que não é nosso — o PBX de um cliente
  (Issabel, FreePBX, Asterisk), o FreeSWITCH como gateway partilhado, e uma operadora
  (tronco SIP, números móveis GSM, eSIM): as boas práticas de SIP/VoIP que um tronco
  tem de cumprir antes de levar tráfego, o que o repo já impõe, e o que é só regra de
  casa ainda sem portão.
when_to_use: >-
  Quando o pedido falar em «Issabel», «FreePBX», «Asterisk», «PBX do cliente»,
  «interligar», «tronco de operadora», «operadora», «GSM», «eSIM», «SIM box»,
  «gateway GSM», «VoLTE», «IMS», «toll fraud», «CLI», «caller ID», «DTMF», «NAT»,
  «SIP ALG», «codec», «registo SIP» ou «interop», ou quando fores configurar ou rever
  um tronco em `voice/kamailio/`, `voice/freeswitch/sip_profiles/` ou em
  `telephony_trunks.rs`. NÃO a uses para a ponte telefone↔sala, o IVR, o CDR ou o
  custo (`delonix-meet-telefonia`), nem para a appliance de PBX em si, que vive no
  `delonix-runtime`.
---

# Interligação SIP — PBX de cliente, FreeSWITCH e operadora

Esta skill tem duas espécies de afirmação, e nunca as mistures num relatório:

- **Medido** — está no repo, com ficheiro e linha, a 2026-10-03.
- **Regra de casa** — boa prática de SIP/VoIP que adoptamos. **Não tem portão**: nada no
  CI a verifica. Aplicá-la é da revisão, e dizê-la «cumprida» exige a prova da secção
  «Portões».

## Fronteira

- **`delonix-meet-telefonia`** — o que acontece DEPOIS de a chamada entrar: IVR, ponte
  telefone↔sala, censo, troncos como dados (ADR-0009), CDR, custo, R210–R214 e
  R221–R225. Aqui está o que acontece ANTES: como o outro lado se liga a nós e em que
  condições se aceita o tráfego dele.
- **Revisor `delonix-meet-security`** — a allowlist, as credenciais de tronco, o material
  SRTP. Chama-o em qualquer diff que abra uma origem nova.
- **Revisor `delonix-meet-devops`** — `voice/docker-compose.voice.yml`, a imagem do
  FreeSWITCH, portas e NAT no deploy.
- **Fora deste repo** — a appliance de PBX de cliente é do `delonix-runtime` (esteira de
  appliances) e o seu Kind é do `delonix-paas`. Esta skill só cobre a ligação SIP entre
  esse PBX e o Meet.

## A pergunta que fazes a tudo

**Se este par for comprometido amanhã, quanto nos custa e a quem liga em nosso nome?**
Um tronco é uma linha de crédito aberta a quem tiver as credenciais. Toda a regra abaixo
existe para limitar essa conta antes de alguém a apresentar.

## Os três interlocutores

### 1. O PBX do cliente (Issabel, FreePBX, Asterisk)

Trata-o como **um par não confiado que traz tráfego de um inquilino** — nunca como parte
da plataforma.

- **Issabel não é a appliance que entregamos.** Decisão de 2026-09-30, medida no
  repositório deles: o Asterisk mais novo era o 18.19.0 (Setembro de 2023), e o 18 perdeu
  as correcções de segurança em Outubro de 2025. A appliance é FreePBX 17. **Um cliente
  que já tem Issabel interliga-se na mesma** — por tronco, com as regras desta secção —
  mas não o instalamos, não o gerimos e não lhe damos acesso a nada interno.
- **Liga-se por tronco SIP ao nosso bordo (Kamailio), não ao FreeSWITCH directamente.**
  O perfil `internal` do FreeSWITCH é dos ramais e autentica por Digest
  (`voice/freeswitch/sip_profiles/internal.xml:54`); não é porta de troncos.
- **Regra de casa — do lado do Asterisk:** `chan_pjsip`, não `chan_sip` (removido no
  Asterisk 21; num Issabel com Asterisk 18 ainda existe e é o que a interface antiga
  cria — pede o PJSIP). Um endpoint por tronco, com `identify` por IP **e** autenticação;
  `allowguest=no`/sem endpoint anónimo; contexto próprio para o que vem de nós, sem
  acesso a rotas de saída do cliente.
- **Regra de casa — o que NÃO se aceita de um PBX de cliente:** tráfego para fora do
  plano de marcação do inquilino, identidade de chamador que ele não possui, e qualquer
  pedido que não seja `INVITE`/`OPTIONS` e os do diálogo. Sem `REFER` cego para destinos
  externos: é a porta clássica do desvio de chamadas pago por nós.

### 2. O FreeSWITCH (gateway partilhado do Meet)

É **um só, partilhado**, não um por inquilino (decisão de 2026-09-30). O isolamento é por
contexto de dialplan e por domínio SIP, não por processo.

- **Medido:** o bordo é o Kamailio — `sanity`, `dispatcher` com sonda `OPTIONS` a cada
  15 s e `permissions` por lista de endereços (`voice/kamailio/kamailio.cfg:44-62`); só
  aceita `INVITE` e `OPTIONS` (`:80`); escuta TLS em 5061 (`:39`). Não tem `usrloc`,
  registrar nem `auth_db` (`:14-15`) — **não regista ninguém**, só encaminha.
- **Medido:** DTMF por RFC 2833/4733 com payload 101 (`internal.xml:36`);
  `accept-blind-reg` e `accept-blind-auth` a `false`.
- **Medido (2026-10-03, FreeSWITCH 1.11.3, R226) — quem recusa uma chamada em claro à
  entrada é só a variável GLOBAL `rtp_secure_media=mandatory`**: com ela, um `INVITE` sem
  `a=crypto` leva `488`. Nenhum parâmetro de perfil o faz: `rtp-secure-media` não existe
  no sofia nem no mod_conference, e `require-secure-rtp` só liga uma flag que nada lê —
  com qualquer dos dois e sem a global, a chamada em claro foi aceite. Um
  `set rtp_secure_media=mandatory` no dialplan antes do `answer` também não a recusa.
  A global é posta por `sip_profiles/internal.xml` (uma directiva de pré-processamento no
  topo do ficheiro) e por `vars.xml.inc`. Confirma-o com o controlo negativo
  (`scripts/softphone-prova.sh srtp-real`), não com a leitura do XML.
- **Medido (R226) — o `vars.xml.inc` não é incluído por nada.** O compose monta-o
  (`voice/docker-compose.voice.yml:47`) e nenhum `vars.xml` o inclui: tal como o repo a
  monta, a configuração deixa o perfil dos ramais no porto **5060** e o URL e o segredo
  do control plane vazios. Incluído pelo `vars.xml`, funciona — o `srtp-real` mede-o.
- **Duas armadilhas do pré-processador do FreeSWITCH (R226):** uma directiva
  `X-PRE-PROCESS` é executada **mesmo dentro de um comentário** — nunca a escrevas por
  extenso num; e o ambiente lê-se com `cmd="env-set"` e `$NOME`, não com `cmd="set"`.
- **Medido:** o perfil dos ramais **não tem ACL de rede** — o comentário em
  `internal.xml` di-lo, e não existe `acl.conf.xml` no repo. Não o abras a troncos.
- A imagem, o que o FreeSWITCH de stock não faz e as provas contra um FreeSWITCH real:
  `delonix-meet-telefonia`.

### 3. A operadora (tronco SIP, GSM, eSIM)

- **Medido — o que um tronco é no nosso modelo**
  (`server/crates/delonix-meet-domain/src/telephony/trunk.rs`): `scope`
  `national | international`; `transport` `udp | tcp | tls`; `srtp`
  `mandatory | optional | off`, e **SRTP diferente de `off` exige TLS** (`:111`, porque
  as chaves SDES vão no SDP); prefixos, limite de canais e ordem de recurso. O host
  passa pelo guarda de SSRF (R213) e a password fica cifrada em repouso (R214).
- **Medido:** a allowlist de IP do tronco é um ficheiro que **não vai para o git**
  (`voice/kamailio/ao_trunk.txt.example` é o molde).
- **GSM não é um protocolo que nós falemos.** Uma chamada para ou de um telemóvel chega
  até nós como SIP, pela interligação da operadora; o que é GSM, VoLTE ou AMR fica do
  lado dela. Do nosso lado é G.711 — a lei (A ou µ) é a da oferta SIP
  (`phone_bridge/sip.rs:719`) — e a operadora transcodifica.
- **eSIM não é um tronco.** É o perfil de assinante de um aparelho. «Integrar com eSIM»
  só pode querer dizer uma de três coisas, e pergunta qual antes de desenhar:
  1. ligar para números móveis — é PSTN normal, já coberto pelo tronco;
  2. convergência fixo-móvel (o telemóvel como ramal) — é um **produto da operadora**
     sobre o IMS dela, que se contrata; não se constrói do nosso lado;
  3. aprovisionar eSIMs — é outro negócio (operador virtual), fora deste produto.
- **Regra de casa — gateways GSM com cartões SIM («SIM box») não entram.** Terminar
  tráfego por SIMs de consumidor viola os contratos das operadoras e, em regra, a
  regulação. A interligação com uma operadora é por tronco SIP contratado, com a
  conformidade confirmada junto do regulador (em Angola, o INACOM) — **isso é uma
  verificação jurídica do cliente, não uma afirmação nossa**.

## As regras de casa de um tronco (nenhuma tem portão hoje)

**Autenticação e origem**
- IP na allowlist **e** credenciais, quando a operadora o permitir; só IP quando ela não
  autentica. Nunca só credenciais num porto aberto à internet.
- Interligação por rede privada (VPN, ligação dedicada) antes de IP público.
- Um tronco, um inquilino ou um papel. Não se partilham credenciais entre troncos.

**Fraude de tarifação**
- Limite de canais por tronco (existe no modelo) e, por inquilino, tecto de gasto diário
  e destinos internacionais **fechados por omissão**, abertos por prefixo.
- Prefixos de tarifa majorada e destinos de fraude conhecida bloqueados no plano de
  marcação. A emergência (112) nunca é bloqueada nem travada por limite — R210.
- Alarme por volume anormal fora de horas. Sem ele, descobre-se na factura.

**Identidade do chamador**
- Números em **E.164** de ponta a ponta (`telephony/number.rs` normaliza as formas
  nacionais); o formato que a operadora exige converte-se no bordo.
- Só se apresenta um número que o inquilino possui. A identidade vai em
  `P-Asserted-Identity` para a operadora, e o pedido de privacidade (CLIR) respeita-se.

**Sinalização e media**
- TLS para a sinalização e SRTP para a media sempre que o outro lado suportar; quando a
  operadora só oferece UDP em claro, isso fica **escrito no tronco** (`srtp=off`,
  `transport=udp`) e a ligação é por rede privada.
- Codecs: G.711 lei A primeiro, lei µ como recurso; Opus só entre nós. Transcodificar
  duas vezes paga-se em atraso e em CPU.
- DTMF por RFC 2833/4733, payload 101; in-band só como último recurso, e nunca com
  codec comprimido. Um PIN de IVR que «não entra» é quase sempre isto.
- Session timers (RFC 4028) e `OPTIONS` de vida, para uma chamada morta não ficar a
  facturar.

**NAT**
- SIP ALG desligado no router do cliente — é a primeira causa de áudio num só sentido e
  de registos que caem.
- RTP simétrico e o endereço público anunciado explicitamente no SDP. O intervalo de
  portas RTP aberto no firewall é o mesmo que o configurado, e não colide com o efémero
  do sistema.

**Segredos**
- Passwords de tronco só pela API, nunca num ficheiro do repo; o IP real de uma
  operadora também não. Rotação quando alguém sai.

## Portões

| O que mexeste | Portão |
|---|---|
| As regras de um tronco (transporte, SRTP, host, prefixos, canais) | os unitários de `telephony/trunk.rs` e `cargo test --release --test telephony` (precisa de `DATABASE_URL`) |
| `voice/kamailio/` | **não há portão automático** — nenhum teste carrega o `kamailio.cfg` |
| Qualquer `*.xml` ou `*.xml.inc` de `voice/freeswitch/` | `bash scripts/check-fs-xml.sh` (R226, no `make fitness` e no CI) — XML bem formado, nenhuma directiva `X-PRE-PROCESS` dentro de um comentário, nenhum `$${NOME_EM_MAIÚSCULAS}`. **Estático**: não carrega a configuração num FreeSWITCH. Os `*.lua`: `scripts/check-lua-sintaxe.sh` |
| A interligação com um PBX ou uma operadora | **prova real, fora do CI**: uma chamada em cada sentido, com captura SIP, e as três medições abaixo |
| O próprio softphone de prova, ou uma regra de DTMF no FreeSWITCH | `bash scripts/softphone-prova.sh selftest` — PIN por DTMF, tons medidos nos dois sentidos, e o controlo negativo (sem SRTP → `488`) com um perfil de teste. **Fora do CI**: precisa da imagem do FreeSWITCH e de docker |
| `voice/freeswitch/sip_profiles/internal.xml`, `vars.xml.inc`, as montagens do compose, ou qualquer regra de SRTP | `bash scripts/softphone-prova.sh srtp-real` (R226) — com os ficheiros que o compose monta: o ramal autentica-se, com SRTP a chamada passa a negociação, **sem SRTP leva `488`**, e o `vars.xml.inc` incluído arranca e lê o ambiente. **Fora do CI**, pelas mesmas razões |

**O que uma interligação tem de mostrar antes de se dizer «a funcionar»:**

1. uma chamada de entrada e uma de saída, com áudio **nos dois sentidos** — medido, não
   ouvido de um lado só (`scripts/softphone-prova.sh par`: dois softphones na mesma sala,
   cada um a medir o tom do outro; `voice/softphone/README.md`);
2. o DTMF a chegar ao IVR (o PIN é aceite);
3. o controlo negativo: um `INVITE` de um IP fora da allowlist é recusado, e uma chamada
   para um prefixo fechado não sai.

## O que NÃO está provado (2026-10-03)

- Nenhuma chamada passou por **uma operadora a sério através do Kamailio**: o que está
  medido é contra um FreeSWITCH local (`delonix-meet-telefonia`, R222).
- Nenhuma interligação com um **Issabel ou FreePBX real** foi feita a partir deste repo.
- O `softphone-prova.sh` **nunca correu contra o Meet a funcionar**: os modos `chamada` e
  `par` foram exercitados contra um FreeSWITCH de teste, sem autenticação Digest, sem
  registo, sem o IVR do dial-in e sem a ponte para a sala. O `srtp-real` autentica um
  ramal por Digest no perfil do repo, mas com um directório estático de andaime — o
  control plane não corre — e **não chega a atender**.
- **O compose de voz não corre como está (R226):** nada inclui o `vars.xml.inc`; o
  contexto `delonix_ramais` não existe para o FreeSWITCH (o ficheiro é montado dentro do
  contexto `default` da vanilla → `404`); a vanilla não carrega `mod_xml_curl` nem
  `mod_curl`; e a imagem do compose (`safarov/freeswitch:latest`) não foi medida.
- **SRTP à entrada de um tronco declarado `srtp=off`:** a global é para todas as pernas,
  e o gateway só a redefine à saída (`telephony_fs_xml.rs:230`). Não medido.
- As regras de casa acima **não têm portão**. Session timers, `P-Asserted-Identity`,
  tecto de gasto e alarme de fraude não foram procurados no código nesta revisão:
  confirma por `grep` antes de os dares como existentes ou em falta.
- A conformidade regulatória de qualquer forma de terminação móvel.

## Ao fechar uma tarefa

Propõe um a três pedidos seguintes (escolhe dos quatro abaixo, ou outros), com o alvo, a prova a medir e o que fica de fora:

1. «Põe o compose de voz a correr com a imagem do repo: o `vars.xml.inc` incluído, o
   contexto `delonix_ramais` no sítio certo, `mod_xml_curl` e `mod_curl` carregados.
   Prova: o `srtp-real` sem avisos e com a chamada do controlo positivo a chegar ao
   `ramais_dial.lua`. Fora: o Kamailio e a operadora.»
2. «Um portão que carregue o `voice/kamailio/kamailio.cfg` (`kamailio -c`) no
   `make fitness`. Prova: partir a configuração e ver falhar. Fora: o comportamento em
   chamada, e o XML do FreeSWITCH, que já tem o `check-fs-xml.sh`.»
3. «A interligação com um FreePBX 17 de teste por tronco PJSIP sobre TLS: chamada nos dois
   sentidos, DTMF e os dois controlos negativos. Prova: captura SIP e os tons medidos nos
   dois lados. Fora: a operadora.»
4. «Mede no código o que existe das regras de fraude (tecto de gasto, destinos fechados
   por omissão, alarme) e escreve o que falta como achados. Prova: `grep` com ficheiro e
   linha. Fora: implementar.»
