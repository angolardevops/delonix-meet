-- Gravações: o contrato de dados da UI nova (R183).
--
-- Havia dois servidores a divergir. A UI nova foi construída contra um modelo
-- de gravação mais rico (milissegundos, codecs, miniatura, tipo de sessão,
-- descrição, etiquetas, publicação para a organização) e esta linha tinha o
-- seu próprio (segundos, `category`, `title`). Esta linha adopta os NOMES, as
-- UNIDADES e os ENUMS da UI; a estrutura (rotas por superfície, envelope de
-- erro, regra de acesso única) continua a desta linha.
--
-- Decisões que não estão nos nomes das colunas:
--  * `title` sai. A UI renomeia pelo `filename` (é o nome que mostra e com que
--    se descarrega); dois nomes para a mesma coisa divergiam, e um campo que a
--    UI nunca escreve não se mantém. Um título já dado passa a ser o `filename`.
--  * `status` NÃO ganha `processing` nem `transcribing`. O `recorder` só insere
--    a linha depois de o ffmpeg acabar (nunca se observa «a compor»), e «a
--    transcrever» deriva da reserva da fila (0041) — um estado guardado em duas
--    colunas é um estado que se contradiz. A API deriva os dois.
--  * `transcript_error` NÃO se cria: já existe `transcription_error` (0041),
--    escrito pelo `TranscriptionService`. A API expõe-o como `error`.

ALTER TABLE recordings
    -- Medidos com ffprobe (`media_probe`). NULL = não foi possível medir.
    ADD COLUMN duration_ms   BIGINT   CHECK (duration_ms IS NULL OR duration_ms >= 0),
    ADD COLUMN fps           REAL     CHECK (fps IS NULL OR fps > 0),
    ADD COLUMN video_codec   TEXT,
    ADD COLUMN audio_codec   TEXT,
    -- A miniatura vive ao lado do ficheiro: RECORDINGS_DIR/<id>.jpg.
    ADD COLUMN has_thumbnail BOOLEAN  NOT NULL DEFAULT false,
    ADD COLUMN probed_at     TIMESTAMPTZ,
    -- Tipo de sessão (formato da sala, ou declarado no upload).
    ADD COLUMN kind TEXT NOT NULL DEFAULT 'meeting'
        CHECK (kind IN ('meeting', 'training', 'broadcast', 'hybrid')),
    -- Progresso do passo em curso. Nenhum escritor nesta linha ainda: fica NULL
    -- (a API devolve `null`, nunca um número inventado).
    ADD COLUMN progress_pct  SMALLINT CHECK (progress_pct IS NULL OR progress_pct BETWEEN 0 AND 100),
    ADD COLUMN progress_at   TIMESTAMPTZ,
    -- Transcrição com tempos: [{"start_ms","end_ms","text","confidence"}].
    ADD COLUMN transcript_segments JSONB NOT NULL DEFAULT '[]'::jsonb
        CHECK (jsonb_typeof(transcript_segments) = 'array'),
    ADD COLUMN transcript_language   TEXT,
    ADD COLUMN transcript_confidence REAL
        CHECK (transcript_confidence IS NULL OR transcript_confidence BETWEEN 0 AND 1),
    ADD COLUMN chapters_generated_at TIMESTAMPTZ,
    -- Descrição, etiquetas e publicação.
    ADD COLUMN description TEXT NOT NULL DEFAULT '' CHECK (char_length(description) <= 8000),
    ADD COLUMN tags        TEXT[] NOT NULL DEFAULT '{}',
    -- private — quem participou, quem carregou, com quem foi partilhada, e quem
    --           a gere (admin activo da org do autor);
    -- org     — além desses, os membros ACTIVOS de uma organização do autor.
    -- Nunca público por esta coluna: o link público é `recording_share_links`.
    ADD COLUMN visibility  TEXT NOT NULL DEFAULT 'private' CHECK (visibility IN ('private', 'org')),
    ADD COLUMN published_at TIMESTAMPTZ;

-- Conversões dos dados que já existem.
UPDATE recordings SET duration_ms = duration_secs::bigint * 1000 WHERE duration_secs IS NOT NULL;
UPDATE recordings SET kind = CASE category
        WHEN 'lecture'   THEN 'training'
        WHEN 'broadcast' THEN 'broadcast'
        ELSE 'meeting' END;   -- meeting → meeting, other → meeting
UPDATE recordings SET filename = title WHERE title IS NOT NULL;

-- Publicada ⇔ visível à organização. Duas colunas que se podiam contradizer
-- (`org` sem data, ou data com `private`) passam a não poder.
ALTER TABLE recordings ADD CONSTRAINT recordings_publication_check
    CHECK ((visibility = 'org') = (published_at IS NOT NULL));

-- A coluna de pesquisa gerada depende de `title`: sai antes dela e volta com a
-- descrição e as etiquetas.
DROP INDEX IF EXISTS idx_recordings_search;
ALTER TABLE recordings DROP COLUMN search_vector;
ALTER TABLE recordings DROP COLUMN duration_secs, DROP COLUMN category, DROP COLUMN title;

-- `array_to_string` é STABLE (depende da saída do tipo do elemento) e uma coluna
-- gerada só aceita expressões IMMUTABLE. Para `text[]` a saída não muda.
CREATE FUNCTION recording_tags_text(tags TEXT[]) RETURNS TEXT
    LANGUAGE sql IMMUTABLE PARALLEL SAFE
    AS $$ SELECT coalesce(array_to_string(tags, ' '), '') $$;

-- Configuração `simple`, como na 0045 (transcrições em várias línguas). O nome
-- e as etiquetas pesam mais (A) do que a descrição (B) e a transcrição (C).
ALTER TABLE recordings
    ADD COLUMN search_vector tsvector GENERATED ALWAYS AS (
        setweight(to_tsvector('simple'::regconfig,
                  filename || ' ' || recording_tags_text(tags)), 'A')
        || setweight(to_tsvector('simple'::regconfig, description), 'B')
        || setweight(to_tsvector('simple'::regconfig, coalesce(transcript, '')), 'C')
    ) STORED;
CREATE INDEX idx_recordings_search ON recordings USING GIN (search_vector);
CREATE INDEX idx_recordings_published
    ON recordings (published_at DESC) WHERE published_at IS NOT NULL;
