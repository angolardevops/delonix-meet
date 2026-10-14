-- ════════════════════════════════════════════════════════════════════════
--  O convite de calendário viaja DENTRO da mensagem de correio.
--  PORQUE EXISTE. O servidor já manda correio (ADR-0025), mas o convite era
--  texto com um link: quem o recebia tinha de copiar a reunião para a agenda à
--  mão. Era a lacuna nº2 do levantamento de 2026-10-13 — e das que bloqueiam
--  uma venda é a de melhor relação esforço/valor, porque a espinha do correio
--  já estava feita.
--  O QUE ISTO MUDA NO ENVIO: com estas duas colunas preenchidas, a mensagem
--  sai em `multipart/alternative` com uma parte `text/calendar; method=…`. É
--  isso que faz o Outlook e o Google mostrarem «aceitar/recusar» em vez de um
--  anexo, e substituírem o evento quando a hora muda (mesmo UID, SEQUENCE
--  maior) em vez de criarem outro.
--  PORQUE É TEXTO E NÃO UMA REFERÊNCIA À REUNIÃO: o `.ics` é o que se ENVIOU.
--  Guardá-lo resolvido torna o envio independente do que a reunião seja depois
--  (remarcada, apagada), e a fila continua a ser o livro de entregas — uma
--  mensagem reivindicada não volta à base para reconstruir nada.
-- ════════════════════════════════════════════════════════════════════════

ALTER TABLE mail_messages
    -- O texto iCalendar já construído (`scheduling::calendar::ics`). NULL na
    -- esmagadora maioria das mensagens: só as de reunião o levam.
    ADD COLUMN calendar_ics    TEXT NULL,
    -- O `METHOD` de dentro desse texto (`REQUEST`, `CANCEL`, `PUBLISH`), que
    -- tem de ir TAMBÉM no `Content-Type` da parte — é de lá que os clientes de
    -- correio o lêem. Em texto, como o `purpose`: um método novo não exige
    -- migração, e quem enfileira escolhe de uma lista fechada no código.
    ADD COLUMN calendar_method TEXT NULL,
    -- Um sem o outro é uma mensagem que não se sabe enviar.
    ADD CONSTRAINT mail_messages_calendar_completo
        CHECK ((calendar_ics IS NULL) = (calendar_method IS NULL));
