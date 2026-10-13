-- Recortes de uma gravação, e o short vertical (ADR-0027, plano do MCP).
--
-- NUMERAÇÃO: escolhida ao abrir a PR. A última era a 0114 (OAuth).
--
-- Uma tabela para os dois: um short é um recorte com enquadramento 9:16 e,
-- se se pedir, legendas queimadas. O que muda é o que o render faz, não o que
-- o pedido é — e duas tabelas com as mesmas seis colunas de estado seriam duas
-- filas a manter.
--
-- O ESTADO é o mesmo molde das outras filas desta casa (`delonix_meet_core::jobs`):
-- `status` + `attempts` + `next_attempt_at` + `running_at` como marca de posse.
-- `running_at` é lido DEPOIS da reivindicação e exigido de volta na escrita
-- final: é isso que impede uma reivindicação velha (já stale, já levada por
-- outro nó) de fechar um render que não é dela.
--
-- E por isso a MARCA e o PRAZO são duas colunas, como no gravador
-- (`progress_at` + `compose_lease_expires_at`): o sinal de vida renova
-- `lease_expires_at` e **nunca** toca em `running_at`. Com uma coluna só, o
-- escritor de progresso mudava a marca a cada dois segundos e a escrita final
-- não a reconhecia — um render que acabava bem ficava `processing` para sempre
-- (medido na prova `o_corte_e_exacto_e_a_duracao_mede_se_nas_pistas`).
CREATE TABLE IF NOT EXISTS recording_clips (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    recording_id    UUID NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    created_by      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL CHECK (kind IN ('clip', 'short')),
    label           TEXT NOT NULL CHECK (char_length(label) BETWEEN 1 AND 120),
    start_ms        BIGINT NOT NULL CHECK (start_ms >= 0),
    end_ms          BIGINT NOT NULL,
    CONSTRAINT recording_clips_intervalo CHECK (end_ms - start_ms >= 1000),
    -- Enquadramento do short: que parte da largura fica. Não se aplica ao
    -- recorte normal, e por isso tem omissão em vez de ser nulo.
    focus           TEXT NOT NULL DEFAULT 'center' CHECK (focus IN ('center', 'left', 'right')),
    -- Língua do WebVTT que sai AO LADO do recorte. NULL = nenhum.
    --
    -- Não é «queimar na imagem»: o ffmpeg desta imagem é compilado com
    -- `--disable-autodetect` e só libvpx/libopus/zlib — sem libass e sem
    -- libfreetype não existem os filtros `subtitles` nem `drawtext`. Queimar
    -- exigiria mudar a imagem e decidir a licença da FreeType, que é decisão
    -- do dono (ver docs/plano-mcp-2026-10-13.md).
    captions_lang   TEXT CHECK (captions_lang IS NULL OR char_length(captions_lang) BETWEEN 2 AND 12),

    status          TEXT NOT NULL DEFAULT 'queued'
                    CHECK (status IN ('queued', 'processing', 'ready', 'failed')),
    progress        DOUBLE PRECISION NOT NULL DEFAULT 0
                    CHECK (progress >= 0 AND progress <= 1),
    attempts        INT NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ,
    -- A MARCA desta reivindicação: escrita uma vez, nunca mexida.
    running_at      TIMESTAMPTZ,
    -- O PRAZO da posse: renovado pelo sinal de vida enquanto o render anda.
    lease_expires_at TIMESTAMPTZ,

    -- O que o render produziu. Vazio até ficar `ready`.
    filename        TEXT,
    size_bytes      BIGINT,
    duration_ms     BIGINT,
    -- Código ESTÁVEL da falha (é contrato: a tool e a consola mostram-no).
    error_code      TEXT,

    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    ready_at        TIMESTAMPTZ
);

-- A lista de uma gravação, mais recente primeiro (keyset por (created_at, id)).
CREATE INDEX IF NOT EXISTS recording_clips_da_gravacao_idx
    ON recording_clips (recording_id, created_at DESC, id DESC);

-- A varredura da fila: só olha para o que está por fazer. Parcial, para não
-- percorrer os recortes já prontos — que são a maioria depois de uma semana.
CREATE INDEX IF NOT EXISTS recording_clips_por_fazer_idx
    ON recording_clips (created_at)
    WHERE status IN ('queued', 'processing');
