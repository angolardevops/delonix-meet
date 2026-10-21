-- ════════════════════════════════════════════════════════════════════════
--  Prova do endereço de email (D7, decidido a 2026-10-09).
--
--  PORQUE EXISTE. Nenhum endereço de email era provado: o `users.email` é
--  AFIRMADO — por quem se regista, ou por um administrador que o escreve — e
--  nunca confirmado. Enquanto o email só identifica a conta, isso é inofensivo.
--  Deixa de ser no dia em que a reposição de password pela própria pessoa (E3)
--  manda um token para lá: a reposição por email dá a conta a quem controla o
--  ENDEREÇO, e um `joao@gmai.com` mal escrito tornava-se uma porta para uma
--  conta que já tem dados. Por isso o D7 pôs a prova ANTES do E3.
--
--  O token é a CREDENCIAL: só o hash (SHA-256) e o prefixo, uso único — o
--  mesmo desenho do `password_resets` (0106) e do `org_invitations` (0055).
--  Não se inventa um terceiro molde de token.
-- ════════════════════════════════════════════════════════════════════════

ALTER TABLE users ADD COLUMN email_verified_at TIMESTAMPTZ;

CREATE TABLE email_verifications (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- O endereço que este token prova, como estava no momento da emissão. Se a
    -- conta mudar de email entretanto, o token não prova o endereço NOVO.
    email        TEXT NOT NULL,
    token_hash   TEXT NOT NULL UNIQUE,
    token_prefix TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'pending'
                     CHECK (status IN ('pending', 'used', 'revoked', 'expired')),
    expires_at   TIMESTAMPTZ NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    used_at      TIMESTAMPTZ
);

-- Uma pendente por pessoa: pedir outra revoga a anterior (na mesma transacção).
CREATE UNIQUE INDEX email_verifications_pending_user_uidx
    ON email_verifications (user_id) WHERE status = 'pending';

-- Mudar o email ANULA a prova. Hoje nenhuma rota muda o `users.email` depois
-- de a conta nascer; a regra fica na base, e não num handler, para que a
-- primeira rota que o venha a mudar não a possa esquecer.
CREATE FUNCTION users_email_change_clears_proof() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.email IS DISTINCT FROM OLD.email THEN
        NEW.email_verified_at := NULL;
        UPDATE email_verifications SET status = 'revoked'
         WHERE user_id = NEW.id AND status = 'pending';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER users_email_change_clears_proof
    BEFORE UPDATE OF email ON users
    FOR EACH ROW EXECUTE FUNCTION users_email_change_clears_proof();
