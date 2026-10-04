-- Ramais: PIN secreto, ramais da empresa (sem pessoa) e o intervalo de onde
-- saem os números automáticos (plano de produção, item 3.8, lote 1; R276).
--
-- A decisão do dono (2026-10-04) separa três coisas: o NÚMERO do ramal
-- (identidade, não é secreto), a PASSWORD SIP (credencial do aparelho, já
-- existia) e o PIN (código secreto de 6 dígitos, novo aqui).

-- 1. Ramal da empresa: recepção, sala, portaria — sem pessoa. `member_id`
--    passa a poder ser NULL. A FK composta (org_id, member_id) → org_members é
--    MATCH SIMPLE: com `member_id` NULL não se verifica, e o UNIQUE
--    (org_id, member_id) continua a valer só para os ramais de pessoa (o
--    Postgres não faz colidir NULLs). Um ramal sem pessoa TEM de dizer o que é.
ALTER TABLE voice_extensions ALTER COLUMN member_id DROP NOT NULL;
ALTER TABLE voice_extensions
    ADD CONSTRAINT voice_extensions_company_label_chk
    CHECK (member_id IS NOT NULL OR btrim(label) <> '');

-- 2. PIN do ramal. Só o hash (Argon2, o mesmo helper das passwords); o valor
--    em claro existe apenas na resposta que o gera. `pin_hash` NULL = «por
--    definir». Cinco falhas seguidas bloqueiam até `pin_locked_until`.
ALTER TABLE voice_extensions ADD COLUMN pin_hash TEXT;
ALTER TABLE voice_extensions ADD COLUMN pin_set_at TIMESTAMPTZ;
ALTER TABLE voice_extensions ADD COLUMN pin_failed_attempts INT NOT NULL DEFAULT 0
    CHECK (pin_failed_attempts >= 0);
ALTER TABLE voice_extensions ADD COLUMN pin_locked_until TIMESTAMPTZ;

-- 3. Intervalo de numeração automática, por organização. Sem linha vale a
--    omissão do domínio (1000–1999). Isolamento como nas tabelas vizinhas da
--    voz (`voice_extensions`, `voice_did`): `org_id` em todas as leituras e
--    escritas, feitas atrás de `org::require_admin_pub`.
CREATE TABLE voice_extension_ranges (
    org_id UUID PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
    range_start INT NOT NULL,
    range_end INT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- A mesma forma de um número curto: 3 a 5 dígitos, sem zero à esquerda.
    CONSTRAINT voice_extension_ranges_bounds_chk
        CHECK (range_start >= 100 AND range_end <= 99999 AND range_start <= range_end)
);
