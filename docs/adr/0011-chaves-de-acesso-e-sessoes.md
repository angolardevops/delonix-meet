# ADR-0011 — Chaves de acesso (WebAuthn) como segundo factor, e sessões revogáveis

**Estado:** Proposto · **Data:** 2026-09-17 · **Contexto:** template Navegavel3,
ecrã `DelonixProfile` («Segurança: chave de acesso — Adicionar», «Sessões activas —
Terminar»), frente B do backend v3.

## Contexto

Medido na base `d3ffd8f` antes de escrever código:

1. **O segundo factor era só TOTP** (`mfa.rs`, migração `mfa_totp`), com códigos de
   recuperação. Nenhuma chave de acesso, nenhum WebAuthn.
2. **Não havia sessão.** O login emitia um JWT de acesso de 15 min e um refresh
   token solto em `refresh_tokens`. Nenhum dos dois dizia de que login vinha, por
   isso «terminar o iPhone» era impossível sem revogar tudo — e mesmo revogando, o
   access token continuava a abrir a API até expirar, e o `/rtc` e o `/ws` abertos
   continuavam ligados.
3. **Contas geridas pelo Odoo** validam a password contra o Odoo em cada login
   (`auth::password_matches`). É esse pedido que diz que a pessoa continua activa no
   ERP; o hash local é só a cache para o Odoo em baixo.
4. **O servidor corre com várias réplicas** (ADR-0001): estado entre dois pedidos
   HTTP não pode viver na memória de um nó.

## Decisão

### 1. Chave de acesso = segundo factor. Sem palavra-passe: NÃO, por agora

- A chave de acesso **substitui o código TOTP**, não a password. O login continua
  `password → desafio (mfa_token) → segundo factor`. O desafio anuncia os factores
  da conta em `methods` (`totp`, `passkey`); a chave responde em
  `POST /api/auth/login/mfa/passkey-options` + `POST /api/auth/login/mfa/passkey`.
- **Porque não sem password:**
  - numa conta gerida pelo Odoo, entrar só com a chave **contornava a autoridade do
    ERP** — alguém desactivado no Odoo continuava a entrar enquanto tivesse o
    telemóvel. Resolver isso exige perguntar ao Odoo sem password (uma API de
    estado da conta), que não existe no `nk_delonix_meet`;
  - numa org com `enforce_sso`, a chave tinha de passar pelo IdP da org, outra
    decisão;
  - o fluxo descoberto (`start_discoverable_authentication`) não é provável com o
    autenticador de software da crate na versão fixada: o `SoftPasskey` 0.5.5 não
    devolve `user_handle`. Um fluxo sem teste não entra (regra da casa).
- Reabre-se com um ADR sucessor que resolva a autoridade Odoo e traga o teste.

### 2. Crate: `webauthn-rs` (Kanidm), versão fixada

- `webauthn-rs =0.5.5`: mantida, com auditoria pública (SUSE, 2022) e usada em
  produção pelo Kanidm. Nada de verificação WebAuthn escrita à mão.
- A política é a da crate para passkeys: **verificação do utilizador obrigatória**
  (PIN/biometria), contador actualizado a cada uso.
- Feature `danger-allow-state-serialisation`: o estado da cerimónia vai para a base
  (`webauthn_ceremonies`), porque o «finish» pode cair noutra réplica. O nome da
  feature avisa para o risco que ela abre — reutilizar um desafio. Mitigado:
  consumido com `DELETE … RETURNING` (uma vez), preso à pessoa e ao tipo, e expira
  em 5 minutos.
- **RP ID e origem por configuração** (`WEBAUTHN_RP_ID`, `WEBAUTHN_RP_ORIGIN`). Sem
  os dois, as rotas respondem `503 passkeys.not_configured` e o resumo de
  segurança diz `availability: not_configured`. Nunca se deriva a origem do
  cabeçalho `Host`/`Origin` do pedido (seria o atacante a escolhê-la).

### 3. Alterar factores exige reautenticação recente

- `POST /api/users/me/reauthentication` (password — no Odoo numa conta gerida — ou
  código TOTP/recuperação) marca `user_sessions.reauthenticated_at`. Vale 5 minutos
  e só para ESSA sessão. O login conta como reautenticação.
- Exigem-na: começar e concluir o registo de uma chave, remover uma chave,
  regenerar códigos de recuperação. O `mfa/disable` já exigia um código válido e
  mantém-se assim. Sem ela: `403 auth.reauthentication_required`.
- Limite conhecido: uma conta só com SSO e sem TOTP não tem como se reautenticar
  aqui (`reauthentication.no_method`). A reautenticação com a própria chave de
  acesso fica para o sucessor.

### 4. O último factor não sai quando a organização exige 2FA

- `organizations.require_mfa` (definida no `PATCH /api/orgs/{org_id}`).
- Regra pura em `domain::identity::factors::check_removal`, a mais restritiva:
  basta UMA organização activa da pessoa exigir. Remover o TOTP ou a última chave
  que deixaria a conta sem factor é `409 security.last_factor_required`, verificado
  antes de consumir o código.
- Limite: `require_mfa` ainda **não obriga** a inscrever um factor no login.

### 5. Sessões revogáveis de imediato

- Cada login cria uma `user_sessions`; o refresh token roda dentro dela; o access
  token e o room token levam `sid`.
- `AuthUser`, `/rtc` e `/ws` recusam um `sid` terminado (`401 auth.session_revoked`)
  — uma leitura por chave primária por pedido, e o «visto por último» escrito no
  máximo uma vez por minuto.
- Terminar revoga os refresh tokens da sessão e acorda o `shutdown` das ligações
  dela neste nó (`sessions::KillRegistry`); os outros nós recebem o id pelo canal
  Redis `dlx:session-revoked`.
- Tokens anteriores (sem `sid`) continuam válidos até expirarem (15 min); o refresh
  de um token sem sessão cria-lhe uma (`auth_method = legacy`). A migração cria uma
  sessão `legacy` para cada refresh token vivo, para a lista não esconder logins
  antigos.
- Só a própria pessoa lista e termina as suas sessões. Um administrador da org não:
  isso é suspender a conta, outra superfície.

## Consequências

- **+** «Terminar» corta a API, o refresh e o tempo real no mesmo instante.
- **+** A chave de acesso resiste a phishing onde o TOTP não resiste.
- **−** Uma consulta extra à base por pedido autenticado.
- **−** Nova dependência nativa: o `webauthn-rs` usa OpenSSL (a imagem `cc-debian12`
  já traz a `libssl`; o build `rust:1-bookworm` já traz os cabeçalhos).
- **−** Sem login sem password: quem tem chave continua a escrever a password.
