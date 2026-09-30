-- Telefonia (ADR-0009): troncos SIP por organização, histórico de preços e
-- taxas de câmbio. As operadoras (Unitel, Africell, Movicel, internacional) são
-- DADOS destas tabelas — nenhuma está no código.

CREATE TABLE telephony_trunks (
    id               UUID PRIMARY KEY,
    org_id           UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name             TEXT NOT NULL,
    -- Sigla de 2-4 letras para o cartão (UNI, AFR).
    short_code       TEXT NOT NULL,
    -- national | international: só muda o rótulo e o filtro da lista.
    scope            TEXT NOT NULL CHECK (scope IN ('national','international')),
    host             TEXT NOT NULL,
    port             INT  NOT NULL CHECK (port BETWEEN 1 AND 65535),
    transport        TEXT NOT NULL CHECK (transport IN ('udp','tcp','tls')),
    srtp             TEXT NOT NULL CHECK (srtp IN ('mandatory','optional','off')),
    -- Registo SIP com credenciais, ou autenticação por IP (register = false).
    register         BOOLEAN NOT NULL DEFAULT TRUE,
    username         TEXT NOT NULL DEFAULT '',
    -- '' = sem password; senão `enc:v1:…` (core::secret_box, aad
    -- telephony_trunks.password:<id>). NUNCA sai pela API.
    password_sealed  TEXT NOT NULL DEFAULT '',
    -- Prefixos que a operadora serve, como o cliente os lê (`9`, `95`, `00`).
    prefixes         TEXT[] NOT NULL DEFAULT '{}',
    max_channels     INT  NOT NULL CHECK (max_channels BETWEEN 1 AND 10000),
    -- Ordem de encaminhamento dentro da org: 0 = primária.
    position         INT  NOT NULL,
    enabled          BOOLEAN NOT NULL DEFAULT TRUE,
    created_by       UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, name)
);
CREATE INDEX telephony_trunks_org_pos_idx ON telephony_trunks (org_id, position, id);

-- Preço por minuto com HISTÓRICO: nunca se altera uma linha; um preço novo é
-- uma linha nova. O custo de uma chamada usa o de `valid_from` mais recente
-- que não seja posterior ao início da chamada.
CREATE TABLE telephony_trunk_prices (
    id             UUID PRIMARY KEY,
    org_id         UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    trunk_id       UUID NOT NULL REFERENCES telephony_trunks(id) ON DELETE CASCADE,
    currency       TEXT NOT NULL CHECK (currency IN ('AOA','USD')),
    -- Décimas-milésimas da unidade (9,40 Kz → 94000).
    price_per_min_e4 BIGINT NOT NULL CHECK (price_per_min_e4 >= 0),
    valid_from     TIMESTAMPTZ NOT NULL,
    created_by     UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (trunk_id, valid_from)
);
CREATE INDEX telephony_trunk_prices_lookup_idx ON telephony_trunk_prices (trunk_id, valid_from DESC);

-- Kz por unidade de moeda estrangeira, com histórico (mesma regra dos preços).
CREATE TABLE telephony_exchange_rates (
    id           UUID PRIMARY KEY,
    org_id       UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    currency     TEXT NOT NULL CHECK (currency IN ('USD')),
    -- Milionésimas: 912,50 Kz → 912500000.
    aoa_per_unit_e6 BIGINT NOT NULL CHECK (aoa_per_unit_e6 > 0),
    valid_from   TIMESTAMPTZ NOT NULL,
    created_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, currency, valid_from)
);
CREATE INDEX telephony_exchange_rates_lookup_idx
    ON telephony_exchange_rates (org_id, currency, valid_from DESC);
