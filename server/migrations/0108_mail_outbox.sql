-- ════════════════════════════════════════════════════════════════════════
--  A caixa de saída do correio.
--
--  PORQUE EXISTE. Até aqui o servidor não enviava correio nenhum: a única
--  ocorrência de «smtp» em todo o `server/` era um comentário a dizer que não
--  enviava. Isso travava três coisas do plano de lacunas — os convites (E2), a
--  reposição de password pela própria pessoa (E3) e o calendário (E7).
--
--  O D7 foi decidido a 2026-10-09 (ver ADR): **relay do operador primeiro**,
--  SMTP por organização depois, atrás de um guarda de saída em TCP que ainda
--  não existe. Por isso esta tabela NÃO tem coluna de fornecedor: hoje há um
--  só, configurado pelo operador. Quando o SMTP por organização entrar,
--  acrescenta-se a referência à configuração da org — e o livro de entregas
--  não muda de forma.
--
--  O MOLDE É O `webhook_deliveries` (migração 0043 + 0090), de propósito: o
--  livro de entregas É a fila, não há broker, e um reinício não perde o que
--  está agendado. A reivindicação passa pela peça comum
--  (`delonix_meet_core::jobs`), como manda o `check-filas-reivindicacao.sh` —
--  não se escreve outro `FOR UPDATE SKIP LOCKED` à mão.
-- ════════════════════════════════════════════════════════════════════════

CREATE TABLE mail_messages (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- NULL quando a mensagem não é de nenhuma organização: a reposição de
    -- password de uma conta sem org, ou correio da própria plataforma. Quando
    -- há org, serve a justiça entre inquilinos da fila (`tenant_column`) e a
    -- auditoria.
    org_id        UUID NULL REFERENCES organizations(id) ON DELETE CASCADE,
    -- Para que serve esta mensagem: `password_reset`, `invitation`, … Fica em
    -- texto e não em enum para uma mensagem nova não exigir migração; quem
    -- enfileira valida contra a sua lista.
    purpose       TEXT NOT NULL,
    to_address    TEXT NOT NULL,
    subject       TEXT NOT NULL,
    body_text     TEXT NOT NULL,

    -- Quantas vezes já se tentou. Sobe a cada falha; a política de repetição
    -- (`jobs::RETRY_WEBHOOK`: 5 tentativas, 30 s a 1 h, ±20 %) decide quando
    -- deixa de haver próxima.
    attempt       INT NOT NULL DEFAULT 1 CHECK (attempt >= 1),
    -- `sending` é a MARCA DE POSSE da reivindicação, e não um enfeite: sem um
    -- estado distinto do `pending`, a linha continuava a bater na condição de
    -- «pronta» e era reclamada outra vez a cada volta do worker.
    status        TEXT NOT NULL DEFAULT 'pending'
                      CHECK (status IN ('pending', 'sending', 'sent', 'failed')),
    error         TEXT NULL,
    -- Marca uma `failed` que ainda vai ser repetida, e quando. NULL = não há
    -- repetição agendada (enviada, ou falha que repetir não cura: endereço
    -- inválido, configuração errada, tentativas esgotadas).
    retry_at      TIMESTAMPTZ NULL,
    -- Quando a tentativa em curso foi reclamada. Serve ao varredor: uma linha
    -- `sending` há demasiado tempo é de um processo que morreu a meio do envio.
    claimed_at    TIMESTAMPTZ NULL,

    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    sent_at       TIMESTAMPTZ NULL
);

-- O que o worker varre, nos dois casos: as que esperam primeiro envio e as que
-- falharam com repetição vencida. Quase nenhuma linha é uma coisa ou outra.
CREATE INDEX mail_messages_ready_idx
    ON mail_messages (created_at)
    WHERE status IN ('pending', 'failed');

-- Reivindicações penduradas, para o varredor das abandonadas.
CREATE INDEX mail_messages_sending_idx
    ON mail_messages (claimed_at) WHERE status = 'sending';

-- Varredor de retenção, e a listagem por organização.
CREATE INDEX mail_messages_org_created_idx
    ON mail_messages (org_id, created_at DESC, id DESC);
