# ADR-0004 — Organização-alvo do backend: camadas, crates e superfícies de API

**Estado:** Aceite (2026-09-16, pedido do dono do produto) · §3 sucedido pelo [ADR-0006](0006-backend-enterprise-contextos-edicoes-e-entrega.md) · **Data:** 2026-09-16 · **Contexto:** [auditoria do backend de 2026-09-16](../auditoria-2026-09-16-backend.md)

> **Aceite a 2026-09-16.** O dono do produto pediu a reestruturação do backend
> (SOLID/Clean Architecture/DDD, REST + gRPC, K8s e `delonix-runtime`, SaaS e
> on-premise). A migração do §6 arranca. A lista de crates do §3 é refinada pelo
> [ADR-0006](0006-backend-enterprise-contextos-edicoes-e-entrega.md), que acrescenta
> o crate de domínio por contextos e as edições; o resto deste ADR mantém-se.

## Contexto

O backend é um crate único (`delonix-server`), com 34 módulos planos e 23 mil linhas.
A auditoria mediu quatro coisas que decidem o desenho:

1. **Um ciclo de 18 módulos.** Fecha-se sobretudo por utilitários e tipos no sítio
   errado (`signaling → apikeys::ct_eq`, `apikeys → odoo::gen_token_pub`,
   `pubsub → signaling::ServerMsg`). Com o ciclo, **dividir em crates não compila**.
2. **Não há camadas.** Os handlers fazem o SQL (302 queries de runtime), e as regras de
   negócio copiadas entre a BFF e a v1 **já divergiram**. Três das divergências são falhas
   de segurança (auditoria S1–S3).
3. **A `/api/v1` mistura três públicos e quatro autenticações**: inquilino, Odoo e operador.
4. **Não há gRPC nem OpenAPI.** Não há contrato verificável em nenhuma superfície.

Uma reescrita não resolve isto. A regra da casa é **descobrir, mapear, classificar,
comparar e planear ANTES de refactorizar**, e a auditoria é essa descoberta.

## Decisão

### 1. Camadas dentro de cada domínio

```
http (axum: extractors, DTOs, status)  →  service (regras, autorização)  →  store (sqlx)
```

- **Handler:** extrai, valida a forma, chama UM serviço e mapeia o resultado para HTTP.
  Não escreve SQL, não decide autorização por conta própria.
- **Serviço:** é a ÚNICA implementação de cada regra, partilhada pela BFF e pela v1.
  Exemplos: `meetings::service::create`, `users::provision_by_email`.
- **Store:** repositórios por agregado (`MeetingRepo`, `OrgRepo`…). Aqui mora o
  `tenant_tx` do [ADR-0002](0002-tenant-isolation-rls.md).

### 2. Utilitários com um dono único

| Capacidade | Dono | Substitui |
|---|---|---|
| sha256, tokens aleatórios, tempo constante, argon2 | `crypto` | 4 + 7 + 3 + 3 cópias |
| Leitura do `Bearer`, extractors `AuthUser` / `ApiKey` / `OdooToken` / `SharedSecret` / `RoomTokenQuery` | `auth::extract` | 3 + 2 + 3 cópias |
| Pertença à org (sempre com `archived_at IS NULL`) | `org::membership` | 29 linhas em 12 módulos |
| Pedido de saída para URL escolhido pelo cliente (SSRF, timeout, sem redirects) | `net_guard::outbound_client` | 4 clientes `reqwest` |
| Mapeamento de erro (`from_unique`, …) | `error` | 14 cópias |
| Mensagens de WebSocket (`ClientMsg`, `ServerMsg`, …) | `protocol` | tipos hoje dentro de `signaling` |

### 3. Workspace — o destino, com nomes semânticos

```
server/
├── Cargo.toml                       [workspace] + [workspace.dependencies]
└── crates/
    ├── delonix-meet-core/           erros, ids, config, crypto, rate_limit, metrics, dlp
    ├── delonix-meet-protocol/       mensagens WS (só serde) — contrato com o web
    ├── delonix-meet-store/          repositórios sqlx, tenant_tx, migrations/
    ├── delonix-meet-identity/       auth::extract, mfa, users, org, audit, api_keys
    ├── delonix-meet-integrations/   odoo, sso, webhooks, storage, ai, telephony, net_guard
    ├── delonix-meet-media/          sfu, recording, livestream   ← único com webrtc
    ├── delonix-meet-realtime/       signaling, collaboration, presence, pubsub
    ├── delonix-meet-api/            http: bff/, v1/, operator/, integrations/, AppState, router
    └── delonix-meet-server/         binário: main, tarefas de fundo, shutdown
```

As dependências formam um grafo sem ciclos, e a regra é que **só se depende para baixo**:

```
core ◄─ protocol
core ◄─ store
core, store ◄─ identity
core, store, identity ◄─ integrations
core, protocol ◄─ media
core, protocol, store, media ◄─ realtime
todos ◄─ api ◄─ server
```

Três dependências têm de ser invertidas:

| Hoje | Passa a ser |
|---|---|
| `recorder → webhooks/org` | trait `RecordingEvents` em `media`, implementado em `api` |
| `presence → rooms::insert_room` | a função desce para `store` |
| `auth::login → odoo_sso` | o fluxo de login sobe para `api` |

**Renomeações** (acontecem na mudança de crate, não antes):

| Hoje | Passa a |
|---|---|
| `apikeys.rs` | `api_keys` (CRUD) + `api/v1/*` |
| `meetings_v1.rs` | `api/v1/meetings` |
| `broadcast.rs` | `livestream` |
| `recorder.rs` | `media::recording` |
| `room_tools.rs` | `collaboration` |
| `actions.rs` | `agenda` + `action_plan` |
| `voice.rs` | `telephony` |
| `odoo.rs::public_settings` | `platform_settings` |
| funções `*_pub` | `pub(crate)` com o nome real |
| `mls.rs` | apagar, ou pôr atrás de uma feature com ADR |

### 4. Superfícies de API — um público e uma autenticação cada

| Superfície | Prefixo | Público | Auth | Contrato |
|---|---|---|---|---|
| BFF | `/api/…` | o web Delonix | sessão (JWT + refresh em cookie) | instável, muda com o web |
| Pública | `/api/v1/…` | SDK, mobile, integrações do inquilino | chave `dlx_` com **escopos** | estável, OpenAPI gerado e verificado no CI |
| Operador | `/api/operator/v1/…` | quem opera a plataforma | identidade de operador **explícita na config**, nunca derivada de `org_members` | estável, fora do SDK do inquilino |
| Integração Odoo | `/api/integrations/odoo/v1/…` | módulo `nk_delonix_meet` | token `dlxo_` | estável, com o consumidor |
| Tempo real | `/ws`, `/rtc`, `/api/rooms/{code}/broadcast` | browser | token de sala / access token | `delonix-meet-protocol` |
| **Interna máquina-a-máquina** | **gRPC** numa porta sem ingress, mTLS | voz/IVR, ai-worker/whisper | mTLS | `.proto` versionado |

**Onde gRPC entra:**

- **Voz/IVR ↔ servidor.** Substitui `/api/voice/ivr/*`, que sai da árvore pública.
- **ai-worker/whisper ↔ servidor.** Streaming bidireccional de áudio e transcrição, com
  contra-pressão e prazos.
- **Coordenação entre nós**, só com evidência escrita do que o Redis pub/sub
  ([ADR-0001](0001-room-shard-affinity.md)) não resolve.

**Onde gRPC NÃO entra:**

- **Browser → servidor.** Fica REST + WebSocket + WebRTC; gRPC-Web não compensa o proxy extra.
- **A v1 pública.** Quem integra espera REST/JSON.
- **O Odoo.** É Python e fala HTTP.

**Regras da v1:**

- **Estado HTTP:** `201` + `Location` ao criar, `204` ao apagar, `202` + operação para
  trabalho assíncrono, `409` e `422` com significado.
- **Envelope de erro:** `{"error":{"code":"meeting.host_not_found","message":…,"details":[…],"request_id":…}}`.
  O `code` é estável e a `message` é para humanos.
- **Paginação por cursor opaco:** `page_size` (máx. 100) + `page_token` → `next_page_token`.
  Nenhuma listagem sem limite, e nenhum limite silencioso.
- **Concorrência e repetição:** `Idempotency-Key` em todo o `POST` que cria; `ETag`/`If-Match` em `PATCH`.
- **Forma dos recursos:** completos (`GET /meetings/{id}` existe se existe `PATCH`).
  Acções que não são CRUD são *custom methods* nomeados e documentados como tal
  (`POST /meetings/{id}/ring`).
- **Rate-limit** por chave, com `Retry-After`.

### 5. Regras para código NOVO — valem já, mesmo com o ADR proposto

1. **Não se escreve `FROM org_members` fora de `org.rs`.** A pertença decide-se com
   `org::role_in_org` / `require_member_pub` / `require_admin_pub` (filtram `archived_at`).
2. **Não se lê `Authorization` à mão.** Usa-se um extractor que já existe, ou cria-se um em `auth.rs`.
3. **Não se cria cliente `reqwest`.** Usa-se o `state.outbound` do `net_guard` (timeout,
   sem redirects, guarda na resolução de DNS de cada ligação). Um URL que o cliente
   escolhe passa por `check_tenant_url` e vai pelo `tenant()`; um URL do operador por
   `check_operator_url` e `operator()`; o fluxo OIDC usa `outbound.oidc()`.
4. **Não se reimplementa cripto** (sha256, token, tempo constante, argon2): usa-se a
   função que existe; a próxima cópia tem de ser a extracção para `crypto`.
5. **Não se cria função `*_pub`.** Usa-se `pub(crate)`.
6. **Nenhuma rota nova responde `{"ok": true}`.** Usa-se `201`/`204` ou devolve-se o recurso.
7. **Nenhuma rota nova dentro de `/api/v1` usa `AuthUser`.** A sessão é da BFF.
8. **Uma regra usada pela BFF e pela v1 vive numa função partilhada**, nunca copiada.
9. **Identificadores novos em inglês**; comentários, documentação e mensagens ao
   utilizador em português (regra de fronteira de 2026-09-03). O código existente não
   se renomeia por isso — renomeia-se quando muda de crate (§3).
10. **Uma listagem nova tem limite e cursor.**

As regras 1–7 são contadas por `scripts/check-arquitectura-catraca.sh`: o número pode
descer e não pode subir. As 8–10 são de revisão (agentes `delonix-meet-architecture` e
`delonix-meet-api`).

### 6. Ordem de migração — cada passo deixa a árvore verde

| # | Passo | Porquê nesta ordem | Portão de saída |
|---|---|---|---|
| 0 | **Segurança S1–S3** — ✅ feito no #76 (R121) | falhas activas; não esperam por arquitectura | `isolamento.mjs` com os três casos negativos |
| 1 | `src/lib.rs` + `sfu_e2e` para `tests/` | abre testes de integração sem mexer em código | `cargo test` verde |
| 2 | Testes `#[sqlx::test]` dos handlers de org, meetings, recordings | a migração mecânica do SQL não se pode fazer às cegas | job com Postgres no CI |
| 3 | Extrair `crypto`, `auth::extract`, `org::membership`, `net_guard`, `protocol` | parte o ciclo de 18 módulos | grafo sem componente > 1 |
| 4 | Serviços: `users::provision_by_email`, `meetings::service` | junta as cópias divergentes | a catraca desce |
| 5 | Separar a v1 em inquilino / operador / Odoo; OpenAPI com `utoipa`; envelope de erro; paginação | contrato antes do SDK | spec gerado = spec commitado |
| 6 | Workspace (§3), crate a crate, a começar por `core` e `protocol` | só possível sem ciclo | `cargo build` por crate |
| 7 | gRPC para voz/IVR e transcrição | depende de `api` separado do `server` | `/api/voice/ivr/*` fora do ingress |

## Consequências

- **+** Uma regra passa a ter uma só implementação. As divergências de segurança
  deixam de poder reaparecer por cópia.
- **+** O `webrtc` fica isolado num crate: mexer num handler deixa de recompilar a media.
- **+** O SDK e o mobile nascem sobre um contrato verificado, não sobre rotas que mudam.
- **−** Partir o `AppState` toca em cerca de 231 sítios.
- **−** Sem os testes de base do passo 2, a migração do passo 3 pode partir queries em
  silêncio. Por isso o passo 2 vem antes, e não se salta.
- **−** Três routers e três autenticações na v1 são mais superfície para documentar. É
  o custo de não expor rotas de operador ao SDK do inquilino.
