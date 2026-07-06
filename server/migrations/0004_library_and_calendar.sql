-- Participação: quem esteve em cada sala (para "gravações onde participei").
CREATE TABLE room_participants (
    room_id   UUID NOT NULL REFERENCES rooms(id) ON DELETE CASCADE,
    user_id   UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (room_id, user_id)
);
CREATE INDEX room_participants_user_idx ON room_participants(user_id);

-- Partilha de gravações (só leitura) com utilizadores específicos.
CREATE TABLE recording_shares (
    recording_id UUID NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    shared_by    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (recording_id, user_id)
);
CREATE INDEX recording_shares_user_idx ON recording_shares(user_id);

-- Reuniões agendadas (calendário).
CREATE TABLE meetings (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title        TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    kind         TEXT NOT NULL DEFAULT 'video',   -- 'video' | 'voice'
    starts_at    TIMESTAMPTZ NOT NULL,
    duration_min INT NOT NULL DEFAULT 30,
    room_code    TEXT,                            -- preenchido ao iniciar
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX meetings_owner_idx ON meetings(owner_id);
CREATE INDEX meetings_starts_idx ON meetings(starts_at);

CREATE TABLE meeting_invitees (
    meeting_id UUID NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (meeting_id, user_id)
);
CREATE INDEX meeting_invitees_user_idx ON meeting_invitees(user_id);
