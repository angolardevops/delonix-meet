-- Provisionamento do softphone por QR de uso único (plano de produção, item
-- 3.8, lote 3; R278).
--
-- A password SIP de um ramal é longa, aleatória e mostrada uma vez; na
-- atribuição em massa ninguém a vê. Em vez de alguém a digitar, a consola
-- emite um BILHETE: um token aleatório que vai num URL (o QR), que o Linphone
-- lê e troca pela configuração da conta. Ao ser resgatado, o bilhete gasta-se
-- e o ramal recebe uma password SIP nova — a que sai nessa configuração.
--
-- Só o HASH do token fica aqui (SHA-256 de 256 bits de aleatoriedade: não há
-- nada para adivinhar, por isso não precisa de Argon2). Um bilhete serve uma
-- vez (`consumed_at`) e dura minutos (`expires_at`). Cada ramal tem no máximo
-- um bilhete vivo: emitir outro apaga os anteriores (`extension_provisioning.rs`).
--
-- Isolamento como nas tabelas vizinhas da voz: `org_id` em todas as escritas,
-- feitas atrás de `org::require_admin_pub` / `require_member_pub`. O resgate é
-- público e só conhece o token; a organização e o ramal vêm da linha.
CREATE TABLE voice_extension_provisioning_tickets (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id       UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    extension_id UUID NOT NULL REFERENCES voice_extensions(id) ON DELETE CASCADE,
    token_hash   TEXT NOT NULL,
    created_by   UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at   TIMESTAMPTZ NOT NULL,
    consumed_at  TIMESTAMPTZ
);
CREATE UNIQUE INDEX voice_extension_provisioning_tickets_token_uidx
    ON voice_extension_provisioning_tickets (token_hash);
CREATE INDEX voice_extension_provisioning_tickets_extension_idx
    ON voice_extension_provisioning_tickets (extension_id);
