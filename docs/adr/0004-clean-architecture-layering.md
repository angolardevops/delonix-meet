# ADR-0004 — Camadas de Clean Architecture nos módulos CRUD (domain/application/infrastructure/interface)

**Estado:** Aceite (rollout faseado) · **Data:** 2026-09-14 · **Contexto:** pedido de
redesenho do backend para separação limpa backend/frontend, SaaS ou on-premise

## Contexto

`server/src/` (21 569 linhas, 33 módulos) está hoje em **transaction script**: cada
ficheiro por feature mistura handler axum, SQL cru inline (`sqlx::query`/`query_as`,
sem verificação em compile-time), validação, autorização e chamadas de auditoria no
mesmo corpo de função. Medido contra o código real, não por opinião:

- **Structs de DB a fazer de DTO de API** (`#[derive(sqlx::FromRow, Serialize)]` no
  mesmo tipo) já causou um bug de produção real: o comentário em `meetings.rs:565`
  documenta que a migração 0022 partiu `start`/`ics` em silêncio porque a lista de
  colunas do `FromRow` é um contrato de *runtime*, não de compilação.
- **Validação duplicada e inconsistente**: password mínimo 8 em 3 sítios diferentes
  (`auth.rs` exige `8..=128`, `users.rs` só `>= 8` sem máximo, `org.rs` replica
  `8..=128` outra vez); email validado com regras diferentes em `auth.rs` vs `org.rs`.
- **Regras de negócio duplicadas por construção**: `meetings.rs` e `meetings_v1.rs`
  reimplementam criação/patch/ring de reuniões em paralelo (um para o BFF interno, um
  para a superfície pública `/api/v1`), com SQL e regras de autorização copiadas.
- **Zero abstração de storage real**: `recordings.rs` escreve directamente em
  `tokio::fs` num directório local; `storage.rs` (config NFS/WebDAV) nunca é chamado
  por `recordings.rs` — é só CRUD de configuração + teste de conectividade. É o maior
  gap concreto para o objetivo "SaaS ou on-premise": hoje trocar de backend de storage
  exige editar `recordings.rs` diretamente.
- **Um precedente bom já existe**: `org.rs::require_admin_pub`/`require_member_pub` é
  reutilizado por `meetings.rs` e `apikeys.rs` — centralizar regras funciona quando se
  faz.
- **A pilha de realtime é outra categoria de risco**: `signaling.rs` (2836 linhas) e
  `sfu.rs` (2181 linhas) têm 13+ regressões documentadas (`docs/reference/regressions.md`,
  R13–R60) presas a ordem exacta de locks/tasks/channels — não a regras de negócio.
  `sfu.rs` já não tem SQL nem acoplamento a `AppState`; já é quase um bounded context
  isolado por natureza.

## Decisão

Introduzir quatro camadas nos módulos CRUD sobre Postgres, preservando byte-a-byte o
contrato HTTP documentado em `docs/reference/api-contract.md` (BFF interna `/api/...`
vs pública `/api/v1/...`):

```
domain/
  <contexto>.rs   — entidades, value objects, regras puras (zero axum/sqlx/reqwest)
  ports.rs        — traits: RecordingStorage, MeetingRepository, OrgRepository, ...
  validation.rs   — regras de campo partilhadas (email, password)
application/
  <contexto>_service.rs — Facade: orquestra repo + regras + auditoria + quota;
                           recebe Command, devolve View DTO
infrastructure/
  postgres/<contexto>_repo.rs — implementa os traits de domain::ports com sqlx
  storage/{local_fs,webdav}.rs — implementa RecordingStorage
interface/http/<contexto>.rs — handlers axum finos: extrair → Command → Service → View → JSON
```

**Regra que fecha o bug de FromRow-como-DTO:** um struct com `#[derive(sqlx::FromRow)]`
nunca também deriva `Serialize` nem é devolvido diretamente por um handler. O
repositório mapeia a row para uma entidade/view; a camada HTTP nunca vê `FromRow`.

**Bounded contexts** (orientação, sem renomear ficheiros estáveis à toa): Identidade &
Acesso (`auth`, `users`, `mfa`, `apikeys`, `odoo_sso`) · Organização (`org`) · Reuniões
& Salas (`meetings`, `meetings_v1`, `rooms`, partes não-realtime de `room_tools`) ·
Gravações & Conteúdo (`recordings`, `storage`, `whiteboards`, `dlp`) · Voz/PSTN
(`voice`) · Realtime (`signaling`, `sfu`, `presence`, `broadcast`, `recorder`,
`pubsub`, `redis_state` — **intocado por esta ADR**, ver secção "Fora de escopo").

`AppState` **não é desmontado** por esta decisão — os serviços de `application`
recebem `Arc<AppState>` ou campos específicos por agora; decompor `AppState` em
sub-contextos fica para depois de o padrão estar validado em produção.

**SaaS vs on-premise:** mesmo binário, sem flag de "modo de deployment" nova — a
diferença fica inteiramente na configuração, seguindo o padrão já bom em `config.rs`
(config ausente ⇒ adaptador no-op, ex. `ollama_url` vazio desliga IA). O
`RecordingStorage` ganha `LocalFsStorage` (omissão, seguro para on-prem) e
`WebDavStorage`; a organização/multi-tenancy já resolve "SaaS" (várias orgs no mesmo
processo) vs "on-premise" (uma org, self-hosted) — não é preciso um conceito novo.

## Fora de escopo (decisão explícita, não uma omissão)

- `signaling.rs`, `sfu.rs`, `presence.rs`, `broadcast.rs`, `recorder.rs`, `pubsub.rs`,
  `redis_state.rs`: nenhuma restruturação por esta ADR. No máximo, *code motion* sem
  mudança de comportamento do que já é puro e já testado (seleção de camada simulcast,
  `AudioMeter`, formas de mensagem) — só depois de as fases seguintes validarem o
  padrão noutro sítio, e nunca sem harness de teste com pares reais primeiro.
- `odoo_sso.rs`: fica anotado o achado (config de Odoo *platform-wide* conflada com uso
  por-tenant — um cliente on-prem não consegue o seu próprio Odoo para
  auto-provisioning sem mexer em código) — corrige-se numa ADR própria, não nesta.
- `voice.rs`: `MediaBackend` já é uma boa semente de porta; só vira `trait` a sério
  quando a integração FreeSWITCH real chegar.

## Rollout FASEADO (por isto é uma ADR viva)

1. **Fase 1 — piloto: Gravações & Storage** (`recordings.rs` + `storage.rs`). Maior
   gap concreto, sem regressões documentadas, serve diretamente o objetivo
   SaaS/on-prem. Corrige de caminho o bug de config encontrado: `recordings.rs` deixa
   de reler `RECORDINGS_DIR` por sua conta.
2. **Fase 2 — Identidade & Acesso** (`auth.rs`, `users.rs`, `mfa.rs`). Unifica
   `domain::validation` (mata as 3 versões inconsistentes de password/email), mas é
   security-critical (invariante nº1 do AGENTS.md) — exige testes de caracterização do
   comportamento atual antes de tocar.
3. **Fase 3 — Organização & Reuniões** (`org.rs`, `meetings.rs`, `meetings_v1.rs`,
   `rooms.rs`, `apikeys.rs`). Unifica `meetings.rs`/`meetings_v1.rs` num único
   `MeetingService` consumido pelos dois conjuntos de handlers, sem colapsar a
   fronteira de contrato `/api/...` vs `/api/v1/...`.
4. **Fase 4 (opcional)**: `webhooks.rs` (já é bom, baixa prioridade), `odoo.rs`
   (extrair `IdentityProviderPort`), `voice.rs` (quando FreeSWITCH real chegar).
5. **Transversal, em paralelo, baixo risco**: gerar OpenAPI a partir das rotas axum
   (ex. `utoipa`) módulo a módulo à medida que é tocado, e gerar o cliente TS a partir
   dele para substituir os ~40 DTOs duplicados à mão em `web/src/api.ts`.

Cada fase: converter os sites → testes de caracterização (fases 2 e 3) → `cargo test
--release` + `scripts/check-clippy-ratchet.sh` + `scripts/check-route-auth.sh` +
`scripts/check-tenant-rls.sh` + `scripts/check-room-affinity.sh` +
`scripts/check-docs-drift.sh` verdes → smoke manual via `make dev` → PR próprio
(worktree isolado, CLAUDE.md §0.1).

## Consequências

- **+** Elimina a classe de bug já vista em produção (FromRow como contrato de API).
- **+** Um único lugar para regras de validação e autorização por contexto, em vez de
  cópias divergentes.
- **+** `RecordingStorage` como porta torna o backend de storage uma escolha de
  configuração, não de código — é o que "SaaS ou on-premise" precisa estruturalmente.
- **+** A fronteira `/api/...` vs `/api/v1/...` (já nomeada em `api-contract.md`) fica
  mais fácil de manter porque a lógica de negócio deixa de estar duplicada nos dois
  lados.
- **−** Mais indireção por módulo tocado (trait + repo + service + handler fino) —
  custo aceite porque cada camada é testável isoladamente sem WebSocket/RTP real.
- **−** Rollout mais lento que um refactor de uma vez — aceite deliberadamente: a
  alternativa é o tipo de reescrita cega que a Regra 0 do workspace proíbe, num
  sistema com 25 regressões documentadas e 9 invariantes de segurança a não quebrar.
- A pilha de realtime continuar fora de escopo é uma escolha de risco, não uma lacuna:
  reabre-se com uma ADR sucessora quando houver harness de teste com pares reais.
