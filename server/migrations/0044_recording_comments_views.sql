-- Comentários com marca temporal e visualizações.
CREATE TABLE IF NOT EXISTS recording_comments (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    recording_id UUID NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Instante do vídeo a que o comentário se refere; NULL = à gravação inteira.
    t_ms         BIGINT CHECK (t_ms >= 0),
    body         TEXT NOT NULL CHECK (char_length(body) BETWEEN 1 AND 2000),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_recording_comments_page
    ON recording_comments (recording_id, created_at, id);

-- Uma visualização por pessoa por dia: recarregar o leitor não é audiência.
CREATE TABLE IF NOT EXISTS recording_views (
    recording_id UUID NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    viewed_on    DATE NOT NULL DEFAULT CURRENT_DATE,
    first_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (recording_id, user_id, viewed_on)
);
