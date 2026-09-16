---
name: delonix-meet-api
description: Contrato de API do Delonix Meet — as superfícies (BFF `/api`, pública `/api/v1`, operador, integração Odoo, tempo real, interna gRPC), a checklist de uma rota nova, códigos de estado, envelope de erro, paginação por cursor, idempotência, OpenAPI gerado, e ONDE gRPC entra e onde não entra. Usa-a quando fores criar, alterar ou rever uma rota em `server/src/main.rs`, mexer na `/api/v1`, falar de SDK/mobile/integração, OpenAPI, contrato, «REST», «gRPC», «protobuf», «status code», «paginação». NÃO a uses para a organização interna do código Rust (isso é `delonix-meet-backend`) nem para as mensagens do WebSocket da sala (regressões em `docs/reference/regressions.md`).
---

# Contrato de API do Delonix Meet

**Autoridade:** [ADR-0004 §4](../../../docs/adr/0004-organizacao-alvo-do-backend.md) (Proposto) e [`docs/reference/api-contract.md`](../../../docs/reference/api-contract.md).
**Evidência:** [auditoria de 2026-09-16 §2.4](../../../docs/auditoria-2026-09-16-backend.md).

## O estado real (2026-09-16)

- **105 `.route(`**, todas em `server/src/main.rs`. O router de `mls.rs` não está montado.
- **O que está bem:**
  - A fronteira BFF (`/api/…`, instável) vs pública (`/api/v1`, estável) está escrita no
    `main.rs` e no `api-contract.md`.
  - O `check-route-auth.sh` garante que cada rota tem autenticação ou está em
    `scripts/rotas-publicas.txt` com razão.
  - O `check-isolamento-cobertura.sh` garante que cada rota de org é exercitada por
    `web/e2e/isolamento.mjs`.
- **O que falta:**
  - Não há OpenAPI, testes de contrato da v1, `201`/`204`/`202`, código de erro estável,
    paginação nem `Idempotency-Key`.
  - Não há **gRPC em lado nenhum**.
- **A v1 mistura três públicos:** o inquilino (`dlx_`), o Odoo (`dlxo_`, em
  `/integration/odoo/*`) e o operador (`/admin/orgs` com segredo de plataforma,
  `/platform/storage*` com sessão).

## Superfícies — um público e uma autenticação cada

| Superfície | Hoje | Destino | Auth |
|---|---|---|---|
| BFF do web | `/api/…` | igual | sessão |
| Pública do inquilino | `/api/v1/…` | igual, **só** chave `dlx_` com escopos | `ApiKeyAuth` |
| Operador | misturado na v1 | `/api/operator/v1/…` | identidade de operador explícita na config |
| Integração Odoo | `/api/v1/integration/odoo/*` | `/api/integrations/odoo/v1/…` | `OdooTokenAuth` |
| Tempo real | `/ws`, `/rtc`, `/api/rooms/{code}/broadcast` | igual | token de sala / access token |
| Máquina-a-máquina | `/api/voice/ivr/*` (HTTP público com segredo) | **gRPC**, porta sem ingress, mTLS | mTLS |

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

7. **Erro** — envelope PLANO em todas as superfícies (ADR-0005 §3):
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
| FreeSWITCH/IVR ↔ servidor (hoje `/api/voice/ivr/{validate,cdr}`) | contrato tipado, baixa latência, sai da árvore pública |
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

- **Recursos incompletos e sem limite:**
  - `DELETE /api/meetings/{id}` sem `GET`, e na v1 `PATCH`/`DELETE` sem `GET`;
  - `v1/recordings` com `LIMIT 200` fixo, `v1/meetings?since=` com corte aos 500;
    `meetings::list` e `recordings::library` sem limite nenhum.
- **Semântica das rotas da BFF:**
  - `POST /api/orgs/{id}/settings` a fazer update;
  - `POST /api/meetings/{id}/minutes` a fazer upsert;
  - `/api/recordings/{id}/share` no singular para uma colecção;
  - `/api/whiteboards/{id}/share` e `/api/whiteboards/shared/{token}` para a mesma coisa;
  - `/api/action-items/{id}`, `/api/quarantine/analytics` e `/api/missed-calls/ack` fora
    da hierarquia;
  - `/api/rooms/{code}/minutes` duplica `/api/meetings/{id}/minutes`.
- **Na v1:**
  - `/admin/orgs` e `/platform/storage*` na superfície do inquilino;
  - chaves sem escopos;
  - `revoke` responde `{"ok":true}` mesmo sem a chave existir;
  - rate-limit por IP e não por chave.

## Portões

```bash
bash scripts/check-route-auth.sh
bash scripts/check-isolamento-cobertura.sh
bash scripts/check-arquitectura-catraca.sh
node web/e2e/isolamento.mjs     # contra servidor e Postgres reais — ver o job `isolamento` do CI
```

Uma rota nova só está pronta com os quatro verdes na árvore de integração. O último
corre contra infraestrutura real: «não corri» diz-se no relatório.
