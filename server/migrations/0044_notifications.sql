-- Centro de notificações (G8): caixa de entrada PESSOAL e persistida.
--
-- Até aqui o centro de notificações do AppShell vivia só no cliente: o que
-- chegava pelo `/rtc` enquanto a app estava fechada, ou noutro dispositivo,
-- perdia-se. Esta tabela é a memória; o `/rtc` passa a ser só o aviso imediato.
--
-- As regras (tipos, textos, coalescência, retenção) estão em
-- `delonix_meet_domain::notification`. Um tipo novo entra na CHECK e no enum
-- do domínio, nunca só num dos dois.
CREATE TABLE notifications (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Organização do destinatário quando a notificação nasceu (NULL se não
    -- tinha). Serve a retenção/remoção por inquilino; o ACESSO é sempre por
    -- `user_id` da sessão.
    org_id      UUID REFERENCES organizations(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN (
                    'meeting.invited', 'meeting.starting', 'meeting.cancelled',
                    'call.missed', 'recording.ready', 'transcription.ready')),
    title       TEXT NOT NULL CHECK (char_length(title) BETWEEN 1 AND 140),
    body        TEXT NOT NULL DEFAULT '' CHECK (char_length(body) <= 500),
    -- Caminho RELATIVO da app. Nunca um URL externo (seria phishing servido
    -- por nós): a guarda do domínio decide, a CHECK é a rede de segurança.
    link        TEXT NOT NULL DEFAULT '/'
                CHECK (link LIKE '/%' AND link NOT LIKE '//%' AND char_length(link) <= 512),
    data        JSONB NOT NULL DEFAULT '{}'::jsonb,
    -- Coalescência: por destinatário, no máximo uma com a mesma chave.
    dedupe_key  TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    read_at     TIMESTAMPTZ
);

-- Listagem: mais recentes primeiro, paginada por (created_at, id).
CREATE INDEX notifications_user_page_idx
    ON notifications (user_id, created_at DESC, id DESC);
-- Filtro `unread_only` e contagem de não lidas.
CREATE INDEX notifications_user_unread_idx
    ON notifications (user_id, created_at DESC, id DESC) WHERE read_at IS NULL;
CREATE UNIQUE INDEX notifications_user_dedupe_idx
    ON notifications (user_id, dedupe_key) WHERE dedupe_key IS NOT NULL;
-- Varredor de retenção.
CREATE INDEX notifications_created_idx ON notifications (created_at);
