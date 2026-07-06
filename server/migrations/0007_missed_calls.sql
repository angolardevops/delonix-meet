-- Chamadas perdidas: registadas quando um alvo estava offline ao ser chamado.
CREATE TABLE missed_calls (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    room_code   TEXT NOT NULL,
    caller_id   UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    caller_name TEXT NOT NULL,
    kind        TEXT NOT NULL DEFAULT 'video',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    seen        BOOLEAN NOT NULL DEFAULT FALSE
);
CREATE INDEX missed_calls_user_idx ON missed_calls(user_id, seen);
