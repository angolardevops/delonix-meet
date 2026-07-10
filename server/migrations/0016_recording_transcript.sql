-- Transcrição/ata por gravação (preenchidas pelo worker de IA em GPU) e marca
-- de processamento (idempotência — o worker só pega no que ainda não fez).
ALTER TABLE recordings ADD COLUMN transcript TEXT NOT NULL DEFAULT '';
ALTER TABLE recordings ADD COLUMN minutes TEXT NOT NULL DEFAULT '';
ALTER TABLE recordings ADD COLUMN transcribed_at TIMESTAMPTZ;
