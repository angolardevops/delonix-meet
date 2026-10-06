-- Tecto de participantes concorrentes por organização (NULL = o valor por
-- omissão do nó, `ORG_MAX_PARTICIPANTS`; sem ele, ilimitado). Define-o o
-- OPERADOR da plataforma, nunca a própria organização: é um limite do plano, e
-- uma org que o pudesse subir não estaria limitada.
ALTER TABLE organizations
    ADD COLUMN max_concurrent_participants INT
        CHECK (max_concurrent_participants IS NULL OR max_concurrent_participants > 0);
