-- Sessões de emissão de um canal de TV (RFC-0001, §8–9, §13).
--
-- 1) Capacidade `broadcast.go_live` (catálogo v3, ADR-0008): PÔR NO AR é uma
--    capacidade distinta de PREPARAR (`broadcast.manage_channels`, RF-19).
--    owner/admin = allow; member/external_guest = deny; papéis personalizados
--    recusam até um admin a conceder (fail-closed, como na 0074).
-- 2) `tv_broadcast_sessions`: uma emissão. Separa a INTENÇÃO (`desired_state`,
--    escrita pelo plano de controlo) do ESTADO OBSERVADO (`state`, escrito só
--    pelo executor com o lease válido). Sem executor, fica em `requested`.
--    O lease (`executor_id`, `lease_expires_at`, `fencing_token`) impede dois
--    executores activos na mesma sessão (RNF-10): quem toma o lease de outro
--    sobe o `fencing_token`, e qualquer escrita com o token antigo é recusada.
INSERT INTO system_role_capability_defaults (system_key, capability, value)
VALUES ('owner', 'broadcast.go_live', 'allow'),
       ('admin', 'broadcast.go_live', 'allow'),
       ('member', 'broadcast.go_live', 'deny'),
       ('external_guest', 'broadcast.go_live', 'deny')
ON CONFLICT DO NOTHING;

SELECT seed_system_roles(id) FROM organizations;

CREATE TABLE tv_broadcast_sessions (
    id               UUID PRIMARY KEY,
    org_id           UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    channel_id       UUID NOT NULL REFERENCES tv_channels(id) ON DELETE CASCADE,
    desired_state    TEXT NOT NULL DEFAULT 'live' CHECK (desired_state IN ('live', 'stopped')),
    state            TEXT NOT NULL DEFAULT 'requested'
                     CHECK (state IN ('requested', 'starting', 'live', 'ending', 'ended', 'failed')),
    failure_reason   TEXT,
    requested_by     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    requested_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at       TIMESTAMPTZ,
    ended_at         TIMESTAMPTZ,
    executor_id      TEXT,
    lease_expires_at TIMESTAMPTZ,
    fencing_token    BIGINT NOT NULL DEFAULT 0,
    -- Terminal ⇔ tem hora de fim. Um estado terminal sem `ended_at` (ou o
    -- inverso) deixava o canal preso ou reutilizável por engano.
    CONSTRAINT tv_broadcast_terminal_has_end
        CHECK ((state IN ('ended', 'failed')) = (ended_at IS NOT NULL))
);
-- No máximo UMA sessão por terminar por canal: dois «pôr no ar» em corrida
-- não criam duas emissões.
CREATE UNIQUE INDEX tv_broadcast_one_active_idx
    ON tv_broadcast_sessions (channel_id) WHERE ended_at IS NULL;
CREATE INDEX tv_broadcast_channel_page_idx
    ON tv_broadcast_sessions (channel_id, requested_at DESC, id DESC);
