-- ════════════════════════════════════════════════════════════════════════
--  SMTP por organização: o relay do próprio cliente.
--
--  PORQUE EXISTE. O ADR-0025 decidiu o correio em dois tempos: **relay do
--  operador primeiro**, SMTP por organização depois — e nomeou a condição para
--  o segundo, que não era o SMTP: era **um guarda de saída que valide
--  `host:porta` antes de abrir a ligação**. O `net_guard` só guardava URLs
--  HTTP, e um host de SMTP escolhido pelo inquilino é uma ligação de saída
--  arbitrária a partir dos nossos servidores (varrer a rede interna, chegar ao
--  169.254.169.254, servir de sonda).
--
--  Esse guarda passa a existir (`Outbound::resolve_tenant_target`): resolve o
--  nome, descarta os endereços que a política de inquilino recusa, e quem liga
--  liga-se ao ENDEREÇO APROVADO, nunca ao nome — é o equivalente TCP do
--  `GuardedResolver`, e fecha a janela do DNS rebinding. Com ele, esta tabela
--  deixa de ser perigosa.
--
--  UMA LINHA POR ORGANIZAÇÃO. Sem linha, ou com `enabled = FALSE`, o correio
--  dessa organização sai pelo relay do operador, como antes: isto ACRESCENTA
--  um caminho, não substitui nenhum.
--
--  A password é um segredo em repouso, pelo MESMO molde dos troncos SIP
--  (`telephony_trunks.password_sealed` + `secrets_at_rest::seal` com o AAD
--  preso à linha) — não se inventa um segundo. Nunca sai numa resposta de API:
--  a leitura devolve só `password_configured`.
-- ════════════════════════════════════════════════════════════════════════

CREATE TABLE org_mail_settings (
    org_id          UUID PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,

    host            TEXT NOT NULL,
    port            INT  NOT NULL CHECK (port BETWEEN 1 AND 65535),
    username        TEXT NOT NULL DEFAULT '',
    -- Selada com o AAD preso ao `org_id`: uma linha copiada para outra
    -- organização não se abre.
    password_sealed TEXT NOT NULL DEFAULT '',
    -- STARTTLS obrigatório por omissão. `FALSE` só serve um relay na própria
    -- rede do cliente, e a API exige que quem o desliga o diga por escrito.
    starttls        BOOLEAN NOT NULL DEFAULT TRUE,
    -- O remetente desta organização. É ele que leva o SPF/DKIM do domínio do
    -- cliente — a razão pela qual alguém quer o seu próprio SMTP.
    from_address    TEXT NOT NULL,

    -- Desligar sem apagar: o correio volta ao relay do operador e a
    -- configuração fica guardada para se religar.
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,

    -- Quem mexeu e quando, para a auditoria da organização.
    updated_by      UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
