-- Tempos de estabelecimento de chamada — os números que faltavam para haver
-- SLO nenhum sobre «quanto demora a entrar numa reunião».
--
-- A auditoria de 2026-08-25 mediu que se recolhiam três métricas de qualidade
-- e ZERO de tempo. E são as de tempo que um cliente pergunta primeiro: «quanto
-- demora a entrar?» não se responde com bitrate.
--
-- Tabela à parte da `call_quality_samples` de propósito: aquela é uma série
-- (uma linha a cada 30 s durante a chamada), esta é UM registo por sessão. Pôr
-- as duas coisas na mesma tabela obrigava a filtrar sempre uma para ler a
-- outra, e as médias sairiam erradas ao primeiro descuido.

CREATE TABLE IF NOT EXISTS call_timings (
    id         BIGSERIAL PRIMARY KEY,
    room_id    UUID NOT NULL REFERENCES rooms(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Da intenção de entrar até haver media. É o número que o utilizador
    -- SENTE, e o que se publica como SLO.
    join_ms    INT,
    -- Da intenção até o WebSocket abrir. Isola a API e a rede de sinal: sem
    -- ele, um join lento não diz se o problema é o servidor a responder ou o
    -- ICE a negociar.
    ws_ms      INT,
    ice_gathering_ms INT,
    first_audio_ms   INT,
    first_video_ms   INT,
    -- Reinícios de ICE e recuperações completas NESTA sessão. Uma chamada que
    -- entrou depressa mas recuperou cinco vezes não foi uma boa chamada.
    ice_restarts INT NOT NULL DEFAULT 0,
    reconnects   INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_timings_time ON call_timings (created_at DESC);
-- O painel pergunta «quais foram lentas», não «lista tudo e filtra».
CREATE INDEX IF NOT EXISTS idx_timings_lentos
    ON call_timings (created_at DESC) WHERE join_ms > 5000;
