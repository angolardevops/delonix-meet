-- ════════════════════════════════════════════════════════════════════════
--  Convites de organização, regras de entrada e aprovisionamento
--  (ADR-0008 §9 e §11).
-- ════════════════════════════════════════════════════════════════════════

-- O token é a CREDENCIAL do convite: guarda-se só o hash (SHA-256) e o prefixo
-- para o reconhecer. Uso único; reenviar roda-o.
CREATE TABLE org_invitations (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id        UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    email         TEXT NOT NULL,
    role_id       UUID NOT NULL,
    department_id UUID,
    external      BOOLEAN NOT NULL DEFAULT FALSE,
    -- `link` = link com token longo; `code` = código curto para convidados.
    delivery      TEXT NOT NULL DEFAULT 'link' CHECK (delivery IN ('link', 'code')),
    token_hash    TEXT NOT NULL UNIQUE,
    token_prefix  TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'pending'
                      CHECK (status IN ('pending', 'accepted', 'revoked', 'expired')),
    expires_at    TIMESTAMPTZ NOT NULL,
    invited_by    UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    resent_at     TIMESTAMPTZ,
    accepted_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    accepted_at   TIMESTAMPTZ,
    FOREIGN KEY (org_id, role_id) REFERENCES org_roles (org_id, id) ON DELETE CASCADE,
    FOREIGN KEY (org_id, department_id) REFERENCES departments (org_id, id)
);
CREATE UNIQUE INDEX org_invitations_pending_email_uidx
    ON org_invitations (org_id, lower(email)) WHERE status = 'pending';
CREATE INDEX org_invitations_org_created_idx ON org_invitations (org_id, created_at, id);
-- Contagem mensal de convites externos por quem convida (limite do papel).
CREATE INDEX org_invitations_external_idx ON org_invitations (org_id, invited_by, created_at)
    WHERE external;

-- Validade do acesso de um convidado externo.
ALTER TABLE org_members ADD COLUMN access_expires_at TIMESTAMPTZ;
CREATE INDEX org_members_access_expires_idx ON org_members (access_expires_at)
    WHERE access_expires_at IS NOT NULL AND archived_at IS NULL;

-- Grupos do Odoo desta pessoa na última sincronização que os leu (NULL = nunca lidos).
ALTER TABLE org_members ADD COLUMN odoo_groups TEXT[];

-- Regras de entrada por organização. Sem linha = os valores por omissão, que são
-- o comportamento de hoje (criar conta na primeira entrada, sem restrição de
-- domínio, não suspender ao sair do Odoo).
CREATE TABLE org_entry_rules (
    org_id                        UUID PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
    create_account_on_first_login BOOLEAN NOT NULL DEFAULT TRUE,
    approved_domains              TEXT[] NOT NULL DEFAULT '{}',
    suspend_on_odoo_exit          BOOLEAN NOT NULL DEFAULT FALSE,
    external_guest_ttl_hours      INTEGER NOT NULL DEFAULT 24
                                      CHECK (external_guest_ttl_hours BETWEEN 1 AND 8760),
    updated_at                    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by                    UUID REFERENCES users(id) ON DELETE SET NULL
);

-- Resultado da última sincronização do directório (o que o ecrã mostra).
ALTER TABLE organizations ADD COLUMN odoo_last_sync JSONB;
