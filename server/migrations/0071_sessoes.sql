-- Sessões da própria conta: listar, terminar uma, terminar todas as outras.
--
-- Antes desta migração não havia SESSÃO: havia refresh tokens soltos, e o
-- access token (JWT de 15 min) não dizia de que login vinha. Terminar «o
-- iPhone» era impossível sem revogar tudo — e mesmo revogando, o access token
-- continuava a abrir a API até expirar.
--
-- Agora:
--   - cada login cria uma `user_sessions`; o refresh token roda DENTRO dela;
--   - o access token leva `sid`, e o extractor de sessão recusa um `sid`
--     revogado — terminar é imediato, não «daqui a 15 min»;
--   - `reauthenticated_at` é a prova recente de identidade que alterar
--     factores exige.
CREATE TABLE IF NOT EXISTS user_sessions (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id             UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- `password` | `mfa` | `passkey` | `sso` | `odoo` | `legacy`
    auth_method         TEXT NOT NULL CHECK (auth_method IN ('password', 'mfa', 'passkey', 'sso', 'odoo', 'legacy')),
    user_agent          TEXT NOT NULL DEFAULT '' CHECK (char_length(user_agent) <= 512),
    -- IP completo, só para a auditoria e para o dono o ver mascarado.
    ip                  TEXT NOT NULL DEFAULT '' CHECK (char_length(ip) <= 64),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    reauthenticated_at  TIMESTAMPTZ,
    revoked_at          TIMESTAMPTZ,
    revoked_reason      TEXT CHECK (revoked_reason IN ('logout', 'user_revoked', 'user_revoked_others', 'expired'))
);
CREATE INDEX IF NOT EXISTS user_sessions_user_active_idx
    ON user_sessions (user_id, last_seen_at DESC) WHERE revoked_at IS NULL;

ALTER TABLE refresh_tokens ADD COLUMN IF NOT EXISTS session_id UUID
    REFERENCES user_sessions(id) ON DELETE CASCADE;
CREATE INDEX IF NOT EXISTS refresh_tokens_session_idx ON refresh_tokens (session_id);

-- Backfill: cada refresh token VIVO herdado passa a ser uma sessão `legacy`
-- (sem dispositivo conhecido). Assim a lista não esconde logins antigos que
-- ainda abrem a conta — e terminá-los funciona. Tokens mortos ficam sem
-- sessão: não abrem nada.
DO $$
DECLARE r RECORD; sid UUID;
BEGIN
    FOR r IN SELECT token_hash, user_id, created_at FROM refresh_tokens
             WHERE session_id IS NULL AND NOT revoked AND expires_at > now()
    LOOP
        INSERT INTO user_sessions (user_id, auth_method, created_at, last_seen_at)
        VALUES (r.user_id, 'legacy', r.created_at, r.created_at)
        RETURNING id INTO sid;
        UPDATE refresh_tokens SET session_id = sid WHERE token_hash = r.token_hash;
    END LOOP;
END $$;
