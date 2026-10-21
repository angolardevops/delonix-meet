-- ════════════════════════════════════════════════════════════════════════
--  Reposição de password pedida pela própria pessoa, por email (E3).
--
--  A tabela `password_resets` (0106) nasceu para o administrador: a reposição
--  era sempre emitida EM NOME de uma organização. A pedida pela própria pessoa
--  não age em nome de org nenhuma — e uma conta particular (ADR-0019) nem tem
--  uma. Por isso o `org_id` passa a opcional, e o canal fica gravado.
--
--  Mesma tabela, de propósito: o índice único de UMA pendente por pessoa vale
--  para os dois canais, e pedir uma por email revoga a que o administrador
--  tenha emitido (e vice-versa). Dois tokens válidos para a mesma conta, um em
--  cada canal, eram duas portas.
-- ════════════════════════════════════════════════════════════════════════

ALTER TABLE password_resets ALTER COLUMN org_id DROP NOT NULL;

ALTER TABLE password_resets
    ADD COLUMN channel TEXT NOT NULL DEFAULT 'manual'
        CHECK (channel IN ('manual', 'email'));

-- O administrador emite sempre em nome de uma org; só o canal `email` vive sem.
ALTER TABLE password_resets
    ADD CONSTRAINT password_resets_manual_tem_org
        CHECK (channel = 'email' OR org_id IS NOT NULL);
