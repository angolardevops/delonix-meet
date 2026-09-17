-- Chat da sala: conversa directa (mensagem privada a um participante).
--
-- `to_user_id` é a conta que a recebe; NULL = mensagem pública. Uma privada só
-- é devolvida pelo histórico a quem a enviou e a quem a recebeu — nem o
-- anfitrião lê privadas alheias. `to_username` guarda o nome como estava no
-- momento, tal como `username` já faz para o remetente. Apagar a conta que a
-- recebeu apaga a mensagem (não fica uma privada órfã visível a mais ninguém).
ALTER TABLE room_chat_messages
    ADD COLUMN IF NOT EXISTS to_user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    ADD COLUMN IF NOT EXISTS to_username TEXT;

CREATE INDEX IF NOT EXISTS room_chat_direct_idx
    ON room_chat_messages(room_id, to_user_id) WHERE to_user_id IS NOT NULL;
