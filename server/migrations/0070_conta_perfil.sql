-- «A minha conta» (Navegavel3, DelonixProfile + DelonixTour) — frente B.
--
-- NUMERAÇÃO: 0070–0074 são da frente B (backend-v3-comum.md). O buraco entre
-- 0048 e 0070 é esperado até à integração.

-- 1. Perfil. `username` continua a ser o identificador único que o resto do
--    produto já usa; `display_name` é o nome que aparece na sala e nas
--    legendas (NULL = usa o `username`). `legal_name` e `department` são da
--    autoridade Odoo numa conta gerida — escritos pela sincronização, nunca
--    pela rota do perfil.
ALTER TABLE users ADD COLUMN IF NOT EXISTS display_name TEXT
    CHECK (display_name IS NULL OR char_length(display_name) BETWEEN 1 AND 80);
ALTER TABLE users ADD COLUMN IF NOT EXISTS legal_name TEXT
    CHECK (legal_name IS NULL OR char_length(legal_name) <= 200);
ALTER TABLE users ADD COLUMN IF NOT EXISTS department TEXT
    CHECK (department IS NULL OR char_length(department) <= 200);
ALTER TABLE users ADD COLUMN IF NOT EXISTS job_title TEXT NOT NULL DEFAULT ''
    CHECK (char_length(job_title) <= 100);
-- Nome IANA; validado no domínio contra a base `tz` compilada.
ALTER TABLE users ADD COLUMN IF NOT EXISTS timezone TEXT NOT NULL DEFAULT 'Africa/Luanda'
    CHECK (char_length(timezone) <= 64);
-- Última alteração LOCAL da password (NULL = desconhecida: contas antigas e
-- contas geridas pelo Odoo, onde a password muda lá).
ALTER TABLE users ADD COLUMN IF NOT EXISTS password_changed_at TIMESTAMPTZ;
-- `pt-AO`, `fr-FR`, `zh-CN` cabem nos 8 de `locale` (0023).

-- 2. Fotografia. Na base (como os PNG dos quadros): é pequena (≤ 1 MiB,
--    imposto no domínio e aqui) e segue as cópias de segurança da conta.
CREATE TABLE IF NOT EXISTS user_avatars (
    user_id     UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    content_type TEXT NOT NULL CHECK (content_type IN ('image/png', 'image/jpeg', 'image/webp')),
    bytes       BYTEA NOT NULL CHECK (octet_length(bytes) BETWEEN 1 AND 1048576),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 3. Telefone do MEMBRO — MESMAS colunas e regra da branch
--    `delonix-meet-backend/sms-contactos` (migração 0040 dela), com
--    `IF NOT EXISTS`: quando as duas se juntarem, a segunda a correr não faz
--    nada. `phone_source = 'manual'` quando é a própria pessoa a escrever.
ALTER TABLE org_members ADD COLUMN IF NOT EXISTS phone_e164 TEXT;
ALTER TABLE org_members ADD COLUMN IF NOT EXISTS phone_source TEXT
    CHECK (phone_source IN ('odoo', 'manual'));
ALTER TABLE org_members ADD COLUMN IF NOT EXISTS phone_updated_at TIMESTAMPTZ;

-- 4. A organização exige 2FA. Por agora só impede remover o último factor
--    (domínio `identity::factors`); não força a inscrição no login.
ALTER TABLE organizations ADD COLUMN IF NOT EXISTS require_mfa BOOLEAN NOT NULL DEFAULT FALSE;

-- 5. «Como entro nas sessões». Uma linha por pessoa, criada na primeira
--    escrita; sem linha valem as omissões do domínio (o comportamento de hoje).
CREATE TABLE IF NOT EXISTS user_join_preferences (
    user_id               UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    join_muted            BOOLEAN NOT NULL DEFAULT FALSE,
    join_camera_off       BOOLEAN NOT NULL DEFAULT FALSE,
    blur_background       BOOLEAN NOT NULL DEFAULT FALSE,
    noise_suppression     BOOLEAN NOT NULL DEFAULT FALSE,
    captions_always_on    BOOLEAN NOT NULL DEFAULT FALSE,
    captions_language     TEXT CHECK (captions_language IS NULL OR char_length(captions_language) <= 8),
    warn_before_recording BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 6. Preferências de notificação: só as que a pessoa mudou (sem linha = a
--    omissão do domínio `notification::preferences`). Os tipos seguem a CHECK
--    de `notifications.kind` (0044).
CREATE TABLE IF NOT EXISTS user_notification_preferences (
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN (
                   'meeting.invited', 'meeting.starting', 'meeting.cancelled',
                   'call.missed', 'recording.ready', 'transcription.ready')),
    channel    TEXT NOT NULL CHECK (channel IN ('email', 'in_app', 'sms')),
    enabled    BOOLEAN NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, kind, channel)
);

-- 7. Guia (Tour): progresso por pessoa. Só ids de passo, validados no domínio
--    contra a lista versionada; nenhum conteúdo.
CREATE TABLE IF NOT EXISTS user_tour_state (
    user_id         UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    version         TEXT NOT NULL,
    completed_steps TEXT[] NOT NULL DEFAULT '{}'
                    CHECK (cardinality(completed_steps) <= 64),
    skipped_at      TIMESTAMPTZ,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
