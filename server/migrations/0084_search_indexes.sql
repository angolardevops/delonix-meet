-- Índices da pesquisa (ADR-0007 §4). Cada um serve uma consulta do
-- `server/src/search/` — o EXPLAIN com ≥ 100 k linhas está no relatório da
-- implementação. Nomes `idx_search_*` para se saber de onde vieram.

-- Gravações: ALINHA com a 0045 em vez de duplicar. A coluna gerada
-- `search_vector` e o índice `idx_recordings_search` mantêm nome e papel; só a
-- configuração passa de `simple` para `dlx_search` (acentos). Uma coluna
-- gerada não se altera: sai e volta, o que reescreve a tabela uma vez.
DROP INDEX IF EXISTS idx_recordings_search;
ALTER TABLE recordings DROP COLUMN IF EXISTS search_vector;
ALTER TABLE recordings
    ADD COLUMN search_vector tsvector GENERATED ALWAYS AS (
        setweight(to_tsvector('dlx_search'::regconfig, coalesce(title, '') || ' ' || filename), 'A')
        || setweight(to_tsvector('dlx_search'::regconfig, transcript), 'B')
    ) STORED;
CREATE INDEX idx_recordings_search ON recordings USING GIN (search_vector);
CREATE INDEX IF NOT EXISTS idx_search_recordings_name_trgm
    ON recordings USING GIN (dlx_fold(coalesce(title, '') || ' ' || filename) gin_trgm_ops);
-- «As minhas» e a visibilidade por quem carregou.
CREATE INDEX IF NOT EXISTS idx_search_recordings_uploader
    ON recordings (uploader_id, created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_search_recording_chapters_fts
    ON recording_chapters USING GIN (to_tsvector('dlx_search'::regconfig, title));
CREATE INDEX IF NOT EXISTS idx_search_recording_comments_fts
    ON recording_comments USING GIN (to_tsvector('dlx_search'::regconfig, body))
    WHERE deleted_at IS NULL;

-- Reuniões: título (A), descrição (B), acta (C).
ALTER TABLE meetings
    ADD COLUMN IF NOT EXISTS search_vector tsvector GENERATED ALWAYS AS (
        setweight(to_tsvector('dlx_search'::regconfig, title), 'A')
        || setweight(to_tsvector('dlx_search'::regconfig, description), 'B')
        || setweight(to_tsvector('dlx_search'::regconfig, minutes), 'C')
    ) STORED;
CREATE INDEX IF NOT EXISTS idx_search_meetings_fts ON meetings USING GIN (search_vector);
CREATE INDEX IF NOT EXISTS idx_search_meetings_title_trgm
    ON meetings USING GIN (dlx_fold(title) gin_trgm_ops);
CREATE INDEX IF NOT EXISTS idx_search_meetings_owner_starts
    ON meetings (owner_id, starts_at, id);

-- Pessoas: nome e email.
CREATE INDEX IF NOT EXISTS idx_search_users_trgm
    ON users USING GIN (dlx_fold(username || ' ' || email) gin_trgm_ops);

-- Quadros.
CREATE INDEX IF NOT EXISTS idx_search_whiteboards_trgm
    ON whiteboards USING GIN (dlx_fold(title || ' ' || room_code) gin_trgm_ops);
CREATE INDEX IF NOT EXISTS idx_search_whiteboards_org_page
    ON whiteboards (org_id, created_at DESC, id DESC);

-- Salas.
CREATE INDEX IF NOT EXISTS idx_search_rooms_trgm
    ON rooms USING GIN (dlx_fold(code || ' ' || name) gin_trgm_ops);

-- Mensagens de chat persistidas.
CREATE INDEX IF NOT EXISTS idx_search_room_chat_fts
    ON room_chat_messages USING GIN (to_tsvector('dlx_search'::regconfig, message));

-- Auditoria: índice, não coluna — a tabela recusa UPDATE por gatilho e a
-- cadeia de hash não deve depender de uma reescrita. Página por
-- (created_at DESC, id DESC) dentro da org.
CREATE INDEX IF NOT EXISTS idx_search_audit_trgm
    ON audit_logs USING GIN (dlx_fold(action || ' ' || target || ' ' || actor_name) gin_trgm_ops);
CREATE INDEX IF NOT EXISTS idx_search_audit_org_page
    ON audit_logs (org_id, created_at DESC, id DESC);
