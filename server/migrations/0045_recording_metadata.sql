-- Metadados, capítulos, comentários e pesquisa das gravações (G4–G6).
--
-- G4. `duration_secs`, `width`, `height` ficam NULL quando não se sabem: uma
-- gravação carregada pelo browser chega como bytes webm e o servidor não a
-- sonda; as do servidor recebem-nos no `recorder` (ver o comentário lá). NULL
-- é «não se sabe», nunca zero. O `title` é o nome que a pessoa dá; NULL = a UI
-- mostra o `filename`, que continua a ser o nome do ficheiro descarregado.
ALTER TABLE recordings
    ADD COLUMN IF NOT EXISTS duration_secs INT CHECK (duration_secs IS NULL OR duration_secs >= 0),
    ADD COLUMN IF NOT EXISTS width INT CHECK (width IS NULL OR width > 0),
    ADD COLUMN IF NOT EXISTS height INT CHECK (height IS NULL OR height > 0),
    ADD COLUMN IF NOT EXISTS category TEXT NOT NULL DEFAULT 'meeting'
        CHECK (category IN ('meeting', 'lecture', 'broadcast', 'other')),
    ADD COLUMN IF NOT EXISTS title TEXT CHECK (title IS NULL OR char_length(title) BETWEEN 1 AND 120);

-- G6. Pesquisa de texto na transcrição e no nome.
--
-- Configuração `simple` (sem stemming nem stop-words), e não `portuguese`: a
-- transcrição sai na língua da reunião (a UI tem pt, fr, en) e o stemmer
-- português estragava as outras duas — «meetings» e «réunions» deixavam de se
-- encontrar a si próprias. O custo é que «reunião» não encontra «reuniões»;
-- a pesquisa usa prefixos (`reuni:*`), que cobrem o caso comum de escrever o
-- começo da palavra.
--
-- Coluna gerada e não índice de expressão: a consulta usa a coluna pelo nome e
-- não tem de repetir a expressão letra a letra para o índice servir. O nome
-- pesa mais (A) do que a transcrição (B). Reescreve a tabela uma vez.
ALTER TABLE recordings
    ADD COLUMN IF NOT EXISTS search_vector tsvector GENERATED ALWAYS AS (
        setweight(to_tsvector('simple'::regconfig, coalesce(title, '') || ' ' || filename), 'A')
        || setweight(to_tsvector('simple'::regconfig, transcript), 'B')
    ) STORED;
CREATE INDEX IF NOT EXISTS idx_recordings_search ON recordings USING GIN (search_vector);

-- A biblioteca pagina por (created_at DESC, id DESC).
CREATE INDEX IF NOT EXISTS idx_recordings_page ON recordings (created_at DESC, id DESC);

-- G5. Capítulos: índice da gravação, escrito pelo dono ou por um admin.
CREATE TABLE IF NOT EXISTS recording_chapters (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    recording_id UUID NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    at_secs      INT NOT NULL CHECK (at_secs >= 0),
    title        TEXT NOT NULL CHECK (char_length(title) BETWEEN 1 AND 120),
    created_by   UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_recording_chapters_page
    ON recording_chapters (recording_id, at_secs, id);

-- G5. Comentários, com ou sem marca temporal. Apagar é LÓGICO (`deleted_at`):
-- a conversa de uma gravação é dado da organização e entra na retenção e na
-- auditoria como o resto; o `ON DELETE CASCADE` só actua quando a gravação sai.
-- O `body` entra já censurado pelo DLP.
CREATE TABLE IF NOT EXISTS recording_comments (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    recording_id UUID NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    at_secs      INT CHECK (at_secs IS NULL OR at_secs >= 0),
    body         TEXT NOT NULL,
    author_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    edited_at    TIMESTAMPTZ,
    deleted_at   TIMESTAMPTZ
);
-- A ordem da listagem: marca temporal (as sem marca no fim), depois criação.
CREATE INDEX IF NOT EXISTS idx_recording_comments_page
    ON recording_comments (recording_id, (COALESCE(at_secs, 2147483647)), created_at, id)
    WHERE deleted_at IS NULL;
