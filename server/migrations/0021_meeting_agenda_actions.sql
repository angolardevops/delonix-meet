-- Agenda de reunião: tópicos pré-reunião com controlo de execução.
CREATE TABLE meeting_agenda_items (
    id           UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    meeting_id   UUID         NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    position     SMALLINT     NOT NULL DEFAULT 0,
    topic        TEXT         NOT NULL,
    description  TEXT         NOT NULL DEFAULT '',
    duration_min SMALLINT     NOT NULL DEFAULT 5,
    done         BOOLEAN      NOT NULL DEFAULT FALSE,
    done_at      TIMESTAMPTZ,
    done_by_id   UUID         REFERENCES users(id),
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT now()
);
CREATE INDEX agenda_items_meeting ON meeting_agenda_items(meeting_id, position);

-- Plano de Ação 5W2H: uma por reunião, com META global.
CREATE TABLE action_plans (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    meeting_id  UUID        NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    goal        TEXT        NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- Uma única plano por reunião.
CREATE UNIQUE INDEX action_plan_meeting ON action_plans(meeting_id);

-- Itens do Plano de Ação (linhas 5W2H).
-- status: 'todo' = A SER FEITO ↓ | 'doing' = EM ANDAMENTO → | 'done' = REALIZADO ↑
CREATE TABLE action_items (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    plan_id     UUID        NOT NULL REFERENCES action_plans(id) ON DELETE CASCADE,
    position    SMALLINT    NOT NULL DEFAULT 0,
    what        TEXT        NOT NULL DEFAULT '',   -- O QUE (Medida ou Ações)
    when_date   DATE,                              -- QUANDO
    where_text  TEXT        NOT NULL DEFAULT '',   -- ONDE
    who_id      UUID        REFERENCES users(id),  -- QUEM (user)
    who_name    TEXT        NOT NULL DEFAULT '',   -- nome em cache (ex-membros)
    why         TEXT        NOT NULL DEFAULT '',   -- RAZÃO / PORQUÊ
    how         TEXT        NOT NULL DEFAULT '',   -- COMO (procedimento)
    resources   TEXT        NOT NULL DEFAULT '',   -- QUANTO (R$ / recursos)
    status      TEXT        NOT NULL DEFAULT 'todo'
                CHECK (status IN ('todo', 'doing', 'done')),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX action_items_plan ON action_items(plan_id, position);
