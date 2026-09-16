-- Legendas por língua, servidas ao leitor como WebVTT.
CREATE TABLE IF NOT EXISTS recording_captions (
    recording_id UUID NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    -- BCP 47 curto: `pt`, `pt-AO`, `en`, `zh`.
    lang         TEXT NOT NULL CHECK (lang ~ '^[a-z]{2,3}(-[A-Za-z0-9]{2,8})?$'),
    vtt          TEXT NOT NULL DEFAULT '',
    -- `upload` = enviada por uma pessoa; `transcript` = gerada dos segmentos;
    -- `translation` = traduzida pelo LLM local a partir da transcrição.
    source       TEXT NOT NULL CHECK (source IN ('upload', 'transcript', 'translation')),
    status       TEXT NOT NULL CHECK (status IN ('generating', 'draft', 'published', 'failed')),
    progress_pct SMALLINT CHECK (progress_pct BETWEEN 0 AND 100),
    error        TEXT,
    updated_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    published_at TIMESTAMPTZ,
    PRIMARY KEY (recording_id, lang)
);
