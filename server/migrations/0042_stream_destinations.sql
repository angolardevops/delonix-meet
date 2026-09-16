-- Destinos de emissão em directo guardados por organização (G1; ADR-0003).
--
-- A chave RTMP é uma credencial de TERCEIROS (a conta YouTube/Facebook da
-- empresa) e tem de poder voltar a ser lida pelo servidor para alimentar o
-- `ffmpeg` — por isso não é um hash. Fica CIFRADA (`stream_key_sealed`,
-- core::secret_box, contexto = id da linha) e nunca sai numa listagem: só na
-- criação e na rotação. Uma primeira versão desta tabela (num ramo da UI)
-- guardava-a em claro — a mesma falha da auditoria S5.
CREATE TABLE stream_destinations (
    id                UUID PRIMARY KEY,
    org_id            UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    kind              TEXT NOT NULL CHECK (kind IN ('youtube','facebook','linkedin','rtmp','internal')),
    label             TEXT NOT NULL,
    url               TEXT NOT NULL,
    -- '' = sem chave configurada; senão `enc:v1:<kid>:…`.
    stream_key_sealed TEXT NOT NULL DEFAULT '',
    key_prefix        TEXT NOT NULL DEFAULT '',
    state             TEXT NOT NULL DEFAULT 'ready' CHECK (state IN ('ready','expired','error')),
    created_by        UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- A listagem pagina por (created_at, id) dentro da org.
CREATE INDEX stream_destinations_org_page_idx ON stream_destinations (org_id, created_at, id);
