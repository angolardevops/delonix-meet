-- Documentos do estúdio de TV (ADR-0014 §5). Nasceu como 0069 no ramo de
-- trabalho; renumerada para 0111 no porte para o develop.
--
-- Seis tipos com o MESMO contrato (cenas de mistura, macros, sobreposições,
-- cenas de luz, perfis de correcção por câmara e alinhamentos). Um tipo por
-- valor em `kind` em vez de seis tabelas: o contrato, a autorização, a
-- paginação e o conflito de versão são idênticos, e seis tabelas seriam seis
-- cópias das mesmas seis queries — a duplicação que a catraca da arquitectura
-- recusa. O que é próprio de cada tipo vive no `body`, validado pelo domínio
-- (`delonix_meet_domain::studio::document`) antes de chegar aqui.
CREATE TABLE studio_documents (
    id         UUID PRIMARY KEY,
    studio_id  UUID NOT NULL REFERENCES studios(id) ON DELETE CASCADE,
    -- Desnormalizado de propósito: cada rota filtra por org_id ANTES de olhar
    -- para o estúdio, e um JOIN por leitura só para reencontrar a org é o
    -- caminho onde o isolamento se esquece.
    org_id     UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN
                 ('mixer_scene','macro','overlay','light_scene','camera_profile','rundown')),
    name       TEXT NOT NULL,
    -- Tecla de atalho (`F2`, `mod+1`) quando o tipo a tem; `NULL` quando não.
    key        TEXT,
    version    INT NOT NULL DEFAULT 1 CHECK (version >= 1),
    body       JSONB NOT NULL,
    -- Calculado no servidor (hoje só o alinhamento: duração total e contagem).
    summary    JSONB,
    created_by UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    updated_by UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- A página é por estúdio E tipo — é sempre assim que se lê.
CREATE INDEX studio_documents_page_idx ON studio_documents (studio_id, kind, created_at, id);
-- Uma tecla não se repete no mesmo tipo e estúdio (F2 de macro e F2 de cena de
-- luz são teclas DIFERENTES: o `kind` entra na chave).
CREATE UNIQUE INDEX studio_documents_key_uidx
    ON studio_documents (studio_id, kind, key) WHERE key IS NOT NULL;

-- Histórico: uma linha por versão gravada, a actual incluída. Guarda-se o
-- corpo inteiro e não um diff — uma cena de som são alguns KiB, e um histórico
-- que precisa de ser reconstruído por aplicação de diffs é um histórico que
-- ninguém consegue ler no dia em que é preciso.
CREATE TABLE studio_document_versions (
    document_id UUID NOT NULL REFERENCES studio_documents(id) ON DELETE CASCADE,
    version     INT NOT NULL CHECK (version >= 1),
    name        TEXT NOT NULL,
    body        JSONB NOT NULL,
    summary     JSONB,
    updated_by  UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (document_id, version)
);
-- Mais recente primeiro, como a rota das versões a serve.
CREATE INDEX studio_document_versions_recent_idx
    ON studio_document_versions (document_id, version DESC);
