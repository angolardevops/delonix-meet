-- Integração Odoo ↔ Delonix Meet
-- Token de integração + flags de UI por org; mapeamento de uid por utilizador.

ALTER TABLE organizations
  ADD COLUMN IF NOT EXISTS odoo_enabled        BOOLEAN NOT NULL DEFAULT FALSE,
  ADD COLUMN IF NOT EXISTS odoo_url            TEXT,
  ADD COLUMN IF NOT EXISTS odoo_db             TEXT,
  ADD COLUMN IF NOT EXISTS odoo_token_hash     TEXT,
  ADD COLUMN IF NOT EXISTS odoo_token_prefix   TEXT,
  ADD COLUMN IF NOT EXISTS odoo_admin_id       UUID REFERENCES users(id),
  ADD COLUMN IF NOT EXISTS odoo_synced_at      TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS hide_org_creation   BOOLEAN NOT NULL DEFAULT FALSE,
  ADD COLUMN IF NOT EXISTS hide_sso_button     BOOLEAN NOT NULL DEFAULT FALSE;

CREATE UNIQUE INDEX IF NOT EXISTS organizations_odoo_token_hash_idx
  ON organizations (odoo_token_hash)
  WHERE odoo_token_hash IS NOT NULL;

-- Utilizadores provisionados pelo Odoo: uid + flag para distinguir de registos manuais
ALTER TABLE users
  ADD COLUMN IF NOT EXISTS odoo_uid     INTEGER,
  ADD COLUMN IF NOT EXISTS odoo_managed BOOLEAN NOT NULL DEFAULT FALSE;
