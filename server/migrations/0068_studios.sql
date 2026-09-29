-- Estúdio de TV num PC (ADR-0014). Número de TRABALHO: renumera-se na integração.
-- (0068 e não 0080: o portão de higiene recusa buracos na sequência, e a main
-- está em 0067. Se outra frente chegar primeiro a 0068, renumera-se aqui.)
--
-- Um estúdio é da organização e tem uma sala SFU própria. Apagar o estúdio NÃO
-- apaga a sala: as gravações penduram-se na sala (`recordings.room_id`), e
-- perdê-las por arrumar a lista de estúdios seria o pior efeito lateral possível.
CREATE TABLE studios (
    id            UUID PRIMARY KEY,
    org_id        UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    room_id       UUID NOT NULL UNIQUE REFERENCES rooms(id) ON DELETE CASCADE,
    iso_recording BOOLEAN NOT NULL DEFAULT TRUE,
    created_by    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX studios_org_page_idx ON studios (org_id, created_at, id);

-- Código de emparelhamento da app Delonix Câmara. Só o HASH do segredo; o
-- localizador (4 símbolos) é único entre os códigos que ainda podem ser usados.
CREATE TABLE studio_pairing_codes (
    id           UUID PRIMARY KEY,
    studio_id    UUID NOT NULL REFERENCES studios(id) ON DELETE CASCADE,
    org_id       UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    locator      TEXT NOT NULL CHECK (length(locator) = 4),
    secret_hash  TEXT NOT NULL,
    number       INT CHECK (number IS NULL OR number BETWEEN 1 AND 16),
    label        TEXT NOT NULL DEFAULT '',
    attempts     INT NOT NULL DEFAULT 0,
    expires_at   TIMESTAMPTZ NOT NULL,
    consumed_at  TIMESTAMPTZ,
    revoked_at   TIMESTAMPTZ,
    created_by   UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- Um localizador não se repete entre códigos ainda utilizáveis (não consumidos,
-- não revogados). A validade e as tentativas decidem-se na leitura; o índice só
-- impede duas linhas vivas com o mesmo localizador.
CREATE UNIQUE INDEX studio_pairing_codes_live_locator_uidx
    ON studio_pairing_codes (locator) WHERE consumed_at IS NULL AND revoked_at IS NULL;
CREATE INDEX studio_pairing_codes_studio_page_idx ON studio_pairing_codes (studio_id, created_at, id);

-- Uma fonte emparelhada (sem conta). `revoked_at` fecha o token no próximo upgrade
-- do /ws e expulsa o socket vivo.
CREATE TABLE studio_sources (
    id            UUID PRIMARY KEY,
    studio_id     UUID NOT NULL REFERENCES studios(id) ON DELETE CASCADE,
    org_id        UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    number        INT NOT NULL CHECK (number BETWEEN 1 AND 16),
    label         TEXT NOT NULL DEFAULT '',
    kind          TEXT NOT NULL DEFAULT 'phone_app' CHECK (kind IN ('phone_app')),
    device_model  TEXT NOT NULL DEFAULT '',
    device_platform TEXT NOT NULL DEFAULT '',
    app_version   TEXT NOT NULL DEFAULT '',
    pairing_code_id UUID REFERENCES studio_pairing_codes(id) ON DELETE SET NULL,
    paired_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at    TIMESTAMPTZ,
    last_seen_at  TIMESTAMPTZ,
    last_status   JSONB,
    last_tally    TEXT NOT NULL DEFAULT 'free' CHECK (last_tally IN ('program','preview','free')),
    -- Último estado de ligação visto pelo pod da sala (a REST de outro pod lê-o).
    connected     BOOLEAN NOT NULL DEFAULT FALSE
);
-- CAM n é único entre as fontes ACTIVAS de um estúdio.
CREATE UNIQUE INDEX studio_sources_active_number_uidx
    ON studio_sources (studio_id, number) WHERE revoked_at IS NULL;
CREATE INDEX studio_sources_studio_page_idx ON studio_sources (studio_id, paired_at, id);
