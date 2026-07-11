# ADR-0002 — Isolamento multi-tenant estrutural com Row-Level Security (RLS)

**Estado:** Aceite (rollout faseado) · **Data:** 2026-07-11 · **Contexto:** avaliação de arquitetura (Martin Fowler, ponto #1)

## Contexto

O isolamento multi-tenant é a invariante nº 1 do produto (soberania de dados, BNA/LGPD).
Até agora era **defensivo**: cada handler tem de se lembrar de chamar `can_access_room`
/ `require_member` e escrever `WHERE org_id = $1` à mão (~123 refs só em `org.rs`). Uma
query esquecida = fuga cross-org. Não havia barreira *fail-closed* ao nível dos dados —
só ao nível dos segredos (`config.rs`).

## Decisão

Adicionar **Postgres Row-Level Security como defesa em profundidade**, sem remover o
isolamento defensivo (que continua como primeira linha). O modelo:

- **Contexto de tenant por-request:** `AppState::tenant_tx(user_id)` abre uma transação e
  define `SET LOCAL app.user_id = <uuid>` (via `set_config(..., is_local := true)`,
  parametrizado — sem injeção). As queries a tabelas com RLS correm nessa `tx`.
- **Política por tabela** (`org_id`): uma linha só é visível/escrevível se a sua org
  pertencer ao utilizador em `app.user_id`:
  ```sql
  USING (org_id IN (SELECT org_id FROM org_members
                    WHERE user_id = NULLIF(current_setting('app.user_id', true), '')::uuid))
  ```
- **Fail-closed:** sem `app.user_id` (NULL), a subquery é vazia → 0 linhas. Uma query que
  se esqueça do contexto **quebra visível** em vez de vazar. `FORCE ROW LEVEL SECURITY`
  sujeita também o dono da tabela (o role `delonix`, que NÃO é superuser — confirmado).

## Rollout FASEADO (por isto é uma ADR viva)

RLS é aplicado tabela-a-tabela: uma tabela só ganha RLS `FORCE` **depois** de TODAS as
suas queries correrem em `tenant_tx`. Enquanto isso, o isolamento defensivo cobre-a.

1. **Fatia inicial (feita, migração 0024):** `employee_groups` — self-contained em
   `org.rs`, só contexto de utilizador. Prova fail-closed testada na BD live.
2. **Próximas (por org_id, self-contained, sem caminhos sem-utilizador):** `branches`,
   `group_members`, `meeting_agenda_items`, `action_items`, `action_plans`, `whiteboards`.
3. **Cuidado — tabelas com caminhos SEM utilizador** (não podem usar RLS keyed em
   `app.user_id` sem um bypass explícito): `org_webhooks` (entrega `fire()` em background),
   `org_api_keys`/`org_sso_configs`/`recording_share_links` (auth por token/chave, sem
   sessão). Para estas: ou uma política adicional keyed noutro GUC (`app.system=true`
   definido só nesses caminhos), ou mantê-las no isolamento defensivo + revisão dedicada.
4. **Rooms/participants/recordings:** tocadas em vários módulos (signaling, recorder) —
   converter todos os sites antes de ativar RLS; é a maior fatia.

Cada fatia: converter os sites para `tenant_tx` → migração `ALTER TABLE ... ENABLE/FORCE
RLS + CREATE POLICY` → testar fail-closed (0 linhas sem GUC; dados certos com GUC; nada
cross-org) → deploy.

## Consequências

- **+** Barreira de BD que apanha a "query esquecida" — o objetivo do Fowler.
- **+** A `tenant_tx` também dá atomicidade (transação) a operações multi-query.
- **−** Cada query a uma tabela com RLS TEM de correr em `tenant_tx` (esquecer = 0 linhas,
  mas isso é fail-closed, apanhado em teste). Custo: uma transação por operação.
- **−** Caminhos sem-utilizador precisam de tratamento explícito (ver ponto 3).
- Guardado por um teste de fumo (ver `scripts/`) e, à medida que converte, pelos testes
  de integração (Arq #2).
