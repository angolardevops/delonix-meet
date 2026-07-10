-- Co-anfitriões de admissões persistentes: participantes que o anfitrião
-- promoveu a admitir convidados da sala de espera. Persiste entre reconexões
-- (ao contrário do estado runtime em memória) — um promovido que caia e volte
-- reentra direto e recupera o poder de admitir.
CREATE TABLE IF NOT EXISTS room_admitters (
    room_id    UUID NOT NULL REFERENCES rooms(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    granted_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (room_id, user_id)
);
