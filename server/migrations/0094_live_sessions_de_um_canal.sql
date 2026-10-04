-- Um canal de TV tem várias `live_sessions` (RFC-0001; ADR-0015, «Relação com as
-- live_sessions»).
--
-- A `live_sessions` (0074, ADR-0013) é o REGISTO OBSERVADO de uma emissão: o
-- estado de cada destino e o registo minuto a minuto. Nasceu presa a uma sala.
-- Uma emissão de canal em playout não tem sala, por isso:
--   * `room_id` passa a poder ser nulo;
--   * ganha `channel_id`;
--   * tem de ter uma coisa OU outra (nunca nenhuma): o CHECK impede uma sessão
--     órfã que ninguém saberia atribuir.
--
-- Um canal tem várias ao longo do tempo, mas só UMA em curso — o canal emite um
-- sinal de cada vez. O índice parcial é o que o garante, como o `one_open_per_room`
-- já faz para as salas (os NULL de `room_id` não colidem entre si).
--
-- Apagar o canal apaga o seu histórico (CASCADE), como a 0089 já faz com as
-- `tv_broadcast_sessions`; o handler recusa apagar um canal com emissão em curso.
--
-- `started_by` continua NOT NULL: uma emissão agendada sem pessoa por trás (o
-- playout) precisa de um actor de sistema, que ainda não existe. Fica por decidir
-- quando houver agendador — não se inventa aqui um utilizador falso.
--
-- `tv_broadcast_sessions` (0089) mantém o seu papel de PLANO DE CONTROLO: a
-- intenção e o lease do executor. Quando o executor arranca a emissão cria a
-- `live_session` e liga-a por `live_session_id`.
ALTER TABLE live_sessions ALTER COLUMN room_id DROP NOT NULL;
ALTER TABLE live_sessions
    ADD COLUMN channel_id UUID REFERENCES tv_channels(id) ON DELETE CASCADE,
    ADD CONSTRAINT live_sessions_room_or_channel
        CHECK (room_id IS NOT NULL OR channel_id IS NOT NULL);
CREATE UNIQUE INDEX live_sessions_one_open_per_channel ON live_sessions (channel_id)
    WHERE ended_at IS NULL AND channel_id IS NOT NULL;
CREATE INDEX live_sessions_channel_idx ON live_sessions (channel_id, started_at DESC)
    WHERE channel_id IS NOT NULL;

ALTER TABLE tv_broadcast_sessions
    ADD COLUMN live_session_id UUID REFERENCES live_sessions(id) ON DELETE SET NULL;
