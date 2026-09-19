-- ════════════════════════════════════════════════════════════════════════
--  Departamentos, lugares e último acesso (ADR-0008 §7 e §11).
-- ════════════════════════════════════════════════════════════════════════

-- Departamento: entidade da org. `odoo` = vem da sincronização (não se edita à
-- mão); `manual` = criado na consola.
CREATE TABLE departments (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id       UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    source       TEXT NOT NULL DEFAULT 'manual' CHECK (source IN ('manual', 'odoo')),
    external_ref TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX departments_name_uidx ON departments (org_id, lower(name));
CREATE UNIQUE INDEX departments_external_ref_uidx ON departments (org_id, external_ref)
    WHERE external_ref IS NOT NULL;
CREATE UNIQUE INDEX departments_org_id_uidx ON departments (org_id, id);

-- O departamento é um ATRIBUTO DA PERTENÇA (ADR-0008 §2): é o que a policy
-- recebe como âmbito. FK composta: nunca o departamento de outra org.
ALTER TABLE org_members ADD COLUMN department_id UUID;
ALTER TABLE org_members
    ADD CONSTRAINT org_members_department_same_org_fk
    FOREIGN KEY (org_id, department_id) REFERENCES departments (org_id, id);
CREATE INDEX org_members_department_idx ON org_members (department_id)
    WHERE department_id IS NOT NULL;

-- Um papel de âmbito departamento aponta para um departamento da mesma org.
ALTER TABLE org_roles
    ADD CONSTRAINT org_roles_department_same_org_fk
    FOREIGN KEY (org_id, scope_department_id) REFERENCES departments (org_id, id);

-- Lugares: tecto do OPERADOR (NULL = sem tecto). O uso não se guarda — mede-se.
ALTER TABLE organizations
    ADD COLUMN max_seats BIGINT CHECK (max_seats IS NULL OR max_seats >= 1);

-- Último acesso: escrito em `auth::issue_tokens`. Preenche-se a partir dos
-- inícios de sessão já auditados.
ALTER TABLE users ADD COLUMN last_access_at TIMESTAMPTZ;
UPDATE users u
   SET last_access_at = a.last_login
  FROM (SELECT actor_id, MAX(created_at) AS last_login
          FROM audit_logs
         WHERE action LIKE 'auth.login%'
         GROUP BY actor_id) a
 WHERE a.actor_id = u.id;
