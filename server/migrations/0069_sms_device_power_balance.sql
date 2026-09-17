-- Envio de SMS no ecrã de telefonia (ADR-0009 §8, sobre o ADR-0005): bateria
-- de um telefone-gateway e saldo do SIM, SÓ quando o agente os reporta. Sem
-- relatório ficam NULL e a API diz porquê — nunca um valor inventado.
ALTER TABLE sms_device
    ADD COLUMN battery_percent INT CHECK (battery_percent BETWEEN 0 AND 100),
    ADD COLUMN balance_e4      BIGINT,
    ADD COLUMN balance_currency TEXT CHECK (balance_currency IN ('AOA','USD')),
    ADD COLUMN balance_at      TIMESTAMPTZ;
CREATE INDEX sms_message_org_day_idx ON sms_message (org_id, route, created_at);
