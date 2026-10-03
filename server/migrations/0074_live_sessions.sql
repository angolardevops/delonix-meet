-- Emissão que sobrevive à queda da rede (ADR-0013).
--
-- O estado de uma emissão vive na memória do pod que a recebe — e um drain ou
-- um reinício leva essa memória. O que se mostra ao anfitrião, e o que fica
-- anexo à gravação, é por isso escrito AQUI à medida que acontece (§8): a
-- sessão, o último estado de cada destino e cada evento do registo.

CREATE TABLE live_sessions (
    id             UUID PRIMARY KEY,
    room_id        UUID NOT NULL REFERENCES rooms(id) ON DELETE CASCADE,
    started_by     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- `pubsub::NODE_ID` do processo que tem a emissão (muda a cada arranque).
    node_id        UUID NOT NULL,
    status         TEXT NOT NULL DEFAULT 'live'
                   CHECK (status IN ('live', 'recording_only', 'ended', 'interrupted')),
    started_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    ended_at       TIMESTAMPTZ,
    end_reason     TEXT,
    -- A gravação do fluxo emitido (§1). Apagar a gravação apaga a sessão e o
    -- registo com ela: a retenção do registo É a da gravação.
    recording_id   UUID REFERENCES recordings(id) ON DELETE CASCADE,
    -- Retrato dos números vivos (débito, bytes gravados, disco), escrito pelo
    -- pod da emissão a cada 5 s. `observed_at` diz quão velho está: uma sessão
    -- aberta com o retrato parado é de um processo que morreu.
    snapshot       JSONB NOT NULL DEFAULT '{}'::jsonb,
    observed_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX live_sessions_room_idx ON live_sessions (room_id, started_at DESC);
-- Uma sala emite uma vez de cada vez: é o árbitro entre dois pods.
CREATE UNIQUE INDEX live_sessions_one_open_per_room ON live_sessions (room_id)
    WHERE ended_at IS NULL;
CREATE INDEX live_sessions_open_idx ON live_sessions (observed_at) WHERE ended_at IS NULL;

CREATE TABLE live_session_destinations (
    id                   UUID PRIMARY KEY,
    session_id           UUID NOT NULL REFERENCES live_sessions(id) ON DELETE CASCADE,
    position             INT NOT NULL,
    label                TEXT NOT NULL,
    -- youtube | facebook | linkedin | rtmp | internal. NUNCA o URL nem a chave.
    kind                 TEXT NOT NULL,
    saved_destination_id UUID REFERENCES stream_destinations(id) ON DELETE SET NULL,
    state                TEXT NOT NULL
                         CHECK (state IN ('connecting', 'live', 'degraded', 'interrupted',
                                          'retrying', 'lost', 'stopped')),
    attempt              INT,
    max_attempts         INT NOT NULL,
    retry_at             TIMESTAMPTZ,
    profile              TEXT NOT NULL DEFAULT 'source' CHECK (profile IN ('source', '1080p')),
    -- Motivo legível da última queda, já sem a chave (ADR-0013 §9).
    last_reason          TEXT,
    -- Milissegundos que o destino passou fora do ar depois de ter estado no ar.
    offair_ms            BIGINT NOT NULL DEFAULT 0,
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (session_id, position)
);

CREATE TABLE live_session_events (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    session_id     UUID NOT NULL REFERENCES live_sessions(id) ON DELETE CASCADE,
    -- Ordem total dentro da sessão; é também o cursor da listagem.
    seq            BIGINT NOT NULL,
    at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Desde o início da sessão: é a hora que o registo mostra.
    offset_ms      BIGINT NOT NULL CHECK (offset_ms >= 0),
    kind           TEXT NOT NULL
                   CHECK (kind IN ('normal', 'warning', 'failure', 'safe', 'recovering')),
    code           TEXT NOT NULL,
    title          TEXT NOT NULL,
    detail         TEXT NOT NULL DEFAULT '',
    destination_id UUID REFERENCES live_session_destinations(id) ON DELETE CASCADE,
    UNIQUE (session_id, seq)
);
