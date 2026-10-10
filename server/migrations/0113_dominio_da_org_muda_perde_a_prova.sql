-- ════════════════════════════════════════════════════════════════════════
--  O domínio de email de uma organização que MUDA perde a prova.
--
--  A prova de posse (0110, A3) vale para o domínio que foi provado. Se o
--  `organizations.email_domain` mudar e a mesma escrita não provar o novo, a
--  prova antiga deixava o JIT e o `enforce_sso` confiar num domínio que ninguém
--  provou. Hoje há duas escritas: a provisão pelo operador (que prova o novo na
--  mesma escrita — decisão do dono, 2026-10-10) e a conversão de conta
--  particular em empresa (que não prova). A regra fica na base, como a do email
--  das pessoas (0111), para a próxima escrita não a poder esquecer.
--
--  «A mesma escrita prova o novo» = muda também o `email_domain_verified_at`.
--  Uma escrita que mexe só no domínio fica sem prova.
-- ════════════════════════════════════════════════════════════════════════

CREATE FUNCTION organizations_domain_change_clears_proof() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.email_domain IS DISTINCT FROM OLD.email_domain
       AND NEW.email_domain_verified_at IS NOT DISTINCT FROM OLD.email_domain_verified_at THEN
        NEW.email_domain_verified_at := NULL;
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER organizations_domain_change_clears_proof
    BEFORE UPDATE OF email_domain ON organizations
    FOR EACH ROW EXECUTE FUNCTION organizations_domain_change_clears_proof();
