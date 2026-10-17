-- Captura de TV por HDMI (RFC-0002, Fase 1; ADR-0031) — o plano de controlo do
-- INVENTÁRIO. Não há aqui nada de media: isto é quem reporta, o que reportou, e
-- a que canal cada entrada está ligada.
--
-- Porque NÃO há capacidade nova: gerir agentes de captura é preparar um canal,
-- e essa decisão já existe no catálogo como `broadcast.manage_channels`
-- (migração 0088). Pôr no ar continua a ser `broadcast.go_live`.

-- A caixa onde os capturadores estão ligados. Molde: `sms_gateway` (0039) — o
-- token `dlxtv_` só existe na resposta que o cria; guarda-se o SHA-256.
CREATE TABLE tv_capture_agents (
    id            UUID PRIMARY KEY,
    org_id        UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    -- Onde a caixa está, em texto livre («sala técnica», «casa do Walter»).
    location      TEXT NOT NULL DEFAULT '',
    token_prefix  TEXT NOT NULL,
    token_hash    TEXT NOT NULL UNIQUE,
    -- A versão que o agente diz ter; nula até ao primeiro relatório.
    agent_version TEXT,
    created_by    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at  TIMESTAMPTZ,
    revoked_at    TIMESTAMPTZ
);
CREATE INDEX tv_capture_agents_org_page_idx ON tv_capture_agents (org_id, created_at, id);

-- Uma entrada física: a PORTA onde um descodificador está ligado, não o
-- aparelho (ADR-0031 §2). `device_path` é o caminho físico do barramento
-- (`ID_PATH` do udev, o mesmo que o `by-path`): é a única chave estável, porque
-- `/dev/videoN` muda com a ordem de ligação e dois dongles do mesmo modelo
-- partilham `vendor:product` — e a «série», quando existe, chega a ser uma
-- constante do fabricante (medido: `SN0001` na webcam da máquina de dev).
--
-- Uma entrada que deixa de aparecer num relatório NÃO é apagada: fica sem
-- sinal. É deliberado — um cabo solto não pode apagar a configuração do canal.
CREATE TABLE tv_capture_inputs (
    id           UUID PRIMARY KEY,
    agent_id     UUID NOT NULL REFERENCES tv_capture_agents(id) ON DELETE CASCADE,
    org_id       UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    device_path  TEXT NOT NULL,
    -- Etiqueta dada pela pessoa («ZAP sala», «DStv estúdio»); vazia até lá.
    label        TEXT NOT NULL DEFAULT '',
    vendor_id    TEXT NOT NULL DEFAULT '',
    product_id   TEXT NOT NULL DEFAULT '',
    serial       TEXT,
    -- Os nós do SO neste momento. São informativos e MUDAM: nunca são chave.
    video_node   TEXT,
    audio_node   TEXT,
    -- O que o aparelho declara (formatos, resoluções, fps), como o agente o viu.
    capabilities JSONB NOT NULL DEFAULT '{}'::jsonb,
    -- `protected` é o caso HDCP, dito pelo nome: imagem preta COM áudio. Não é
    -- `no_signal`, e não se resolve em software (RFC-0002 §12.1).
    signal       TEXT NOT NULL DEFAULT 'no_signal'
                 CHECK (signal IN ('live', 'no_signal', 'protected', 'error')),
    -- Como se muda de canal nesta entrada. `none` é o estado legítimo de uma
    -- entrada fixa num canal; `cec` e `ir` chegam com a fila de sintonia.
    tuner_kind   TEXT NOT NULL DEFAULT 'none'
                 CHECK (tuner_kind IN ('none', 'cec', 'ir')),
    tuner_config JSONB NOT NULL DEFAULT '{}'::jsonb,
    -- O canal que esta entrada alimenta. Nulo = entrada inventariada e sem uso.
    channel_id   UUID REFERENCES tv_channels(id) ON DELETE SET NULL,
    version      INTEGER NOT NULL DEFAULT 1,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_signal_at TIMESTAMPTZ,
    CONSTRAINT tv_capture_inputs_agent_path_key UNIQUE (agent_id, device_path)
);
CREATE INDEX tv_capture_inputs_org_page_idx ON tv_capture_inputs (org_id, created_at, id);
-- Um canal é alimentado por UMA entrada: duas a publicar no mesmo canal era
-- sinal a lutar com sinal.
CREATE UNIQUE INDEX tv_capture_inputs_channel_key ON tv_capture_inputs (channel_id)
    WHERE channel_id IS NOT NULL;
