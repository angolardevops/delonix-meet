# ADR-0005 — Gateway de SMS: telefone por USB e operadores móveis angolanos

**Estado:** Proposto · **Data:** 2026-09-16 · **Contexto:** pedido de produto — «ao lado
do SIP, um serviço de SMS: ligo o meu telefone por USB, o Delonix Meet reconhece-o, eu
escolho-o como ponto de envio; e o backend fica preparado para a Unitel, a Movicel e a
Africell, para mostrar a ferramenta a estes operadores»

## Contexto

### Os factos que decidem o desenho

1. **O servidor não vê USB.** Corre numa imagem distroless sem root
   (`Dockerfile.server`), num pod K8s. O telefone está ligado a OUTRA máquina — o
   portátil do operador, ou um nó dedicado. Portanto, **a detecção do telefone não
   pode ser do servidor**: é de um agente que corre onde o cabo está.
2. **Nem todo o telefone expõe o modem.** Um modem GSM aceita comandos AT
   (`AT+CMGS`) numa porta série (`/dev/ttyACM*`, `/dev/ttyUSB*`). As pens 3G/4G e os
   telefones antigos expõem-na. **A maior parte dos Android modernos não**: por USB
   mostram só MTP (ficheiros) e, com depuração activa, ADB. Não há forma suportada e
   sem root de enviar SMS por ADB. O agente tem de o **dizer** ao utilizador em vez de
   fingir que o telefone está pronto.
3. **Medido a 2026-09-16, com o telefone do pedido ligado:** um Samsung (`04e8:6860`)
   com uma só interface, `06/01/01 "MTP"`, e nenhuma porta série. É exactamente o caso
   acima — este telefone, tal como está, **não envia SMS por USB**.
4. **O `ModemManager` agarra os modems.** Medido nesta máquina a 2026-09-16: está
   activo. Se está, as portas série do modem já estão ocupadas por ele e o AT directo
   entra em conflito. Por isso há dois transportes: `modemmanager` (via `mmcli`)
   quando ele gere o modem, e `at_serial` quando não.
5. **Os operadores falam SMPP.** A interface normal de SMS em volume com um operador
   é SMPP 3.4 (`bind_transmitter`, `submit_sm`). **Nenhum contrato existe hoje** —
   host, porta, `system_id` e *sender ID* só chegam com o contrato. O que se prepara é
   o cliente e o encaminhamento, provados contra um SMSC falso.
6. **Não há cifra de segredos em repouso** (S5 aberto). Uma credencial SMPP numa
   tabela seria mais um segredo em claro. Ela vem do ambiente.

## Decisão

### Três peças

```
 telefone ──USB── [delonix-sms-gateway]  ──HTTPS (sai; sem ingress)──►  [delonix-server]
                   descobre, sonda, envia     token dlxg_ da org           fila, rota, auditoria
                                                                                 │
                                                          SMPP 3.4 (TCP) ◄───────┘ worker
                                                          Unitel / Movicel / Africell
```

- **`sms-gateway/`** — binário Rust à parte (como `ai-worker/`), corre na máquina do
  telefone. Lê `/sys/bus/usb/devices` (sem libusb), classifica cada dispositivo, sonda os
  candidatos com AT ou `mmcli`, reporta o inventário a cada 5 s, pede mensagens e envia.
  **Liga-se para fora**: não abre portas, funciona atrás de NAT.
- **`server/src/sms.rs`** — fila, encaminhamento, gestão e as duas superfícies HTTP.
- **`server/src/sms_codec.rs`** — GSM 03.38 / UCS-2, segmentação e PDU SMS-SUBMIT
  (`sms_smpp.rs` usa as mesmas partes). **É o único sítio nosso que codifica SMS**: no
  transporte `at_serial` o agente escreve os PDUs feitos. No transporte `modemmanager`
  quem codifica e divide é o ModemManager, a partir do texto — e o número de partes
  cobradas pode diferir do que a consola estimou.

### Posse

| Recurso | Dono | Porquê |
|---|---|---|
| Gateway USB e o seu token | a **organização** | o telefone é dela; o custo do SMS é dela |
| Ligação SMPP a um operador | a **plataforma** (env) | o contrato é da Delonix com o operador, não de cada inquilino |
| Mensagem | a **organização** | isolamento como tudo o resto |

### Encaminhamento (por mensagem, decidido no pedido e gravado)

1. Normaliza o destino para E.164. Só `+244` nesta fase; outro país → `422`.
2. Identifica o operador pelo plano de numeração (tabela abaixo).
3. Se o pedido diz `route: "operator"` ou `"auto"` **e** esse operador tem ligação
   configurada → `operator`.
4. Senão, se `route` é `"usb"` ou `"auto"` **e** a org tem um dispositivo seleccionado,
   capaz e visto há menos de 30 s → `usb`.
5. Senão → `422` com a razão por extenso. **Nunca** se aceita e fica parado. (O código
   estável `sms.no_route` entra quando o envelope de erro da v1 nascer em `error.rs` —
   `delonix-meet-api` §7; não se inventa um segundo formato num handler.)

#### Plano de numeração — A CONFIRMAR

Números móveis angolanos: `+244 9XX XXX XXX`. Os prefixos abaixo são o conhecimento
corrente e **não foram confirmados contra a publicação do regulador**. Vivem numa só
tabela (`sms::operator_for`), e confirmar é a primeira tarefa antes de qualquer demo a
um operador — mostrar a um operador um número dele atribuído a outro é o pior erro
possível na sala. Não há portabilidade de número considerada.

| Prefixo | Operador |
|---|---|
| 92, 93, 94 | Unitel |
| 91, 99 | Movicel |
| 95 | Africell |

### Estados de uma mensagem

`queued` → `claimed` → `sent` | `failed`. `delivered` fica reservado para os recibos
(`deliver_sm`), que **não estão implementados**.

**No máximo uma vez.** Uma mensagem `claimed` há mais de 10 min passa a `failed`
(«o gateway não confirmou») e **não** volta à fila: se o agente enviou e morreu antes de
confirmar, reenviar mandava o SMS duas vezes, e um SMS duplicado custa dinheiro e
confunde quem o recebe. Quem quiser repetir cria uma mensagem nova.

### Contrato HTTP

**BFF — sessão, só administrador da org** (enviar SMS custa dinheiro: é superfície de
fraude, como a portagem do dial-in).

| Método e caminho | Resposta |
|---|---|
| `GET /api/orgs/{org_id}/sms/gateways` | `200` `[{id,name,prefix,created_at,last_seen_at,online}]` |
| `POST /api/orgs/{org_id}/sms/gateways` `{name}` | `201` `{id,name,prefix,token}` — o token só aparece aqui |
| `DELETE /api/orgs/{org_id}/sms/gateways/{gateway_id}` | `204`; `404` se não for da org |
| `GET /api/orgs/{org_id}/sms/devices` | `200` `[{id,gateway_id,gateway_name,device_key,vendor_id,product_id,manufacturer,product,serial,kind,transport,port,capable,reason,operator_name,signal_percent,last_seen_at,online,selected}]` |
| `GET /api/orgs/{org_id}/sms/route` | `200` `{device_id\|null, operators:[{operator,label,prefixes,configured}]}` |
| `PUT /api/orgs/{org_id}/sms/route` `{device_id\|null}` | `200` igual ao GET; `422` se o dispositivo não for capaz; `404` se não for da org |
| `GET /api/orgs/{org_id}/sms/messages?page_size=` | `200` `{items:[Message], next_page_token:null}` — `page_size` ≤ 100, omissão 50 |
| `POST /api/orgs/{org_id}/sms/messages` `{to, body, route?}` | `202` `Message`; cabeçalho `Idempotency-Key` opcional → a mesma chave devolve a mesma mensagem |
| `GET /api/orgs/{org_id}/sms/messages/{message_id}` | `200` `Message` |

`Message = {id,to,body,encoding,segments,route,operator,device_id,status,error,provider_ref,created_at,sent_at}`.

**Agente — `Authorization: Bearer dlxg_…`** (extractor `sms::SmsGatewayAuth`). Máquina a
máquina; pelo ADR-0004 §4 o destino é gRPC, e fica HTTP pela mesma razão que o IVR: o
agente corre fora do cluster e hoje não há porta gRPC.

| Método e caminho | Corpo / resposta |
|---|---|
| `PUT /api/integrations/sms-agent/v1/devices` | corpo `{devices:[Device]}` — substitui o inventário deste gateway. `200` `{poll_interval_secs}` |
| `POST /api/integrations/sms-agent/v1/claim` | `200` `{messages:[{id,device_key,to,body,pdus:[{hex,tpdu_len}]}]}` — até 5, só da org do token e só para o dispositivo seleccionado **se** for deste gateway |
| `POST /api/integrations/sms-agent/v1/messages/{message_id}/result` | corpo `{ok:bool, error?, provider_ref?}` → `204`; `404` se a mensagem não foi reclamada por ESTE gateway |

`Device` (reportado pelo agente) =
`{device_key, vendor_id, product_id, manufacturer, product, serial, kind, transport, port, capable, reason, operator_name, signal_percent}` com
`kind ∈ {modem, android_adb, android_mtp, mass_storage_modem, unknown}` e
`transport ∈ {at_serial, modemmanager, none}`. Campos opcionais vão a `null`, nunca `""`.
`device_key` é estável entre ligações: `vendor:product:série` quando a série identifica
alguma coisa, senão `vendor:product@caminho-usb`. Medido a 2026-09-16: há aparelhos com
séries como `000000000` e `SN0001`, que colidiriam entre dois aparelhos iguais.

### Configuração dos operadores (plataforma)

```
SMS_UNITEL_SMPP=smpp://system_id:password@host:2775?source_addr=DELONIX
SMS_MOVICEL_SMPP=...
SMS_AFRICELL_SMPP=...
```

Ausente → operador `configured: false`, e o encaminhamento não o escolhe. A password
nunca aparece em log (`Secret`) nem em resposta.

## Contactos e reuniões (extensão de 2026-09-16, `delonix-meet-backend/sms-contactos`)

Pedido: «enviar SMS a qualquer contacto directo, e convites/lembretes de reunião por SMS».
Não muda nada do que está acima; acrescenta quem pode enviar a QUEM.

### Onde vive o número, e porquê

| Dado | Tabela | Porquê |
|---|---|---|
| Telefone (E.164) | `org_members.phone_e164` + `phone_source` | é o contacto da pessoa **nesta** org (o que o Odoo da empresa conhece); um membro arquivado deixa de ser alcançável no mesmo instante; a sincronização do directório é por org |
| Consentimento | `users.sms_contact_opt_out`, `users.sms_meeting_opt_out` | é vontade de quem recebe, e não muda com a org que envia |
| Quem pode enviar a contactos | `organizations.sms_send_policy` (`admins` por omissão \| `members`) | um SMS custa dinheiro: abrir a membros é decisão explícita |

Validação do número = a do envio (`normalize_msisdn`): só `+244 9…`. Não se guarda um
número que o encaminhamento não sabe usar — internacional entra quando houver rota.

**Regra de sincronização com o Odoo** (provision `dlxo_` e pull do directório no login):
`mobile_phone` primeiro, `work_phone` depois. `phone_source = 'manual'` (escrito pelo
próprio ou por um admin) **nunca** é sobrescrito; `'odoo'` acompanha o directório, incluindo
ser apagado quando o Odoo manda `false`/`""`. Campo **ausente** não mexe no número (um
integrador antigo não apaga telefones). Número inutilizável (fora de Angola) não grava nem
apaga, e sai em `phones_rejected` na resposta do provision. Voltar a seguir o directório é
explícito: `PUT …/phone {"follow_directory": true}`.

### Contrato HTTP (BFF, sessão)

| Método e caminho | Quem | Resposta |
|---|---|---|
| `PUT /api/orgs/{org_id}/members/{user_id}/phone` `{phone\|null}` ou `{follow_directory:true}` | o próprio ou admin | `200` `{user_id, phone, phone_source}`; `403` outro membro; `404` não-membro/arquivado/outra org; `422` número fora do plano |
| `GET /api/orgs/{org_id}/members` | membro | cada linha ganha `phone` (só para admin ou o próprio; senão `null`), `phone_source`, `can_sms` (tem número e não desligou contactos) |
| `GET /api/orgs/{org_id}/sms/policy` | membro | `{send_policy}` |
| `PUT /api/orgs/{org_id}/sms/policy` `{send_policy}` | admin | `200`; `403` membro; `400` valor desconhecido |
| `GET/PUT /api/users/me/sms-preferences` `{contact_opt_out?, meeting_opt_out?}` | o próprio | `{contact_opt_out, meeting_opt_out, phones:[{org_id, org_name, phone, phone_source}]}` |
| `POST /api/orgs/{org_id}/sms/messages` `{user_id, body, route?}` | membro se a política for `members`; admin sempre | `202` `Message` com `purpose:"contact"`, corpo prefixado `«<remetente> (Delonix Meet): »`. `403` sem permissão; `404` destinatário não é membro ACTIVO desta org; `409 sms.recipient_opted_out`; `422 sms.recipient_no_phone`; `400 sms.target_ambiguous` (`user_id` **e** `to`); `429` limite |
| `POST /api/orgs/{org_id}/sms/messages` `{to, body, route?}` | **só admin**, com qualquer política | como antes; um membro leva `403` |
| `GET /api/orgs/{org_id}/sms/messages[/{id}]` | membro | admin vê tudo; membro vê só as que **criou**, com `to` mascarado (`+244*******00`) |
| `POST /api/meetings` `{…, sms_invite?, sms_reminder_min?}` | quem agenda | `403` antes de criar a reunião se quem agenda não puder enviar a contactos; `422 sms.reminder_recurring_unsupported` com `recurrence_freq`; resposta ganha `sms:{invite:{queued, skipped:[{user_id, reason}]}\|null, reminder_min}` |

`Message` ganha `purpose` (`direct|contact|meeting_invite|meeting_reminder`),
`recipient_user_id`, `meeting_id`, `created_by`.

Os códigos (`sms.recipient_opted_out`, `sms.recipient_no_phone`, `sms.target_ambiguous`,
`sms.recipient_not_member`, `sms.no_route`, `sms.too_many_recipients`, `sms.invalid_body`,
`sms.no_org`, `sms.idempotency_key_in_use`) vão no **início** do texto de `error` — o
envelope `{error:{code,…}}` nasce em `error.rs` com a v1 (`delonix-meet-api` §7), não aqui.

### Limites

- Por org: 30/min (como antes). Por utilizador na org: 5/min (`sms::USER_SENDS_PER_WINDOW`).
- **Ordem:** permissão, destinatário, corpo e rota são verificados ANTES de gastar quota —
  um pedido recusado já não consome o limite (antes, sim).
- SMS de reunião não passam pelos limites por minuto: o tecto é 50 convidados por reunião
  (`sms_notify::MAX_RECIPIENTS_PER_MEETING`); os restantes saem com `sms.too_many_recipients`.
- Os limitadores são em memória **por pod**: com N pods o tecto efectivo é N×. Igual ao de org.

### Reuniões

- **Convite:** ao agendar, um SMS por convidado membro activo da org do anfitrião, com
  telefone e sem `meeting_opt_out`. Idempotência `meeting-invite:{meeting}:{user}:{início}`.
- **Lembrete:** `sms_reminder_min` (5–1440). Um passo do worker de SMS existente (a cada
  20 s) reivindica as reuniões vencidas marcando `sms_reminder_done_at` com
  `FOR UPDATE SKIP LOCKED` **antes** de enfileirar — no máximo uma vez entre pods. Volta a
  ver a permissão do anfitrião; salta quem recusou o convite; não lembra depois da hora.
  Remarcar pela v1 (`PATCH /api/v1/meetings/{id}` com `starts_at`) rearma o lembrete.
- **Texto:** PT, letras sem equivalente GSM trocadas pela simples («Reuniao») para ficar em
  GSM-7, título encurtado até caber num segmento, hora em WAT (UTC+1 fixo), link
  `https://<domínio da org>/#/calendar` só quando a org tem domínio.
- **Recorrência:** lembrete recusado (`422`) — as instâncias da série nascem sem ele.

### O que esta extensão NÃO faz

- **«STOP» por SMS** — não há SMS recebidos (MO). O opt-out é só no perfil.
- **Recibos de entrega** — `sent` continua a ser «o SMSC/modem aceitou».
- **SMS pela v1** (`dlx_`) nem a partir do `meetings_v1::create` — só a BFF.
- **Lembrete em séries recorrentes**, e convite reenviado quando se acrescenta um convidado
  pela v1.
- **Quota/custo por SMS** e histórico por destinatário.

## O que fica de fora (e é dito)

- **Recibos de entrega** (`deliver_sm`, `bind_transceiver` de longa duração). Sem eles,
  `sent` é «o SMSC/modem aceitou», não «chegou».
- **SMS recebidos** (MO) — nem pelo modem nem pelo SMPP.
- **SMPP sobre TLS.** O cliente fala TCP simples; cada operador diz se exige TLS ou VPN.
- **Facturação e quota de SMS.** Há limite de envio por org (`sms_send_limiter`), não há
  custo.
- **Android sem modem exposto.** Detectado e explicado; não enviado.
- **Confirmação dos prefixos** junto do regulador.

## Consequências

- Um passo manual no caminho do cliente: instalar e arrancar o agente com o token. É um
  bloqueio para produção, não para a demonstração; o empacotamento (`.deb`, unit
  `systemd` de utilizador) é a tarefa seguinte.
- A superfície do agente sai da árvore pública quando existir a porta gRPC (ADR-0004 §6
  passo 7), junto com o IVR.
