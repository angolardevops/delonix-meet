# ADR-0005 — Backend enterprise: contextos de domínio, edições, gRPC interno e entrega

**Estado:** Aceite · **Data:** 2026-09-16 · **Sucede:** [ADR-0004](0004-organizacao-alvo-do-backend.md) §3 (só a lista de crates); o resto do ADR-0004 mantém-se.
**Pedido que o origina:** reestruturação do backend pedida pelo dono do produto a 2026-09-16:
«API REST e gRPC, SOLID/Clean Architecture/DDD, pronto para Kubernetes e `delonix-runtime`,
a funcionar como SaaS e on-premise, numa estrutura enterprise ou de um só utilizador,
preservando toda a funcionalidade já feita».

## Contexto

O ADR-0004 fixou as camadas (http → serviço → store), as superfícies de API e a ordem de
migração. Faltavam quatro decisões:

1. **Onde vive o domínio.** A lista de crates do ADR-0004 §3 separa por tecnologia
   (`store`, `media`, `api`), mas não diz onde ficam as regras de reuniões, gravações ou
   salas. Na prática, isso deixava-as nos handlers, que é o problema de partida.
2. **Edições.** Medido a 2026-09-16:
   - não há nenhuma noção de SaaS, on-premise ou utilizador único;
   - o registo é sempre aberto e obriga a criar uma organização (`auth.rs:249-325`).
   Uma instalação on-premise não consegue fechar o registo, e uma pessoa sozinha não
   consegue usar o produto sem inventar uma empresa.
3. **Como se consome o gRPC.** O ADR-0004 §4 diz onde entra. Falta dizer em que porta, com
   que autenticação e com que contrato.
4. **Entrega.**
   - O servidor não serve a UI (é o nginx que a serve).
   - Não há manifesto para o `delonix-runtime` nem para o PaaS NgolaCloud.
   - O Kubernetes é um Kustomize único, sem variantes por edição.

## Decisão

### 1. Contextos de domínio (DDD) e a regra da dependência (Clean Architecture)

O backend organiza-se em **contextos delimitados**. Cada um tem a sua linguagem, as suas
invariantes e os seus portos:

| Contexto | Agregados e conceitos | Vem de (hoje) |
|---|---|---|
| `identity` | User, Credential, Session, MfaFactor, ApiKey (com escopos), SsoConnection | `auth`, `users`, `mfa`, `apikeys` (CRUD), `odoo_sso` (login) |
| `organization` | Organization, Membership (activa/arquivada), Branch, Group, MeetingRoom, Quota, OrgSettings | `org`, `storage` (quota) |
| `scheduling` | Meeting, Invitation, Recurrence, AgendaItem, ActionPlan, Minutes, Quarantine | `meetings`, `meetings_v1`, `actions` |
| `conferencing` | Room, AdmissionPolicy, RoomToken, IceCredentials, ChatLog, CallQuality | `rooms`, parte de `signaling` |
| `content` | Recording (com estado), Share, ShareLink, Whiteboard, Transcript, StreamDestination | `recordings`, `whiteboards`, `recorder` (metadados) |
| `telephony` | Did, VoiceRoom, CallDetailRecord, Tariff | `voice` |
| `compliance` | AuditEntry (cadeia de hash), RetentionPolicy, DlpPolicy | `audit`, `dlp`, retenção |
| `integration` | Webhook, WebhookDelivery, OdooLink, StorageTarget, AiProvider | `webhooks`, `odoo`, `storage`, `ai` |

**A regra da dependência.** As setas apontam só para dentro. O domínio não conhece
axum, sqlx, tonic, reqwest, redis nem webrtc:

```
             ┌──────────────────────── delonix-meet-server (composição, arranque) ───────────────────────┐
             │                                                                                            │
   entrada:  delonix-meet-api (http: bff, v1, operator, integrations · grpc: interno)                     │
             │                                                                                            │
             ▼                                                                                            ▼
   núcleo:   delonix-meet-domain (contextos: entidades, objectos de valor, políticas, casos de uso, PORTOS)
             │                        ▲                                  ▲
             ▼                        │ implementa                        │ implementa
   base:     delonix-meet-core        delonix-meet-store (sqlx, migrações, tenant_tx)
             delonix-meet-protocol    delonix-meet-integrations (webhooks, odoo, oidc, armazenamento, ia, net_guard)
                                      delonix-meet-media (sfu, gravação, directo — o único com webrtc)
                                      delonix-meet-realtime (sinalização, presença, colaboração, redis)
```

| Crate | Pode depender de | Não pode depender de |
|---|---|---|
| `delonix-meet-core` | serde, uuid, chrono, thiserror, cripto pura | qualquer crate `delonix-meet-*`, IO |
| `delonix-meet-protocol` | core, serde, prost/tonic (só geração) | domain, store, api |
| `delonix-meet-domain` | core, async-trait | sqlx, axum, tonic, reqwest, redis, webrtc |
| `delonix-meet-store` | core, domain, sqlx | axum, tonic, webrtc |
| `delonix-meet-integrations` | core, domain, reqwest, openidconnect | axum, sqlx, webrtc |
| `delonix-meet-media` | core, protocol, webrtc | axum, sqlx (fala com o domínio por portos) |
| `delonix-meet-realtime` | core, protocol, domain, media, redis | axum (só o upgrade de WS entra pela api), sqlx |
| `delonix-meet-api` | todos os anteriores, axum, tonic, utoipa | server |
| `delonix-meet-server` | todos | — |

Um portão novo (`scripts/check-crate-deps.sh`) lê os `Cargo.toml` e falha se uma destas
linhas for violada. **Não se confia na disciplina:** confia-se no compilador e no portão.

**SOLID, no concreto:**
- Um caso de uso é uma função ou um tipo com UMA responsabilidade
  (`scheduling::CreateMeeting`), partilhado pela BFF, pela v1 e pelo gRPC (ADR-0004 §5 regra 8).
- Os portos são traits pequenos por agregado (`MeetingRepository`, `MembershipQuery`,
  `RecordingStorage`, `Clock`, `EventPublisher`). Nenhum repositório genérico «faz-tudo».
- A composição acontece num só sítio (`delonix-meet-server`). O `AppState` passa a ser
  um conjunto de serviços já montados, não um saco de ligações.

**Transição (padrão estrangulador).** O crate actual `delonix-server` entra no workspace
como membro transitório. O código sai dele para os crates acima, fatia a fatia, e cada
fatia compila e passa nos testes. Esta ordem só é possível porque um crate folha
(`core`, `protocol`) **não consegue** depender do monólito: o ciclo de 18 módulos do
ADR-0004 parte-se por construção, em vez de ter de estar partido antes.

### 2. Edições — SaaS, Enterprise on-premise e Pessoal

Um único binário, três perfis. O perfil fixa os **valores por omissão**, e cada política
pode ser sobreposta à parte:

| Política (`env`) | `saas` | `enterprise` | `personal` |
|---|---|---|---|
| `DELONIX_EDITION` | `saas` | `enterprise` | `personal` |
| `REGISTRATION_MODE` — `open` \| `domain` \| `invite` \| `closed` | `open` | `invite` | `closed` depois do primeiro utilizador |
| `TENANCY_MODE` — `multi` \| `single` | `multi` | `single` (uma org, criada no arranque) | `single` (org pessoal implícita) |
| Superfície de operador `/api/operator/v1` | activa | activa | desligada |
| UI servida pelo próprio binário | não (CDN/nginx) | opcional | sim |
| Redis | obrigatório com mais de 1 réplica | opcional | não |

- **Preservação.** Sem `DELONIX_EDITION`, o comportamento é exactamente o de hoje:
  registo aberto e uma org por domínio de email. Isto é o perfil `saas`. Nenhuma
  instalação existente muda por actualizar o binário.
- **Edição pessoal.** O primeiro registo cria o utilizador e uma org pessoal com
  `kind = personal`. O frontend não mostra a administração de empresa, porque o
  `GET /api/public/settings` publica `edition` e `capabilities`.
- **As edições não são licenciamento.** São perfis de configuração, e nenhuma
  funcionalidade fica fechada no código por edição. Uma capacidade anunciada em
  `capabilities` tem código por trás (`check-capability-claims.sh`).

### 3. API — REST para pessoas e browsers, gRPC entre máquinas

Mantém-se o ADR-0004 §4. Acrescenta-se:

**REST (axum).**
- **OpenAPI 3.1 gerado com `utoipa`**, para a BFF e para a v1:
  - dois documentos, `/api/openapi.json` (BFF, marcado *unstable*) e
    `/api/v1/openapi.json` (estável);
  - o spec commitado em `docs/reference/openapi/`, com um portão que exige
    spec gerado = spec commitado;
  - o cliente TypeScript do web **gera-se** a partir deste spec. É assim que a UI nova
    deixa de escrever tipos à mão.
- **Envelope de erro com código estável**, em todas as superfícies (também na BFF),
  **plano**:
  `{"error": "<mensagem>", "code": "meeting.host_not_found", "details": [...], "request_id": "…"}`.
  - Porquê plano e não `{"error": {"code": …}}` (a forma do ADR-0004 §4): o
    `web/src/api.ts` e o módulo Odoo lêem `body.error` como texto, e a v1 só
    quebra com v2. O plano acrescenta sem remover.
  - Nasce uma só vez (`ApiError` a partir do `DomainError` do `core`). As
    recusas dos extractores do axum e o 404/405 de rota passam pelo mesmo
    envelope (`error::normalize_error_body`).
  - `request_id` é o `X-Request-Id` (aceite do proxy se for seguro, gerado
    se não), igual no cabeçalho, no span de log e no corpo.
- **Compatibilidade da BFF.** Uma rota da BFF que muda de forma (por exemplo,
  `POST /orgs/{id}/settings` → `PATCH`):
  - ganha a forma nova;
  - mantém a antiga como alias, com o cabeçalho `Deprecation`;
  - o alias só sai quando o web deixar de o chamar, medido por `grep` no `web/src`.

**gRPC (tonic).**
- Porta própria (`GRPC_BIND_ADDR`, por omissão desligada), **sem ingress**.
- mTLS obrigatório (`GRPC_TLS_CERT`, `GRPC_TLS_KEY`, `GRPC_CLIENT_CA`). Texto claro só
  com `DELONIX_ALLOW_INSECURE=1`.
- Pacotes versionados em `server/proto/delonix/meet/<serviço>/v1/`. `buf lint` e
  `buf breaking` no CI.
- Serviços:
  - `delonix.meet.telephony.v1.IvrService`: `ValidatePin` e `RecordCdr`. O FreeSWITCH
    chama-o hoje por HTTP a partir de Lua, que não fala gRPC, por isso as rotas
    `/api/voice/ivr/*` **passam para o listener interno**, fora do router público, até
    haver um sidecar gRPC. O objectivo de segurança (sair da árvore pública) cumpre-se
    já; o transporte gRPC fica disponível para o sidecar.
  - `delonix.meet.transcription.v1.TranscriptionService`: `ClaimJob`,
    `CompleteJob` e `ReportProgress`. **Substitui o `ai-worker` a escrever directamente
    no Postgres** (hoje a app Python faz polling e `UPDATE` às tabelas: é uma fronteira
    violada, porque as regras de DLP e de auditoria são contornadas).
  - `grpc.health.v1.Health` e reflection, para as sondas e o `grpcurl`.
- **Não há gRPC para o browser.** O pedido original falava em «REST e gRPC para
  comunicação com o frontend». A resposta a essa parte é o ADR-0004 §4, que se mantém:
  - o gRPC-Web obriga a um proxy (Envoy) na frente de cada instalação, incluindo a
    pessoal;
  - não transporta o WebSocket da sala nem o WebRTC;
  - não ganha nada face a REST com OpenAPI e cliente gerado.

  Reabre-se com um ADR sucessor que meça um ganho concreto.

### 4. Entrega

| Alvo | Artefacto | Edições |
|---|---|---|
| Kubernetes | base `deploy/k8s/` (inalterada) + overlays `deploy/k8s-overlays/{saas,enterprise}` — fora da base porque o Kustomize recusa um overlay dentro da própria base («cycle detected»): portas interna e gRPC num `Service` ClusterIP sem ingress, `NetworkPolicy`, mTLS por cert-manager, `startupProbe`; portão `scripts/check-k8s-render.sh` | saas, enterprise |
| `delonix-runtime` (um host) | `deploy/delonix/meet-stack.yaml` (`kind: Stack`), aplicado com `delonix apply -f` | enterprise, personal |
| PaaS NgolaCloud | `deploy/delonix/meet-application.yaml` (`kind: Application`), aplicado com `delonixctl apply -f` (CLAUDE.md §1: carga acima do PaaS, nunca Ansible) | saas |
| Binário único | feature `embedded-ui` (a UI de `web/dist` embebida), ou `UI_DIR` em runtime | personal, enterprise pequeno |

Invariantes que não se negoceiam:

- **O ADR-0001 mantém-se.** O `/ws` continua com afinidade por sala, num `Service`
  dedicado, com `scripts/check-room-affinity.sh`.
- **O drain mantém-se.** `/ready` em 503, aviso aos participantes, espera das salas.
- **Doze factores do lado de quem fornece:**
  - configuração só por ambiente, nunca ficheiros montados obrigatórios;
  - migrações no arranque, com `DELONIX_MIGRATE=0` para as correr num `Job` separado
    em SaaS;
  - logs em JSON em stdout (`LOG_FORMAT=json`);
  - nenhum estado em disco local, excepto as gravações, que vão para o porto
    `RecordingStorage`.

### 5. O que sai

- `server/src/mls.rs`: um rascunho não montado, com `#![allow(dead_code)]` (ADR-0004 §3
  já mandava apagar ou pôr atrás de ADR). Volta com um ADR de E2EE por MLS.
- **Frontend antigo: NÃO sai por este trabalho.**
  - O `web/` está a ser reconstruído no mesmo sítio por outra sessão
    (`frontend/ui-*`, commit `af2ee92`).
  - Um ramo de backend que apagasse `web/` destruía esse trabalho no merge.
  - Aqui sai apenas o que é frontend servido *pelo* backend, e hoje não há nenhum.

## Ordem

Estende o ADR-0004 §6. Cada linha é um PR que deixa a `main` verde:

| # | Entrega | Portão |
|---|---|---|
| A | `lib.rs`; testes de integração HTTP contra Postgres real; job no CI; workspace com `core`; `check-crate-deps.sh` | `cargo test --workspace` com Postgres |
| B | Extrair `crypto`, `auth::extract`, `membership`, `net_guard`, `protocol` e o envelope de erro | catraca desce; grafo sem ciclo nos crates novos |
| C | Contrato: OpenAPI BFF + v1, paginação, 201/204, separação operador/integração com aliases | spec gerado = commitado; `isolamento.mjs` |
| D | Domínio + store por contexto (identity → organization → scheduling → content → conferencing → telephony) | testes de integração por contexto |
| E | Edições, UI embebida, gRPC interno, manifestos K8s/`delonix-runtime` | `delonix apply --dry-run`; `kubectl kustomize`; `grpcurl` |
| F | Capacidades novas pedidas pela UI nova (ver `docs/backend-gaps-ui-2026-09-16.md`) | e2e por capacidade |
| G | `media` e `realtime` em crates, sem tocar na lógica (as regressões R1…R40 mandam) | `sfu_e2e` + `reuniao.mjs` |

## Consequências

- **+** Uma regra tem um sítio. A BFF, a v1 e o gRPC chamam o mesmo caso de uso.
- **+** O domínio testa-se sem base de dados, e os adaptadores testam-se contra Postgres real.
- **+** Uma instalação on-premise fecha o registo e uma pessoa usa o produto sem empresa,
  sem um fork.
- **+** O `ai-worker` deixa de escrever na base por baixo das regras.
- **−** Mais crates, mais `Cargo.toml` e mais portão para manter.
- **−** Durante a transição coexistem o monólito e os crates novos. O risco é ficar a meio:
  a catraca e o `check-crate-deps.sh` medem o progresso, e nenhuma fatia nova volta a
  entrar no monólito.
- **−** Os aliases de compatibilidade da BFF são dívida com prazo. Cada um tem o seu
  `grep` de saída.
