-- ════════════════════════════════════════════════════════════════════════
--  Reuniões criadas por um sistema externo (Odoo / nk_delonix_meet).
--
--  Porquê uma tabela de mapeamento e não uma coluna em `meetings`:
--    1. A unicidade da referência externa é por ORGANIZAÇÃO — duas empresas
--       podem usar o mesmo id de `calendar.event` na sua própria BD Odoo.
--       `meetings` não tem `org_id` (só `owner_id`), portanto uma coluna
--       simples não conseguiria exprimir esse âmbito.
--    2. Mantém `meetings` intocada — o `FromRow` derivado de `Meeting` lê
--       todas as colunas e adicionar uma partiria as queries que não fossem
--       actualizadas (foi exactamente o que a 0022 fez a `start`/`ics`).
--
--  A PK (org_id, external_ref) é o que dá IDEMPOTÊNCIA ao POST /api/v1/meetings:
--  repetir o pedido com a mesma referência devolve a reunião já criada em vez
--  de duplicar. Sem isto, cada gravação do evento no Odoo criava uma sala nova.
-- ════════════════════════════════════════════════════════════════════════

CREATE TABLE meeting_external_refs (
    org_id       UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    -- Referência opaca do sistema chamador, ex.:
    -- 'odoo:kaeso_prod:calendar.event:4821'. O Delonix não a interpreta.
    external_ref TEXT NOT NULL,
    meeting_id   UUID NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, external_ref)
);

-- Sentido inverso (reunião -> referência externa): usado pelo GET /meetings
-- para o cliente reconciliar sem manter o seu próprio mapa, e pelo DELETE.
CREATE UNIQUE INDEX meeting_external_refs_meeting_uidx
    ON meeting_external_refs (meeting_id);

-- NOTA RLS: esta tabela NÃO leva política (ver 0024 — só `employee_groups`
-- está no rollout até agora). O isolamento aqui é defensivo: todos os acessos
-- passam por `org_id = <org da chave de API>`. Quando o rollout do RLS chegar
-- às restantes tabelas, esta entra com a política padrão por `org_id`.
