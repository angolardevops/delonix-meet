-- Gateway de SMS (ADR-0005): telefone/modem por USB através de um agente, e
-- operadores móveis por SMPP (credenciais no ambiente, NUNCA aqui).

-- Um agente instalado na máquina onde o telefone está ligado. O token (dlxg_)
-- só existe na resposta que o cria; guarda-se o SHA-256.
CREATE TABLE sms_gateway (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    token_prefix TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    created_by UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ
);
CREATE INDEX sms_gateway_org_idx ON sms_gateway(org_id);

-- Inventário USB reportado pelo agente. Substituído a cada relatório.
CREATE TABLE sms_device (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    gateway_id UUID NOT NULL REFERENCES sms_gateway(id) ON DELETE CASCADE,
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    device_key TEXT NOT NULL,
    vendor_id TEXT NOT NULL,
    product_id TEXT NOT NULL,
    manufacturer TEXT,
    product TEXT,
    serial TEXT,
    kind TEXT NOT NULL,          -- modem | android_adb | android_mtp | mass_storage_modem | unknown
    transport TEXT NOT NULL,     -- at_serial | modemmanager | none
    port TEXT,
    capable BOOLEAN NOT NULL DEFAULT FALSE,
    reason TEXT,
    operator_name TEXT,
    signal_percent INT,
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (gateway_id, device_key)
);
CREATE INDEX sms_device_org_idx ON sms_device(org_id);

-- O ponto de envio escolhido pela org (um só).
CREATE TABLE sms_org_route (
    org_id UUID PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
    device_id UUID REFERENCES sms_device(id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users(id) ON DELETE SET NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE sms_message (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    to_e164 TEXT NOT NULL,
    body TEXT NOT NULL,
    encoding TEXT NOT NULL,      -- gsm7 | ucs2
    segments INT NOT NULL,
    route TEXT NOT NULL,         -- usb | operator
    operator TEXT,               -- unitel | movicel | africell
    device_id UUID REFERENCES sms_device(id) ON DELETE SET NULL,
    -- O gateway que a reclamou: só ELE pode reportar o resultado.
    claimed_by UUID REFERENCES sms_gateway(id) ON DELETE SET NULL,
    status TEXT NOT NULL DEFAULT 'queued',  -- queued | claimed | sent | failed
    error TEXT,
    provider_ref TEXT,
    idempotency_key TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    claimed_at TIMESTAMPTZ,
    sent_at TIMESTAMPTZ
);
CREATE INDEX sms_message_org_idx ON sms_message(org_id, created_at DESC);
CREATE INDEX sms_message_queue_idx ON sms_message(route, created_at) WHERE status = 'queued';
CREATE UNIQUE INDEX sms_message_idem_uidx ON sms_message(org_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;
