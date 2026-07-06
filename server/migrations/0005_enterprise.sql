-- Organizações (empresas) multi-tenant.
CREATE TABLE organizations (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name       TEXT NOT NULL,
    slug       TEXT NOT NULL UNIQUE,
    created_by UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Filiais / afiliadas de uma organização.
CREATE TABLE branches (
    id       UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id   UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name     TEXT NOT NULL,
    location TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX branches_org_idx ON branches(org_id);

-- Employees = ligação user<->organização, com papel e filial opcional.
CREATE TABLE org_members (
    org_id     UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    branch_id  UUID REFERENCES branches(id) ON DELETE SET NULL,
    role       TEXT NOT NULL DEFAULT 'member',  -- 'admin' | 'member'
    title      TEXT NOT NULL DEFAULT '',        -- cargo (ex.: "Engenheiro")
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, user_id)
);
CREATE INDEX org_members_user_idx ON org_members(user_id);
CREATE INDEX org_members_branch_idx ON org_members(branch_id);

-- Grupos de employees (equipas / departamentos) para chamadas em grupo.
CREATE TABLE employee_groups (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id     UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    created_by UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX employee_groups_org_idx ON employee_groups(org_id);

CREATE TABLE group_members (
    group_id UUID NOT NULL REFERENCES employee_groups(id) ON DELETE CASCADE,
    user_id  UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (group_id, user_id)
);
CREATE INDEX group_members_user_idx ON group_members(user_id);

-- Minutes of Meeting (notas AI) associadas a uma reunião agendada.
ALTER TABLE meetings ADD COLUMN minutes TEXT NOT NULL DEFAULT '';
ALTER TABLE meetings ADD COLUMN transcript TEXT NOT NULL DEFAULT '';
