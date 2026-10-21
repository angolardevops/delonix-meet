-- Legal hold (A-08): suspender a retenção por ordem legal.
--
-- PORQUE EXISTE: a retenção por dias (gravações e chat) apagava o que tinha
-- passado do prazo, sem mais pergunta. Uma organização sob ordem de um tribunal
-- ou de um regulador — preservar as reuniões de uma pessoa, de uma sala, ou de
-- tudo — não tinha como dizer «isto não se apaga até ordem em contrário». É o
-- que fecha a venda no segmento que a soberania abre (banca, seguros, Estado).
--
-- O QUE SE RETÉM: as gravações e as mensagens de chat. As notificações não —
-- são avisos, não prova.
--
-- UM HOLD NUNCA SE APAGA: levanta-se (`released_at`). O registo de que houve
-- uma ordem, quando entrou e quando saiu, é ele próprio prova.
CREATE TABLE legal_holds (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- RESTRICT: uma organização com holds não se apaga por cascata.
    org_id       UUID NOT NULL REFERENCES organizations(id) ON DELETE RESTRICT,
    name         TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 200),
    -- A ordem legal: o processo, o ofício, quem a emitiu. Obrigatória — um
    -- hold sem razão é uma suspensão da retenção sem ninguém a responder.
    reason       TEXT NOT NULL CHECK (char_length(reason) BETWEEN 1 AND 2000),
    -- 'org' = tudo da organização; 'user' = o que uma pessoa gravou ou
    -- escreveu; 'room' = o que aconteceu numa sala.
    scope        TEXT NOT NULL CHECK (scope IN ('org', 'user', 'room')),
    user_id      UUID REFERENCES users(id) ON DELETE RESTRICT,
    room_id      UUID REFERENCES rooms(id) ON DELETE RESTRICT,
    created_by   UUID NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    released_by  UUID REFERENCES users(id) ON DELETE RESTRICT,
    released_at  TIMESTAMPTZ,
    CHECK ((scope = 'user') = (user_id IS NOT NULL)),
    CHECK ((scope = 'room') = (room_id IS NOT NULL)),
    CHECK ((released_at IS NULL) = (released_by IS NULL))
);

CREATE INDEX legal_holds_org_idx ON legal_holds(org_id);
CREATE INDEX legal_holds_activos_idx ON legal_holds(scope) WHERE released_at IS NULL;

-- Uma gravação está retida se um hold ACTIVO a cobre: o da sala dela, o de
-- quem a gravou, ou o de uma organização a que quem a gravou pertence (activo
-- ou já arquivado — um ex-membro continua coberto pela ordem da organização).
CREATE FUNCTION recording_on_legal_hold(p_room UUID, p_uploader UUID) RETURNS BOOLEAN
LANGUAGE sql STABLE AS $$
    SELECT EXISTS (
        SELECT 1 FROM legal_holds h
         WHERE h.released_at IS NULL
           AND (   (h.scope = 'room' AND h.room_id = p_room)
                OR (h.scope = 'user' AND h.user_id = p_uploader)
                OR (h.scope = 'org'  AND h.org_id IN (SELECT m.org_id FROM org_members m
                                                       WHERE m.user_id = p_uploader)))
    )
$$;

-- O chat de uma sala está retido se um hold ACTIVO cobre a sala, o dono dela
-- (por pessoa ou pela organização dele), ou quem escreveu a mensagem.
CREATE FUNCTION chat_on_legal_hold(p_room UUID, p_author UUID) RETURNS BOOLEAN
LANGUAGE sql STABLE AS $$
    SELECT EXISTS (
        SELECT 1 FROM legal_holds h
          JOIN rooms r ON r.id = p_room
         WHERE h.released_at IS NULL
           AND (   (h.scope = 'room' AND h.room_id = p_room)
                OR (h.scope = 'user' AND h.user_id IN (r.owner_id, p_author))
                OR (h.scope = 'org'  AND h.org_id IN (SELECT m.org_id FROM org_members m
                                                       WHERE m.user_id IN (r.owner_id, p_author))))
    )
$$;

-- A GUARDA QUE NÃO DEPENDE DE QUEM APAGA. As varreduras já saltam o que está
-- retido; isto recusa o `DELETE` de uma linha retida venha de onde vier — uma
-- cascata de `rooms` ou de `users`, uma rota nova, um operador com `psql`. O
-- mesmo desenho dos gatilhos da auditoria imutável (0037). A mensagem começa
-- por `legal_hold:` e é isso que o servidor traduz num 409 com razão.
CREATE FUNCTION legal_hold_block_recording_delete() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    IF recording_on_legal_hold(OLD.room_id, OLD.uploader_id) THEN
        RAISE EXCEPTION 'legal_hold: a gravação % está retida por ordem legal', OLD.id;
    END IF;
    RETURN OLD;
END
$$;

CREATE TRIGGER legal_hold_recordings
    BEFORE DELETE ON recordings
    FOR EACH ROW EXECUTE FUNCTION legal_hold_block_recording_delete();

CREATE FUNCTION legal_hold_block_chat_delete() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    IF chat_on_legal_hold(OLD.room_id, OLD.user_id) THEN
        RAISE EXCEPTION 'legal_hold: o chat da sala % está retido por ordem legal', OLD.room_id;
    END IF;
    RETURN OLD;
END
$$;

CREATE TRIGGER legal_hold_room_chat
    BEFORE DELETE ON room_chat_messages
    FOR EACH ROW EXECUTE FUNCTION legal_hold_block_chat_delete();
