-- Registos de chamada (CDR) ingeridos do FreeSWITCH (`mod_json_cdr`) e as
-- chamadas de saída pedidas pela plataforma (ADR-0009 §6).
--
-- A tabela herdada `voice_cdr` (0014) continua a ser escrita pelo IVR de
-- dial-in e alimenta `/voice/billing`; esta é a do ecrã de telefonia, com
-- tronco, resultado e custo ao preço em vigor.

CREATE TABLE telephony_call_records (
    id               UUID PRIMARY KEY,
    org_id           UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    -- Idempotência da ingestão: (fonte, id da chamada na fonte).
    source           TEXT NOT NULL CHECK (source IN ('freeswitch')),
    source_call_id   TEXT NOT NULL,
    -- A chamada de saída nossa que originou este CDR, se houve.
    outbound_call_id UUID,
    direction        TEXT NOT NULL CHECK (direction IN ('inbound','outbound')),
    from_number      TEXT NOT NULL DEFAULT '',
    to_number        TEXT NOT NULL DEFAULT '',
    trunk_id         UUID REFERENCES telephony_trunks(id) ON DELETE SET NULL,
    room_code        TEXT,
    destination_label TEXT,
    outcome          TEXT NOT NULL CHECK (outcome IN
        ('answered','no_answer','busy','failed','wrong_pin','waiting_room','forwarded')),
    hangup_cause     TEXT,
    started_at       TIMESTAMPTZ NOT NULL,
    answered_at      TIMESTAMPTZ,
    ended_at         TIMESTAMPTZ NOT NULL,
    duration_secs    INT NOT NULL CHECK (duration_secs >= 0),
    billsec          INT NOT NULL CHECK (billsec >= 0),
    recorded         BOOLEAN NOT NULL DEFAULT FALSE,
    emergency        BOOLEAN NOT NULL DEFAULT FALSE,
    rule_position    INT,
    jitter_ms        DOUBLE PRECISION,
    loss_pct         DOUBLE PRECISION,
    mos              DOUBLE PRECISION,
    -- Custo congelado na ingestão, ao preço em vigor em `started_at`.
    -- NULL com `cost_reason` quando não há preço (nunca 0 inventado).
    cost_e4          BIGINT,
    cost_currency    TEXT CHECK (cost_currency IN ('AOA','USD')),
    price_id         UUID REFERENCES telephony_trunk_prices(id) ON DELETE SET NULL,
    cost_reason      TEXT,
    ingested_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT telephony_cdr_emergency_never_recorded CHECK (NOT (emergency AND recorded))
);
CREATE UNIQUE INDEX telephony_call_records_source_uidx
    ON telephony_call_records (source, source_call_id);
CREATE INDEX telephony_call_records_org_page_idx
    ON telephony_call_records (org_id, started_at DESC, id DESC);
CREATE INDEX telephony_call_records_trunk_window_idx
    ON telephony_call_records (trunk_id, started_at) WHERE trunk_id IS NOT NULL;
CREATE INDEX telephony_call_records_room_idx
    ON telephony_call_records (org_id, room_code, started_at) WHERE room_code IS NOT NULL;

-- Chamadas de saída pedidas pela plataforma (teste rápido, convite para sala).
CREATE TABLE telephony_outbound_calls (
    id                UUID PRIMARY KEY,
    org_id            UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    created_by        UUID REFERENCES users(id) ON DELETE SET NULL,
    purpose           TEXT NOT NULL CHECK (purpose IN ('quick_test','room_invite')),
    room_code         TEXT,
    -- O número completo fica para «voltar a ligar»; a API só o devolve mascarado.
    to_number         TEXT NOT NULL,
    status            TEXT NOT NULL CHECK (status IN ('dialing','ringing','answered','no_answer','busy','failed')),
    trunk_ids         UUID[] NOT NULL DEFAULT '{}',
    rule_position     INT,
    record            BOOLEAN NOT NULL DEFAULT FALSE,
    emergency         BOOLEAN NOT NULL DEFAULT FALSE,
    answer_latency_ms BIGINT,
    hangup_cause      TEXT,
    error             TEXT,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    answered_at       TIMESTAMPTZ,
    billsec           INT,
    finished_at       TIMESTAMPTZ
);
CREATE INDEX telephony_outbound_calls_org_page_idx
    ON telephony_outbound_calls (org_id, created_at DESC, id DESC);
