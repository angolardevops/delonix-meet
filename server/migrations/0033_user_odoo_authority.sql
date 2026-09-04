-- ════════════════════════════════════════════════════════════════════════
--  Uma conta tem UMA autoridade de autenticação.
--
--  Antes desta migração, `odoo::org_odoo_config` escolhia contra que Odoo
--  validar a password assim:
--
--      JOIN org_members ... WHERE u.email = $1 AND o.odoo_enabled LIMIT 1
--
--  — por EMAIL, sem `ORDER BY`. Para um utilizador em mais do que uma org
--  com Odoo, saía uma arbitrária. Combinado com a sincronização de
--  directório, que casava contas existentes por email e lhes escrevia
--  `odoo_uid`/`odoo_managed`, isso fechava um caminho de TOMADA DE CONTA:
--
--    1. qualquer utilizador cria uma org (é admin dela por construção) e
--       aponta-a a um Odoo que controla (`PUT /integration/odoo`, que só
--       exige `require_admin` DESSA org);
--    2. lista `vitima@empresa.com` como utilizador interno;
--    3. a sync reclama a conta local da vítima e marca-a como gerida;
--    4. no login, a config arbitrária pode devolver o Odoo do atacante, que
--       responde "autenticado" a qualquer password que ele queira aceitar.
--
--  `odoo_org_id` torna a autoridade EXPLÍCITA e única: é gravada quando a
--  conta é CRIADA a partir de um Odoo, e nunca reescrita por outra org.
--
--  NULL = conta local (registo normal). Uma conta local NUNCA é reclamada
--  por uma sincronização de directório: ligá-la a um Odoo tem de ser um acto
--  deliberado do dono da conta, não um efeito lateral de alguém a listar o
--  endereço dela algures.
-- ════════════════════════════════════════════════════════════════════════

ALTER TABLE users
  ADD COLUMN IF NOT EXISTS odoo_org_id UUID REFERENCES organizations(id) ON DELETE SET NULL;

-- Backfill das contas JÁ geridas por Odoo. Determinístico de propósito: a org
-- com Odoo activo a que a conta se juntou PRIMEIRO. Onde a ambiguidade já
-- existe em dados reais (a conta está em duas orgs Odoo), esta escolha é
-- arbitrária mas ESTÁVEL — ao contrário do `LIMIT 1` sem ordem, que podia dar
-- respostas diferentes entre dois logins seguidos.
UPDATE users u
SET odoo_org_id = sub.org_id
FROM (
    SELECT DISTINCT ON (m.user_id) m.user_id, m.org_id
    FROM org_members m
    JOIN organizations o ON o.id = m.org_id
    WHERE o.odoo_enabled = TRUE
      AND o.odoo_url IS NOT NULL
      AND o.odoo_db IS NOT NULL
    ORDER BY m.user_id, m.created_at ASC, m.org_id ASC
) AS sub
WHERE u.id = sub.user_id
  AND u.odoo_managed = TRUE
  AND u.odoo_org_id IS NULL;

-- Uma conta gerida por Odoo SEM autoridade resolvível é um estado incoerente
-- (a org foi apagada, ou a integração foi desligada). Volta a ser local: o
-- hash Argon2 guardado no último login online continua a servi-la, em vez de
-- ficar presa a uma autoridade que já não existe.
UPDATE users
SET odoo_managed = FALSE
WHERE odoo_managed = TRUE
  AND odoo_org_id IS NULL;

CREATE INDEX IF NOT EXISTS users_odoo_org_id_idx
  ON users (odoo_org_id)
  WHERE odoo_org_id IS NOT NULL;
