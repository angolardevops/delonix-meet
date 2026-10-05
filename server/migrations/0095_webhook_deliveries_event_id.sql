-- Id de EVENTO nas entregas de webhooks.
--
-- A entrega é «pelo menos uma vez» (retry automático, reenvio manual). Cada
-- tentativa é uma linha com o seu `id` (o `X-Delonix-Delivery`), por isso o
-- receptor não tinha como saber que três tentativas eram o mesmo evento.
-- `event_id` nasce na primeira tentativa e as repetições e reenvios COPIAM-no;
-- vai no cabeçalho `X-Delonix-Event-Id` e na API.
ALTER TABLE webhook_deliveries ADD COLUMN event_id UUID;

-- Cadeias que já existem: toda a cadeia de `redelivery_of` fica com o id da
-- raiz. Uma tentativa cuja original foi apagada pela retenção (`redelivery_of`
-- passa a NULL) é raiz da sua própria cadeia.
WITH RECURSIVE chain AS (
    SELECT id, id AS root FROM webhook_deliveries WHERE redelivery_of IS NULL
    UNION ALL
    SELECT d.id, c.root FROM webhook_deliveries d JOIN chain c ON d.redelivery_of = c.id
)
UPDATE webhook_deliveries w SET event_id = chain.root FROM chain WHERE w.id = chain.id;

UPDATE webhook_deliveries SET event_id = id WHERE event_id IS NULL;

ALTER TABLE webhook_deliveries
    ALTER COLUMN event_id SET NOT NULL,
    ALTER COLUMN event_id SET DEFAULT gen_random_uuid();

-- Quantas tentativas teve um evento, e agrupar por evento na consola.
CREATE INDEX webhook_deliveries_event_idx ON webhook_deliveries (webhook_id, event_id);
