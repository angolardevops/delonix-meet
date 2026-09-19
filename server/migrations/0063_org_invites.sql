-- Convites por link e suspensão de membros ("Utilizadores e convites" do
-- mockup DelonixUsers, #26).
--
-- Este produto não tem SMTP (nenhum `smtp`/`lettre`/`mailer` no server/) —
-- por isso o convite não é um email enviado, é um LINK com token que o admin
-- copia e partilha pelo canal que preferir. `org_invites` guarda esse token
-- em claro (não hash): ele É a chave de consulta pública
-- (`GET /api/invites/{token}`), não um segredo comparado no servidor, ao
-- contrário do `refresh_tokens.token_hash`.
--
-- `suspended_at` é DELIBERADAMENTE separado de `archived_at`: arquivar
-- (remove_employee) é remoção suave permanente; suspender é um bloqueio
-- reversível — a pessoa continua membro (histórico e auditoria intactos),
-- só perde acesso enquanto durar. Reactivar é instantâneo (NULL de volta).
ALTER TABLE org_members ADD COLUMN suspended_at TIMESTAMPTZ NULL;
ALTER TABLE org_members ADD COLUMN suspended_by UUID NULL REFERENCES users(id);

CREATE TABLE org_invites (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id       UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    email        TEXT NOT NULL,
    role         TEXT NOT NULL DEFAULT 'member',
    branch_id    UUID NULL REFERENCES branches(id) ON DELETE SET NULL,
    title        TEXT NOT NULL DEFAULT '',
    token        TEXT NOT NULL UNIQUE,
    invited_by   UUID NOT NULL REFERENCES users(id),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at   TIMESTAMPTZ NOT NULL,
    accepted_at  TIMESTAMPTZ NULL,
    revoked_at   TIMESTAMPTZ NULL
);
CREATE INDEX org_invites_org_idx ON org_invites(org_id);

-- Um único convite PENDENTE por email por organização de cada vez — evita
-- duas pessoas a gerar dois links para a mesma pessoa (qual é o válido?).
-- Um convite aceite ou revogado sai do âmbito do índice: pode convidar-se de
-- novo o mesmo email depois disso.
CREATE UNIQUE INDEX org_invites_pending_email_idx ON org_invites(org_id, email)
    WHERE accepted_at IS NULL AND revoked_at IS NULL;
