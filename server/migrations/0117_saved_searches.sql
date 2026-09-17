-- Favoritos: pesquisas guardadas por pessoa, opcionalmente partilhadas com a
-- organização (ADR-0007 §6, docs/reference/pesquisa.md §5).
--
-- `org_id` é a pertença ACTIVA de quem guardou, escrita pelo servidor — nunca
-- vem do corpo. Uma partilhada sem organização não existe (CHECK). A `query`
-- guarda-se como a pessoa a escreveu e valida-se ao gravar E ao ler: um
-- schema que mudou devolve `valid: false`, não faz desaparecer o favorito.
CREATE TABLE IF NOT EXISTS saved_searches (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    org_id      UUID REFERENCES organizations(id) ON DELETE CASCADE,
    resource    TEXT NOT NULL CHECK (resource ~ '^[a-z_]{1,40}$'),
    name        TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 80),
    query       JSONB NOT NULL,
    shared      BOOLEAN NOT NULL DEFAULT FALSE,
    is_default  BOOLEAN NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (NOT shared OR org_id IS NOT NULL)
);
CREATE UNIQUE INDEX IF NOT EXISTS saved_searches_name_uidx
    ON saved_searches (user_id, resource, lower(name));
CREATE UNIQUE INDEX IF NOT EXISTS saved_searches_default_uidx
    ON saved_searches (user_id, resource) WHERE is_default;
CREATE INDEX IF NOT EXISTS saved_searches_owner_page
    ON saved_searches (user_id, created_at, id);
CREATE INDEX IF NOT EXISTS saved_searches_shared_page
    ON saved_searches (org_id, created_at, id) WHERE shared;
