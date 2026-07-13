-- Registos de auditoria (trilha de eventos de segurança/administração).
-- Fecha o item "Registos de auditoria" da postura de segurança (antes stub).
-- Escrita best-effort nos pontos-chave (auth, membros, webhooks, api keys,
-- partilhas de gravação, definições); leitura só para admins da org.
-- org_id NULL = evento de âmbito do utilizador (ex.: login) — o admin vê-o
-- se o ator for membro da sua org.
CREATE TABLE IF NOT EXISTS audit_logs (
    id BIGSERIAL PRIMARY KEY,
    org_id UUID REFERENCES organizations(id) ON DELETE CASCADE,
    actor_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    action TEXT NOT NULL,
    target TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_audit_org_time ON audit_logs (org_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_actor_time ON audit_logs (actor_id, created_at DESC);
