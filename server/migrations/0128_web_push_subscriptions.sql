-- Web Push (A-03, ADR-0032): os browsers que aceitaram ser avisados com a
-- aplicação fechada.
--
-- PORQUE EXISTE: até aqui um toque de reunião só chegava por WebSocket. Quem
-- não tinha a aba aberta ficava em `offline` na resposta do `ring` e com uma
-- chamada perdida para ver depois — nunca era avisado A TEMPO. Cada linha é uma
-- subscrição `PushManager.subscribe()` de um browser: o endpoint do serviço de
-- push desse browser (Google, Mozilla, Apple, Microsoft) e as duas chaves com que
-- o servidor cifra o aviso de ponta a ponta (RFC 8291) — o serviço de push vê
-- que houve um aviso, não o que diz.
--
-- PESSOAL, não da organização: uma subscrição é de uma pessoa num browser, e
-- avisa-a de qualquer organização a que pertença. Apagar a conta leva-as.
CREATE TABLE web_push_subscriptions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- ÚNICO de propósito: um browser partilhado onde outra pessoa entra e
    -- subscreve passa a avisar a pessoa NOVA (o `INSERT … ON CONFLICT` muda o
    -- dono). Com uma linha por pessoa, a anterior continuava a receber os
    -- avisos de quem entrou depois dela no mesmo browser.
    endpoint        TEXT NOT NULL UNIQUE,
    -- Chave pública ECDH P-256 do browser (65 bytes, base64url) e o segredo de
    -- autenticação (16 bytes, base64url). Não são segredos do servidor: só
    -- servem para cifrar PARA este browser.
    p256dh          TEXT NOT NULL,
    auth            TEXT NOT NULL,
    user_agent      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_success_at TIMESTAMPTZ,
    last_failure_at TIMESTAMPTZ,
    failures        INT NOT NULL DEFAULT 0
);

CREATE INDEX web_push_subscriptions_user_idx ON web_push_subscriptions(user_id);
