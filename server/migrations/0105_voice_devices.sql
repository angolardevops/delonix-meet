-- Os aparelhos de um ramal móvel e os pedidos de «acordar» (ADR-0023, S-01).
--
-- Um telemóvel com a app morta não tem registo SIP: a chamada para ele morria em 10 ms. Para o acordar
-- o servidor precisa de saber QUE aparelhos tem um ramal e COMO os alcançar (o token de push do
-- fornecedor). Esta migração guarda isso, e o rasto mínimo de cada *wake*.
--
-- `voice_devices`: um aparelho por linha. O `id` é escolhido pela app (idempotência do PUT). Está
-- ligado a uma SESSÃO (ADR-0011): terminar a sessão desliga o aparelho, porque o *wake* só considera
-- aparelhos cuja sessão não foi revogada. O token vive CIFRADO em repouso (`secrets_at_rest`); o hash
-- serve a unicidade sem decifrar.
CREATE TABLE voice_devices (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    extension_id    UUID NOT NULL REFERENCES voice_extensions(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    session_id      UUID NOT NULL REFERENCES user_sessions(id) ON DELETE CASCADE,
    platform        TEXT NOT NULL CHECK (platform IN ('android', 'ios')),
    provider        TEXT NOT NULL CHECK (provider IN ('fcm', 'apns_voip', 'lab', 'delonix')),
    push_token      TEXT NOT NULL,
    push_token_hash TEXT NOT NULL,
    app_version     TEXT NOT NULL DEFAULT '' CHECK (char_length(app_version) <= 64),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at      TIMESTAMPTZ
);
-- O mesmo token no mesmo ramal é o mesmo aparelho: um registo novo (reinstalação, `id` novo) revoga o
-- anterior em vez de acordar o telemóvel duas vezes.
CREATE UNIQUE INDEX voice_devices_token_uidx
    ON voice_devices (extension_id, provider, push_token_hash) WHERE revoked_at IS NULL;
CREATE INDEX voice_devices_extension_idx ON voice_devices (extension_id) WHERE revoked_at IS NULL;
CREATE INDEX voice_devices_session_idx ON voice_devices (session_id);

-- `voice_push_wakes`: uma linha por (chamada, aparelho). Serve duas coisas: a idempotência (o mesmo
-- `call_uuid` não manda dois pushes ao mesmo aparelho) e o limite por ramal e por minuto. Apaga-se ao
-- fim de um dia; nada aqui é histórico de chamadas (isso é o CDR).
CREATE TABLE voice_push_wakes (
    call_uuid    TEXT NOT NULL CHECK (char_length(call_uuid) <= 64),
    device_id    UUID NOT NULL REFERENCES voice_devices(id) ON DELETE CASCADE,
    extension_id UUID NOT NULL REFERENCES voice_extensions(id) ON DELETE CASCADE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (call_uuid, device_id)
);
CREATE INDEX voice_push_wakes_extension_idx ON voice_push_wakes (extension_id, created_at);
