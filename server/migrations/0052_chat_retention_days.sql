-- Retenção do chat configurável por organização (G9).
--
-- A migração 0018 prometeu "até ao fim do dia UTC após a última mensagem" e o
-- `retention_sweep` (room_chat.rs) cumpre-a com um `date_trunc('day', ...)`
-- fixo. NULL preserva exactamente essa promessa (nenhuma organização muda de
-- comportamento por actualizar o binário); um admin que defina um valor troca
-- para "N dias corridos após a última mensagem", como `retention_days` já faz
-- para as gravações.
ALTER TABLE organizations
    ADD COLUMN IF NOT EXISTS chat_retention_days INT
        CHECK (chat_retention_days IS NULL OR chat_retention_days BETWEEN 0 AND 3650);
