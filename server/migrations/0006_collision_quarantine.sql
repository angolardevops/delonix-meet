-- Salas presenciais (físicas) por organização — para reuniões presenciais.
CREATE TABLE meeting_rooms (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id     UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    location   TEXT NOT NULL DEFAULT '',
    capacity   INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX meeting_rooms_org_idx ON meeting_rooms(org_id);

-- Reserva de sala física numa reunião (para detetar dupla-marcação).
ALTER TABLE meetings ADD COLUMN room_ref UUID REFERENCES meeting_rooms(id) ON DELETE SET NULL;

-- Resposta dos convidados: pendente até aceitar/recusar (com motivo).
ALTER TABLE meeting_invitees ADD COLUMN status TEXT NOT NULL DEFAULT 'pending'; -- pending|accepted|declined
ALTER TABLE meeting_invitees ADD COLUMN decline_reason TEXT NOT NULL DEFAULT '';
ALTER TABLE meeting_invitees ADD COLUMN responded_at TIMESTAMPTZ;

-- Quarentena de meet: quem não respondeu a reuniões já começadas.
CREATE TABLE meet_quarantine (
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    meeting_id UUID NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, meeting_id)
);
CREATE INDEX meet_quarantine_user_idx ON meet_quarantine(user_id);
