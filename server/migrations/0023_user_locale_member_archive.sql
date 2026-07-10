-- Preferência de idioma por utilizador (persiste entre dispositivos)
ALTER TABLE users ADD COLUMN IF NOT EXISTS locale VARCHAR(8) NOT NULL DEFAULT 'pt';

-- Soft-delete de membros: nunca apagar, só arquivar (auditoria)
ALTER TABLE org_members ADD COLUMN IF NOT EXISTS archived_at TIMESTAMPTZ;
ALTER TABLE org_members ADD COLUMN IF NOT EXISTS archived_by UUID REFERENCES users(id);

-- Índice: listar membros ativos (não arquivados) é o caminho quente
CREATE INDEX IF NOT EXISTS idx_org_members_active
  ON org_members (org_id)
  WHERE archived_at IS NULL;
