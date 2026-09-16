-- Opções de sessão de uma reunião agendada, passadas à sala no arranque.
--
-- Antes, `meetings::start` criava SEMPRE uma sala 'normal', sem sala de
-- espera, e o formulário de agendamento não tinha onde guardar «videoaula»,
-- «emissão», «gravar automaticamente» ou a qualidade.
ALTER TABLE meetings
    ADD COLUMN IF NOT EXISTS format TEXT NOT NULL DEFAULT 'meeting'
        CHECK (format IN ('meeting', 'training', 'broadcast', 'hybrid')),
    ADD COLUMN IF NOT EXISTS waiting_room   BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS auto_record    BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS record_quality TEXT NOT NULL DEFAULT '1080p'
        CHECK (record_quality IN ('2160p', '1080p', '720p', 'audio'));

-- A sala guarda o que o gravador do servidor tem de cumprir.
-- `record_quality` NULL = composição de sempre (grelha de mosaicos 640×360).
ALTER TABLE rooms
    ADD COLUMN IF NOT EXISTS auto_record    BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS record_quality TEXT
        CHECK (record_quality IN ('2160p', '1080p', '720p', 'audio'));

-- Resposta «tentativa» ao convite (pending|accepted|declined|tentative).
ALTER TABLE meeting_invitees DROP CONSTRAINT IF EXISTS meeting_invitees_status_check;
ALTER TABLE meeting_invitees ADD CONSTRAINT meeting_invitees_status_check
    CHECK (status IN ('pending', 'accepted', 'declined', 'tentative'));
