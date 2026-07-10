-- Links públicos de partilha de gravações: token único, expiração opcional,
-- password opcional (hash argon2). O acesso público não requer autenticação.
CREATE TABLE recording_share_links (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    recording_id UUID        NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    token        TEXT        NOT NULL UNIQUE,
    password_hash TEXT,                   -- NULL = sem password
    expires_at   TIMESTAMPTZ,             -- NULL = nunca expira
    created_by   UUID        NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX rec_share_token_idx ON recording_share_links(token);
-- Só um link por gravação (simplifica a UI — sempre um link ativo por rec).
CREATE UNIQUE INDEX rec_share_one_per_rec ON recording_share_links(recording_id);
