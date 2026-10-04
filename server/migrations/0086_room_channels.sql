-- Canais na sala (ADR-0010): quem se acrescenta a uma reunião por número —
-- chamada de voz, SMS com PIN, convite WhatsApp — e o PIN de uso único.
-- Portada de `delonix-meet-backend/v3-canais` (lá 0075, depois 0095) para a
-- sequência do develop (0086). AINDA SEM CONSUMIDOR: nenhum código em `server/src`
-- lê ou escreve estas tabelas — o censo de canais vive em memória
-- (`signaling::Seat`). Entram com o esquema para o «adicionar por número».

CREATE TABLE room_dial_outs (
    id                 UUID PRIMARY KEY,
    org_id             UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    room_id            UUID NOT NULL REFERENCES rooms(id) ON DELETE CASCADE,
    room_code          TEXT NOT NULL,
    kind               TEXT NOT NULL CHECK (kind IN ('voice','sms_pin','whatsapp_invite','whatsapp_voice')),
    -- phone | whatsapp: o canal com que a pessoa aparece na sala.
    channel            TEXT NOT NULL CHECK (channel IN ('phone','whatsapp')),
    -- unitel | africell | movicel | national | international (só informativo).
    carrier            TEXT,
    -- E.164 completo. Dado pessoal: só sai pela API a quem pode admitir na sala.
    number_e164        TEXT NOT NULL,
    -- Nome dado pelo anfitrião («Identificar»); NULL = «Sem nome».
    display_name       TEXT,
    status             TEXT NOT NULL CHECK (status IN ('queued','dialing','ringing','in_call','sent','ended','declined','no_answer','failed','cancelled')),
    failure_code       TEXT,
    -- A chamada da telefonia (telephony_outbound_calls.id = delonix_call_id).
    telephony_call_id  UUID UNIQUE,
    sms_message_id     UUID,
    whatsapp_message_id TEXT,
    -- Tarifa em vigor no pedido (décimas-milésimas), por minuto na voz.
    rate_e4            BIGINT,
    currency           TEXT CHECK (currency IN ('AOA','USD')),
    muted              BOOLEAN NOT NULL DEFAULT FALSE,
    on_stage           BOOLEAN NOT NULL DEFAULT FALSE,
    -- «Voltar a ligar» aponta para o pedido de origem.
    redial_of          UUID REFERENCES room_dial_outs(id) ON DELETE SET NULL,
    requested_by       UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    answered_at        TIMESTAMPTZ,
    ended_at           TIMESTAMPTZ,
    billsec            INT
);
CREATE INDEX room_dial_outs_room_idx ON room_dial_outs (room_id, created_at DESC, id DESC);
CREATE INDEX room_dial_outs_org_idx ON room_dial_outs (org_id, created_at DESC);

-- PIN de uso único enviado por SMS: entra-se pelo número de acesso (DID) da
-- sala de voz. Guarda-se o HASH (sha256 de `<did_id>:<pin>`), nunca o PIN.
CREATE TABLE room_phone_pins (
    id            UUID PRIMARY KEY,
    org_id        UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    room_id       UUID NOT NULL REFERENCES rooms(id) ON DELETE CASCADE,
    dial_out_id   UUID NOT NULL REFERENCES room_dial_outs(id) ON DELETE CASCADE,
    voice_room_id UUID NOT NULL REFERENCES voice_room(id) ON DELETE CASCADE,
    did_id        UUID NOT NULL REFERENCES voice_did(id) ON DELETE CASCADE,
    pin_hash      TEXT NOT NULL,
    expires_at    TIMESTAMPTZ NOT NULL,
    used_at       TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX room_phone_pins_lookup_idx ON room_phone_pins (did_id, pin_hash) WHERE used_at IS NULL;

-- WhatsApp Business Cloud API por organização. O token é segredo cifrado
-- (`secrets_at_rest`, aad `org_whatsapp_configs.access_token:<org_id>`).
CREATE TABLE org_whatsapp_configs (
    org_id              UUID PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
    phone_number_id     TEXT NOT NULL,
    access_token_sealed TEXT NOT NULL,
    -- Template APROVADO pela Meta com UM parâmetro de corpo: o link da sala.
    invite_template     TEXT NOT NULL,
    template_language   TEXT NOT NULL DEFAULT 'pt_PT',
    updated_by          UUID REFERENCES users(id) ON DELETE SET NULL,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);
