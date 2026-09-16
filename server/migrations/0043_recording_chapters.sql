-- Capítulos de uma gravação: gerados da transcrição (`auto`) ou escritos à mão.
CREATE TABLE IF NOT EXISTS recording_chapters (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    recording_id UUID NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    t_ms         BIGINT NOT NULL CHECK (t_ms >= 0),
    title        TEXT NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
    source       TEXT NOT NULL CHECK (source IN ('auto', 'manual')),
    created_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (recording_id, t_ms)
);
