-- Sondagens/quizzes ficam guardados com a reunião: cada fecho de sondagem
-- (signaling.rs, PollClose) anexa o resultado final aqui.
-- [{question, options, counts, correct, total_votes, total_right, total_wrong, by}]
ALTER TABLE meetings ADD COLUMN IF NOT EXISTS polls JSONB NOT NULL DEFAULT '[]'::jsonb;
