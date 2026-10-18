-- Gravação ISO (ADR-0014 §2.3) — uma linha por PISTA de uma gravação.
--
-- Porque existe: o interruptor `studios.iso_recording` está `NOT NULL DEFAULT
-- TRUE` desde a 0081, com UI no ecrã de fontes, e a tabela que o serve nunca
-- existiu. Um cliente de produção ligava o estúdio, procurava as pistas por
-- câmara e não as encontrava — e o contrato do estúdio já nomeava esta tabela
-- e as duas rotas como «por implementar».
--
-- O SFU já escrevia um ficheiro por publicação durante a gravação e
-- `remove_dir_all`-ava a pasta no fim do caminho feliz (recorder.rs). A
-- matéria-prima existia e era deitada fora; isto é o registo que a torna
-- alcançável.
CREATE TABLE recording_tracks (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    recording_id  UUID NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    -- Quem publicou. É o `publisher` do SFU (um peer id): para uma fonte de
    -- estúdio é o id da fonte, para um telefone é o id da perna. NÃO é uma
    -- chave estrangeira de propósito — um par pode ter saído, e a gravação
    -- continua a ser dele.
    publisher_id  UUID NOT NULL,
    -- "video" | "screen" | "audio", como o `kind` do recorder.
    kind          TEXT NOT NULL CHECK (kind IN ('video', 'screen', 'audio')),
    -- O nome do ficheiro DENTRO da pasta da gravação, nunca um caminho
    -- absoluto: quem serve junta-o ao `recordings_dir` da configuração. Um
    -- caminho na base era uma travessia de directório à espera de acontecer.
    filename      TEXT NOT NULL CHECK (filename !~ '/' AND filename <> ''),
    size_bytes    BIGINT NOT NULL CHECK (size_bytes >= 0),
    -- Onde a pista começa na linha do tempo da gravação. É o
    -- `RecTrackMeta::starts_at_ms()`, e é o que a composição multi-faixa
    -- (fatia 2) vai usar no `-itsoffset`.
    starts_at_ms  BIGINT NOT NULL CHECK (starts_at_ms >= 0),
    -- O número e a etiqueta da fonte de estúdio, resolvidos UMA VEZ ao
    -- finalizar e fotografados aqui. Nulos fora de um estúdio: não se inventa
    -- nome a um par que não é uma câmara. Fotografados porque a fonte pode ser
    -- revogada ou renomeada depois, e a gravação é do que aconteceu.
    source_number INT CHECK (source_number IS NULL OR source_number BETWEEN 1 AND 16),
    source_label  TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX recording_tracks_recording_idx ON recording_tracks(recording_id);

-- Duas pistas da mesma gravação não podem apontar ao mesmo ficheiro: uma
-- repetição da composição que voltasse a inserir daria pistas duplicadas no
-- ecrã, e é por isso que o `finalize` escreve com `ON CONFLICT DO NOTHING`.
CREATE UNIQUE INDEX recording_tracks_file_uidx ON recording_tracks(recording_id, filename);
