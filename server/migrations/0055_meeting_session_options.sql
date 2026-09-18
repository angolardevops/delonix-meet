-- Opções de sessão de uma reunião agendada, passadas à sala (R184).
--
-- Antes, `meetings::start` criava SEMPRE uma sala 'normal' sem sala de espera,
-- a v1 criava-a 'normal', e o formulário de agendamento da UI nova não tinha
-- onde guardar «formação», «emissão», «gravar automaticamente» nem a qualidade.
-- O contrato de dados é o da UI (`MeetingSessionOptions` em `web/src/api.ts`
-- do ramo `integra/validacao-l2`, migração 0046 de lá); a numeração e as rotas
-- são as desta linha.
ALTER TABLE meetings
    ADD COLUMN IF NOT EXISTS format TEXT NOT NULL DEFAULT 'meeting'
        CHECK (format IN ('meeting', 'training', 'broadcast', 'hybrid')),
    ADD COLUMN IF NOT EXISTS waiting_room   BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS auto_record    BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS record_quality TEXT NOT NULL DEFAULT '1080p'
        CHECK (record_quality IN ('2160p', '1080p', '720p', 'audio'));

-- A sala guarda o que o gravador do servidor tem de cumprir.
-- `record_quality` NULL = composição de sempre (mosaicos de 640×360): é o caso
-- das salas criadas fora de uma reunião agendada.
ALTER TABLE rooms
    ADD COLUMN IF NOT EXISTS auto_record    BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS record_quality TEXT
        CHECK (record_quality IN ('2160p', '1080p', '720p', 'audio'));

-- Resposta «talvez» ao convite. A coluna nasceu sem CHECK (0006); passa a ter
-- um, com os quatro valores que o servidor escreve.
ALTER TABLE meeting_invitees DROP CONSTRAINT IF EXISTS meeting_invitees_status_check;
ALTER TABLE meeting_invitees ADD CONSTRAINT meeting_invitees_status_check
    CHECK (status IN ('pending', 'accepted', 'declined', 'tentative'));
