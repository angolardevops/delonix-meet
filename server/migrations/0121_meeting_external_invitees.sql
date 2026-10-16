-- ════════════════════════════════════════════════════════════════════════
--  Convidar um endereço de email que NÃO tem conta aqui.
--  PORQUE EXISTE. A `meeting_invitees` (migração 0004) tem chave
--  `(meeting_id, user_id)`: só sabe convidar quem já é utilizador. Era a outra
--  metade da lacuna nº2 do levantamento de 2026-10-13 — o `.ics` no convite
--  entrou no #348, mas só chegava a quem já tinha conta, e o caso que se perde
--  para o Teams é precisamente o de fora: «convida a maria@outra-empresa.ao».
--  PORQUE É UMA TABELA À PARTE e não colunas na `meeting_invitees`: aquela
--  chave é um contrato de várias consultas (audiência, respostas, índice de
--  pendentes) e um `user_id` nulo partia-as todas em silêncio. Separada, as
--  consultas existentes não mudam de significado.
--  NÃO TEM `status`. Um convidado externo não tem por onde responder: o
--  `PUT /invitees/me` exige sessão. Pôr a coluna aqui era declarar uma resposta
--  que nada pode escrever — e a casa já pagou por eventos que não disparam.
--  Quando houver um caminho de resposta (um link assinado no email), a coluna
--  entra com ele.
-- ════════════════════════════════════════════════════════════════════════

CREATE TABLE meeting_external_invitees (
    meeting_id UUID        NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    -- Normalizado por `identity::validation::normalize_email` antes de entrar,
    -- que é também quem valida a forma: a regra não se escreve duas vezes.
    email      TEXT        NOT NULL,
    -- Quem convidou. Fica para a auditoria responder «quem deu este link a
    -- alguém de fora?», que é a primeira pergunta quando uma reunião interna
    -- aparece numa agenda que não é da casa.
    invited_by UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (meeting_id, email)
);

-- «Em que reuniões é que este endereço foi convidado?» — a pergunta de quem
-- investiga um reenvio ou tira alguém de tudo.
CREATE INDEX meeting_external_invitees_email_idx ON meeting_external_invitees(email);
