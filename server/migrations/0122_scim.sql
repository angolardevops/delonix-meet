-- ════════════════════════════════════════════════════════════════════════
--  SCIM 2.0 (RFC 7643/7644): o directório da empresa escreve os membros.
--  PORQUE EXISTE. É a lacuna nº1 do levantamento de 2026-10-13 e a primeira
--  pergunta de qualquer organização com mais de 200 pessoas: «as entradas e as
--  saídas vêm do nosso directório?». Sem isto, cada saída é manual — e uma
--  conta que não sai é um achado de auditoria.
--  O CICLO JÁ EXISTIA PARA UM SÓ ECOSSISTEMA: o Odoo cria, actualiza, muda
--  papéis e suspende quem saiu (`odoo.rs::provision`). O SCIM é o mesmo ciclo
--  falado pelo protocolo que o Entra ID e o Okta falam.
--  O TOKEN É PRÓPRIO, e isso é a lição da R142: um token emitido para uma
--  superfície não pode abrir outra. O `dlxo_` do Odoo provisiona o directório;
--  o `dlxs_` daqui também — mas quem contrata um não contrata o outro, e o
--  extractor de cada um recusa o prefixo do vizinho.
--  QUEM O SCIM GERE fica marcado na conta (`scim_managed`), como o
--  `odoo_managed`: é o que permite suspender «quem saiu» sem tocar em contas
--  que nunca vieram do directório.
-- ════════════════════════════════════════════════════════════════════════

ALTER TABLE organizations
    -- sha256 do token em claro, que só se vê uma vez (como o do Odoo).
    ADD COLUMN scim_token_hash   TEXT,
    -- Os primeiros 12 caracteres (`dlxs_XXXXXX`), para a consola mostrar QUAL
    -- token está configurado sem o guardar.
    ADD COLUMN scim_token_prefix TEXT,
    -- O extractor exige isto a TRUE: rodar a chave numa org desligada dava um
    -- token que nunca autenticava (o mesmo defeito que o Odoo teve).
    ADD COLUMN scim_enabled      BOOLEAN NOT NULL DEFAULT FALSE,
    -- Última vez que o directório falou. É a pergunta nº1 quando alguém diz
    -- «o SCIM parou»: saber se o problema é nosso ou do lado deles.
    ADD COLUMN scim_last_seen_at TIMESTAMPTZ;

ALTER TABLE users
    -- Esta conta é gerida pelo directório de uma organização.
    ADD COLUMN scim_managed     BOOLEAN NOT NULL DEFAULT FALSE,
    -- O id que o directório usa para esta pessoa (`externalId` do SCIM). É por
    -- aqui que ele a reencontra depois de o email mudar — e o email muda.
    ADD COLUMN scim_external_id TEXT,
    ADD COLUMN scim_org_id      UUID REFERENCES organizations(id) ON DELETE SET NULL;

-- Um `externalId` é único DENTRO da organização que o emite (duas empresas
-- podem usar o mesmo id do lado delas).
CREATE UNIQUE INDEX users_scim_external_idx
    ON users (scim_org_id, scim_external_id)
    WHERE scim_external_id IS NOT NULL;

-- O MOTIVO DE ARQUIVO É UM CATÁLOGO FECHADO (migração 0053), e um valor novo
-- que não entre aqui sai como 500 em vez de «não pode». Foi exactamente o que
-- aconteceu ao escrever isto: os dois testes da SUSPENSÃO — a operação mais
-- importante do SCIM — falhavam com 500 porque `scim_exit` não estava na lista.
-- Entra ao lado do `odoo_exit`, e pela mesma razão: a proveniência do arquivo
-- responde «quem tirou esta pessoa?» na trilha de auditoria. Um `suspended`
-- genérico perderia isso.
ALTER TABLE org_members DROP CONSTRAINT IF EXISTS org_members_archived_reason_check;
ALTER TABLE org_members
    ADD CONSTRAINT org_members_archived_reason_check
    CHECK (archived_reason IN ('suspended', 'removed', 'odoo_exit', 'scim_exit', 'inactive', 'guest_expired'));
