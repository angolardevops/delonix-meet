-- Legendas por língua e visualizações de uma gravação (R183).

-- Legendas servidas ao leitor como WebVTT. Quem só VÊ a gravação vê só as
-- publicadas; rascunhos, falhadas e as que estão a ser geradas são de quem gere.
CREATE TABLE recording_captions (
    recording_id UUID NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    -- BCP 47 curto: `pt`, `pt-AO`, `en`, `zh-Hans`.
    lang         TEXT NOT NULL CHECK (lang ~ '^[a-z]{2,3}(-[A-Za-z0-9]{2,8})?$'),
    vtt          TEXT NOT NULL DEFAULT '' CHECK (octet_length(vtt) <= 2097152),
    -- `upload` = enviada por uma pessoa; `transcript` = gerada dos segmentos;
    -- `translation` = traduzida pelo LLM local. Só `upload` tem escritor nesta
    -- linha; as outras duas chegam com a geração (fora deste lote).
    source       TEXT NOT NULL CHECK (source IN ('upload', 'transcript', 'translation')),
    status       TEXT NOT NULL CHECK (status IN ('generating', 'draft', 'published', 'failed')),
    progress_pct SMALLINT CHECK (progress_pct IS NULL OR progress_pct BETWEEN 0 AND 100),
    error        TEXT,
    updated_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    published_at TIMESTAMPTZ,
    PRIMARY KEY (recording_id, lang),
    CONSTRAINT recording_captions_published_check
        CHECK ((status = 'published') = (published_at IS NOT NULL))
);

-- Uma visualização por pessoa por dia: recarregar o leitor não é audiência.
-- O dia é o do servidor da base (UTC em produção).
CREATE TABLE recording_views (
    recording_id UUID NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    viewed_on    DATE NOT NULL DEFAULT CURRENT_DATE,
    first_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (recording_id, user_id, viewed_on)
);
