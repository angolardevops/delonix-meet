-- Registo de entregas de webhooks e reenvio (G7).
--
-- Uma linha por tentativa de entrega a um webhook. `payload` é o corpo JSON
-- EXACTO que foi (ou vai ser) enviado — é o que o reenvio volta a mandar. O
-- segredo HMAC e a assinatura NÃO se guardam: a assinatura recalcula-se no
-- envio com o segredo actual do webhook.
--
-- Estados: `pending` (inserida antes do envio) → `succeeded` | `failed`. Uma
-- tentativa nova é uma linha nova com `redelivery_of`, nunca uma reescrita.
-- `error` é curto e limpo de URLs (regra em
-- `domain::integration::webhook_delivery::sanitize_error`).
--
-- Retenção: 30 dias, apagadas por um varredor horário (`webhooks::sweep_deliveries`).
CREATE TABLE webhook_deliveries (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id          UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    webhook_id      UUID NOT NULL REFERENCES org_webhooks(id) ON DELETE CASCADE,
    event           TEXT NOT NULL,
    payload         JSONB NOT NULL,
    attempt         INT NOT NULL DEFAULT 1 CHECK (attempt >= 1),
    status          TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'succeeded', 'failed')),
    response_status INT NULL,
    response_ms     INT NULL,
    error           TEXT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    delivered_at    TIMESTAMPTZ NULL,
    -- Se a entrega reenviada for apagada (retenção), o reenvio fica e perde o elo.
    redelivery_of   UUID NULL REFERENCES webhook_deliveries(id) ON DELETE SET NULL
);

-- Listagem por webhook, mais recentes primeiro, com cursor (created_at, id).
CREATE INDEX webhook_deliveries_hook_page_idx
    ON webhook_deliveries (webhook_id, created_at DESC, id DESC);
-- Varredor de retenção e contagem de reenvios recentes.
CREATE INDEX webhook_deliveries_created_idx ON webhook_deliveries (created_at);
