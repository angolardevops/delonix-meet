-- SMS: preferências pessoais, política de envio da org, telefone por pertença.
--
-- O opt-out é da CONTA, não da organização — "não quero SMS" não muda por
-- entrar noutra empresa. O telefone é da PERTENÇA (org_members): a mesma
-- pessoa pode ter números diferentes em organizações diferentes (hoje só
-- `manual`; `phone_source = 'odoo'` fica reservado para quando houver
-- sincronização do directório — não se inventa aqui).
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS sms_contact_opt_out BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS sms_meeting_opt_out BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE organizations
    ADD COLUMN IF NOT EXISTS sms_send_policy TEXT NOT NULL DEFAULT 'admins'
        CHECK (sms_send_policy IN ('admins', 'members'));

ALTER TABLE org_members
    ADD COLUMN IF NOT EXISTS phone TEXT,
    ADD COLUMN IF NOT EXISTS phone_source TEXT
        CHECK (phone_source IS NULL OR phone_source IN ('manual', 'odoo')),
    ADD CONSTRAINT org_members_phone_source_needs_phone
        CHECK (phone_source IS NULL OR phone IS NOT NULL);
