-- Varredura da quarentena sem seq scan (medido a 2026-09-17 com 287 000
-- convidados: 1,2-1,8 s por passagem, e corria em cada GET /api/meetings).
--
-- Só um terço dos convidados está 'pending', e é esse o único conjunto que a
-- varredura lê. A chave começa por `user_id` porque a varredura da analítica
-- parte dos membros da organização; a global lê o índice inteiro.
--
-- Numeração: 0049-0054 estão tomadas por branches em curso (chat directo,
-- SMS, pesquisa); 0055 evita a colisão quando se integrarem.
CREATE INDEX meeting_invitees_pending_idx
    ON meeting_invitees (user_id, meeting_id)
    WHERE status = 'pending';
