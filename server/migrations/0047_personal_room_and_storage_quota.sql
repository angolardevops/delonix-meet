-- «A minha sala» (G2) e quota de armazenamento por organização (G3).
--
-- G2. A sala pessoal é uma linha de `rooms` como as outras — o código continua
-- a ser a credencial e as regras de acesso (`rooms::room_access`) não mudam.
-- O que a distingue é `is_personal`, e o índice único parcial garante UMA por
-- dono: é ele o árbitro do `ON CONFLICT` que torna a criação preguiçosa
-- idempotente quando dois pedidos chegam ao mesmo tempo.
ALTER TABLE rooms
    ADD COLUMN IF NOT EXISTS is_personal BOOLEAN NOT NULL DEFAULT FALSE;
CREATE UNIQUE INDEX IF NOT EXISTS rooms_personal_owner_uidx
    ON rooms(owner_id) WHERE is_personal;

-- G3. Bytes (gravações + quadros) que a organização pode ocupar. NULL =
-- ilimitado, como `max_groups`/`max_rooms`/`max_meetings` (0013). Só se impõe a
-- carregamentos NOVOS: uma organização acima do tecto não perde nada, deixa de
-- poder acrescentar.
ALTER TABLE organizations
    ADD COLUMN IF NOT EXISTS max_storage_bytes BIGINT
        CHECK (max_storage_bytes IS NULL OR max_storage_bytes >= 0);
