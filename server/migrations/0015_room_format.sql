-- Formato da sala: 'normal' (por defeito) ou 'training'. As salas de grupo
-- (breakouts) só estão disponíveis em reuniões de formato 'training'.
ALTER TABLE rooms ADD COLUMN format TEXT NOT NULL DEFAULT 'normal';
