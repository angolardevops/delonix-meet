-- Domínio de produção + política de retenção por organização.
ALTER TABLE organizations ADD COLUMN domain TEXT NOT NULL DEFAULT '';
-- 0 = sem limite; >0 apaga gravações mais antigas que N dias (DLP-lite).
ALTER TABLE organizations ADD COLUMN retention_days INT NOT NULL DEFAULT 0;

-- Webhooks de saída por organização (Slack / Teams / Mattermost / genérico).
CREATE TABLE org_webhooks (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id     UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL,                 -- 'slack' | 'teams' | 'mattermost' | 'generic'
    url        TEXT NOT NULL,
    secret     TEXT NOT NULL DEFAULT '',      -- HMAC-SHA256 dos payloads genéricos
    events     TEXT NOT NULL DEFAULT 'meeting.created,meeting.started,recording.ready',
    active     BOOLEAN NOT NULL DEFAULT TRUE,
    created_by UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX org_webhooks_org_idx ON org_webhooks(org_id);
