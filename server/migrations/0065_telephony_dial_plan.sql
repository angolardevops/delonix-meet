-- Plano de marcação por organização (ADR-0009 §3). O plano é substituído
-- INTEIRO num PUT (a ordem é o significado), dentro de uma transacção.

CREATE TABLE telephony_dial_plans (
    org_id      UUID PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
    -- Incrementa a cada PUT; `If-Match` futuro e cache do xml_curl.
    version     BIGINT NOT NULL DEFAULT 0,
    updated_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE telephony_dial_rules (
    org_id            UUID NOT NULL REFERENCES telephony_dial_plans(org_id) ON DELETE CASCADE,
    position          INT  NOT NULL CHECK (position >= 0),
    pattern           TEXT NOT NULL,
    description       TEXT NOT NULL,
    action            TEXT NOT NULL CHECK (action IN ('external','room_pin','extension','block')),
    trunk_id          UUID REFERENCES telephony_trunks(id) ON DELETE RESTRICT,
    fallback_trunk_id UUID REFERENCES telephony_trunks(id) ON DELETE RESTRICT,
    record            BOOLEAN NOT NULL DEFAULT FALSE,
    emergency         BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (org_id, position),
    -- Invariante também na base: emergência nunca gravada, nunca bloqueada.
    CONSTRAINT telephony_emergency_never_recorded CHECK (NOT (emergency AND record)),
    CONSTRAINT telephony_emergency_is_external CHECK (NOT emergency OR action = 'external')
);
