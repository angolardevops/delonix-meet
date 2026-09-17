-- «Os meus dados»: pedidos de exportação pessoal, assíncronos.
CREATE TABLE IF NOT EXISTS data_exports (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status       TEXT NOT NULL DEFAULT 'queued'
                 CHECK (status IN ('queued', 'running', 'ready', 'failed', 'expired')),
    -- Motivo de falha para pessoas (nunca o erro interno cru).
    error        TEXT,
    size_bytes   BIGINT,
    -- Contagens do que foi incluído (perfil, gravações, transcrições, actividade).
    summary      JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at   TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    expires_at   TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS data_exports_user_idx ON data_exports (user_id, created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS data_exports_queue_idx ON data_exports (created_at) WHERE status IN ('queued', 'running');
-- No máximo UM trabalho activo por pessoa: a regra do domínio responde
-- primeiro com código estável; este índice é o árbitro entre pedidos simultâneos.
CREATE UNIQUE INDEX IF NOT EXISTS data_exports_one_active_uidx
    ON data_exports (user_id) WHERE status IN ('queued', 'running');
