-- Pesquisa, filtros e agrupamentos (ADR-0007): a fundação no Postgres.
--
-- 1. Extensões. `unaccent` (acentos) e `pg_trgm` (erros de escrita,
--    subcadeias, chinês). As duas são *trusted* desde o PG 13: o dono da base
--    cria-as sem superutilizador. A imagem `postgres:17-alpine` traz as duas
--    (medido a 2026-09-17: unaccent 1.1, pg_trgm 1.6).
CREATE EXTENSION IF NOT EXISTS unaccent;
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- 2. `unaccent()` é STABLE (depende do search_path para achar o dicionário),
--    por isso nem índice nem coluna gerada a aceitam. Com o dicionário fixado
--    pelo nome qualificado, o resultado só depende do texto: IMMUTABLE é
--    verdade. É o padrão documentado pela própria extensão.
CREATE OR REPLACE FUNCTION dlx_unaccent(text) RETURNS text
    LANGUAGE sql IMMUTABLE PARALLEL SAFE STRICT
    AS $$ SELECT public.unaccent('public.unaccent'::regdictionary, $1) $$;

-- Texto normalizado para trigramas: sem acentos e em minúsculas.
CREATE OR REPLACE FUNCTION dlx_fold(text) RETURNS text
    LANGUAGE sql IMMUTABLE PARALLEL SAFE STRICT
    AS $$ SELECT lower(public.dlx_unaccent($1)) $$;

-- 3. Configuração de texto `dlx_search`: a `simple` (sem stemming nem
--    stop-words, porque a mesma org escreve em pt, en e fr — ver a 0045) com o
--    `unaccent` à frente nas palavras com letras não-ASCII. «Orçamento» e
--    «orcamento» dão o mesmo lexema.
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_ts_config WHERE cfgname = 'dlx_search') THEN
        CREATE TEXT SEARCH CONFIGURATION dlx_search (COPY = simple);
        ALTER TEXT SEARCH CONFIGURATION dlx_search
            ALTER MAPPING FOR word, hword, hword_part WITH unaccent, simple;
    END IF;
END
$$;

-- 4. Fuso da organização: «esta semana» e o agrupamento por dia calculam-se
--    no fuso de quem usa, não em UTC. Omissão Africa/Luanda (UTC+1, sem hora
--    de verão).
ALTER TABLE organizations
    ADD COLUMN IF NOT EXISTS timezone TEXT NOT NULL DEFAULT 'Africa/Luanda';
