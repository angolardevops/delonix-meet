# ADR-0023 — Acordar um ramal móvel por push (S-01, S-02)

**Estado:** Proposto · **Data:** 2026-10-08 · **Contexto:** `docs/mobile/delonixphone-requisitos.md` (RF-12,
RF-30 a RF-33), ADR-0022 (motor SIP), ADR-0011 (sessões), ADR-0016 e `server/src/ramais.rs`.

## O problema, medido

Um telemóvel com a app morta não tem registo SIP. Hoje, uma chamada para um ramal sem registo **morre em
10 ms**: `originate user/<ramal>@<domínio>` devolve `-ERR USER_NOT_REGISTERED` (medido a 2026-10-08 no
laboratório). Os ramais registam-se **directamente no FreeSWITCH** (perfil `internal`), o Kamailio só serve o
tronco e não tem `usrloc`: não existe nenhum ponto que saiba acordar um aparelho. Sem isto o DelonixPhone
não é um telefone (princípio 1 do documento de requisitos).

## Decisão

O push **não passa pelo motor SIP nem pelo Kamailio**: o servidor de controlo (Rust) é o dono dos aparelhos e
dos fornecedores de push; o FreeSWITCH **segura a chamada** enquanto o aparelho acorda. Três peças:

1. **S-02, FreeSWITCH (feito e medido neste PR).** Em `ramais_dial.lua`, antes do `bridge`: se o destino não
   está registado e `delonix_push_wait_secs` > 0, o Lua pede `POST /internal/v1/voice/push/wake` ao servidor e,
   se a resposta for `{"awaiting":true}`, deixa a chamada a tocar ao chamador (`ring_ready`) e espera pelo
   **registo** até ao limite (≤ 60 s). Registou-se: segue para o `bridge` normal. Não se registou:
   `NO_USER_RESPONSE`. O chamador desligou: pára de esperar. **Desligado por omissão** (`DELONIX_PUSH_WAIT_SECS`,
   0), e sem `awaiting:true` o comportamento é o de antes (falha já).
2. **S-01, servidor (por fazer).** Tabela de aparelhos, rotas de registo, e o *wake* com fornecedores atrás
   de um *trait*.
3. **Aplicação (por fazer).** Recebe o push, regista-se e atende o INVITE que o FreeSWITCH já tem à espera.

### O desenho do servidor (S-01)

**Dados.** `voice_devices`: `id`, `org_id`, `extension_id`, `session_id` (FK para `user_sessions`, ADR-0011),
`platform` (`android`|`ios`), `provider` (`fcm`|`apns_voip`), `push_token` **cifrado em repouso**
(`secrets_at_rest`, como as outras credenciais), `app_version`, `created_at`, `last_seen_at`, `revoked_at`.
Um token por aparelho; o mesmo token noutro ramal substitui o anterior.

**Rotas (sessão, `/api/v1`).** `PUT …/my-extension/devices/{id}` (regista ou renova o token),
`DELETE …/devices/{id}`, `GET …/devices`. Quem as chama é a pessoa dona do ramal ou o administrador da org;
nunca se devolve o token. **Revogar uma sessão (ADR-0011) revoga os aparelhos dessa sessão** e a app deixa
de ser acordada: «terminar o iPhone» passa a querer dizer isso.

**Wake (interno, segredo de voz).** `POST /internal/v1/voice/push/wake` com `{domain, sip_username,
call_uuid, caller_sip_username}`. O servidor resolve o ramal **dentro da organização do domínio** (nunca cruza
orgs: RNF-29), procura aparelhos activos, manda **um** push por aparelho e responde
`{"awaiting": true|false, "devices": n}` (`awaiting` é verdadeiro se houver pelo menos um aparelho). Sem aparelhos: `awaiting:false` e a chamada falha já (como hoje). O
limite de pedidos por ramal e por minuto é do servidor, para um chamador não poder usar isto para inundar o
telemóvel de alguém.

**Conteúdo do push.** Só o que a app precisa para o ecrã de chamada e para casar o INVITE: `call_uuid` e o
**número curto** de quem liga, que o servidor procura (na organização do destino) a partir do utilizador SIP
que o FreeSWITCH autenticou, nunca o `From`. **O utilizador SIP de ninguém vai num push**: é metade da
credencial dele (medido: a primeira versão mandava-o, e a prova ponta-a-ponta apanhou-o). Um chamador que
não é ramal desta organização vai sem nome. Nada de credenciais,
nem número externo em claro quando o chamador vem do tronco (CLI: decisão do dono, RF-65).

**Fornecedores.** *Trait* `PushProvider` com: `fcm` (HTTP v1, mensagem só de dados, prioridade alta),
`apns_voip` (`apns-push-type: voip`, tópico `<bundle>.voip`) e um `lab` para o laboratório e para os testes.
Os pedidos de saída passam pelo guarda de SSRF (`net_guard`), como os webhooks. As credenciais do fornecedor
(conta de serviço do Firebase, chave `.p8` da Apple) vivem no segredo da instalação, nunca no repo.

### O que o iOS impõe (e o desenho tem de respeitar)

- **Cada push VoIP tem de acabar numa chamada reportada ao CallKit**, em segundos, ou o iOS corta o canal.
  Por isso o *wake* só se manda quando há mesmo uma chamada a entrar, nunca «para testar», e a app reporta ao
  CallKit **antes** de se registar.
- O push precisa de uma conta Apple Developer (paga) e de um certificado ou chave de APNs: **sem ela o toque
  no iPhone não se prova**.
- No Android: mensagem FCM de dados de **alta prioridade**, serviço em primeiro plano `phoneCall` e
  notificação de ecrã inteiro (Android 14).

## Medido neste PR (S-02, do lado do FreeSWITCH)

`scripts/ramais-push-espera-prova.py`, no laboratório com TLS nos ramais, simulando o telefone que acorda
tarde (o *wake* é um servidor de papel; o fornecedor de push não existe ainda). **6 em 6:**

| Cenário | Resultado |
|---|---|
| Espera desligada (0), destino sem registo | falha em 0,03 s com `USER_NOT_REGISTERED`, sem pedir *wake* |
| `awaiting:false` | falha em 0,03 s, pediu o *wake* uma vez |
| Aparelho que nunca acorda, limite de 4 s | `NO_USER_RESPONSE` aos 4,05 s |
| Aparelho que se regista aos 5,0 s | um *wake* (ramal certo, segredo de voz presente); o INVITE chegou **15 ms depois do registo** |
| Esse aparelho recusa com 486 | a chamada seguiu para ele (`USER_BUSY` a quem liga) |

## Medido com o servidor REAL (S-01 + S-02), no laboratório

`scripts/ramais-push-espera-prova.py --real`, com o compose levantado por
`make compose-up LAN_IP=… PUSH_LAB_URL=http://…:18890/push`. Uma pessoa de prova com ramal e sessão; o aparelho
`lab` regista-se pela API; o receptor `lab` (esta máquina) faz de «app que acorda» e regista o ramal por TLS 3 s
depois do pedido. **6 em 6:**

| Cenário | Resultado |
|---|---|
| Aparelho registado pela API real | 201, o token não volta na resposta |
| Chamada ao ramal sem registo | o servidor real pediu ao fornecedor o aparelho certo, sem o token; `caller='1902'` |
| O aparelho acorda e regista-se | o INVITE chegou **aos 3,0 s** da chamada |
| O aparelho recusa (486) | a chamada seguiu para ele (`USER_BUSY`) |
| Aparelho revogado | falha em 0,05 s, ninguém é acordado |
| Sessão do aparelho terminada | falha em 0,06 s, ninguém é acordado |

**Defeito apanhado por esta prova, e corrigido:** a primeira versão mandava ao telemóvel o utilizador SIP do
chamador (`ramal_ebec6d65…`), que é metade da credencial dele. Os testes Rust passavam porque eu lhes dava um
valor inventado. O servidor passou a traduzi-lo para o número curto (e a mandar vazio se o chamador não é ramal
desta organização), e há testes para os dois casos.

## Fornecedor `delonix` (delonix-push, open source)

Além do `lab`, o servidor fala com o **delonix-push** (serviço da N'GolaCloud, repo próprio e aberto): `POST
{PUSH_DELONIX_URL}/v1/messages` com a chave `PUSH_DELONIX_KEY`. O token do aparelho é o `device_id` que o serviço
devolveu; o serviço decide se entrega por ligação própria, FCM ou APNs. O pedido leva `priority:high`, TTL de 60 s,
`collapse_key` e `idempotency_key` por chamada, e só o número curto de quem liga (testado: o utilizador SIP não vai).
**Registo:** com o serviço configurado, um `PUT …/devices/{id}` NOVO com `provider:"delonix"` faz o Meet cunhar o
aparelho no delonix-push (`POST /v1/devices`) e devolver `delonix_push:{url, device_id, device_secret}` **uma só vez**
(o token enviado pela app é ignorado; uma renovação não volta a cunhar nem a mostrar o segredo). Desligar o aparelho
no Meet revoga-o também no serviço (melhor esforço). Se o serviço não responde, 422 `devices.push_unavailable`.
Medido contra um serviço de papel (13 testes de `voice_push_wake`); **falta** uma prova com o delonix-push real
ligado ao Meet, e revogar no serviço quando é a *sessão* que termina (hoje só o desligar explícito).

## O que isto NÃO prova

- **Nenhum push real**: nem FCM, nem APNs. O fornecedor `lab` fala com um receptor de papel; o caminho FreeSWITCH →
  servidor real foi exercitado (secção acima), mas só com o `lab`.
- **Que um telemóvel real acorda a tempo** (RNF-02: ≤ 4 a 5 s). Isso depende de FCM/APNs e do aparelho.
- **Outras entradas para o mesmo ramal:** a espera do FreeSWITCH está no `ramais_dial.lua` (chamadas entre
  ramais). As chamadas que o **servidor** origina por ESL (ligar a partir da sala) têm a sua espera no servidor
  (`PUSH_WAIT_SECS`, `dial_outs.rs`): acorda os aparelhos e só origina depois de o ramal se registar (provado com
  um ESL falso, 5 testes, e duas mutações apanhadas). O PSTN para um ramal (DID) passa a usar a mesma espera: o dialplan que o servidor serve em `dialplan-did` corre `ramais_dial.lua did <ramal> <domínio>` em vez de um `bridge` directo (testes Rust e sintaxe Lua verificados; **por provar num FreeSWITCH real**, o laboratório estava em baixo).
- **Credencial por aparelho (S-03):** ainda é uma só credencial por ramal; com dois telemóveis no mesmo
  ramal o primeiro a atender não cancela o outro (RF-35).
- O Lua corre a cada chamada a consultar `sofia_contact` de 500 em 500 ms durante a espera: custo medido só
  com uma chamada.

## Consequências

- O produto passa a poder, **por configuração**, manter uma chamada viva à espera do aparelho: um chamador
  espera até `DELONIX_PUSH_WAIT_SECS`. O valor é do operador; por omissão nada muda.
- O servidor ganha uma tabela, três rotas e um *trait* de fornecedores, com os portões do repo (isolamento
  por org, OpenAPI, cobertura de rotas) e um teste com **duas organizações**.
- Mantém-se a escolha de não pôr `usrloc` no Kamailio (ADR de ramais): o registo continua a ser do
  FreeSWITCH.
