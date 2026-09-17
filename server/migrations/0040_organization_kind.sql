-- Tipo de organização (ADR-0006 §2). `personal` é a org implícita da edição
-- pessoal: uma pessoa usa o produto sem inventar uma empresa. O frontend lê-o
-- para não mostrar a administração de empresa. As orgs existentes são empresas.
ALTER TABLE organizations
    ADD COLUMN kind TEXT NOT NULL DEFAULT 'company'
    CHECK (kind IN ('company', 'personal'));
