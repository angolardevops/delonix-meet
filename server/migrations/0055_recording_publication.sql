-- Descrição, etiquetas e publicação.
--
-- `visibility`:
--   private — quem participou na sala, quem carregou, e com quem foi partilhada;
--   org     — além desses, os membros ACTIVOS de uma organização do autor.
-- Nunca há visibilidade pública por esta coluna: o link público continua a ser
-- `recording_share_links`, com token e prazo.
ALTER TABLE recordings
    ADD COLUMN IF NOT EXISTS description  TEXT NOT NULL DEFAULT ''
        CHECK (char_length(description) <= 8000),
    ADD COLUMN IF NOT EXISTS tags         TEXT[] NOT NULL DEFAULT '{}',
    ADD COLUMN IF NOT EXISTS visibility   TEXT NOT NULL DEFAULT 'private'
        CHECK (visibility IN ('private', 'org')),
    ADD COLUMN IF NOT EXISTS published_at TIMESTAMPTZ;
CREATE INDEX IF NOT EXISTS idx_recordings_tags ON recordings USING GIN (tags);
CREATE INDEX IF NOT EXISTS idx_recordings_published
    ON recordings (published_at DESC) WHERE published_at IS NOT NULL;
