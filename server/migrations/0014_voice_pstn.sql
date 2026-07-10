-- Dial-in PSTN — control plane no backend Rust (Fase 1, sub-fase 1).
-- Configuração por organização: backend de media e modelo de DID (defaults).
ALTER TABLE organizations
    ADD COLUMN voice_media_backend TEXT NOT NULL DEFAULT 'freeswitch',  -- 'freeswitch' | 'provider'
    ADD COLUMN voice_did_model     TEXT NOT NULL DEFAULT 'shared';      -- 'shared' | 'dedicated'

-- Inventário de números DID (org_id NULL = pool partilhado entre tenants).
CREATE TABLE voice_did (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID REFERENCES organizations(id) ON DELETE CASCADE,
    e164 TEXT NOT NULL UNIQUE,                 -- número em formato +E.164
    market TEXT NOT NULL DEFAULT 'AO',
    model TEXT NOT NULL DEFAULT 'shared',      -- 'shared' | 'dedicated'
    provider TEXT NOT NULL DEFAULT '',
    active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX voice_did_org_idx ON voice_did(org_id);

-- Sala de voz: liga uma sala de conferência existente (rooms.code) ao dial-in.
CREATE TABLE voice_room (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    room_code TEXT NOT NULL,
    pin TEXT NOT NULL,
    did_id UUID REFERENCES voice_did(id) ON DELETE SET NULL,
    media_backend TEXT NOT NULL DEFAULT 'freeswitch',
    status TEXT NOT NULL DEFAULT 'active',      -- 'active' | 'closed'
    created_by UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    closed_at TIMESTAMPTZ
);
-- Isolamento: PIN único por DID enquanto a sala está ativa (fronteira multi-tenant).
CREATE UNIQUE INDEX voice_room_did_pin_active_uidx
    ON voice_room(did_id, pin) WHERE status = 'active';
CREATE INDEX voice_room_org_idx ON voice_room(org_id);

-- Participante (PSTN ou WebRTC) numa sala de voz.
CREATE TABLE voice_participant (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    voice_room_id UUID NOT NULL REFERENCES voice_room(id) ON DELETE CASCADE,
    channel TEXT NOT NULL,                      -- 'pstn' | 'webrtc'
    sip_call_id TEXT NOT NULL DEFAULT '',
    caller_number TEXT NOT NULL DEFAULT '',
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    left_at TIMESTAMPTZ
);
CREATE INDEX voice_participant_room_idx ON voice_participant(voice_room_id);

-- CDR: registo de detalhe de chamada para billing/auditoria.
CREATE TABLE voice_cdr (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    voice_room_id UUID REFERENCES voice_room(id) ON DELETE SET NULL,
    direction TEXT NOT NULL,                    -- 'inbound' | 'outbound'
    caller_number TEXT NOT NULL DEFAULT '',
    did_e164 TEXT NOT NULL DEFAULT '',
    duration_secs INT NOT NULL DEFAULT 0,
    cost_estimate DOUBLE PRECISION NOT NULL DEFAULT 0,  -- estimativa; billing recalcula
    tariff_ref TEXT NOT NULL DEFAULT '',
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ended_at TIMESTAMPTZ
);
CREATE INDEX voice_cdr_org_idx ON voice_cdr(org_id, started_at DESC);
