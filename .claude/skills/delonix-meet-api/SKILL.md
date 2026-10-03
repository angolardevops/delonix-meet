---
name: delonix-meet-api
description: >-
  Contrato de API do Delonix Meet — as superfícies (BFF `/api`, pública `/api/v1`,
  operador, integrações, tempo real, interna HTTP e gRPC), a checklist de uma rota
  nova, códigos de estado, envelope de erro, paginação por cursor, idempotência,
  OpenAPI gerado, e ONDE gRPC entra e onde não entra.
when_to_use: >-
  Quando fores criar, alterar ou rever uma rota em `server/src/lib.rs`
  (`build_router`, `internal_routes`), mexer na `/api/v1`, falar de
  SDK/mobile/integração, OpenAPI, contrato, «REST», «gRPC», «protobuf», «status
  code», «paginação». NÃO a uses para a organização interna do código Rust
  (`delonix-meet-backend`) nem para as mensagens do WebSocket da sala (regressões em
  `docs/reference/regressions.md`).
---

# Contrato de API do Delonix Meet

**Autoridade:** [ADR-0004 §4](../../../docs/adr/0004-organizacao-alvo-do-backend.md) e
[ADR-0006 §3](../../../docs/adr/0006-backend-enterprise-contextos-edicoes-e-entrega.md)
(ambos **Aceites**), e [`docs/reference/api-contract.md`](../../../docs/reference/api-contract.md).
**Mapa das rotas:** [`docs/reference/api-routes.md`](../../../docs/reference/api-routes.md).
**Evidência:** [auditoria de 2026-09-16 §2.4](../../../docs/auditoria-2026-09-16-backend.md).

## Fronteira

- **`delonix-meet-backend`** — onde o handler e a função de serviço vivem, que helper se
  chama, a catraca. Esta skill diz a **forma** da rota; aquela diz onde fica o código.
- **`delonix-meet-telefonia`** — o que as rotas `/telephony` e `/internal/v1/telephony`
  **fazem** (troncos, plano de marcação, CDR) e o contrato `room_bridge` com o IVR. Aqui
  só se revê a forma delas.
- **`delonix-meet`** — encaminhamento e a lista completa de portões.
- **Segurança de uma rota** (quem é, o que pode, de quem é a conta): revisor
  `delonix-meet-security`, com o estado em `delonix-meet-backend` §Segurança.
- **Mensagens do WebSocket da sala** não são desta skill: não há skill para elas, manda
  o catálogo de regressões e o revisor `delonix-meet-webrtc`.

## O estado real (2026-10-03, `main` `024583a`)

- **As rotas estão em `server/src/lib.rs`**: `internal_routes` (`:199`) e `build_router`
  (`:232`). O `main.rs` só chama `run()`. O router de `mls.rs` não está montado
  (`lib.rs:344`).
- **O que está bem:**
  - **OpenAPI 3.1 gerado e commitado**, 278/278 operações com `#[utoipa::path]` e catraca
    a zero (`scripts/openapi-baseline.txt`). Quatro specs em `docs/reference/openapi/`:
    `bff.json` (254), `v1.json` (12), `operator.json` (7), `integrations.json` (5).
  - **Uma superfície por público, sem aliases** (reorganização de 2026-09-16).
    `check-route-auth.sh` (que desde a R123 também vê os handlers encadeados) e
    `check-isolamento-cobertura.sh` guardam-na.
  - **Envelope de erro** plano com `code` estável e `request_id` (ADR-0006 §3), também nas
    recusas do axum.
  - Rotas novas seguem o contrato: `201`+`Location`, `204`, `202`, paginação por cursor
    (`delonix_meet_core::page`) — ver `stream_destinations.rs` como referência.
  - **gRPC interno** em serviço (`server/src/grpc.rs`), com `buf lint`/`breaking` em
    `check-proto.sh`.
- **O que falta:**
  - Listagens herdadas sem cursor (ver §Dívida).
  - `Idempotency-Key` só no SMS (`sms.rs:1083`; a telefonia reutiliza-o pela porta
    `SmsGateways`, não tem cabeçalho próprio); nada de `ETag`/`If-Match` em lado nenhum.

## Superfícies — um público e uma autenticação cada

| Superfície | Prefixo | Auth | Spec |
|---|---|---|---|
| BFF do web | `/api/…` | sessão | `docs/reference/openapi/bff.json` |
| Pública do inquilino | `/api/v1/…` | **só** chave `dlx_` com escopos (`ApiKeyAuth`) | `v1.json` |
| Operador | `/api/operator/v1/…` | `PLATFORM_ADMIN_USER_IDS` ou segredo de plataforma | `operator.json` |
| Integrações | `/api/integrations/odoo/v1/…`, `/api/integrations/sms-agent/v1/…` | `dlxo_` / `dlxg_` | `integrations.json` |
| Tempo real | `/ws`, `/rtc`, `/api/rooms/{room_code}/live` | token de sala / access token | — (`protocol`) |
| Máquina-a-máquina | `/internal/v1/voice/ivr/{validate,cdr}` e `/internal/v1/telephony/{call-records,freeswitch-config}` no listener interno (`INTERNAL_BIND_ADDR`), e **gRPC** (`GRPC_BIND_ADDR`, sem ingress, mTLS) | segredo / mTLS | `.proto` |

**Um endpoint novo é da BFF por omissão.** Só entra na v1 por promoção consciente, com
um consumidor externo real. **Nunca** se monta uma rota de operador ou de integração
dentro de `/api/v1` — a catraca conta `rotas_v1_com_sessao`.

## Checklist de uma rota nova

1. **Superfície e autenticação.** Escolhe pela tabela acima, e só um extractor. Se for
   pública, entra em `scripts/rotas-publicas.txt` com a razão.
2. **Recurso, não verbo.** Usa um substantivo no plural e hierárquico:
   `/api/orgs/{org_id}/webhooks/{hook_id}`.
   - Uma acção que não é CRUD é um *custom method* nomeado (`POST /meetings/{id}/ring`),
     e documenta-se como tal.
   - Um sub-recurso fica debaixo do pai (não há `/api/action-items/{id}` solto).
3. **Identificador coerente.** A sala é `{code}`; tudo o resto é `{id}` UUID. Não se
   inventa um terceiro.
4. **Verbos:**
   - `GET` lê e não tem efeitos;
   - `POST` cria ou executa um *custom method*;
   - `PUT` substitui um singleton;
   - `PATCH` altera parcialmente;
   - `DELETE` apaga.

   **`POST` para actualizar é recusado.** O exemplo histórico, `POST /orgs/{id}/settings`,
   já é `PATCH /api/orgs/{org_id}` (`lib.rs:605`).
5. **Recurso completo.** Se há `PATCH`/`DELETE` em `/x/{id}`, há `GET /x/{id}`.
6. **Códigos de estado** — em código novo, «200 para tudo» não entra:

   | Situação | Resposta |
   |---|---|
   | Criou | `201 Created` + `Location` + o recurso |
   | Apagou | `204 No Content` (e o apagar do que não existe NESTA organização dá `404`) |
   | Trabalho assíncrono | `202 Accepted` + a operação a consultar |
   | `PUT` de configuração | devolve o recurso como o `GET` |
   | Telemetria / contagens | `204` / `{"updated": n}` — nunca `{"ok": true}` (R181, catraca `respostas_ok_true=0`) |
   | Forma inválida / regra violada | `400` / `422` |
   | Conflito de estado ou unicidade | `409` |
   | Não autenticado | `401` — só para sessão em falta ou inválida |
   | Sem permissão | `404` a quem não chega ao recurso (não se confirma que existe); `403` com `code` a quem chega mas não pode — nunca `401` (R153) |
   | Rate-limit | `429` + `Retry-After` |

7. **Erro** — envelope PLANO em todas as superfícies (ADR-0006 §3):
   ```json
   {"error": "…", "code": "meeting.host_not_found", "details": [], "request_id": "…"}
   ```
   O `code` é estável e é parte do contrato; o `error` é a mensagem para humanos e pode
   mudar (fica plano porque o web e o Odoo lêem `body.error` como texto). Código novo
   devolve `ApiError::Domain(DomainError::…("contexto.razao", …))`; as variantes
   genéricas de `ApiError` ficam para o código herdado. **Não inventes um segundo
   formato num handler.**
8. **Listagens:** `page_size` (por omissão 50, máximo 100) + `page_token` opaco →
   `next_page_token` (`server/crates/delonix-meet-core/src/page.rs`). **Nenhuma listagem
   nova sem limite, e nenhum limite silencioso.** Numa sincronização (`since`), o cursor
   é obrigatório: um corte aos 500 perde registos.
9. **Idempotência (v1):** `Idempotency-Key` em todo o `POST` que cria. `ETag` +
   `If-Match` em `PATCH`. Um `external_ref` no corpo é idempotência de domínio, não
   substitui o cabeçalho.
10. **JSON:** campos em `snake_case`; datas RFC 3339 em UTC; ids como string.
11. **Isolamento:** a rota de org entra em `web/e2e/isolamento.mjs` com o caso negativo
    (org A não alcança o recurso da org B). O `check-isolamento-cobertura.sh` falha sem ele.
12. **Documentação:** `#[utoipa::path]` no handler, em qualquer das quatro superfícies; o
    spec regenera-se e commita-se, e o `check-openapi.sh` falha se o gerado diferir do
    commitado ou se uma rota montada não tiver anotação.
13. **Chave de API (v1):** um `Scope` do catálogo, `key.require(…)?` na primeira linha do
    handler, e uma linha em `tests/api_key_scopes.rs::routes` (R170/R171).

## gRPC — onde entra e onde não entra

**Entra só entre máquinas nossas** (ADR-0004 §4):

| Fronteira | Porquê gRPC | Estado a 2026-10-03 |
|---|---|---|
| FreeSWITCH/IVR ↔ servidor | contrato tipado, baixa latência, sai da árvore pública | `IvrService` (`ValidatePin`, `RecordCallDetail`) existe; o Lua do IVR continua a usar o HTTP `/internal/v1/voice/ivr/*` (`lib.rs:206-209`) |
| `ai-worker`/`whisper-server` ↔ servidor | trabalhos de transcrição com prazos | `TranscriptionService` (`ClaimJob`, `CompleteJob`, `FailJob`); o `ai-worker/job_source.py` é o cliente. Streaming de áudio: **não existe** |
| Nó ↔ nó | **só** com evidência escrita do que o Redis pub/sub do ADR-0001 não resolve | não existe |

**Não entra:**
- **Browser → servidor.** REST + WS + WebRTC; gRPC-Web obriga a um proxy e não ganha nada.
- **A v1 pública.** Integradores e o SDK esperam REST/JSON.
- **O Odoo.** É Python e fala HTTP.

Quem propuser «gRPC completo em todo o backend» leva esta tabela como resposta.

**Como está feito, e o que um `.proto` novo respeita:**
- `tonic` 0.14 + `prost`; os `.proto` em `server/proto/delonix/meet/<serviço>/v1/*.proto`
  (pacote versionado), gerados para o crate `delonix-meet-protocol`.
- Uma porta própria sem ingress (`GRPC_BIND_ADDR`; vazia = desligado) e mTLS. O
  `check-k8s-render.sh` confirma que as portas internas ficam fora do ingress.
- `buf lint` e `buf breaking` contra a `origin/main` (`scripts/check-proto.sh`).
- Os serviços chamam as mesmas funções que o HTTP (`voice::validate_pin`,
  `transcription::claim`) — **nunca** uma segunda implementação da regra. A tradução de
  erro é uma só: `grpc::status_from`.
- Proto sem campos reutilizados: um campo removido fica `reserved`.

## Dívida conhecida da superfície (não copiar como modelo)

A reorganização de 2026-09-16 tirou a dívida de NOMES e de superfícies: operador fora da
v1, itens debaixo do pai, `PUT` para singletons, `GET` onde havia `PATCH`/`DELETE`. O que
continua, medido a 2026-10-03:

- **Listagens herdadas sem cursor:** `v1/recordings` com `LIMIT 200` fixo
  (`apikeys.rs:606`), `v1/meetings?since=` com corte aos 500 (`apikeys.rs:697`),
  `voice/call-records` com `LIMIT 500` (`voice.rs:620`), `meetings::list`
  (`meetings.rs:746`) sem limite, e `recordings::library` (`recordings.rs:902`) que só
  pagina se o pedido trouxer `page_size`/`page_token` — sem eles devolve tudo
  (`recordings.rs:939`).
- **Três rotas de máquina na árvore pública:** `/api/voice/ivr/{directory,
  resolve-extension,dialplan-did}` (`lib.rs:834-841`), porque os Lua dos ramais já
  chamam esse caminho. Não é modelo: máquina-a-máquina novo vai para `/internal/v1`.
  Autenticam-se pelo segredo de voz no cabeçalho `X-Voice-Secret` ou em HTTP Basic;
  **nunca num `?secret=`** — o servidor deixou de o ler: não autentica (R227).
- **Idempotência e `ETag`:** ver §O que falta.

## Portões

```bash
bash scripts/check-route-auth.sh
bash scripts/check-isolamento-cobertura.sh
bash scripts/check-arquitectura-catraca.sh
bash scripts/check-openapi.sh   # spec gerado = commitado; rotas_sem_openapi não sobe
bash scripts/check-proto.sh     # só se tocaste em server/proto
node web/e2e/isolamento.mjs     # contra servidor e Postgres reais — ver o job `isolamento` do CI
```

Uma rota nova só está pronta com todos verdes na árvore de integração. O último corre
contra infraestrutura real: «não corri» diz-se no relatório.
