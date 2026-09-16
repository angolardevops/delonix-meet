-- Metadados de media e ciclo de vida de uma gravação.
--
-- Até aqui a biblioteca só sabia o nome e o tamanho: a duração, a resolução e o
-- codec eram lidos do ficheiro NO BROWSER, depois de o descarregar inteiro. A
-- lista não os podia mostrar, e o filtro «4K» não tinha onde se apoiar.
--
-- Preenchidos pelo servidor com `ffprobe` (recorder::finalize e upload). NULL
-- quer dizer «não foi possível medir» — nunca um valor inventado.

ALTER TABLE recordings
    ADD COLUMN IF NOT EXISTS duration_ms  BIGINT CHECK (duration_ms >= 0),
    ADD COLUMN IF NOT EXISTS width        INT    CHECK (width > 0),
    ADD COLUMN IF NOT EXISTS height       INT    CHECK (height > 0),
    ADD COLUMN IF NOT EXISTS fps          REAL   CHECK (fps > 0),
    ADD COLUMN IF NOT EXISTS video_codec  TEXT,
    ADD COLUMN IF NOT EXISTS audio_codec  TEXT,
    -- A miniatura vive ao lado do ficheiro: RECORDINGS_DIR/<id>.jpg.
    ADD COLUMN IF NOT EXISTS has_thumbnail BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS probed_at    TIMESTAMPTZ,
    -- Tipo de sessão. Parte do formato da sala; o estúdio pode declará-lo no upload.
    ADD COLUMN IF NOT EXISTS kind TEXT NOT NULL DEFAULT 'meeting'
        CHECK (kind IN ('meeting', 'training', 'broadcast', 'hybrid')),
    -- Progresso do passo em curso (composição ou transcrição). NULL fora dele.
    ADD COLUMN IF NOT EXISTS progress_pct SMALLINT CHECK (progress_pct BETWEEN 0 AND 100),
    -- Último sinal de vida de quem processa: sem ele, um pod que morre a meio
    -- deixava a gravação «a processar» para sempre.
    ADD COLUMN IF NOT EXISTS progress_at  TIMESTAMPTZ;

-- As gravações que já existem herdam o formato da sala.
UPDATE recordings r SET kind = 'training'
  FROM rooms rm WHERE rm.id = r.room_id AND rm.format = 'training';

-- Estados: `processing` (a compor), `transcribing` (há ficheiro; o ai-worker
-- está a transcrever), `ready`, `failed`. «Publicada» não é um estado do
-- ficheiro: é `published_at` (0055).
ALTER TABLE recordings DROP CONSTRAINT IF EXISTS recordings_status_check;
ALTER TABLE recordings ADD CONSTRAINT recordings_status_check
    CHECK (status IN ('processing', 'transcribing', 'ready', 'failed'));

CREATE INDEX IF NOT EXISTS idx_recordings_processing
    ON recordings (progress_at) WHERE status IN ('processing', 'transcribing');
