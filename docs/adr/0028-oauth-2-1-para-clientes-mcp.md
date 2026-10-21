# ADR-0028 — O Meet autoriza clientes MCP por OAuth 2.1, e o token do agente morre com a sessão da pessoa

**Estado:** Proposto · **Data:** 2026-10-13 ·
**Contexto:** um cliente MCP (Claude Desktop, Claude Code, outro agente) tem de
agir **por** uma pessoa desta instalação, sem que ninguém lhe passe uma
password, uma chave de admin ou um token que não expira ·
**Assenta em:** [ADR-0011](0011-chaves-de-acesso-e-sessoes.md) (sessões
revogáveis) e [ADR-0008](0008-papeis-capacidades-e-ambito.md) ·
**Serve:** [ADR-0027](0027-superficie-mcp-para-agentes.md).

## Contexto

Medido na `origin/develop` (`dec0c207`) a 2026-10-13:

1. **Não há servidor de autorização.** O Meet é **cliente** OIDC no SSO
   (`auth::sso_login`, `sso_callback`) e nunca o contrário. Não existe
   `authorize`, `token`, registo de cliente nem metadata de AS.

2. **Mas a metade difícil está feita, e é a revogação.** Desde a migração 0078
   há `user_sessions` com `revoked_at`; o access token leva `sid`; e o
   extractor consulta `sessions::ensure_active` **em cada pedido**
   (`auth.rs:190`). Terminar uma sessão corta a API no mesmo instante, fecha os
   WebSockets desse `sid` neste nó e publica no Redis para os outros
   (`KillRegistry`). Um servidor de autorização que ignorasse isto criaria uma
   segunda vida de credenciais, invisível no ecrã «Sessões activas».

3. **As credenciais que existem não servem** (ADR-0027 §Contexto 4): `dlx_` é
   de integração e emitida por admin; `dlxo_`, `dlxg_`, `dlxi_`, `dlxv_`,
   `dlxr_`, `dlxs_`, `dlxprov_` têm cada uma o seu portador e nenhuma é de uma
   pessoa a delegar a um agente.

4. **`auth_method` é um `CHECK` fechado** em `user_sessions`: `password`, `mfa`,
   `passkey`, `sso`, `odoo`, `legacy` (`0078_sessoes.sql`). Uma sessão de agente
   precisa de valor próprio — e isso é migração, não `INSERT`.

5. **Há rate-limit por público** (`auth_rate_limit`, `v1_rate_limit`,
   `ip_rate_limit`) e a frente de identidade está a mexer em SSO neste sprint.
   Este ADR não toca no login humano: acrescenta ao lado.

## Decisão

### 1. O Meet é servidor de autorização das suas próprias coisas — e só delas

Não passamos a IdP. Não emitimos identidade para outras aplicações, não há
`id_token`, não há perfil OIDC. É um **servidor de autorização OAuth 2.1 para o
recurso `/api/mcp`**, o que a especificação MCP exige de um servidor remoto.

Grants: **só `authorization_code` com PKCE `S256`** e `refresh_token`. Sem
implícito, sem `password`, sem `client_credentials` — um agente age sempre por
uma pessoa que consentiu, nunca por si.

### 2. Metadata e público do token

| Documento | Caminho | Para quê |
|---|---|---|
| Protected Resource Metadata (RFC 9728) | `/.well-known/oauth-protected-resource` | o cliente descobre QUEM autoriza este recurso |
| AS Metadata (RFC 8414) | `/.well-known/oauth-authorization-server` | endpoints, métodos, escopos |

`resource` (RFC 8707) é **obrigatório** no `authorize` e no `token`, e vai para
o `aud` do token. Consequência deliberada: **um token de MCP não abre `/api/*`**.
O extractor do BFF exige `typ: "access"`; o do MCP exige `typ: "mcp"` e o `aud`
certo. Um token roubado de um agente não é uma sessão web.

### 3. Registo dinâmico de cliente (RFC 7591), apertado

Os clientes MCP não se pré-registam à mão, por isso `POST /api/oauth/register`
existe. Com três limites:

- **clientes públicos apenas** — devolve `client_id`, nunca um segredo (PKCE é
  que prova);
- **`redirect_uri` só `https://` ou loopback** (`http://127.0.0.1:*`,
  `http://[::1]:*`); nada de `http://` noutro host, nada de wildcard;
- **rate-limit por IP** e um interruptor do operador
  (`OAUTH_DYNAMIC_REGISTRATION=off`) para quem quiser lista fechada.

### 4. O token do agente vive dentro da sessão da pessoa

É esta a decisão que o resto serve:

- **Access token** — JWT, `typ: "mcp"`, com `sub`, `sid`, `aud`, `scope`,
  `client_id`; validade igual à do access humano (`access_ttl_secs`, 15 min por
  omissão). O extractor `McpAuth` chama `sessions::ensure_active` como o
  `AuthUser` chama.
- **Refresh token** — opaco, guardado só em SHA-256 (como `refresh_tokens` já
  faz), **rotativo**: usar um refresh invalida-o e emite outro; reutilizar um já
  usado revoga a cadeia inteira.
- **Cada autorização abre uma linha em `user_sessions`** com
  `auth_method = 'oauth'` (migração que alarga o `CHECK`) e o nome do cliente no
  `user_agent`. Logo:
  - o agente **aparece** em «A minha conta → Sessões activas»;
  - «terminar» mata-o **já**, pela máquina que já existe;
  - `revoke-others` também o mata, e isso é a resposta certa a «perdi o
    portátil».

### 5. O ecrã de consentimento é da web, e diz a verdade

`GET /api/oauth/authorize` **não** desenha HTML: valida e redirecciona para
`/oauth/consent` na web, que exige sessão e mostra, em linguagem de pessoa:
o nome do cliente (e que ele se registou dinamicamente, quando for o caso), a
organização em que vai agir, **cada escopo pedido com o que abre**, e a frase
que importa — o agente age como a pessoa, e o que ele fizer fica na trilha de
auditoria com o nome dela.

Negar devolve `access_denied` ao cliente. Consentir grava a concessão (pessoa,
cliente, escopos) e é revogável no mesmo ecrã das sessões. Um pedido de escopos
**maiores** do que uma concessão anterior volta a pedir consentimento; iguais ou
menores, não.

### 6. O catálogo de escopos MCP é fechado, e é outro do das chaves `dlx_`

Misturar os dois era o caminho fácil e errado: os sete escopos `dlx_` descrevem
rotas da v1, e um deles (`bots:join`) põe bots em salas.

| Escopo | Abre |
|---|---|
| `meet.read` | reuniões, salas e participantes que a pessoa já vê |
| `minutes.read` | ler a acta e a transcrição |
| `minutes.write` | gerar e gravar a acta |
| `recordings.read` | biblioteca, detalhes, transcrição, legendas publicadas |
| `captions.write` | gerar e traduzir legendas, escrever VTT |
| `chapters.write` | gerar e editar capítulos |
| `clips.write` | criar e compor recortes |
| `shorts.publish` | publicar um *short* num destino da organização |
| `live.translate` | ligar e configurar a tradução ao vivo de uma sala |

Um escopo **não é** uma capacidade: dá ou tira acesso à tool, e a capacidade da
pessoa decide depois (ADR-0027 §4). Um escopo novo acrescentado amanhã **não
chega** às concessões que já existem — a mesma regra do `Scope::ALL`.

### 7. Revogação e auditoria

`POST /api/oauth/revoke` (RFC 7009) para o cliente devolver o que já não quer.
Do nosso lado, o que revoga é terminar a sessão. E entram na trilha: concessão
dada, concessão revogada, refresh reutilizado (que é sinal de roubo), registo de
cliente novo.

## Alternativas recusadas

| Alternativa | Porque não |
|---|---|
| Token pessoal colado à mão (`dlxm_`) | mais simples, e foi a primeira proposta; recusada pelo dono a 2026-10-13 a favor da especificação. Fica registada: é o caminho de recuo se o OAuth atrasar o MCP |
| Reutilizar o access token da sessão web | um token com `typ: "access"` abre as 302 rotas do BFF; dá-lo a um agente é dar-lhe a conta inteira |
| Alargar os escopos `dlx_` | a chave é de admin e de integração; e um escopo novo no catálogo chegaria a chaves criadas sem `scopes`, que recebem a lista completa |
| Sessão de agente fora de `user_sessions` | criava credenciais que não aparecem em «Sessões activas» e que «terminar sessão» não mata |

## Consequências

- **Boas:** o agente é uma sessão como as outras — visível, revogável,
  auditada; o público do token impede que sirva no BFF; e o consentimento é um
  ecrã que a pessoa leu.
- **Más:** é uma frente de identidade a mais num sprint que já mexe em SSO
  (plano do Sprint 3, frente B). O risco é de colisão de migrações e de
  revisão, não de desenho: as duas tocam em `auth.rs` e em `user_sessions`.
- **Por decidir:** se o mesmo servidor de autorização passa a servir outros
  clientes além do MCP (CLI, extensão de browser). Fica fora: o `resource` é
  obrigatório, e alargar é acrescentar um recurso, não reabrir este ADR.
