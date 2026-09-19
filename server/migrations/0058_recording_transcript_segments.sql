-- Transcrição com tempos.
--
-- O ai-worker (faster-whisper) produz segmentos com início, fim, língua e
-- confiança, e deitava tudo fora menos o texto — a legenda no leitor, o
-- capítulo automático e a pesquisa «em que minuto se disse isto» ficavam
-- impossíveis. Os segmentos passam a ficar guardados.
--
-- Forma de cada elemento de `transcript_segments`:
--   {"start_ms": 0, "end_ms": 4200, "text": "…", "confidence": 0.91}

ALTER TABLE recordings
    ADD COLUMN IF NOT EXISTS transcript_segments   JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS transcript_language   TEXT,
    -- Média de exp(avg_logprob) dos segmentos, entre 0 e 1.
    ADD COLUMN IF NOT EXISTS transcript_confidence REAL CHECK (transcript_confidence BETWEEN 0 AND 1),
    -- Preenchido quando a transcrição falhou; `transcribed_at` fica marcado na
    -- mesma para o worker não repetir a mesma gravação em ciclo.
    ADD COLUMN IF NOT EXISTS transcript_error      TEXT,
    ADD COLUMN IF NOT EXISTS chapters_generated_at TIMESTAMPTZ;

-- Pesquisa na transcrição e no nome. Configuração `simple`: as transcrições
-- são em várias línguas e um stemmer português estragava as inglesas.
ALTER TABLE recordings ADD COLUMN IF NOT EXISTS search_tsv tsvector
    GENERATED ALWAYS AS (
        to_tsvector('simple', coalesce(filename, '') || ' ' || coalesce(transcript, ''))
    ) STORED;
CREATE INDEX IF NOT EXISTS idx_recordings_search ON recordings USING GIN (search_tsv);
