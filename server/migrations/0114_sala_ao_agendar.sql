-- A sala de uma reunião nasce COM a reunião, não quando o anfitrião carrega
-- em «Entrar».
--
-- Até aqui `meetings.room_code` era NULL até ao `start`, e isso tinha três
-- consequências que o dono reportou a 2026-10-13: não havia link nenhum para
-- partilhar com a equipa ao agendar; o `.ics` saía sem URL; e a `join_url` da
-- v1 era construída com o código vazio (`https://<org>/#/r/`), um link
-- partido. Quem chegasse antes do anfitrião levava com
-- `400 "a reunião ainda não foi iniciada"` em vez de um ecrã de espera.
--
-- Com a sala a nascer cedo, `room_code IS NOT NULL` deixa de poder significar
-- «já começou» — era isso que `start` usava para só tocar aos convidados uma
-- vez e para disparar `meeting.started` uma vez. Esse facto passa a ter
-- coluna própria.
ALTER TABLE meetings ADD COLUMN IF NOT EXISTS started_at TIMESTAMPTZ;

-- Reuniões antigas: ter `room_code` ERA ter começado. Sem instante guardado,
-- o `created_at` é o mais próximo da verdade e chega para o único uso da
-- coluna (não voltar a tocar nem a disparar `meeting.started`).
UPDATE meetings SET started_at = created_at
 WHERE room_code IS NOT NULL AND started_at IS NULL;

-- `on_room_emptied` e as varreduras olham para isto; sem índice, uma sala a
-- esvaziar-se varre a tabela.
CREATE INDEX IF NOT EXISTS meetings_started_at_idx ON meetings(started_at);
