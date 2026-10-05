# «Ligar a…» a partir da sala — desenho (rascunho)

**Estado:** Proposto · **Data:** 2026-10-05 · **Só desenho, sem código.** Pedido do dono do produto: a partir do Meet
(a sala), ligar a um ramal configurado num Linphone e fazê-lo tocar, para a pessoa atender e entrar na sala.
Relaciona-se com o [ADR-0009](adr/0009-telefonia-troncos-encaminhamento-e-custo.md) (troncos, custo) e o
[ADR-0010](adr/0010-ponte-telefone-sala.md) (ponte telefone↔sala).

## 1. O que existe (verificado a 2026-10-05, `origin/develop`)

| Peça | Onde | O que dá |
|---|---|---|
| Normalização E.164 e bloqueio de emergência | `domain/conferencing/channels.rs` (`normalize_e164`, `check_dial_policy`) | recusa códigos curtos e números de emergência antes de qualquer tronco |
| Tipos do pedido | `channels.rs` (`DialOutKind`: `voice`, `sms_pin`, `whatsapp_invite`, `whatsapp_voice`; `Carrier`) | o vocabulário já existe |
| Registo da chamada | `migrations/0086_room_channels.sql` (`room_dial_outs`) | estados `queued → dialing → ringing → in_call → ended`, mais `declined`, `no_answer`, `failed`, `cancelled`; tarifa, moeda, `redial_of`, `muted`, `on_stage` |
| Vista para o anfitrião | `signaling.rs` (`DialOutView`, `ServerMsg::DialOutUpdated`) | número completo só a anfitriões; `mask_number` para os restantes |
| Originar e **ligar à ponte** | `telephony_esl.rs` (`AfterAnswer::RoomBridge`, `originate … &bridge(…)`) | depois de atendida, a perna segue para o UA SIP da ponte, com SDES-SRTP **obrigatório** |
| Eventos de progresso | `telephony_esl.rs` (`CHANNEL_PROGRESS`, `CHANNEL_ANSWER`) | `ringing` é observável |
| Causas de fim | `telephony_service.rs` (`USER_BUSY`/`CALL_REJECTED` → ocupado; `NO_ANSWER`… → sem resposta) | mapeamento já escrito |
| Ponte telefone↔sala | `phone_bridge/` | funciona (medido a 2026-10-04: um ramal que **entra** por `8000`+PIN aparece na sala) |
| Chamar um ramal **de outro ramal** | `dialplan/default/00_delonix_extensions.xml` + `ramais_dial.lua` | ramal-a-ramal interno, números de 3 a 5 dígitos |

## 2. O que NÃO existe — o buraco real

1. **Nenhuma rota nem serviço** usa `room_dial_outs`. As mensagens e os tipos entraram «sem consumidor»
   (catálogo de regressões: «só os testes os constroem»). **Hoje não há como uma sala ligar a ninguém.**
2. **O `originate` só monta pernas de tronco** (`sofia/gateway/<gateway>/<número>`). Para um **ramal interno** não
   há perna (`user/<ramal>@<domínio>`), e o plano de marcação devolve **zero pernas** para um destino interno
   (`RuleAction::Extension → ResolutionOutcome::Internal`).
3. **O Event Socket** estava ligado só em loopback e a **ponte exigia IPs exactos** (medido a 2026-10-04). A ponte por
   nome fechou-se na #212; o ESL passa a estar ligado no compose e no cluster local com password do `.env`
   (`TELEPHONY_ESL_PASSWORD`) e ACL só para redes privadas (esta PR). Sem password, o ESL continua em loopback.
4. **A tabela `room_dial_outs` não tem `extension_id`** (só `number_e164`, `NOT NULL`): para ligar a um ramal interno a F1
   precisa de uma migração (coluna opcional `extension_id` e `number_e164` a poder ser nulo, ou o número curto do ramal
   guardado como texto). Decide-se com a F1; não está feito.
5. Não há capacidade nem papel para «ligar a alguém» (o catálogo de capacidades é fechado e versionado).
6. Não há botão, lista de chamadas em curso, nem textos nas quatro línguas.

## 3. Âmbito por fatias

| Fatia | O que entrega | Custo | Depende de |
|---|---|---|---|
| **F0 — pré-requisitos** | a ponte estável (o servidor aceitar **nomes** na lista de origens e detectar o IP próprio) e o ESL ligado no compose e no cluster | nenhum | decisão de segurança (§9, D1) |
| **F1 — ramal interno** | a anfitriã carrega «Ligar a…», escolhe **um ramal activo da mesma organização**, o Linphone toca, atende, aparece na sala | **zero** (sem tronco, sem tarifa) | F0, perna `user/` |
| **F2 — número externo por tronco** | o mesmo, para um E.164, com política de marcação, tarifa e quota | por minuto (ADR-0009) | F1 |
| Fora | SMS-PIN e WhatsApp (os tipos existem, mas são outro produto) | | |

Recomendação: **F1 sozinha** é a fatia mais pequena que responde ao pedido, e não toca em dinheiro.

## 4. Contrato (proposta)

`POST /api/rooms/{code}/dial-outs` — `{ "extension_id": "<uuid>" }` (F1) ou `{ "number": "+244…" }` (F2). `202` com a
`DialOutView` em `queued`. `Idempotency-Key` obrigatório (um duplo clique não toca duas vezes).
`GET /api/rooms/{code}/dial-outs` — lista, mais recente primeiro, com cursor.
`POST /api/rooms/{code}/dial-outs/{id}/hangup` — cancela enquanto toca ou desliga em chamada; idempotente.
Erros com código estável, como o resto: `dial_out.room_e2ee`, `dial_out.not_host`, `dial_out.extension_inactive`,
`dial_out.bridge_not_configured`, `dial_out.too_many`, `dial_out.number_refused`.

Camadas (ADR-0004): `http → service → store`. A regra (quem pode ligar a quê, limites, transições) vive no domínio;
o adaptador ESL só executa.

## 5. Máquina de estados do dial-out

Escreve o **serviço**, a partir do resultado do `originate` e dos eventos; **nunca o navegador**.

| De | Evento | Para | Efeito |
|---|---|---|---|
| — | pedido válido | `queued` | grava a linha; devolve `202` |
| `queued` | `originate` lançado | `dialing` | |
| `dialing` | `CHANNEL_PROGRESS` | `ringing` | o ramal toca |
| `dialing`, `ringing` | `CHANNEL_ANSWER` | `in_call` | `answered_at`; a perna vai à ponte |
| `dialing`, `ringing` | `USER_BUSY`, `CALL_REJECTED` | `declined` | |
| `dialing`, `ringing` | `NO_ANSWER`, `NO_USER_RESPONSE`, tempo esgotado | `no_answer` | |
| `dialing`, `ringing` | qualquer outra causa, ponte inacessível, ESL em baixo | `failed` | `failure_code` |
| `queued`, `dialing`, `ringing` | hangup do anfitrião | `cancelled` | `hupall` |
| `in_call` | desligar (qualquer lado) | `ended` | `billsec`, custo |

Cada transição emite `DialOutUpdated` aos anfitriões e escreve um evento de auditoria (`room.dial_out.*`).

## 6. Segurança e abuso

- **Só anfitrião ou co-anfitrião** (a decisão de poder ser delegado a um papel é D2).
- **Salas E2EE recusam** (`dial_out.room_e2ee`): a ponte não decifra, e pôr uma perna telefónica numa sala cifrada de
  ponta a ponta quebraria a promessa. É a mesma regra do directo (ADR-0003).
- **F1 só alcança ramais activos da própria organização.** Nunca sai para a rede pública.
- **F2:** `normalize_e164` + `check_dial_policy` (emergência já bloqueada), plano de marcação da organização,
  quota e tecto de custo; **nenhuma chamada sem tronco e tarifa conhecidas**.
- **Limites:** chamadas em simultâneo por sala e por organização, e por pessoa por minuto; `429` com `Retry-After`.
- **O que o chamado ouve e vê:** o *caller-id* é o nome da sala/anfitrião; se a sala **está a gravar**, o chamado
  tem de ser avisado antes de entrar (D3). Hoje existe o aviso na sala (`ServerRecording`), não no telefone.
- **Privacidade:** o número completo só vai a anfitriões (já assim em `DialOutView`); os outros veem `mask_number`.
- **Comandos ESL só com valores validados** (já é regra do adaptador): o id do ramal vem da base, nunca do cliente.

## 7. Falhas e recuperação

- **O servidor reinicia com o `originate` em curso:** a perna fica órfã no FreeSWITCH. No arranque, um varredor fecha
  as linhas em `queued/dialing/ringing` com mais de N segundos e manda `hupall` pelo `delonix_call_id`; as `in_call`
  reconciliam-se com a ponte (uma perna sem sala fecha-se). **Não provado.**
- **A ponte recusa a perna** (IP fora da lista, SDES em falta): `failed` com a causa; a pessoa nunca fica «a tocar para sempre».
- **O ramal não está registado:** `failed` com `USER_NOT_REGISTERED`, e a anfitriã vê-o já, sem esperar o tempo esgotado.
- **Dois cliques / duas anfitriãs:** um índice parcial único por `(room_id, extension_id)` em estados vivos (depende da coluna nova do §2.4), como se fez
  nos canais de TV.

## 8. Como se prova (plano)

Com um originador **falso** (um *trait*), sem FreeSWITCH: a máquina de estados linha a linha; recusa em sala E2EE; só
anfitrião; ramal de outra organização recusado; duplo clique; reinício a meio. Controlo negativo para cada regra.
Depois, **contra o FreeSWITCH real** do laboratório: o Linphone toca, atende, e **aparece na sala** (a prova que o
dono pediu); `declined` e `no_answer` com o telemóvel; e o áudio nos dois sentidos medido com o softphone de prova
(`scripts/softphone-prova.sh`) em vez de «ouvi».

**O que estes testes não provam:** a qualidade do áudio, o comportamento com uma operadora real (F2), o Android em
segundo plano (uma chamada não toca se o sistema adormecer o Linphone — medido como risco, não resolvido), e o
aviso de gravação ao chamado enquanto não houver mensagem de voz para ele.

## 9. Decisões pendentes (do dono do produto)

- **D1 — a ponte estável (F0).** Aceitar nomes na lista de origens e detectar o IP próprio mexe numa **allowlist de
  segurança** (quem pode abrir uma perna SIP na ponte). Pede revisão de segurança. Sem isto, F1 não corre de forma
  fiável fora de um arranjo manual.
- **D2 — capacidade.** Nova capacidade `sessions.dial_out` (catálogo v4, migração que a semeia, papéis de sistema:
  `owner`/`admin` permitem, `member` recusa, personalizados fail-closed) ou reutilizar a de anfitrião?
- **D3 — aviso de gravação** ao chamado, e se a chamada pode entrar numa sala que grava.
- **D4 — destinos de F1:** qualquer ramal activo da organização, ou só os de pessoas **convidadas** para a reunião?
- **D5 — identificação do chamador** que o Linphone mostra (o nome da sala, o do anfitrião, ou ambos).

## 10. Limites desta proposta

Está assente em leitura de código; **nada disto foi corrido**. Em particular, o comportamento do `originate` para um
ramal interno (`user/` contra a pasta do `mod_xml_curl`) **não foi medido**, e o `ringing` só se observa se o
FreeSWITCH emitir `CHANNEL_PROGRESS` para uma perna interna (esperado, não visto). A estimativa de esforço fica por fazer
até D1 estar decidida, porque é ela que determina se F1 é pequena ou não.
