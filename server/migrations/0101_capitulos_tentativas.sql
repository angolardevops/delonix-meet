-- Tentativas e espera na geração automática de capítulos.
--
-- O QUE ESTAVA MAL: o `auto_chapters_sweep` (cron de 5 min) re-selecciona as
-- gravações com `chapters_generated_at IS NULL`, o que lhe dá idempotência de
-- graça e duas defesas bem pensadas — pára a volta se o LLM estiver em baixo, e
-- exclui a pílula envenenada (`error_code = 'ai.bad_response'`).
--
-- **Mas o braço de erro e o `ai.timeout` não contam nada.** Uma gravação cujo
-- texto faça o modelo estourar o prazo é repetida **a cada 5 min para sempre**,
-- sem backoff e sem tecto, a ocupar 1 das 2 vagas por volta e a atrasar as
-- gravações atrás dela. Levantamento:
-- `docs/levantamento-2026-10-07-trabalho-assincrono.md` §1, nível C.
--
-- PORQUE AS COLUNAS FICAM EM `recordings` e não em
-- `recording_chapter_generations`: o ESTADO do trabalho (quem pediu, com que
-- resultado, porque falhou) é dessa tabela e lá fica — a migração 0076
-- argumentou-o bem. Isto é a escrituração da FILA, e a fila selecciona de
-- `recordings`, onde o `chapters_generated_at` já vive. Uma fila que lesse o
-- «pronto?» de uma tabela e as tentativas de outra não caberia na peça comum
-- (`delonix_meet_core::jobs::Queue`, que é de uma tabela só) e teria de voltar
-- a ser SQL à mão, que é o que este trabalho existe para acabar.
ALTER TABLE recordings
    ADD COLUMN chapters_attempts        INT NOT NULL DEFAULT 0
                                        CHECK (chapters_attempts >= 0),
    ADD COLUMN chapters_next_attempt_at TIMESTAMPTZ NULL;

-- Só as que estão à espera da próxima tentativa. Parcial porque em regime
-- quase nenhuma o é: uma gravação é transcrita e os capítulos saem à primeira.
CREATE INDEX recordings_chapters_backoff_idx
    ON recordings (chapters_next_attempt_at)
    WHERE chapters_generated_at IS NULL AND chapters_next_attempt_at IS NOT NULL;
