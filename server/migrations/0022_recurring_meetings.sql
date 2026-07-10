-- Recorrência de reuniões.
-- freq      : periodicidade (daily | weekly | monthly | yearly)
-- interval  : a cada N períodos (padrão 1)
-- until     : data de fim (exclusive); NULL = sem data de fim
-- count     : número máximo de ocorrências; NULL = ilimitado
-- byday     : para weekly, dias da semana separados por vírgula ('MON,WED,FRI')
-- parent_id : NULL → reunião pai ou one-off; UUID → instância gerada (filho)

ALTER TABLE meetings
  ADD COLUMN recurrence_freq      TEXT     CHECK (recurrence_freq IN ('daily','weekly','monthly','yearly')),
  ADD COLUMN recurrence_interval  SMALLINT NOT NULL DEFAULT 1,
  ADD COLUMN recurrence_until     DATE,
  ADD COLUMN recurrence_count     SMALLINT,
  ADD COLUMN recurrence_byday     TEXT,
  ADD COLUMN recurrence_parent_id UUID     REFERENCES meetings(id) ON DELETE CASCADE;

CREATE INDEX meetings_recurrence_parent ON meetings(recurrence_parent_id)
    WHERE recurrence_parent_id IS NOT NULL;

CREATE INDEX meetings_starts_at_idx ON meetings(starts_at);
