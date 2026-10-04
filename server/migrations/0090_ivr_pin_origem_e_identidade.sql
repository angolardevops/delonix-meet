-- O IVR usa o PIN do ramal (plano de produção, item 3.8, lote 2; R277).
--
-- Pré-condição de dar à verificação um consumidor:
--
-- 1. O contador do ramal ganha JANELA e o bloqueio ganha NÍVEL (duração
--    crescente). As regras vivem sem IO em
--    `delonix_meet_domain::telephony::extension_pin::Throttle`.
--    `pin_failure_window_at`: quando começou a janela das falhas em curso.
--    `pin_lock_level`: quantos bloqueios seguidos (0 = nenhum). O
--    `pin_locked_until` deixa de voltar a NULL numa falha que não bloqueia: é
--    por ele que se sabe há quanto tempo acabou o último bloqueio.
ALTER TABLE voice_extensions ADD COLUMN pin_failure_window_at TIMESTAMPTZ;
ALTER TABLE voice_extensions ADD COLUMN pin_lock_level INT NOT NULL DEFAULT 0
    CHECK (pin_lock_level >= 0);

-- 2. O travão por ORIGEM (quem liga), antes de a falha contar no ramal.
--
--    Em Postgres e não no `RateLimiter` em memória (`rate_limit.rs`): o
--    servidor corre em várias réplicas (`deploy/k8s/02-server.yaml`), e o
--    pedido do FreeSWITCH cai em qualquer uma — um contador por processo dava
--    a cada origem tantas tentativas quantas réplicas houvesse. O Redis do
--    repo é o barramento de pub/sub e o estado das salas, não um travão
--    canónico, e é opcional (`REDIS_URL`). O contador do ramal já vive aqui,
--    com `FOR UPDATE`; o da origem segue-o.
--
--    A chave é composta pelo servidor a partir do que o FreeSWITCH sabe da
--    chamada (`<rede>|<número>`) e é GLOBAL, não por organização: é um facto
--    da rede telefónica, e quem ataca uma organização não ganha tentativas
--    novas por mudar de alvo. Sem `org_id`, não há leitura desta tabela fora
--    de `extension_pin.rs`.
CREATE TABLE voice_pin_origins (
    origin TEXT PRIMARY KEY CHECK (char_length(origin) BETWEEN 1 AND 200),
    failures INT NOT NULL DEFAULT 0 CHECK (failures >= 0),
    window_started_at TIMESTAMPTZ,
    lock_level INT NOT NULL DEFAULT 0 CHECK (lock_level >= 0),
    locked_until TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- A limpeza das origens que já não contam (`extension_pin::forget_stale_origins`)
-- procura por `updated_at`.
CREATE INDEX voice_pin_origins_updated_idx ON voice_pin_origins (updated_at);

-- 3. Quem entra na sala pela ponte, IDENTIFICADO. O IVR não passa o nome nem
--    a pessoa à ponte (o Lua não é fonte de verdade de identidade): passa um
--    bilhete opaco, de uso único e de dois minutos, que o servidor emitiu
--    quando identificou quem liga. A ponte troca-o pela identidade ao sentar
--    a chamada no censo (`server/src/voice_caller.rs`). Só o hash (sha256)
--    fica guardado, e o bilhete vale para UMA sala. Isolamento: a tabela só
--    se lê pelo hash do bilhete; `org_id` fica para a limpeza em cascata.
CREATE TABLE voice_caller_tickets (
    token_hash TEXT PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    room_code TEXT NOT NULL,
    extension_id UUID NOT NULL REFERENCES voice_extensions(id) ON DELETE CASCADE,
    member_id UUID,
    display_name TEXT NOT NULL CHECK (char_length(display_name) BETWEEN 1 AND 120),
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX voice_caller_tickets_expires_idx ON voice_caller_tickets (expires_at);
