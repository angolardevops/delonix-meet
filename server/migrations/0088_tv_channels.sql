-- Canais de TV pela Internet (RFC-0001, Fase 1; docs/tv/).
--
-- 1) A capacidade `broadcast.manage_channels` (catálogo v2, ADR-0008): semeada
--    nos papéis de sistema — owner/admin = allow, member/external_guest = deny,
--    como as outras capacidades de emissão. Os papéis PERSONALIZADOS não ganham
--    linha efectiva: sem decisão gravada, recusa-se (fail-closed) até um admin
--    a conceder na matriz.
-- 2) A tabela `tv_channels`: identidade persistente de um canal, por organização.
--    `status` é só leitura por agora (`draft`): o motor que o fará passar a
--    `idle`/`on_air` ainda não existe, e um canal não se apresenta operacional
--    sem ele. `version` é a concorrência optimista (RNF-10): um PATCH com uma
--    versão antiga é recusado, não aplicado por cima.
INSERT INTO system_role_capability_defaults (system_key, capability, value)
VALUES ('owner', 'broadcast.manage_channels', 'allow'),
       ('admin', 'broadcast.manage_channels', 'allow'),
       ('member', 'broadcast.manage_channels', 'deny'),
       ('external_guest', 'broadcast.manage_channels', 'deny')
ON CONFLICT DO NOTHING;

-- Semeia nas orgs existentes (idempotente: ON CONFLICT DO NOTHING).
SELECT seed_system_roles(id) FROM organizations;

CREATE TABLE tv_channels (
    id                       UUID PRIMARY KEY,
    org_id                   UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    slug                     TEXT NOT NULL,
    name                     TEXT NOT NULL,
    description              TEXT NOT NULL DEFAULT '',
    timezone                 TEXT NOT NULL DEFAULT 'Africa/Luanda',
    visibility               TEXT NOT NULL DEFAULT 'private'
                             CHECK (visibility IN ('public', 'private', 'restricted')),
    status                   TEXT NOT NULL DEFAULT 'draft'
                             CHECK (status IN ('draft', 'idle', 'on_air', 'degraded', 'suspended')),
    recording_retention_days INTEGER CHECK (recording_retention_days IS NULL
                                            OR recording_retention_days BETWEEN 1 AND 3650),
    version                  INTEGER NOT NULL DEFAULT 1,
    created_by               UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- O endereço público é único na organização. (Um domínio personalizado e a
    -- unicidade global do endereço chegam com o player, RF-12.)
    CONSTRAINT tv_channels_org_slug_key UNIQUE (org_id, slug)
);
CREATE INDEX tv_channels_org_page_idx ON tv_channels (org_id, created_at, id);
