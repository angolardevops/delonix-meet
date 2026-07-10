-- SSO / OIDC: configuração por organização.
-- Cada org pode ter UM provedor OIDC (Azure AD, Google Workspace, Okta, etc.).
-- `enforce_sso` impede o login por email/password — só SSO entra.
CREATE TABLE org_sso_configs (
    org_id        UUID PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
    issuer_url    TEXT NOT NULL,           -- ex: https://accounts.google.com
    client_id     TEXT NOT NULL,
    client_secret TEXT NOT NULL,           -- encriptado em repouso (app-level)
    enforce_sso   BOOLEAN NOT NULL DEFAULT FALSE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Login provisionado por SSO: regista o IdP subject (sub) para evitar
-- re-provisioning e garantir idempotência do JIT.
ALTER TABLE users ADD COLUMN sso_provider TEXT NOT NULL DEFAULT '';
ALTER TABLE users ADD COLUMN sso_subject  TEXT NOT NULL DEFAULT '';

-- Index para lookup rápido por (provider, subject) no callback OIDC.
CREATE INDEX users_sso_idx ON users(sso_provider, sso_subject)
    WHERE sso_provider <> '';
