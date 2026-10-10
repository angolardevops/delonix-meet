-- A2 (revisão de segurança, 2026-10-09): o `state` anti-CSRF do fluxo OIDC
-- (entre /api/auth/sso/authorize e /api/auth/sso/callback) vivia num
-- DashMap em memória de UM processo. A instalação de produção corre várias
-- réplicas sem afinidade de sessão -- um login que caísse numa réplica
-- diferente da que emitiu o `state` respondia 401 sem motivo nenhum visível
-- para quem entra. Passa para Postgres, que já é o ponto de encontro de
-- todas as réplicas: o mesmo papel do `webauthn_ceremonies` (0079) para o
-- desafio WebAuthn.
CREATE TABLE sso_pending_states (
    state      TEXT PRIMARY KEY,
    org_id     UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    verifier   TEXT NOT NULL,
    nonce      TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- Varrido pelo `sessions::sweep` (a cada hora), junto com as outras
-- cerimónias de curta duração; o consumo em `sso_callback` já é atómico
-- (DELETE ... WHERE created_at > ... RETURNING) e não depende deste índice
-- para correcção, só para o varrimento não percorrer a tabela inteira.
CREATE INDEX sso_pending_states_created_idx ON sso_pending_states (created_at);
