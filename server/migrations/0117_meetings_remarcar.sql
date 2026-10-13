-- ════════════════════════════════════════════════════════════════════════
--  Remarcar uma reunião: o número de revisão que o calendário precisa.
--
--  PORQUE EXISTE. Até aqui NADA no servidor mudava o `starts_at` de uma
--  reunião: o `PATCH /api/meetings/{id}` só mexe nas opções de sessão
--  (formato, sala de espera, gravação automática) e não existia nenhum
--  `UPDATE … starts_at` em todo o `server/src`. Uma reunião criada tinha hora
--  fixa; para a mudar, cancelava-se e criava-se outra — e quem já a tinha na
--  agenda ficava com a antiga.
--
--  O `sequence` é do iCalendar (RFC 5545 §3.8.7.4): um cliente de calendário
--  só aceita uma versão NOVA de um evento que já conhece se o `SEQUENCE`
--  subir. Sem ele, o `.ics` que exportamos depois de remarcar era
--  **silenciosamente ignorado** pelo Outlook e pelo Google Calendar — a hora
--  mudava aqui e não mudava na agenda de ninguém.
--
--  Começa em 0 (a primeira versão) e sobe a cada remarcação.
-- ════════════════════════════════════════════════════════════════════════

ALTER TABLE meetings
  ADD COLUMN IF NOT EXISTS sequence INT NOT NULL DEFAULT 0;
