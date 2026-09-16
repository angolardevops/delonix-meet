-- Destinos de emissão em directo guardados por ORGANIZAÇÃO (frontend/b1-emissao).
--
-- Antes: os destinos viviam só na memória do Estúdio e tinham de ser
-- reescritos, com a chave, a cada emissão. Agora a organização guarda-os uma
-- vez e o Estúdio refere-os por `id` — a chave nunca mais volta ao browser.
--
-- A chave de emissão NUNCA fica em claro: `key_enc` é o blob do
-- `crypto::SecretsKey::seal` (AES-256-GCM, chave na config `SECRETS_KEY`,
-- AAD = `stream_destination/<org_id>/<id>`). Um blob copiado para outra linha
-- não decifra.
CREATE TABLE stream_destinations (
    id           UUID PRIMARY KEY,
    org_id       UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    label        TEXT NOT NULL CHECK (char_length(label) BETWEEN 1 AND 80),
    platform     TEXT NOT NULL
                 CHECK (platform IN ('youtube', 'facebook', 'linkedin', 'twitch', 'rtmp')),
    -- URL base SEM a chave (ex.: rtmp://a.rtmp.youtube.com/live2).
    rtmp_url     TEXT NOT NULL CHECK (rtmp_url ~ '^rtmps?://'),
    key_enc      BYTEA NOT NULL,
    created_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Último uso numa emissão e como acabou (para «4 de 5 ligados»):
    -- 'ok' = chegou a estar no ar e terminou sem desistir; 'erro' = nunca
    -- chegou ao ar ou esgotou as tentativas (motivo em `last_error`).
    last_used_at TIMESTAMPTZ,
    last_status  TEXT CHECK (last_status IN ('ok', 'erro')),
    last_error   TEXT,
    UNIQUE (org_id, label)
);
CREATE INDEX stream_destinations_org_idx ON stream_destinations(org_id);
