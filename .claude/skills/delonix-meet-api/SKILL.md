---
name: delonix-meet-api
description: Contrato de API do Delonix Meet — as superfícies (BFF `/api`, pública `/api/v1`, operador, integração Odoo, tempo real, interna gRPC), a checklist de uma rota nova, códigos de estado, envelope de erro, paginação por cursor, idempotência, OpenAPI gerado, e ONDE gRPC entra e onde não entra. Usa-a quando fores criar, alterar ou rever uma rota em `server/src/main.rs`, mexer na `/api/v1`, falar de SDK/mobile/integração, OpenAPI, contrato, «REST», «gRPC», «protobuf», «status code», «paginação». NÃO a uses para a organização interna do código Rust (isso é `delonix-meet-backend`) nem para as mensagens do WebSocket da sala (regressões em `docs/reference/regressions.md`).
---

# Contrato de API do Delonix Meet

**Autoridade:** [ADR-0004 §4](../../../docs/adr/0004-organizacao-alvo-do-backend.md) (Proposto) e [`docs/reference/api-contract.md`](../../../docs/reference/api-contract.md).
**Evidência:** [auditoria de 2026-09-16 §2.4](../../../docs/auditoria-2026-09-16-backend.md).

## O estado real (2026-09-16, ramo `integra/backend-enterprise`)

- **As rotas estão em `server/src/lib.rs`** (`build_router`, `internal_routes`). O router de
  `mls.rs` não está montado.
- **O que está bem:**
  - BFF (`/api/…`) vs pública (`/api/v1`) escrita e medida; `check-route-auth.sh` (que desde
    o R123 também vê os handlers encadeados) e `check-isolamento-cobertura.sh`.
  - **OpenAPI gerado**, 158/158, catraca a zero: rota nova sem `#[utoipa::path]` falha.
  - **Envelope de erro** plano com `code` estável e `request_id` (ADR-0006 §3), também nas
    recusas do axum.
  - Rotas novas já seguem o contrato: `201`+`Location`, `204`, `202`, paginação por cursor
    (`delonix_meet_core::page`) — ver `stream_destinations.rs` como referência.
  - **gRPC interno** (`server/proto`, `buf lint`/`breaking` em `check-proto.sh`).
- **O que falta:**
  - As rotas HERDADAS mantêm `200`/`{"ok":true}` (22) e listagens sem limite.
  - `Idempotency-Key` só no SMS; nada de `ETag`.
- **Uma superfície por público, sem aliases** (reorganização de 2026-09-16, mapa em
  `docs/reference/api-routes.md`): BFF `/api`, inquilino `/api/v1`, operador
  `/api/operator/v1`, integrações `/api/integrations/{odoo,sms-agent}/v1`, interna
  `/internal/v1`. Quatro specs: `docs/reference/openapi/{bff,v1,operator,integrations}.json`.

## Superfícies — um público e uma autenticação cada

| Superfície | Prefixo | Auth | Spec |
|---|---|---|---|
| BFF do web | `/api/…` | sessão | `docs/reference/openapi/bff.json` |
| Pública do inquilino | `/api/v1/…` | **só** chave `dlx_` com escopos (`ApiKeyAuth`) | `v1.json` |
| Operador | `/api/operator/v1/…` | `PLATFORM_ADMIN_USER_IDS` ou segredo de plataforma | `operator.json` |
| Integrações | `/api/integrations/odoo/v1/…`, `/api/integrations/sms-agent/v1/…` | `dlxo_` / `dlxg_` | `integrations.json` |
| Tempo real | `/ws`, `/rtc`, `/api/rooms/{room_code}/live` | token de sala / access token | — (`protocol`) |
| Máquina-a-máquina | `/internal/v1/voice/ivr/*` (listener interno) e **gRPC** (porta sem ingress, mTLS) | segredo / mTLS | `.proto` |

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

   **`POST` para actualizar é recusado.** O `POST /orgs/{id}/settings` é dívida, não é
   modelo.
5. **Recurso completo.** Se há `PATCH`/`DELETE` em `/x/{id}`, há `GET /x/{id}`.
6. **Códigos de estado** — em código novo, «200 para tudo» não entra:

   | Situação | Resposta |
   |---|---|
   | Criou | `201 Created` + `Location` + o recurso |
   | Apagou | `204 No Content` (e o apagar do que não existe dá `404`, não `{"ok":true}`) |
   | Trabalho assíncrono | `202 Accepted` + a operação a consultar |
   | Forma inválida / regra violada | `400` / `422` |
   | Conflito de estado ou unicidade | `409` |
   | Não autenticado / sem permissão | `401` / `403`. Um recurso de outra org é `404` — não se confirma que existe |
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
   `next_page_token`. **Nenhuma listagem nova sem limite, e nenhum limite silencioso.**
   Numa sincronização (`since`), o cursor é obrigatório: um corte aos 500 perde registos.
9. **Idempotência (v1):** `Idempotency-Key` em todo o `POST` que cria. `ETag` +
   `If-Match` em `PATCH`. Um `external_ref` no corpo é idempotência de domínio, não
   substitui o cabeçalho.
10. **JSON:** campos em `snake_case`; datas RFC 3339 em UTC; ids como string.
11. **Isolamento:** a rota de org entra em `web/e2e/isolamento.mjs` com o caso negativo
    (org A não alcança o recurso da org B). O `check-isolamento-cobertura.sh` falha sem ele.
12. **Documentação:** na v1, o contrato fica no `api-contract.md` até o OpenAPI existir;
    depois, `#[utoipa::path]` no handler e o spec gerado passa no portão.

## gRPC — onde entra e onde não entra

**Entra só entre máquinas nossas** (ADR-0004 §4):

| Fronteira | Porquê gRPC |
|---|---|
| FreeSWITCH/IVR ↔ servidor (hoje `/internal/v1/voice/ivr/{validate,cdr}`) | contrato tipado, baixa latência, sai da árvore pública |
| `ai-worker`/`whisper-server` ↔ servidor | streaming bidireccional de áudio e texto, com contra-pressão e prazos |
| Nó ↔ nó | **só** com evidência escrita do que o Redis pub/sub do ADR-0001 não resolve |

**Não entra:**
- **Browser → servidor.** REST + WS + WebRTC; gRPC-Web obriga a um proxy e não ganha nada.
- **A v1 pública.** Integradores e o SDK esperam REST/JSON.
- **O Odoo.** É Python e fala HTTP.

Quem propuser «gRPC completo em todo o backend» leva esta tabela como resposta.

**Quando se implementar:**
- `tonic` + `prost`, com os `.proto` em `server/proto/delonix/meet/<serviço>/v1/*.proto`
  (pacote versionado).
- Uma porta própria sem ingress e mTLS.
- `buf lint` e `buf breaking` no CI contra a `origin/main`.
- Os serviços chamam as mesmas funções de serviço que o HTTP — **nunca** uma segunda
  implementação da regra.
- Proto sem campos reutilizados: um campo removido fica `reserved`.

## Dívida conhecida da superfície (não copiar como modelo)

A reorganização de 2026-09-16 (sem aliases, `docs/reference/api-routes.md`) tirou a dívida
de NOMES e de superfícies: operador fora da v1, itens debaixo do pai, `PUT` para
singletons, `GET` onde havia `PATCH`/`DELETE`. O que continua:

- **Listagens herdadas sem cursor:** `v1/recordings` com `LIMIT 200` fixo,
  `v1/meetings?since=` com corte aos 500, `meetings::list` e `recordings::library` sem
  limite (a biblioteca só pagina com `page_size`/`q`).
- **Respostas herdadas** `{"ok": true}` (catraca `respostas_ok_true`) e `200` onde devia ser
  `201`/`204` — cada uma sai quando o handler for tocado, com o teste ao lado.
- **Chaves de API:** R170/R171 fechados. Rota v1 nova com chave: um `Scope` do catálogo,
  `key.require(…)?` na primeira linha, e uma linha em `tests/api_key_scopes.rs::routes`.

## Portões

```bash
bash scripts/check-route-auth.sh
bash scripts/check-isolamento-cobertura.sh
bash scripts/check-arquitectura-catraca.sh
node web/e2e/isolamento.mjs     # contra servidor e Postgres reais — ver o job `isolamento` do CI
```

Uma rota nova só está pronta com os quatro verdes na árvore de integração. O último
corre contra infraestrutura real: «não corri» diz-se no relatório.
