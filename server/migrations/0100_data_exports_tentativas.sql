-- Tentativas nas exportações «os meus dados».
--
-- O QUE ESTAVA MAL: a fila das exportações tinha tudo o que uma fila precisa —
-- `SKIP LOCKED`, reserva de 15 min, requeue do abandonado no topo do
-- `run_queue`, cron de um minuto — **menos tentativas**. Uma falha transitória
-- (o Postgres a reiniciar, o disco cheio por um minuto) fechava o pedido como
-- `failed` com a mensagem «não foi possível gerar a exportação; peça outra».
-- A PESSOA era o mecanismo de retry, e pela regra da casa um passo manual no
-- caminho do cliente é um bloqueio, não uma nota de rodapé.
--
-- Levantamento: `docs/levantamento-2026-10-07-trabalho-assincrono.md` §1,
-- nível B («fila durável sem tentativas»).
--
-- `attempts` conta as reivindicações; `next_attempt_at` é a espera entre elas —
-- o backoff de `delonix_meet_core::jobs`, com espalhamento. NULL = pronta já.
ALTER TABLE data_exports
    ADD COLUMN attempts        INT NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    ADD COLUMN next_attempt_at TIMESTAMPTZ NULL;

-- A fila passa a olhar também para `next_attempt_at`. O índice de 0080 cobre
-- `created_at` com `status IN ('queued','running')` e serve a ordem; este cobre
-- a espera, e é parcial porque em regime quase nenhuma linha está a esperar.
CREATE INDEX data_exports_backoff_idx
    ON data_exports (next_attempt_at)
    WHERE status = 'queued' AND next_attempt_at IS NOT NULL;
