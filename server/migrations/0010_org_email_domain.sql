-- Modelo org-first: cada organização tem um domínio de email único.
-- O registo público cria a organização + primeiro admin; os restantes
-- utilizadores são adicionados no workspace com email do mesmo domínio.
ALTER TABLE organizations ADD COLUMN email_domain TEXT NOT NULL DEFAULT '';

-- Dois orgs não podem partilhar o mesmo domínio de email (ignora vazios das
-- organizações legadas, se existirem).
CREATE UNIQUE INDEX organizations_email_domain_uidx
    ON organizations (email_domain) WHERE email_domain <> '';
