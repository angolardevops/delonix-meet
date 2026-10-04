-- Retry automático de webhooks.
--
-- `retry_at` marca uma entrega `failed` que ainda vai ser repetida, e quando.
-- NULL = não há repetição agendada (sucesso, falha definitiva, tentativas
-- esgotadas, ou já repetida). Quem repete (`webhooks::retry_due`) limpa-o na
-- mesma transacção em que insere a linha da tentativa seguinte
-- (`redelivery_of`), por isso uma entrega nunca é repetida duas vezes, nem por
-- dois nós, nem por um reenvio manual a meio.
--
-- O livro de entregas já é a fila: não há broker, e um reinício não perde o
-- que está agendado.
ALTER TABLE webhook_deliveries ADD COLUMN retry_at TIMESTAMPTZ NULL;

-- Só as linhas com repetição pendente: é o que o worker varre a cada poucos
-- segundos, e quase nenhuma linha o é.
CREATE INDEX webhook_deliveries_retry_idx
    ON webhook_deliveries (retry_at) WHERE retry_at IS NOT NULL;
