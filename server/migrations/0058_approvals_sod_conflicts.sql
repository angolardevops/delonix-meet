-- ════════════════════════════════════════════════════════════════════════
--  Pedidos de aprovação, segregação de funções e conflitos de papel vindos do
--  Odoo (ADR-0008 §6, §9, §10).
-- ════════════════════════════════════════════════════════════════════════

CREATE TABLE approval_requests (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id       UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    requester_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    capability   TEXT NOT NULL,
    action       TEXT NOT NULL,
    -- JSON canónico do alvo e o seu SHA-256: a aprovação só serve para ESTE alvo.
    target       JSONB NOT NULL,
    target_hash  TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'pending'
                     CHECK (status IN ('pending', 'approved', 'rejected', 'consumed', 'expired', 'invalidated')),
    reason       TEXT NOT NULL DEFAULT '',
    decided_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    decided_at   TIMESTAMPTZ,
    consumed_at  TIMESTAMPTZ,
    expires_at   TIMESTAMPTZ NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- Um pedido vivo por pessoa e tripla.
CREATE UNIQUE INDEX approval_requests_live_uidx
    ON approval_requests (org_id, requester_id, capability, action, target_hash)
    WHERE status IN ('pending', 'approved');
CREATE INDEX approval_requests_org_idx ON approval_requests (org_id, created_at, id);
CREATE INDEX approval_requests_expiry_idx ON approval_requests (expires_at)
    WHERE status IN ('pending', 'approved');

CREATE TABLE sod_rules (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id          UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    capabilities    TEXT[] NOT NULL CHECK (cardinality(capabilities) >= 2),
    exempt_role_ids UUID[] NOT NULL DEFAULT '{}',
    created_by      UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX sod_rules_name_uidx ON sod_rules (org_id, lower(name));

-- «Aceitar risco»: vale enquanto a pessoa tiver o MESMO papel.
CREATE TABLE sod_risk_acceptances (
    rule_id       UUID NOT NULL REFERENCES sod_rules(id) ON DELETE CASCADE,
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id       UUID NOT NULL REFERENCES org_roles(id) ON DELETE CASCADE,
    justification TEXT NOT NULL CHECK (length(btrim(justification)) >= 10),
    accepted_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    accepted_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (rule_id, user_id)
);

-- Conflito de papel: a sincronização não decide sozinha (ADR-0008 §9).
CREATE TABLE role_conflicts (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id            UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id           UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    current_role_id   UUID NOT NULL REFERENCES org_roles(id) ON DELETE CASCADE,
    proposed_role_ids UUID[] NOT NULL,
    odoo_groups       TEXT[] NOT NULL DEFAULT '{}',
    status            TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'resolved', 'superseded')),
    decision          TEXT CHECK (decision IN ('keep_current', 'apply_proposed')),
    applied_role_id   UUID REFERENCES org_roles(id) ON DELETE SET NULL,
    resolved_by       UUID REFERENCES users(id) ON DELETE SET NULL,
    resolved_at       TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX role_conflicts_pending_uidx ON role_conflicts (org_id, user_id)
    WHERE status = 'pending';
CREATE INDEX role_conflicts_org_idx ON role_conflicts (org_id, created_at, id);
