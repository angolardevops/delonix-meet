-- Chat persistente por sala: mensagens guardadas durante a reunião.
-- Retidas até ao fim do dia (UTC) após a última mensagem (retention sweep).
CREATE TABLE room_chat_messages (
    id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    room_id    UUID        NOT NULL REFERENCES rooms(id) ON DELETE CASCADE,
    user_id    UUID        NOT NULL,
    username   TEXT        NOT NULL,
    message    TEXT        NOT NULL CHECK (char_length(message) > 0 AND char_length(message) <= 4000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX room_chat_room_idx ON room_chat_messages(room_id, created_at ASC);
