-- Reserva de trabalho de transcrição (gRPC TranscriptionService, ADR-0005 §3).
--
-- Antes, o ai-worker fazia polling a `transcribed_at IS NULL` directamente no
-- Postgres e escrevia por baixo do servidor: dois workers apanhavam a mesma
-- gravação, uma gravação que rebentasse o modelo era repetida para sempre, e o
-- texto entrava na base sem DLP. A reserva com prazo resolve os três: um
-- worker reserva (lease), e só quem tem o token entrega; um prazo expirado
-- devolve o trabalho à fila; ao fim de N tentativas a gravação sai da fila.
ALTER TABLE recordings
    ADD COLUMN IF NOT EXISTS transcription_lease_token TEXT,
    ADD COLUMN IF NOT EXISTS transcription_lease_expires_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS transcription_attempts INT NOT NULL DEFAULT 0,
    -- Última razão de falha (texto do worker, sem dados da reunião).
    ADD COLUMN IF NOT EXISTS transcription_error TEXT,
    -- Falhou de vez (retryable=false ou tentativas esgotadas).
    ADD COLUMN IF NOT EXISTS transcription_failed_at TIMESTAMPTZ;

-- A fila: gravações prontas e por transcrever, da mais antiga para a mais nova.
CREATE INDEX IF NOT EXISTS idx_recordings_transcription_queue
    ON recordings (created_at)
    WHERE transcribed_at IS NULL AND transcription_failed_at IS NULL AND status = 'ready';
