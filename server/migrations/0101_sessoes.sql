-- Sessões da própria conta: terminar de imediato, terminar todas as outras,
-- reautenticação recente.
--
-- NUMERAÇÃO: era a 0071 da frente B; entra como 0101.
--
-- A 0065 já deu identidade à sessão: `refresh_tokens.session_id` nasce no
-- login e viaja a cada refresh, e é dela que a lista de sessões se lê
-- (`account::list_sessions`). O que faltava era o ESTADO da sessão — sem ele,
-- terminar «o iPhone» revogava o refresh token mas o access token (JWT de
-- 15 min) continuava a abrir a API até expirar.
--
-- Agora:
--   - `user_sessions.id` É o `refresh_tokens.session_id` (a mesma identidade,
--     sem segunda numeração);
--   - o access token leva `sid`, e o extractor de sessão recusa um `sid`
--     revogado — terminar é imediato, não «daqui a 15 min»;
--   - `reauthenticated_at` é a prova recente de identidade que alterar
--     factores exige.
--
-- Sem chave estrangeira de `refresh_tokens.session_id`: a coluna da 0065 é
-- NOT NULL com omissão aleatória, e os tokens mortos herdados têm ids sem
-- sessão (não abrem nada).
CREATE TABLE IF NOT EXISTS user_sessions (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id             UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- `password` | `mfa` | `passkey` | `sso` | `odoo` | `legacy`
    auth_method         TEXT NOT NULL CHECK (auth_method IN ('password', 'mfa', 'passkey', 'sso', 'odoo', 'legacy')),
    user_agent          TEXT NOT NULL DEFAULT '' CHECK (char_length(user_agent) <= 512),
    -- IP completo, só para a auditoria e para o dono o ver.
    ip                  TEXT NOT NULL DEFAULT '' CHECK (char_length(ip) <= 64),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    reauthenticated_at  TIMESTAMPTZ,
    revoked_at          TIMESTAMPTZ,
    revoked_reason      TEXT CHECK (revoked_reason IN ('logout', 'user_revoked', 'user_revoked_others', 'expired'))
);
CREATE INDEX IF NOT EXISTS user_sessions_user_active_idx
    ON user_sessions (user_id, last_seen_at DESC) WHERE revoked_at IS NULL;

-- Backfill: cada sessão com refresh token VIVO passa a ter estado, como
-- `legacy` (sem prova de identidade recente). Assim terminá-la funciona já.
-- Tokens mortos ficam sem sessão: não abrem nada.
INSERT INTO user_sessions (id, user_id, auth_method, user_agent, ip, created_at, last_seen_at)
SELECT DISTINCT ON (session_id)
       session_id, user_id, 'legacy',
       left(coalesce(user_agent, ''), 512), left(coalesce(ip_address, ''), 64),
       session_started_at, created_at
  FROM refresh_tokens
 WHERE NOT revoked AND expires_at > now()
 ORDER BY session_id, created_at DESC
ON CONFLICT (id) DO NOTHING;
