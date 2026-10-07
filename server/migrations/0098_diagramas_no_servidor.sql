-- Diagramas no servidor (ADR-0020). Até aqui o modelo editável de um diagrama
-- vivia SÓ no IndexedDB do browser (`delonix-diagramas`): trocar de computador
-- perdia tudo, e a única forma de o levar era exportar o JSON à mão. A
-- biblioteca de `whiteboards` guarda o PNG — a imagem achatada —, que serve
-- para mostrar e não para continuar a editar.
--
-- A CHAVE É (owner_id, id), e não um UUID novo. O `id` é escolhido pelo
-- CLIENTE (`uid('d')` no `web/src/pages/diagrams/model.ts`), que é o que faz o
-- offline-first funcionar: o diagrama tem id antes de haver rede. Dois
-- utilizadores podem escolher o mesmo `id` — com a chave composta, a linha de
-- um nunca toca na do outro. Uma PK só no `id` dava o contrário: o primeiro a
-- gravar ficava com ele, e o segundo levava um conflito ou, pior, escrevia por
-- cima.
--
-- O `org_id` fica para a pertença (um diagrama é da pessoa DENTRO de uma
-- organização) e para o apagar em cascata quando a organização sai.
CREATE TABLE diagrams (
    id          TEXT        NOT NULL,
    owner_id    UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    org_id      UUID        NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    title       TEXT        NOT NULL DEFAULT '',
    notation    TEXT        NOT NULL DEFAULT 'free',
    room_code   TEXT        NOT NULL DEFAULT '',
    -- O documento inteiro, como o cliente o escreve. JSONB e não TEXT: dá para
    -- perguntar pelo conteúdo (quantos nós, que notação) sem o descodificar na
    -- aplicação.
    doc         JSONB       NOT NULL,
    -- Desnormalizado de propósito: a LISTA mostra-o e `jsonb_array_length` em
    -- cada linha de cada listagem é trabalho a mais para um número que o
    -- cliente já sabe quando grava.
    elements    INTEGER     NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (owner_id, id)
);

-- A listagem é sempre «os meus, mais recentes primeiro».
CREATE INDEX diagrams_owner_idx ON diagrams (owner_id, updated_at DESC);
-- Para o apagar em cascata e para contar por organização (vizinho ruidoso).
CREATE INDEX diagrams_org_idx ON diagrams (org_id);
