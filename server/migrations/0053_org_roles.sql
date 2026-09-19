-- ════════════════════════════════════════════════════════════════════════
--  Papéis e capacidades por organização (ADR-0008 §3–§5).
--
--  A FONTE do papel passa a ser `org_members.role_id`. A coluna herdada
--  `org_members.role` ('admin' | 'member') fica, DERIVADA por gatilho num só
--  sentido (role_id → role), porque três leituras por texto e cinco
--  escritores herdados ainda a usam:
--    - um INSERT só com `role` recebe o role_id de sistema correspondente;
--    - um UPDATE que mude `role` sem mudar `role_id` LEVANTA EXCEPÇÃO — um
--      `role='member'` herdado nunca esmaga um papel personalizado em silêncio;
--    - `owner`/`admin` → 'admin', qualquer outro → 'member'.
--
--  Os valores semeados dos papéis de sistema estão em
--  `system_role_capability_defaults` e TÊM de ser iguais a
--  `SystemRole::default_value` no domínio (há teste que compara os dois).
-- ════════════════════════════════════════════════════════════════════════

CREATE TABLE org_roles (
    id                              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id                          UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    system_key                      TEXT CHECK (system_key IN ('owner', 'admin', 'member', 'external_guest')),
    name                            TEXT NOT NULL,
    description                     TEXT NOT NULL DEFAULT '',
    inherits_from                   UUID REFERENCES org_roles(id),  -- NO ACTION: verificado no fim da instrução (cascata da org)
    scope                           TEXT NOT NULL DEFAULT 'organization'
                                        CHECK (scope IN ('organization', 'department')),
    scope_department_id             UUID,
    max_simultaneous_destinations   INTEGER CHECK (max_simultaneous_destinations >= 0),
    max_external_guests_per_month   INTEGER CHECK (max_external_guests_per_month >= 0),
    -- Limites EFECTIVOS (herança resolvida pela policy), gravados com a matriz.
    eff_max_simultaneous_destinations INTEGER,
    eff_max_external_guests_per_month INTEGER,
    odoo_group                      TEXT,
    created_at                      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by                      UUID REFERENCES users(id) ON DELETE SET NULL,
    UNIQUE (org_id, system_key),
    -- Um papel de sistema não herda, não tem âmbito de departamento nem grupo de dono.
    CHECK (system_key IS NULL OR (inherits_from IS NULL AND scope = 'organization')),
    CHECK (system_key IS DISTINCT FROM 'owner' OR odoo_group IS NULL)
);
CREATE UNIQUE INDEX org_roles_name_uidx ON org_roles (org_id, lower(name));
CREATE UNIQUE INDEX org_roles_odoo_group_uidx ON org_roles (org_id, odoo_group)
    WHERE odoo_group IS NOT NULL;
-- Para as FKs compostas: um membro só pode ter um papel da SUA org.
CREATE UNIQUE INDEX org_roles_org_id_uidx ON org_roles (org_id, id);

CREATE TABLE org_role_capabilities (
    role_id    UUID NOT NULL REFERENCES org_roles(id) ON DELETE CASCADE,
    capability TEXT NOT NULL,
    value      TEXT NOT NULL CHECK (value IN ('allow', 'deny', 'inherit', 'requires_approval')),
    PRIMARY KEY (role_id, capability)
);

-- Decisão efectiva por papel e capacidade nos dois âmbitos que existem
-- (ADR-0008 §4): recurso da organização, e recurso do departamento DA pertença.
-- Dado DERIVADO: gravado pela policy do domínio na mesma transacção de qualquer
-- escrita de papéis. É o que `org::require_capability` e o SQL da biblioteca lêem.
CREATE TABLE org_role_effective_capabilities (
    role_id       UUID NOT NULL REFERENCES org_roles(id) ON DELETE CASCADE,
    capability    TEXT NOT NULL,
    org_decision  TEXT NOT NULL CHECK (org_decision IN ('allow', 'deny', 'requires_approval')),
    dept_decision TEXT NOT NULL CHECK (dept_decision IN ('allow', 'deny', 'requires_approval')),
    PRIMARY KEY (role_id, capability)
);

CREATE TABLE system_role_capability_defaults (
    system_key TEXT NOT NULL,
    capability TEXT NOT NULL,
    value      TEXT NOT NULL CHECK (value IN ('allow', 'deny')),
    PRIMARY KEY (system_key, capability)
);

INSERT INTO system_role_capability_defaults (system_key, capability, value)
SELECT k, c,
       CASE
         WHEN k IN ('owner', 'admin') THEN 'allow'
         WHEN k = 'member' AND c IN ('sessions.create', 'recordings.record_4k') THEN 'allow'
         ELSE 'deny'
       END
FROM unnest(ARRAY['owner', 'admin', 'member', 'external_guest']) AS k
CROSS JOIN unnest(ARRAY[
    'sessions.create', 'sessions.admit_waiting_room', 'sessions.mute_remove',
    'sessions.breakout_rooms', 'recordings.record_4k', 'recordings.view_others',
    'recordings.publish', 'recordings.delete', 'broadcast.public_destinations',
    'broadcast.manage_rtmp_keys', 'broadcast.highlight_questions', 'studio.edit_timeline',
    'studio.generate_captions', 'studio.export_4k', 'admin.manage_accounts',
    'admin.manage_roles', 'admin.view_audit', 'admin.change_retention', 'org.administer'
]) AS c;

-- Semeia os quatro papéis de sistema de uma org (idempotente).
CREATE OR REPLACE FUNCTION seed_system_roles(p_org UUID) RETURNS VOID AS $$
BEGIN
    INSERT INTO org_roles (org_id, system_key, name, description)
    VALUES
        (p_org, 'owner', 'Proprietário', 'Toda a organização · não pode ser removido'),
        (p_org, 'admin', 'Administrador', 'Toda a organização'),
        (p_org, 'member', 'Membro', 'Entra, participa e grava as suas sessões'),
        (p_org, 'external_guest', 'Convidado externo', 'Acesso limitado e com validade')
    ON CONFLICT (org_id, system_key) DO NOTHING;

    INSERT INTO org_role_capabilities (role_id, capability, value)
    SELECT r.id, d.capability, d.value
      FROM org_roles r
      JOIN system_role_capability_defaults d ON d.system_key = r.system_key
     WHERE r.org_id = p_org
    ON CONFLICT DO NOTHING;

    INSERT INTO org_role_effective_capabilities (role_id, capability, org_decision, dept_decision)
    SELECT r.id, d.capability, d.value, d.value
      FROM org_roles r
      JOIN system_role_capability_defaults d ON d.system_key = r.system_key
     WHERE r.org_id = p_org
    ON CONFLICT DO NOTHING;
END;
$$ LANGUAGE plpgsql;

SELECT seed_system_roles(id) FROM organizations;

CREATE OR REPLACE FUNCTION organizations_seed_roles() RETURNS TRIGGER AS $$
BEGIN
    PERFORM seed_system_roles(NEW.id);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER organizations_seed_roles_trg
    AFTER INSERT ON organizations
    FOR EACH ROW EXECUTE FUNCTION organizations_seed_roles();

-- ---- pertença ----
ALTER TABLE org_members
    ADD COLUMN role_id         UUID,
    ADD COLUMN role_source     TEXT NOT NULL DEFAULT 'manual'
        CHECK (role_source IN ('manual', 'odoo_group')),
    -- Suspender = arquivar com razão (ADR-0008 §7). NULL nas linhas herdadas
    -- arquivadas = `removed`.
    ADD COLUMN archived_reason TEXT
        CHECK (archived_reason IN ('suspended', 'removed', 'odoo_exit', 'inactive', 'guest_expired')),
    ADD COLUMN origin          TEXT NOT NULL DEFAULT 'manual'
        CHECK (origin IN ('manual', 'registration', 'odoo_sso', 'sso', 'invitation', 'code', 'api'));

UPDATE org_members SET archived_reason = 'removed' WHERE archived_at IS NOT NULL;

UPDATE org_members m
   SET origin = 'odoo_sso'
  FROM users u
 WHERE u.id = m.user_id AND u.odoo_org_id = m.org_id;

-- Papel de sistema a partir do texto herdado.
UPDATE org_members m
   SET role_id = r.id
  FROM org_roles r
 WHERE r.org_id = m.org_id
   AND r.system_key = CASE WHEN m.role = 'admin' THEN 'admin' ELSE 'member' END;

-- Dono: o criador se for admin activo e humano; senão o admin activo humano mais antigo.
WITH candidates AS (
    SELECT DISTINCT ON (m.org_id) m.org_id, m.user_id
      FROM org_members m
      JOIN organizations o ON o.id = m.org_id
      JOIN users u ON u.id = m.user_id
     WHERE m.archived_at IS NULL
       AND m.role = 'admin'
       AND u.email <> 'provisioning@delonix.internal'
     ORDER BY m.org_id, (m.user_id = o.created_by) DESC, m.created_at, m.user_id
)
UPDATE org_members m
   SET role_id = r.id
  FROM candidates c
  JOIN org_roles r ON r.org_id = c.org_id AND r.system_key = 'owner'
 WHERE m.org_id = c.org_id AND m.user_id = c.user_id;

ALTER TABLE org_members ALTER COLUMN role_id SET NOT NULL;
ALTER TABLE org_members
    ADD CONSTRAINT org_members_role_same_org_fk
    FOREIGN KEY (org_id, role_id) REFERENCES org_roles (org_id, id);
CREATE INDEX org_members_role_idx ON org_members (role_id);

CREATE OR REPLACE FUNCTION is_service_account(p_user UUID) RETURNS BOOLEAN AS $$
    SELECT EXISTS (SELECT 1 FROM users WHERE id = p_user AND email = 'provisioning@delonix.internal');
$$ LANGUAGE sql STABLE;

-- role_id → role, num só sentido.
CREATE OR REPLACE FUNCTION org_members_derive_role() RETURNS TRIGGER AS $$
DECLARE
    k TEXT;
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NEW.role_id IS NULL THEN
            SELECT id INTO NEW.role_id FROM org_roles
             WHERE org_id = NEW.org_id
               AND system_key = CASE WHEN NEW.role = 'admin' THEN 'admin' ELSE 'member' END;
            -- O primeiro admin HUMANO de uma org sem dono activo fica dono.
            IF NEW.role = 'admin' AND NOT is_service_account(NEW.user_id) AND NOT EXISTS (
                SELECT 1 FROM org_members m JOIN org_roles r ON r.id = m.role_id
                 WHERE m.org_id = NEW.org_id AND m.archived_at IS NULL AND r.system_key = 'owner'
            ) THEN
                SELECT id INTO NEW.role_id FROM org_roles
                 WHERE org_id = NEW.org_id AND system_key = 'owner';
            END IF;
        END IF;
        IF NEW.origin = 'manual' AND EXISTS (
            SELECT 1 FROM users WHERE id = NEW.user_id AND odoo_org_id = NEW.org_id
        ) THEN
            NEW.origin := 'odoo_sso';
        END IF;
    ELSIF NEW.role_id IS NOT DISTINCT FROM OLD.role_id AND NEW.role IS DISTINCT FROM OLD.role THEN
        RAISE EXCEPTION 'org_members.role é derivado de role_id; use org::set_system_role'
            USING ERRCODE = 'check_violation';
    END IF;

    SELECT system_key INTO k FROM org_roles WHERE id = NEW.role_id;
    IF k = 'owner' AND is_service_account(NEW.user_id) THEN
        RAISE EXCEPTION 'role.service_account: o utilizador de serviço nunca é dono'
            USING ERRCODE = 'check_violation';
    END IF;
    NEW.role := CASE WHEN k IN ('owner', 'admin') THEN 'admin' ELSE 'member' END;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER org_members_derive_role_trg
    BEFORE INSERT OR UPDATE ON org_members
    FOR EACH ROW EXECUTE FUNCTION org_members_derive_role();

-- Segunda linha do invariante «≥ 1 owner activo SE houver ≥ 1 humano activo».
-- A primeira é o serviço (`role.last_owner`, 409); esta apanha o que lhe escapar.
-- Adiada: uma transacção que passa o dono de A para B é válida no fim.
CREATE OR REPLACE FUNCTION org_members_last_owner_guard() RETURNS TRIGGER AS $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM organizations WHERE id = OLD.org_id) THEN
        RETURN NULL; -- a org está a ser apagada
    END IF;
    IF EXISTS (
        SELECT 1 FROM org_members m JOIN users u ON u.id = m.user_id
         WHERE m.org_id = OLD.org_id AND m.archived_at IS NULL
           AND u.email <> 'provisioning@delonix.internal'
    ) AND NOT EXISTS (
        SELECT 1 FROM org_members m JOIN org_roles r ON r.id = m.role_id
         WHERE m.org_id = OLD.org_id AND m.archived_at IS NULL AND r.system_key = 'owner'
    ) AND EXISTS (
        -- só quando QUEM mudou era dono activo: não bloqueia orgs herdadas sem dono
        SELECT 1 FROM org_roles r WHERE r.id = OLD.role_id AND r.system_key = 'owner'
    ) AND OLD.archived_at IS NULL THEN
        RAISE EXCEPTION 'role.last_owner: a organização ficava sem Proprietário'
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE CONSTRAINT TRIGGER org_members_last_owner_guard_trg
    AFTER UPDATE OR DELETE ON org_members
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION org_members_last_owner_guard();
