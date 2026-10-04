-- Ramal automático quando entra um membro (plano de produção, item 3.8,
-- lote 3; R278).
--
-- Uma definição por organização, na mesma linha do intervalo de numeração: é
-- desse intervalo que sai o número. Desligada por omissão — uma organização
-- que nunca mexeu nisto continua a atribuir ramais só quando o administrador
-- carrega em «Atribuir ramais a todos». Sem linha na tabela vale FALSE.
ALTER TABLE voice_extension_ranges
    ADD COLUMN auto_assign_on_join BOOLEAN NOT NULL DEFAULT FALSE;
