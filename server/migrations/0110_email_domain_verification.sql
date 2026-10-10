-- A3, a raiz (revisão de segurança, 2026-10-09): o registo não verifica o
-- email, e `organizations_email_domain_uidx` (migração 0010) dá o domínio a
-- quem chegar primeiro. Isso é inofensivo enquanto o domínio só identifica a
-- organização -- deixa de o ser quando passa a ACTIVAR confiança real:
-- `enforce_sso` (bloqueia o login por password de toda a gente desse
-- domínio) e o provisionamento automático (JIT) de contas novas a partir
-- dele. Sem prova de posse, uma org estranha registada com um email de um
-- domínio alheio ficava dona desse domínio para sempre.
ALTER TABLE organizations
    ADD COLUMN email_domain_verify_token TEXT,
    ADD COLUMN email_domain_verified_at TIMESTAMPTZ;

-- Orgs já existentes com um domínio reivindicado: GRANDFATHERED como
-- verificadas. A alternativa -- desligar enforce_sso/JIT de toda a gente no
-- deploy desta migração -- parava o SSO em produção para quem já o usava.
-- Fecha o que entra a partir de agora; não castiga quem já cá estava.
UPDATE organizations
   SET email_domain_verified_at = created_at
 WHERE email_domain <> '' AND email_domain_verified_at IS NULL;
