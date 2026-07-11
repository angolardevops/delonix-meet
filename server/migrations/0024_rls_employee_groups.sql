-- ════════════════════════════════════════════════════════════════════════
--  Isolamento multi-tenant ESTRUTURAL (RLS backstop) — avaliação de arquitetura
--  #1 (Martin Fowler). Fatia inicial / prova fail-closed: `employee_groups`.
--  Ver docs/adr/0002-tenant-isolation-rls.md para o modelo e o rollout das
--  restantes tabelas.
--
--  Defesa em PROFUNDIDADE: o isolamento defensivo (WHERE org_id + can_access_*)
--  continua; o RLS é a barreira ao nível da BD que apanha a "query esquecida".
--  A app define `app.user_id` por-request via AppState::tenant_tx; a política só
--  deixa ver/escrever linhas cujas orgs pertencem a esse utilizador. Sem esse
--  contexto (NULL) → 0 linhas → FAIL-CLOSED (quebra visível, nunca vaza).
-- ════════════════════════════════════════════════════════════════════════

ALTER TABLE employee_groups ENABLE ROW LEVEL SECURITY;
-- FORCE: sujeita também o dono da tabela (o role da app) à política.
ALTER TABLE employee_groups FORCE ROW LEVEL SECURITY;

CREATE POLICY tenant_isolation ON employee_groups
    USING (
        org_id IN (
            SELECT org_id FROM org_members
            WHERE user_id = NULLIF(current_setting('app.user_id', true), '')::uuid
        )
    )
    WITH CHECK (
        org_id IN (
            SELECT org_id FROM org_members
            WHERE user_id = NULLIF(current_setting('app.user_id', true), '')::uuid
        )
    );
