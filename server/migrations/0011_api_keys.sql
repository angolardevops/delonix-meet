-- Chaves de API por organização (integração externa via REST /api/v1).
CREATE TABLE org_api_keys (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id       UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name         TEXT NOT NULL DEFAULT '',
    prefix       TEXT NOT NULL,             -- ex.: 'dlx_1a2b3c4d' (mostrado na UI)
    key_hash     TEXT NOT NULL,             -- SHA-256 da chave completa
    created_by   UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ
);
CREATE INDEX org_api_keys_org_idx ON org_api_keys(org_id);
CREATE UNIQUE INDEX org_api_keys_prefix_uidx ON org_api_keys(prefix);
