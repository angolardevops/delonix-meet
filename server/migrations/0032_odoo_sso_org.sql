-- ════════════════════════════════════════════════════════════════════════
--  Login com conta Odoo: a organização nasce do primeiro login.
--
--  Uma organização Delonix passa a poder ser a projeção de uma EMPRESA Odoo.
--  A identidade dessa projeção é o par (base de dados Odoo, id da empresa) —
--  não o nome, que muda, e não o slug, que é derivado do nome. Sem esta
--  chave, dois logins simultâneos de utilizadores da mesma empresa criariam
--  duas organizações, e um rename da empresa no Odoo criaria uma terceira.
--
--  O índice é PARCIAL: só as orgs vindas do Odoo participam. As criadas à
--  mão continuam com estas colunas a NULL, sem restrição nenhuma.
-- ════════════════════════════════════════════════════════════════════════

ALTER TABLE organizations
  ADD COLUMN IF NOT EXISTS odoo_company_id INTEGER;

CREATE UNIQUE INDEX IF NOT EXISTS organizations_odoo_company_uidx
  ON organizations (odoo_db, odoo_company_id)
  WHERE odoo_db IS NOT NULL AND odoo_company_id IS NOT NULL;

-- Quando é que os utilizadores desta org foram sincronizados a partir do
-- Odoo. `odoo_synced_at` já existe (0029) e é reutilizado: o login só volta
-- a puxar a lista completa quando ela está velha, para não pagar uma leitura
-- de N utilizadores em cada autenticação.
