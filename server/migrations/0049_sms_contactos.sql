-- SMS a contactos e a convidados de reunião (ADR-0005, extensão «contactos»).
--
-- NUMERAÇÃO: nasceu 0040 na `main` a 2026-09-16 e colidia com a linha da UI
-- nova (0039–0047). Renumerada para 0049 na `frontend/chamadas-voz-sms`, a seguir
-- ao gateway (0039 → 0048). Uma base que já correu 0039/0040 da `main` precisa
-- de reconciliação da `_sqlx_migrations` na integração.

-- 1. Telefone do MEMBRO. Fica em `org_members` e não em `users`: o número que
--    interessa é o contacto da pessoa NESTA organização (o telemóvel de serviço
--    que o Odoo da empresa conhece), deixa de ser alcançável quando o membro é
--    arquivado, e a sincronização do directório é feita por organização.
--
--    `phone_source` diz quem escreveu o valor, e é a regra de sincronização:
--      NULL     → sem número; a sincronização do directório pode preencher
--      'odoo'   → veio do directório; a sincronização seguinte pode mudá-lo ou limpá-lo
--      'manual' → editado pelo próprio ou por um admin; a sincronização NUNCA lhe toca
--    Voltar a seguir o directório é um acto explícito (`follow_directory`).
ALTER TABLE org_members ADD COLUMN IF NOT EXISTS phone_e164 TEXT;
ALTER TABLE org_members ADD COLUMN IF NOT EXISTS phone_source TEXT
    CHECK (phone_source IN ('odoo', 'manual'));
ALTER TABLE org_members ADD COLUMN IF NOT EXISTS phone_updated_at TIMESTAMPTZ;

-- 2. Consentimento da PESSOA. Fica em `users`: desligar SMS é uma vontade de
--    quem recebe, e não muda com a organização que envia.
ALTER TABLE users ADD COLUMN IF NOT EXISTS sms_contact_opt_out BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE users ADD COLUMN IF NOT EXISTS sms_meeting_opt_out BOOLEAN NOT NULL DEFAULT FALSE;

-- 3. Quem pode enviar SMS a contactos da org. Por omissão só administradores:
--    um SMS custa dinheiro e abrir o envio a membros é decisão explícita.
ALTER TABLE organizations ADD COLUMN IF NOT EXISTS sms_send_policy TEXT NOT NULL DEFAULT 'admins'
    CHECK (sms_send_policy IN ('admins', 'members'));

-- 4. Porquê e para quem uma mensagem existe. `recipient_user_id` é o contacto
--    resolvido no servidor (modo `user_id`); nulo no modo `to` do admin.
ALTER TABLE sms_message ADD COLUMN IF NOT EXISTS purpose TEXT NOT NULL DEFAULT 'direct'
    CHECK (purpose IN ('direct', 'contact', 'meeting_invite', 'meeting_reminder'));
ALTER TABLE sms_message ADD COLUMN IF NOT EXISTS recipient_user_id UUID
    REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE sms_message ADD COLUMN IF NOT EXISTS meeting_id UUID
    REFERENCES meetings(id) ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS sms_message_creator_idx ON sms_message(org_id, created_by, created_at DESC);

-- 5. SMS de reunião. `sms_reminder_min` nulo = sem lembrete. O varrimento marca
--    `sms_reminder_done_at` ANTES de enfileirar (no máximo uma vez entre pods).
ALTER TABLE meetings ADD COLUMN IF NOT EXISTS sms_reminder_min INT
    CHECK (sms_reminder_min IS NULL OR sms_reminder_min BETWEEN 5 AND 1440);
ALTER TABLE meetings ADD COLUMN IF NOT EXISTS sms_reminder_done_at TIMESTAMPTZ;
CREATE INDEX IF NOT EXISTS meetings_sms_reminder_due_idx ON meetings(starts_at)
    WHERE sms_reminder_min IS NOT NULL AND sms_reminder_done_at IS NULL;
