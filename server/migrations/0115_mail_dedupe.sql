-- ════════════════════════════════════════════════════════════════════════
--  Uma mensagem por acontecimento: o dedupe da caixa de saída.
--
--  PORQUE EXISTE. O centro de notificações (G8) passa a entregar também por
--  correio, e os seus produtores são REPETÍVEIS de propósito: o lembrete de
--  reunião é um passo de varrimento, e uma notificação pode ser produzida
--  outra vez depois de um reinício. A tabela `notifications` já se defende
--  disso com `dedupe_key` e um `ON CONFLICT DO NOTHING`; a caixa de saída não
--  tinha defesa nenhuma, e sem ela um lembrete repetido mandava dois emails à
--  mesma pessoa.
--
--  É o MESMO mecanismo da `notifications`, de propósito — não se inventa um
--  segundo. O par é (destinatário, chave): a mesma chave para endereços
--  diferentes são mensagens diferentes, e é o que acontece quando duas pessoas
--  são avisadas da mesma reunião.
--
--  `NULL` = sem dedupe, que é o caso de tudo o que é pedido à mão (reposição
--  de password, convite de organização, prova de endereço): aí quem pede
--  outra vez QUER outra mensagem.
-- ════════════════════════════════════════════════════════════════════════

ALTER TABLE mail_messages ADD COLUMN dedupe_key TEXT NULL;

CREATE UNIQUE INDEX mail_messages_dedupe_uidx
    ON mail_messages (to_address, dedupe_key)
    WHERE dedupe_key IS NOT NULL;
