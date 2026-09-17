# ADR-0009 — Telefonia: troncos, encaminhamento e custo

**Estado:** Proposto · **Data:** 2026-09-17 · **Contexto:** ecrã `DelonixTelecom` do template
Navegavel3 («Telefonia, SIP e SMS»), frente C do backend v3
(`notas-ui-template/backend-v3-comum.md`). **Assenta em:** ADR-0004/0006 (camadas, contexto
`telephony`, superfícies), ADR-0005 (gateway de SMS). **Consumido por:** ADR-0010 (canais na
sala, frente D) através de `notas-ui-template/contrato-telefonia.md`.

## Contexto

Medido a 2026-09-17 contra `origin/seg/ssrf-saida` (`2f830bc`) e `notas-ui-template/sip-realidade.md`:

1. **O que é real é o plano de controlo.** Salas de voz, DIDs, PIN, `voice_cdr` e um resumo de
   facturação a uma tarifa única de ambiente (`VOICE_TARIFF_INBOUND`, omissão `0.0`).
2. **A media nunca correu.** A configuração Kamailio/FreeSWITCH em `voice/` tem defeitos lidos
   (allowlist vazia, `tls.cfg` não montado, `vars.xml.inc` não incluído, `dispatcher.list` com
   um nome que não resolve em `network_mode: host`). Não há ponte FreeSWITCH↔SFU: quem liga
   não ouve a reunião.
3. **Não há saída, troncos, plano de marcação nem custo por chamada.** Nada origina uma chamada;
   não há tabela de tarifas; um CDR não sabe por que operadora saiu.
4. **O ecrã pede números medidos:** canais em uso, ASR por operadora, jitter/perda, estado do
   SBC, consumo do mês em Kz. A regra do produto é **não simular**.
5. **Não há FreeSWITCH nem Kamailio nesta máquina** (nem imagem local), e o ADR não autoriza
   descarregar imagens de terceiros para a prova. O que depende deles é provado contra um
   servidor ESL falso e dito como não provado contra o real.

## Decisão

### 1. Modelo — operadoras são dados, não código

| Tabela (migração) | O quê |
|---|---|
| `telephony_trunks` (0065) | tronco SIP por org: nome, sigla, âmbito, host/porta, transporte, SRTP, registo, utilizador, password **cifrada** (`secret_box`, aad `telephony_trunks.password:<id>`), prefixos, canais máximos, **posição** (ordem de encaminhamento), activo |
| `telephony_trunk_prices` (0065) | preço/minuto **com histórico** (append-only, `valid_from`, AOA/USD em décimas-milésimas) |
| `telephony_exchange_rates` (0065) | Kz por USD com histórico |
| `telephony_dial_plans` + `telephony_dial_rules` (0066) | plano ordenado, com `CHECK` de emergência na base |
| `telephony_sip_settings` (0067) | domínio, SBC, transporte, SRTP, codecs, conta SIP cifrada |
| `telephony_call_records` + `telephony_outbound_calls` (0068) | CDRs ingeridos com custo congelado; chamadas de saída pedidas pela plataforma |
| `sms_device.battery_percent/balance_*` (0069) | bateria e saldo **só quando o agente os reporta** |

Unitel, Africell, Movicel e «internacional» não aparecem em nenhum `match`: são linhas.
A tabela `voice_cdr` herdada fica (o IVR continua a escrevê-la e `/voice/billing` lê-a); o
ecrã novo lê `telephony_call_records`.

**SRTP exige TLS.** Com SDES as chaves vão no SDP; SRTP sobre UDP/TCP em claro é segurança
fingida e é recusado (`telephony.srtp_requires_tls`). **O host de um tronco passa pela guarda
de saída do inquilino** (`net_guard::check_tenant_config_url`): é o FreeSWITCH que liga a ele em
nome do inquilino, e um tronco para `10.0.0.5` seria SSRF por SIP.

### 2. Encaminhamento — a primeira regra que casar vale

Padrões de PBX sobre o número normalizado para a forma nacional (`+244 923 447 108` →
`923447108`, outro país → `00…`, curtos ficam): dígitos, `X` `Z` `N`, `[1-5]`, `.`/`!` no
fim, e listas `112,113,115`. Casa o número inteiro. Acções: `external` (operadora + reserva),
`room_pin`, `extension`, `block`. Tudo em `delonix_meet_domain::telephony::dial_plan`, sem IO,
com tabela de casos.

Os troncos de uma regra externa tentam-se pela ordem **principal → reserva**, saltando os
inactivos. A ordem das operadoras (`PUT …/trunk-order`, a lista inteira) decide o papel
(`primary`, `reserve` N) e a reserva da emergência.

### 3. Emergência — invariante do servidor

Os números de emergência são dado da instalação (`TELEPHONY_EMERGENCY_NUMBERS`, omissão
`112,113,115`) e resolvem-se **antes** das regras:

- **nunca gravados**: a regra `emergency` com `record: true` é recusada; a resolução, a
  extensão gerada para o FreeSWITCH e a ingestão de CDR forçam `record=false`; e a base tem
  `CHECK (NOT (emergency AND record))` nas regras e nos CDRs;
- **nunca bloqueados**: uma regra `block` que case um número de emergência é recusada ao gravar;
  a resolução usa os troncos da regra de emergência e junta **todos os outros activos** como
  reserva; a extensão gerada não aplica o limite de canais (`bridge` directo, sem
  `limit_execute`);
- sem regra de emergência a resolução continua a ter caminho (todos os troncos activos);
- sem nenhum tronco activo diz `no_available_trunk` — não finge.

O que NÃO é emergência: o **teste rápido** recusa números de emergência
(`telephony.test_call_emergency_refused`) e a frente D não os convida para uma sala
(`telephony.emergency_not_invitable`). O invariante é sobre quem MARCA, não um botão para
chamar os bombeiros.

### 4. Portas e adaptadores

| Porta (domínio) | Adaptador real | Falso (só testes) |
|---|---|---|
| `SipControl` | `telephony_esl::FreeswitchSipControl`: ESL `version`, `uptime s`, `show channels count`, `global_getvar outbound_codec_prefs`, `sofia xmlstatus gateway dlx-<id>`, `limit_usage hash delonix_trunk <id>`; «reiniciar» = `sofia profile <p> killgw` + `rescan`. SBC por JSON-RPC do Kamailio (`core.version`, `core.uptime`) pelo cliente `operator()` do `net_guard` | servidor ESL TCP em `tests/telephony.rs` |
| `CallOriginator` | ESL `api originate {vars}[leg]sofia/gateway/dlx-A/N\|[leg]sofia/gateway/dlx-B/N &app` numa ligação própria; `+OK` = atendida (latência medida), `-ERR CAUSA` = não | o mesmo servidor ESL falso |
| `CdrSource` | `telephony_cdr::FreeswitchJsonCdr` (formato `mod_json_cdr`) | payloads JSON nos testes |
| `SmsGateways` | `sms::enqueue` (a regra do ADR-0005, extraída de `send_message`, sem cópia) | — |

Sem `TELEPHONY_ESL_ADDR` nem `TELEPHONY_KAMAILIO_RPC_URL` os adaptadores não existem e a API
responde `not_configured` / `422 telephony.not_configured`. Nenhum valor de estado é inventado.

**Os comandos ESL só levam valores validados** (dígitos, UUIDs, `[a-z0-9-]`); um `\n` é
recusado antes de escrever.

### 5. Registo SIP e credenciais

`GET …/sip-registration` junta o **configurado** (domínio, SBC, transporte, codecs) e o
**medido** (versão e uptime do Kamailio e do FreeSWITCH, codecs oferecidos, canais em uso,
estado de cada tronco, jitter/perda/MOS médios dos CDRs de 24 h). `state`:
`not_configured` | `down` (media server não responde) | `degraded` (SBC configurado mas não
responde, tronco em baixo ou degradado) | `healthy`.

«Ver credenciais» (`POST …/sip-settings/reveal-credentials`) exige **reautenticação** (password
da conta; código MFA para contas SSO sem password local; sem nenhum dos dois → `403
telephony.reauth_unavailable`), trava falhas (5 em 5 min por conta) e audita sucesso e falha.

### 6. CDR — `mod_json_cdr`, idempotente, custo congelado

**Decisão: `mod_json_cdr` → `POST /internal/v1/telephony/call-records`**, e não um subscritor
ESL. O módulo guarda o CDR em disco e reenvia enquanto o servidor não responder `2xx`; um
subscritor ESL perde tudo o que acontece enquanto está desligado.

- Autenticação: `VOICE_INTERNAL_SECRET` por HTTP Basic (`cred`) ou `X-Voice-Secret`; só no
  listener interno com `INTERNAL_BIND_ADDR`.
- **Idempotência:** `UNIQUE (source, source_call_id)`; reenvio → `200 {duplicate: true}` sem
  segundo custo.
- **Org:** `delonix_org_id` (posto por nós no `originate` e na extensão gerada) ou, no dial-in
  herdado, a org da `delonix_voice_room_id`. Sem org → `422 telephony.cdr_org_unresolved`
  (fica no `log-dir` do FreeSWITCH). Um tronco de outra org não é atribuído.
- **Custo:** só saída atendida, por começo de minuto, ao preço em vigor em `started_at`,
  congelado na linha (`cost_e4`, `price_id`). Sem preço → `cost` `null` com `cost_reason`
  (`no_price_in_force`, `no_trunk`, `inbound_not_billed`). **Preços e taxas não são
  retroactivos** (`telephony.price_backdated`): um preço novo nunca reescreve chamadas taxadas.
- **Qualidade:** perda = `skip/(packets+skip)`; jitter = √ da variância máxima do intervalo
  entre pacotes do FreeSWITCH (`rtp_audio_in_jitter_max_variance`) — **não é o jitter do
  RFC 3550**, e a UI deve rotulá-lo como estimativa do media server; MOS do FreeSWITCH.
- **Lista:** `GET …/call-records`, números **mascarados** (`+244 923 ***108`), cursor com
  impressão digital dos filtros (`search.page_token_mismatch`). A pesquisa livre do motor da
  ADR-0007 **não está nesta base** (`delonix-meet-backend/pesquisa-profunda`); os filtros exactos
  seguem o contrato de `docs/reference/pesquisa.md` e o `search-schema` liga-se na integração.

### 7. Configuração do FreeSWITCH — `mod_xml_curl`

**Decisão: `mod_xml_curl` → `POST /internal/v1/telephony/freeswitch-config`**, e não ficheiros
gerados. O plano muda por org e a qualquer hora; ficheiros exigiam volume partilhado e
`reloadxml`, e um reload falhado deixava o plano velho em silêncio.

- `section=dialplan`, contexto `delonix-outbound`: o servidor devolve UMA extensão para o número
  pedido, construída a partir de `telephony_service::resolve_number` — **a mesma função do
  `dial-plan/test`**; o teste e a chamada real não divergem. Troncos em failover com
  `limit_execute hash delonix_trunk <id> <max>` (canais máximos), gravação só quando a regra o
  diz, `respond 403/404/503` para bloqueado / sem regra / sem tronco (nunca cai num plano por
  omissão).
- `section=directory`, `purpose=gateways`: os gateways `dlx-<trunk_id>` de todos os troncos
  activos, com a password decifrada — o perfil sofia usa `<domain name="all" parse="true"/>`.
  É a única saída de passwords em claro, e só no listener interno com o segredo.

### 8. SMS no mesmo ecrã

`GET /api/orgs/{org_id}/sms/overview` junta os dispositivos (modem USB / telefone), as ligações
SMPP da plataforma e as contagens do dia (fuso `Africa/Luanda`). Bateria e saldo só aparecem
quando o agente os reporta (`DeviceReport.battery_percent`, `balance`); senão `null` com razão.
«Emparelhar aparelho» é o fluxo que já existe (`POST …/sms/gateways` → token `dlxg_`).
«Enviar SMS» do teste rápido é `POST …/sms/messages` — não há segunda rota.

### 9. Superfície

Todas as rotas da consola: BFF, sessão, **administrador da org** (até a frente A trazer
`require_capability`; capacidades propostas no relatório). `org_id` só do caminho. Isolamento
em `web/e2e/isolamento.mjs`. O pedido falava em `dial-plan:test`; seguiu-se a convenção do repo
(`/dial-plan/test`, como `/rotate-key`).

## EXTERNAL — o que depende de infraestrutura que não existe aqui

| Peça | Estado | O que falta para ligar |
|---|---|---|
| FreeSWITCH com ESL (`mod_event_socket`) | **não provado** contra real | subir FreeSWITCH, `TELEPHONY_ESL_ADDR/PASSWORD`; confirmar o XML de `sofia xmlstatus gateway` e o `originate` com failover e `origination_uuid` |
| `mod_json_cdr` | **não provado** contra real | `url=http://<interno>/internal/v1/telephony/call-records`, `cred=freeswitch:<VOICE_INTERNAL_SECRET>`, `encode-values=true`, `log-dir` |
| `mod_xml_curl` (dialplan + gateways) | **não provado** contra real | binding `dialplan` e `directory`, `gateway-credentials`, perfil `external` com `<domain name="all" parse="true"/>`, `mod_hash` para `limit` |
| Kamailio `jsonrpcs` | **não provado** contra real | `loadmodule "jsonrpcs.so"` + `xhttp`, `TELEPHONY_KAMAILIO_RPC_URL` |
| Troncos das operadoras | inexistentes | contratos Unitel/Africell/Movicel/internacional; credenciais e IPs |
| Ponte FreeSWITCH↔SFU | **não existe** (`sip-realidade.md` §3) | sem ela, `AfterAnswer::Conference` põe a pessoa na conferência do FreeSWITCH, não na reunião |
| `rtpengine` | não medido | nada neste ADR lê o rtpengine; o ecrã mostra-o só se configurado |
| Agente SMS: bateria e saldo | o agente (`sms-gateway/`) **não** os envia ainda | ler bateria (Android) e saldo (USSD) no agente |
| Recibos de entrega SMS | não existem (ADR-0005) | `deliver_sm` |
| Prefixos das operadoras | «A CONFIRMAR» (ADR-0005) | regulador |

## Consequências

- **+** Uma resolução de número, usada pelo teste, pela chamada real (xml_curl) e pela frente D.
- **+** O custo de uma chamada é auditável: preço, id do preço e data ficam na linha.
- **+** O ecrã nunca mostra um número que não foi medido; `null` vem sempre com razão.
- **−** Cada chamada pelo contexto `delonix-outbound` faz um pedido HTTP ao servidor
  (xml_curl). Se o servidor cair, as chamadas de saída caem — uma `fallback` estática só para
  emergência é o passo seguinte a medir com FreeSWITCH real.
- **−** O endpoint de gateways devolve passwords em claro ao FreeSWITCH. Mitigado por listener
  interno + segredo; mTLS entre FreeSWITCH e servidor fica para quando houver sidecar gRPC.
- **−** O estado dos troncos faz uma ida ao ESL por pedido de lista (timeouts de 3–4 s).
  Uma cache curta é o passo seguinte se a latência medida o pedir.
