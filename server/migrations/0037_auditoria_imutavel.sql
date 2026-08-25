-- Auditoria imutável e verificável.
--
-- Três buracos, medidos no código anterior:
--
-- 1. QUALQUER pessoa com acesso de escrita à base de dados podia fazer UPDATE
--    ou DELETE numa linha para apagar o que fez. Um registo de auditoria que
--    se pode editar não é um registo de auditoria — e o adversário que
--    interessa aqui é precisamente alguém com privilégios.
--
-- 2. `ON DELETE CASCADE` para `users`: apagar um utilizador APAGAVA a trilha
--    dele. É o inverso do que uma auditoria tem de fazer — a história tem de
--    sobreviver às entidades que descreve. E o `list` fazia `JOIN users`, por
--    isso mesmo sem o cascade os eventos de um utilizador apagado já
--    desapareciam da vista do administrador.
--
-- 3. A escrita era best-effort com `warn!`. Uma auditoria permanentemente
--    partida produz avisos que ninguém lê.
--
-- A defesa contra (1) é uma CADEIA DE HASH por organização: cada linha inclui
-- o hash da anterior. Editar ou apagar uma linha parte a cadeia, e a quebra é
-- detectável mesmo por quem não confia em quem administra a base de dados. Os
-- gatilhos que recusam UPDATE/DELETE são a primeira barreira; a cadeia é a que
-- sobrevive a quem tenha poder para os remover.

-- --- (2) a história sobrevive às entidades ---
ALTER TABLE audit_logs DROP CONSTRAINT IF EXISTS audit_logs_org_id_fkey;
ALTER TABLE audit_logs DROP CONSTRAINT IF EXISTS audit_logs_actor_id_fkey;
-- Nome do actor guardado NO MOMENTO do evento. É o que faz a linha continuar
-- legível depois de a conta desaparecer — e é também o nome que ele tinha
-- ENTÃO, que é o que uma auditoria quer, não o actual.
ALTER TABLE audit_logs ADD COLUMN IF NOT EXISTS actor_name TEXT NOT NULL DEFAULT '';

-- --- (1) cadeia de hash por organização ---
ALTER TABLE audit_logs
    ADD COLUMN IF NOT EXISTS seq       BIGINT,
    ADD COLUMN IF NOT EXISTS prev_hash TEXT,
    ADD COLUMN IF NOT EXISTS hash      TEXT;

-- Chave da cadeia: a org, ou o UUID nulo para os eventos sem org (ex.: login).
CREATE OR REPLACE FUNCTION audit_chain_key(org UUID) RETURNS UUID AS $$
    SELECT COALESCE(org, '00000000-0000-0000-0000-000000000000'::uuid);
$$ LANGUAGE sql IMMUTABLE;

-- O material que entra no hash. Está numa função para o gatilho e a
-- verificação usarem EXACTAMENTE a mesma definição — duas cópias divergem, e
-- uma verificação que discorda do escritor acusa falsas quebras.
CREATE OR REPLACE FUNCTION audit_material(
    seq BIGINT, org UUID, actor UUID, actor_name TEXT,
    action TEXT, target TEXT, created_at TIMESTAMPTZ, prev_hash TEXT
) RETURNS TEXT AS $$
    SELECT seq::text || '|' || audit_chain_key(org)::text || '|' || actor::text || '|'
        || actor_name || '|' || action || '|' || target || '|'
        -- Microssegundos: o `now()` do Postgres tem essa resolução, e truncar
        -- ao segundo deixaria duas linhas do mesmo segundo com o mesmo material.
        || to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US') || '|'
        || prev_hash;
$$ LANGUAGE sql IMMUTABLE;

CREATE OR REPLACE FUNCTION audit_encadear() RETURNS trigger AS $$
DECLARE
    chave UUID := audit_chain_key(NEW.org_id);
    ant_seq BIGINT;
    ant_hash TEXT;
BEGIN
    -- Serializa por CADEIA. Sem isto, dois INSERTs concorrentes lêem o mesmo
    -- `prev_hash` e produzem um ramo — a cadeia deixa de ser uma linha e a
    -- verificação acusa quebra sem ninguém ter mexido em nada.
    PERFORM pg_advisory_xact_lock(hashtext('audit_chain'), hashtext(chave::text));
    SELECT seq, hash INTO ant_seq, ant_hash
      FROM audit_logs
     WHERE audit_chain_key(org_id) = chave
     ORDER BY seq DESC LIMIT 1;

    NEW.seq := COALESCE(ant_seq, 0) + 1;
    NEW.prev_hash := COALESCE(ant_hash, repeat('0', 64));
    NEW.hash := encode(sha256(convert_to(
        audit_material(NEW.seq, NEW.org_id, NEW.actor_id, NEW.actor_name,
                       NEW.action, NEW.target, NEW.created_at, NEW.prev_hash),
        'UTF8')), 'hex');
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION audit_append_only() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'audit_logs é append-only: % recusado (ver migração 0037)', TG_OP
        USING ERRCODE = 'raise_exception';
END;
$$ LANGUAGE plpgsql;

-- Encadeia as linhas que já existiam, por ordem, para a cadeia começar
-- consistente em vez de começar quebrada.
DO $$
DECLARE r RECORD; k UUID; s BIGINT; p TEXT;
BEGIN
    FOR k IN SELECT DISTINCT audit_chain_key(org_id) FROM audit_logs LOOP
        s := 0; p := repeat('0', 64);
        FOR r IN SELECT * FROM audit_logs
                  WHERE audit_chain_key(org_id) = k ORDER BY id LOOP
            s := s + 1;
            UPDATE audit_logs SET seq = s, prev_hash = p,
                   hash = encode(sha256(convert_to(
                       audit_material(s, r.org_id, r.actor_id, r.actor_name,
                                      r.action, r.target, r.created_at, p), 'UTF8')), 'hex')
             WHERE id = r.id;
            SELECT hash INTO p FROM audit_logs WHERE id = r.id;
        END LOOP;
    END LOOP;
END $$;

ALTER TABLE audit_logs ALTER COLUMN seq SET NOT NULL;
ALTER TABLE audit_logs ALTER COLUMN prev_hash SET NOT NULL;
ALTER TABLE audit_logs ALTER COLUMN hash SET NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_audit_cadeia
    ON audit_logs (audit_chain_key(org_id), seq);

-- Os gatilhos entram DEPOIS do backfill: com o append-only activo, o UPDATE
-- acima seria recusado pela sua própria migração.
DROP TRIGGER IF EXISTS trg_audit_encadear ON audit_logs;
CREATE TRIGGER trg_audit_encadear BEFORE INSERT ON audit_logs
    FOR EACH ROW EXECUTE FUNCTION audit_encadear();

DROP TRIGGER IF EXISTS trg_audit_append_only ON audit_logs;
CREATE TRIGGER trg_audit_append_only BEFORE UPDATE OR DELETE ON audit_logs
    FOR EACH ROW EXECUTE FUNCTION audit_append_only();
