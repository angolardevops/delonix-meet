-- Papéis e permissões: papéis por organização com herança de um único pai,
-- e pedidos de aprovação para capacidades marcadas como "requer aprovação".
-- O catálogo de capacidades é fixo em código (server/src/rbac.rs), não numa
-- tabela — evita um catálogo que cresce sem nenhuma verificação real por
-- trás. Âmbito por departamento, sincronização com grupos do Odoo e
-- "simular utilizador" ficam fora desta versão — cada um precisa do seu
-- próprio desenho, e um campo que não aplica nada é pior do que não existir.
CREATE TABLE org_roles (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id         UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name           TEXT NOT NULL,
    is_system      BOOLEAN NOT NULL DEFAULT FALSE,
    parent_role_id UUID REFERENCES org_roles(id) ON DELETE SET NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, name)
);
CREATE INDEX org_roles_org_idx ON org_roles(org_id);
CREATE INDEX org_roles_parent_idx ON org_roles(parent_role_id);

-- Só as concessões DIRECTAS de um papel; as herdadas vêm de percorrer
-- parent_role_id em tempo de leitura (server/src/rbac.rs::effective_permissions).
CREATE TABLE org_role_permissions (
    role_id           UUID NOT NULL REFERENCES org_roles(id) ON DELETE CASCADE,
    permission         TEXT NOT NULL,
    requires_approval  BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (role_id, permission)
);

-- Pedidos de aprovação: a primeira tentativa de usar uma capacidade marcada
-- "requer aprovação" cria a linha pendente aqui em vez de a conceder — ver
-- rbac::require_permission. Uma aprovação expira; não fica concedida para sempre.
CREATE TABLE org_permission_requests (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id       UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    requester_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    permission   TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'pending',
    decided_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    decided_at   TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at   TIMESTAMPTZ
);
CREATE INDEX org_permission_requests_org_idx ON org_permission_requests(org_id, status);
CREATE INDEX org_permission_requests_requester_idx ON org_permission_requests(requester_id, permission, status);

-- Aditivo: `org_members.role` (TEXT 'admin'|'member') continua a ser a
-- autoridade para as verificações antigas (require_admin_pub e afins), sem
-- nenhuma mudança de comportamento. `role_id` só entra nas verificações NOVAS
-- (rbac::require_permission) — por isso pode nascer já preenchido sem
-- nenhum risco de regressão no que já existia.
ALTER TABLE org_members ADD COLUMN role_id UUID REFERENCES org_roles(id) ON DELETE SET NULL;

-- Semear os dois papéis de sistema em cada organização existente.
INSERT INTO org_roles (org_id, name, is_system)
SELECT o.id, 'Administrador', TRUE FROM organizations o;

INSERT INTO org_roles (org_id, name, is_system)
SELECT o.id, 'Membro', TRUE FROM organizations o;

UPDATE org_members m
SET role_id = r.id
FROM org_roles r
WHERE r.org_id = m.org_id AND r.is_system = TRUE
  AND r.name = (CASE WHEN m.role = 'admin' THEN 'Administrador' ELSE 'Membro' END);
