-- Quotas por organização (NULL = ilimitado). Aplicadas na criação de
-- grupos, salas físicas e reuniões agendadas.
ALTER TABLE organizations ADD COLUMN max_groups   INT,
                          ADD COLUMN max_rooms    INT,
                          ADD COLUMN max_meetings INT;
