-- O resumo da acta pelo LLM passa a ser uma FILA, com estado e tentativas.
--
-- O QUE ESTAVA MAL (levantamento de 2026-10-07, §1 nível E2): o
-- `ai::spawn_mom_summary` é chamado de UM sítio (ao gravar a ata) e, se o
-- Ollama estiver em baixo ou o pod reiniciar, sai por um `return` com um aviso
-- no log e **fica lá**. Não havia coluna de estado, não havia varredor, e não
-- havia rota para pedir outra vez. A ata por regras fica — isso é verdade — mas
-- o Odoo lê o `minutes_ai_at` para saber que o MoM é a versão FINAL
-- (`apikeys.rs`), e passava a ver para sempre uma reunião sem ata final, sem
-- ninguém poder corrigir. Degradação silenciosa e permanente.
--
-- PORQUE UM MARCADOR EXPLÍCITO (`mom_queued_at`) e não um predicado derivado:
-- uma fila com «`minutes_ai_at IS NULL` e transcrição suficiente» passaria a
-- reclamar TODAS as reuniões históricas com transcrição no instante do deploy —
-- centenas de chamadas ao LLM que ninguém pediu. Com o marcador, só entra na
-- fila o que foi explicitamente enfileirado: ao gravar a ata, ou pela rota
-- `POST /api/meetings/{meeting_id}/minutes/summary`.
ALTER TABLE meetings
    ADD COLUMN mom_queued_at        TIMESTAMPTZ NULL,
    ADD COLUMN mom_attempts         INT NOT NULL DEFAULT 0 CHECK (mom_attempts >= 0),
    ADD COLUMN mom_next_attempt_at  TIMESTAMPTZ NULL;

-- O que a fila varre: enfileirado, sem resumo ainda. Parcial porque em regime
-- quase nenhuma reunião o é — o resumo sai na primeira tentativa.
CREATE INDEX meetings_mom_fila_idx
    ON meetings (mom_next_attempt_at NULLS FIRST, mom_queued_at)
    WHERE mom_queued_at IS NOT NULL AND minutes_ai_at IS NULL;
