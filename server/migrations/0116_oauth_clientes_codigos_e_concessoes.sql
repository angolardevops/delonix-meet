-- OAuth 2.1 para clientes MCP (ADR-0028).
--
-- NUMERAÇÃO: 0114 quando se escreveu, 0115 depois da #318 (`sala_ao_agendar`),
-- e 0116 depois da #321 (`mail_dedupe`). Três renumerações numa tarde é a
-- lição das três 0105 em paralelo (#292) levada ao extremo: o número de uma
-- migração não é dela até a PR fundir.
--
-- O que isto NÃO faz, e é a decisão central do ADR: não cria uma segunda vida
-- de credenciais. A sessão do agente é uma linha em `user_sessions` como as
-- outras, por isso aparece em «A minha conta → Sessões activas» e «terminar»
-- mata-a no mesmo pedido (`sessions::ensure_active`, migração 0078). O que
-- entra aqui é só o que o OAuth precisa e as sessões não tinham: quem é o
-- cliente, que escopos a pessoa consentiu, o código de autorização de 60
-- segundos, e a cadeia rotativa de refresh tokens.

-- ---------------------------------------------------------------------------
-- 1. Clientes
-- ---------------------------------------------------------------------------
-- Clientes PÚBLICOS, sem segredo: quem prova é o PKCE (ADR-0028 §3). Por isso
-- não há coluna de hash de segredo — e não é um esquecimento.
CREATE TABLE IF NOT EXISTS oauth_clients (
    id                      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Identificador público (`dlxc_<32 hex>`). É o que viaja nos pedidos.
    client_id               TEXT NOT NULL UNIQUE CHECK (char_length(client_id) BETWEEN 8 AND 128),
    client_name             TEXT NOT NULL CHECK (char_length(client_name) BETWEEN 1 AND 120),
    -- Literal: a comparação no `authorize` é por igualdade, nunca por prefixo.
    redirect_uris           TEXT[] NOT NULL CHECK (cardinality(redirect_uris) BETWEEN 1 AND 5),
    -- Registado por RFC 7591 (o caso normal) ou semeado por um operador.
    registered_dynamically  BOOLEAN NOT NULL DEFAULT TRUE,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Um cliente desactivado não autoriza nem renova; os tokens dele morrem
    -- com a sessão, não por si.
    disabled_at             TIMESTAMPTZ
);

-- ---------------------------------------------------------------------------
-- 2. Códigos de autorização
-- ---------------------------------------------------------------------------
-- Só o SHA-256 do código, como em `refresh_tokens` e `org_api_keys`: o segredo
-- nunca fica em claro na base. Uso único — `consumed_at` é gasto de forma
-- atómica no `UPDATE ... WHERE consumed_at IS NULL`.
CREATE TABLE IF NOT EXISTS oauth_authorization_codes (
    code_hash       TEXT PRIMARY KEY,
    client_id       TEXT NOT NULL REFERENCES oauth_clients(client_id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- A organização em que o agente vai agir, decidida no consentimento e NÃO
    -- aceitável como parâmetro de tool (ADR-0027 §4).
    org_id          UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    scopes          TEXT[] NOT NULL CHECK (cardinality(scopes) > 0),
    redirect_uri    TEXT NOT NULL,
    -- PKCE S256; o método não é coluna porque não há outro aceite.
    code_challenge  TEXT NOT NULL CHECK (char_length(code_challenge) = 43),
    -- RFC 8707: o público do token que este código vai gerar.
    resource        TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at      TIMESTAMPTZ NOT NULL,
    consumed_at     TIMESTAMPTZ
);

-- A varredura que limpa os gastos e os expirados.
CREATE INDEX IF NOT EXISTS oauth_authorization_codes_expiry_idx
    ON oauth_authorization_codes (expires_at);

-- ---------------------------------------------------------------------------
-- 3. Concessões (o consentimento que a pessoa deu)
-- ---------------------------------------------------------------------------
-- Memória do ecrã de consentimento: o mesmo cliente a pedir os MESMOS escopos
-- (ou menos) não volta a perguntar; a pedir MAIS, pergunta (ADR-0028 §5).
CREATE TABLE IF NOT EXISTS oauth_grants (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    client_id       TEXT NOT NULL REFERENCES oauth_clients(client_id) ON DELETE CASCADE,
    org_id          UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    scopes          TEXT[] NOT NULL CHECK (cardinality(scopes) > 0),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at      TIMESTAMPTZ
);

-- Uma concessão viva por (pessoa, cliente, organização). Parcial, para que
-- revogar e consentir outra vez não colida com a linha antiga.
CREATE UNIQUE INDEX IF NOT EXISTS oauth_grants_vivas_idx
    ON oauth_grants (user_id, client_id, org_id) WHERE revoked_at IS NULL;

-- ---------------------------------------------------------------------------
-- 4. Refresh tokens rotativos
-- ---------------------------------------------------------------------------
-- Rotativos: cada uso emite outro e marca `used_at` no anterior. Reapresentar
-- um já usado é a assinatura de um token roubado (RFC 9700 §4.14.2) — e a
-- resposta é revogar a FAMÍLIA inteira, que é o que `family_id` serve.
--
-- Separado de `refresh_tokens` (o da sessão web) de propósito: a tabela antiga
-- não tem cliente, nem escopos, nem público, e misturá-los faria o
-- `/api/auth/refresh` humano ter de conhecer OAuth.
CREATE TABLE IF NOT EXISTS oauth_refresh_tokens (
    token_hash      TEXT PRIMARY KEY,
    family_id       UUID NOT NULL,
    -- A sessão em `user_sessions`: é ela que faz «terminar sessão» matar o
    -- agente. Sem chave estrangeira, pela mesma razão da 0078.
    session_id      UUID NOT NULL,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    org_id          UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    client_id       TEXT NOT NULL REFERENCES oauth_clients(client_id) ON DELETE CASCADE,
    scopes          TEXT[] NOT NULL CHECK (cardinality(scopes) > 0),
    resource        TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at      TIMESTAMPTZ NOT NULL,
    used_at         TIMESTAMPTZ,
    revoked_at      TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS oauth_refresh_tokens_family_idx
    ON oauth_refresh_tokens (family_id) WHERE revoked_at IS NULL;
CREATE INDEX IF NOT EXISTS oauth_refresh_tokens_session_idx
    ON oauth_refresh_tokens (session_id) WHERE revoked_at IS NULL;
CREATE INDEX IF NOT EXISTS oauth_refresh_tokens_expiry_idx
    ON oauth_refresh_tokens (expires_at) WHERE revoked_at IS NULL;

-- ---------------------------------------------------------------------------
-- 5. A sessão do agente é uma sessão
-- ---------------------------------------------------------------------------
-- `auth_method` é um CHECK fechado desde a 0078. Sem alargar, uma autorização
-- OAuth não conseguia abrir linha em `user_sessions` — e era isso que obrigaria
-- a inventar uma segunda vida de credenciais.
ALTER TABLE user_sessions DROP CONSTRAINT IF EXISTS user_sessions_auth_method_check;
ALTER TABLE user_sessions ADD CONSTRAINT user_sessions_auth_method_check
    CHECK (auth_method IN ('password', 'mfa', 'passkey', 'sso', 'odoo', 'legacy', 'oauth'));
