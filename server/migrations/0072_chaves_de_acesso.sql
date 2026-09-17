-- Chaves de acesso (WebAuthn / passkeys) como SEGUNDO factor — ADR-0011.
--
-- `credential` é o `Passkey` do webauthn-rs serializado (chave PÚBLICA, id da
-- credencial, contador). Não é segredo: quem o lê não consegue autenticar-se.
CREATE TABLE IF NOT EXISTS user_passkeys (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- id da credencial em base64url: único na instalação (um autenticador não
    -- regista a mesma credencial em duas contas).
    credential_id TEXT NOT NULL UNIQUE CHECK (char_length(credential_id) <= 1400),
    credential    JSONB NOT NULL,
    name          TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 60),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at  TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS user_passkeys_user_idx ON user_passkeys (user_id, created_at);

-- Estado de uma cerimónia WebAuthn entre o «start» e o «finish». Guardado na
-- base (e não em memória) para o «finish» poder cair noutro nó; consumido UMA
-- vez (`DELETE … RETURNING`), com validade curta — um desafio reutilizável
-- seria um replay.
CREATE TABLE IF NOT EXISTS webauthn_ceremonies (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('registration', 'authentication')),
    state      JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS webauthn_ceremonies_expiry_idx ON webauthn_ceremonies (expires_at);
