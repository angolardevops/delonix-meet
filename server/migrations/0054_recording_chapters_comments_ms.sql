-- Capítulos e comentários em milissegundos, com os nomes da UI (R183).

-- ---------------------------------------------------------------------------
--  Capítulos: at_secs → t_ms, `source` (auto | manual), um por instante.
-- ---------------------------------------------------------------------------
ALTER TABLE recording_chapters
    ADD COLUMN t_ms   BIGINT,
    ADD COLUMN source TEXT NOT NULL DEFAULT 'manual' CHECK (source IN ('auto', 'manual'));

-- A 0045 permitia dois capítulos no mesmo segundo; a unicidade nova é por
-- milissegundo. Em vez de apagar um deles, afasta-se o segundo 1 ms (e o
-- terceiro 2 ms…), pela ordem de criação.
UPDATE recording_chapters c SET t_ms = x.t_ms
  FROM (SELECT id,
               at_secs::bigint * 1000
               + row_number() OVER (PARTITION BY recording_id, at_secs ORDER BY created_at, id) - 1
                 AS t_ms
          FROM recording_chapters) x
 WHERE x.id = c.id;

ALTER TABLE recording_chapters
    ALTER COLUMN t_ms SET NOT NULL,
    ADD CONSTRAINT recording_chapters_t_ms_check CHECK (t_ms >= 0),
    ADD CONSTRAINT recording_chapters_recording_id_t_ms_key UNIQUE (recording_id, t_ms);
DROP INDEX IF EXISTS idx_recording_chapters_page;
ALTER TABLE recording_chapters DROP COLUMN at_secs;

-- Títulos até 200 caracteres (a UI), e o autor pode faltar: um capítulo
-- automático não tem autor, e o de uma conta apagada fica.
ALTER TABLE recording_chapters DROP CONSTRAINT IF EXISTS recording_chapters_title_check;
ALTER TABLE recording_chapters ADD CONSTRAINT recording_chapters_title_check
    CHECK (char_length(title) BETWEEN 1 AND 200);
ALTER TABLE recording_chapters ALTER COLUMN created_by DROP NOT NULL;
ALTER TABLE recording_chapters DROP CONSTRAINT IF EXISTS recording_chapters_created_by_fkey;
ALTER TABLE recording_chapters ADD CONSTRAINT recording_chapters_created_by_fkey
    FOREIGN KEY (created_by) REFERENCES users(id) ON DELETE SET NULL;

-- ---------------------------------------------------------------------------
--  Comentários: at_secs → t_ms, author_id → user_id, corpo 1..2000.
--  O apagar continua LÓGICO (`deleted_at`, 0045): a conversa é dado da
--  organização e entra na retenção e na auditoria como o resto.
-- ---------------------------------------------------------------------------
ALTER TABLE recording_comments
    ADD COLUMN t_ms BIGINT CHECK (t_ms IS NULL OR t_ms >= 0);
UPDATE recording_comments SET t_ms = at_secs::bigint * 1000 WHERE at_secs IS NOT NULL;
DROP INDEX IF EXISTS idx_recording_comments_page;
ALTER TABLE recording_comments DROP COLUMN at_secs;
ALTER TABLE recording_comments RENAME COLUMN author_id TO user_id;
ALTER TABLE recording_comments ADD CONSTRAINT recording_comments_body_check
    CHECK (char_length(body) BETWEEN 1 AND 2000);

-- A ordem da listagem: marca temporal (as sem marca no fim), depois criação.
CREATE INDEX idx_recording_comments_page
    ON recording_comments (recording_id, (COALESCE(t_ms, 9223372036854775807)), created_at, id)
    WHERE deleted_at IS NULL;
