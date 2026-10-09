-- ════════════════════════════════════════════════════════════════════════
--  Reposição de password emitida por um administrador.
--
--  PORQUE EXISTE. Até aqui o ÚNICO caminho para mudar uma password era a
--  própria pessoa, com a password actual ou uma sessão recente
--  (`users.rs`, `sessions::prove_current_password` / `require_recent`). Quem a
--  perdia ficava fora para sempre: não havia reposição pela própria pessoa nem
--  por administrador — nenhuma rota escrevia o `password_hash` de outra conta.
--
--  Esta tabela é a primeira metade da E3 do plano de lacunas, e a metade que
--  NÃO depende de correio electrónico: o administrador emite, o token aparece
--  UMA vez na resposta, e ele entrega-o pelo canal que já usa para os convites.
--  A reposição pela própria pessoa (por email) precisa do D7 decidido e entra
--  depois, nesta mesma tabela.
--
--  O token é a CREDENCIAL: guarda-se só o hash (SHA-256) e o prefixo para o
--  reconhecer num registo. Uso único. É o mesmo desenho do `org_invitations`
--  (migração 0055), de propósito — não se inventa um segundo molde de token.
-- ════════════════════════════════════════════════════════════════════════

CREATE TABLE password_resets (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- A organização em cujo nome o administrador agiu. Fica para a auditoria e
    -- para o isolamento: um administrador de outra org não pode repor aqui.
    org_id       UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    token_hash   TEXT NOT NULL UNIQUE,
    token_prefix TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'pending'
                     CHECK (status IN ('pending', 'used', 'revoked', 'expired')),
    expires_at   TIMESTAMPTZ NOT NULL,
    issued_by    UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    used_at      TIMESTAMPTZ
);

-- Uma pendente por pessoa: emitir outra invalida a anterior (o handler fá-lo
-- numa transacção). Sem isto, dois administradores a emitir ao mesmo tempo
-- deixavam dois tokens válidos, e revogar um não fechava o outro.
CREATE UNIQUE INDEX password_resets_pending_user_uidx
    ON password_resets (user_id) WHERE status = 'pending';

-- A listagem do administrador: as reposições de uma org, por data.
CREATE INDEX password_resets_org_created_idx
    ON password_resets (org_id, created_at DESC, id);
