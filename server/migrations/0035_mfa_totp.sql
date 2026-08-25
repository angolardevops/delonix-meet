-- Segundo factor por TOTP (RFC 6238) e códigos de recuperação.
--
-- O MFA era ZERO — a auditoria de 2026-08-25 mediu-o: nem TOTP, nem WebAuthn,
-- nem passkeys, nem sequer a palavra no código. Para um produto que se vende a
-- organizações e sector público, é o buraco de identidade mais caro que havia.

CREATE TABLE IF NOT EXISTS user_mfa (
    user_id     UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    -- Segredo TOTP em base32 (o que o autenticador lê do QR).
    --
    -- Guardado em CLARO, e isso tem de ser dito: quem lê esta tabela consegue
    -- gerar códigos válidos. Cifrá-lo aqui só moveria o problema — a chave de
    -- cifra teria de estar ao alcance do mesmo processo. A protecção a sério é
    -- Customer-Managed Keys / KMS, que está no §5.3 do mandato e não existe.
    -- Enquanto não existir, esta coluna vale o que valem as credenciais da BD.
    secret      TEXT NOT NULL,
    -- NULL = inscrito mas ainda não confirmado. Só conta como MFA activo
    -- depois de o utilizador provar que o autenticador dele funciona — sem
    -- isto, um erro a ler o QR trancava a conta para sempre.
    enabled_at  TIMESTAMPTZ,
    -- Último passo temporal aceite. É o que impede a REUTILIZAÇÃO do mesmo
    -- código dentro da sua janela de 30 s: sem isto, um código apanhado por
    -- cima do ombro (ou num proxy) servia outra vez durante meio minuto.
    last_step   BIGINT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS user_mfa_backup_codes (
    id         BIGSERIAL PRIMARY KEY,
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Hash Argon2 — os códigos de recuperação são credenciais e nunca ficam
    -- em claro, tal como as passwords.
    code_hash  TEXT NOT NULL,
    used_at    TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_mfa_backup_user ON user_mfa_backup_codes (user_id) WHERE used_at IS NULL;
