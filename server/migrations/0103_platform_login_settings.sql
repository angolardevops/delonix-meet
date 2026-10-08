-- Configuração de LOGIN da plataforma: esconder «criar organização» e/ou o
-- botão SSO no ecrã de entrada, antes de qualquer sessão existir.
--
-- Vivia em organizations.hide_org_creation/hide_sso_button, escrita por
-- QUALQUER admin de tenant em PUT /api/orgs/{org_id}/integrations/odoo.
-- GET /api/public/settings agregava com BOOL_OR sobre TODAS as organizações
-- com odoo_enabled=TRUE: o admin de uma única organização (mesmo recém-criada)
-- escondia o registo de contas e/ou o SSO para TODA a plataforma. É política
-- de instalação, não de tenant — passa a ser um único registo de PLATAFORMA
-- (id=1), escrito só por /api/operator/v1/login-settings
-- (require_platform_admin), seguindo o padrão de `platform_storage` (0030).
CREATE TABLE IF NOT EXISTS platform_login_settings (
    id                SERIAL PRIMARY KEY,
    hide_org_creation BOOLEAN NOT NULL DEFAULT FALSE,
    hide_sso_button   BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- As colunas por-organização desaparecem: nenhum admin de tenant volta a ter
-- uma coluna para escrever aqui, por engano ou de propósito.
ALTER TABLE organizations DROP COLUMN IF EXISTS hide_org_creation;
ALTER TABLE organizations DROP COLUMN IF EXISTS hide_sso_button;
