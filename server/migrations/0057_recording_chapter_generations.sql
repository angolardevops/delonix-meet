-- Geração de capítulos pelo LLM local: o ESTADO do trabalho assíncrono
-- (`POST /api/recordings/{recording_id}/chapters/generate` → 202, lido em
-- `GET …/chapters/generation`). Número de trabalho: renumera-se na integração.
--
-- Porquê uma tabela e não colunas em `recordings`: o estado de um trabalho
-- (quem pediu, quando, com que resultado, porque falhou) não é um atributo da
-- gravação, e `recordings.chapters_generated_at` (0052) continua a dizer só o
-- que diz — que houve uma geração BEM SUCEDIDA. Uma resposta sem capítulos
-- utilizáveis fica aqui como `failed` e NÃO toca nessa coluna (defeito B12 da
-- linha antiga, em que a gravação ficava marcada como gerada para sempre).
--
-- Uma linha por gravação: a última geração. Um trabalho `running` que deixou
-- de dar sinal (o pod morreu a meio) é tratado como interrompido pela API a
-- partir de `updated_at`, sem precisar de um varredor.
CREATE TABLE recording_chapter_generations (
    recording_id  UUID PRIMARY KEY REFERENCES recordings(id) ON DELETE CASCADE,
    status        TEXT NOT NULL CHECK (status IN ('running', 'succeeded', 'failed')),
    -- Código estável da falha (`ai.bad_response`, `ai.timeout`, …) e a
    -- mensagem para pessoas. Só com `failed`.
    error_code    TEXT,
    error         TEXT,
    -- Capítulos automáticos gravados. Só com `succeeded`.
    chapter_count INTEGER CHECK (chapter_count IS NULL OR chapter_count >= 0),
    requested_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    started_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at   TIMESTAMPTZ,
    CONSTRAINT recording_chapter_generations_state_check CHECK (
        (status = 'running'   AND finished_at IS NULL AND error_code IS NULL AND chapter_count IS NULL)
     OR (status = 'succeeded' AND finished_at IS NOT NULL AND error_code IS NULL AND chapter_count IS NOT NULL)
     OR (status = 'failed'    AND finished_at IS NOT NULL AND error_code IS NOT NULL)
    )
);
