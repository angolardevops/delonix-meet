-- Escopos e expiração das chaves de API `dlx_` (auditoria S6, ADR-0004 §4).
--
-- Compatibilidade: as chaves que já existem recebem a lista COMPLETA do
-- catálogo de hoje (`delonix_meet_domain::identity::api_key::Scope::ALL`) —
-- continuam a servir em todas as rotas v1 onde já serviam. O DEFAULT só existe
-- durante o ALTER, para preencher as linhas antigas; depois cai, e uma
-- inserção que esqueça os escopos falha em vez de conceder tudo em silêncio.
ALTER TABLE org_api_keys
    ADD COLUMN scopes TEXT[] NOT NULL DEFAULT ARRAY[
        'org:read', 'rooms:read', 'rooms:write', 'bots:join',
        'meetings:read', 'meetings:write', 'recordings:read'
    ]::TEXT[],
    ADD COLUMN expires_at TIMESTAMPTZ;

ALTER TABLE org_api_keys ALTER COLUMN scopes DROP DEFAULT;

-- Uma chave sem escopos não serve para nada; o domínio já a recusa.
ALTER TABLE org_api_keys
    ADD CONSTRAINT org_api_keys_scopes_not_empty CHECK (cardinality(scopes) > 0);
