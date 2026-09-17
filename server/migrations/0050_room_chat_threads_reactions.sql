-- Chat da sala: fios e reacções (frontend/b1-sala).
--
-- `parent_id` é a mensagem a que esta responde. Apagar a mensagem-mãe não
-- apaga a resposta: o fio perde a referência e a resposta fica.
ALTER TABLE room_chat_messages
    ADD COLUMN IF NOT EXISTS parent_id UUID REFERENCES room_chat_messages(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS room_chat_parent_idx
    ON room_chat_messages(parent_id) WHERE parent_id IS NOT NULL;

-- Uma linha por (mensagem, conta, emoji): reagir duas vezes com o mesmo emoji
-- não conta duas. Cai em cascata com a mensagem (e com a retenção dela).
CREATE TABLE IF NOT EXISTS room_chat_reactions (
    message_id UUID        NOT NULL REFERENCES room_chat_messages(id) ON DELETE CASCADE,
    user_id    UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    emoji      TEXT        NOT NULL CHECK (char_length(emoji) BETWEEN 1 AND 16),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (message_id, user_id, emoji)
);
