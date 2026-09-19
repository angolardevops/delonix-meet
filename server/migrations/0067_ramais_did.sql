-- Ramais internos — Fase 2: ramal alcançável do PSTN via DID dedicado.
--
-- Estende voice_did (migração 0014, server/src/voice.rs) em vez de criar uma
-- tabela de junção: um DID só pode apontar para UMA coisa de cada vez — uma
-- sala de voz EFÉMERA (voice_room.did_id, já existente) ou um ramal
-- PERMANENTE (esta coluna) — nunca as duas. É o mesmo padrão de FK nullable
-- directa que voice_room já usa para apontar para voice_did; não há motivo
-- para um modelo diferente aqui só porque a seta aponta na outra direcção.
--
-- Mutual exclusividade com voice_room ACTIVA: NÃO é imposta por CHECK nesta
-- migração — um CHECK do Postgres não pode consultar outra tabela. Fica a
-- cargo da aplicação:
--   - server/src/ramais.rs::assign_extension_did — verifica-e-rejeita (não
--     há voice_room activa a usar o DID) antes do UPDATE, o mesmo padrão
--     não-transaccional que voice.rs::create_room já usa para escolher um
--     DID livre (ver o retry-loop do PIN lá).
--   - server/src/voice.rs::create_room — as três queries de resolução de DID
--     passam a excluir `extension_id IS NOT NULL`, para não escolher para uma
--     sala efémera um número já preso a um ramal.
-- Nenhuma das duas é atómica ponta-a-ponta (há uma janela entre o SELECT de
-- verificação e o UPDATE/INSERT); aceite pelo mesmo motivo que o padrão já
-- aceite em create_room: a colisão real exige dois admins a atribuir o MESMO
-- número no MESMO instante, e o pior caso é um erro 409 a ser corrigido à mão,
-- não uma chamada perdida.
ALTER TABLE voice_did
    ADD COLUMN extension_id UUID REFERENCES voice_extensions(id) ON DELETE SET NULL;

-- Um ramal, no máximo um DID. Mantém a resolução em
-- ramais.rs::ivr_dialplan_did sem ambiguidade (um número discado → um único
-- ramal) e evita um admin atribuir por engano dois números "principais" ao
-- mesmo ramal. UNIQUE simples chega — o Postgres trata múltiplos NULL como
-- não-colidentes, por isso ramais sem DID (a esmagadora maioria) não entram
-- nesta restrição.
ALTER TABLE voice_did
    ADD CONSTRAINT voice_did_extension_uidx UNIQUE (extension_id);

CREATE INDEX voice_did_extension_idx ON voice_did(extension_id) WHERE extension_id IS NOT NULL;
