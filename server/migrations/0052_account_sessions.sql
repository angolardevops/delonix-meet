-- Sessões da conta: o refresh token roda a cada renovação, mas a sessão que
-- o utilizador reconhece ("o portátil de casa", "o telemóvel") sobrevive a
-- essa rotação — por isso session_id e session_started_at nascem uma vez no
-- login e viajam para a linha seguinte em cada refresh (ver auth::issue_tokens).
ALTER TABLE refresh_tokens
    ADD COLUMN session_id UUID NOT NULL DEFAULT gen_random_uuid(),
    ADD COLUMN session_started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN user_agent TEXT,
    ADD COLUMN ip_address TEXT;

CREATE INDEX refresh_tokens_session_idx ON refresh_tokens(session_id);
