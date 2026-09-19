---
name: delonix-meet-backend
description: Organização do backend Rust do Delonix Meet (`server/`) — onde fica cada coisa, as camadas http→service→store, os helpers canónicos que NÃO se copiam (pertença à org, extractors de auth, cripto, pedidos de saída), a catraca da arquitectura, a divisão-alvo em crates `delonix-meet-*` e a ordem para lá chegar, e as três falhas de segurança abertas. Usa-a quando fores escrever ou rever código em `server/src/`, criar um módulo, mover código, propor crates, ou quando o pedido falar em «duplicado», «refactor», «camada», «crate», «workspace», «nome do módulo». NÃO a uses para o desenho das rotas e do contrato (isso é `delonix-meet-api`), nem para o motor `delonix-runtime`.
---

# Backend do Delonix Meet — organização e regras

**Autoridade:** [ADR-0004](../../../docs/adr/0004-organizacao-alvo-do-backend.md) (Proposto).
**Evidência:** [auditoria de 2026-09-16](../../../docs/auditoria-2026-09-16-backend.md).
**Precedência:** ADR aceite > esta skill > hábito do módulo que estás a editar.

## Regra 0 — descobrir antes de mexer

Antes de mover, extrair ou renomear, segue quatro passos:

1. **Mapeia quem usa o que vais tocar:** `grep -n 'crate::<mod>' server/src/*.rs`.
2. **Classifica cada cópia:** é igual, ou já divergiu?
3. **Escolhe a versão certa.** Se as cópias divergiram, a certa é a mais restritiva.
   Numa regra de acesso, **nunca** se escolhe a mais permissiva por ser a mais usada.
4. **Planeia de modo a que cada commit deixe a árvore verde.**

Uma proposta que comece por apagar código que funciona é recusada na revisão.

## Onde fica código novo (enquanto o crate é um só)

| Estás a escrever… | Vai para | Não vai para |
|---|---|---|
| Uma verificação «é membro/admin da org?» | chamar `org::role_in_org` / `org::require_member_pub` / `org::require_admin_pub` | um `SELECT … FROM org_members` no teu módulo |
| Autenticação de um pedido | um extractor existente (`auth::AuthUser`, `apikeys::ApiKeyAuth`, `odoo::OdooTokenAuth`), ou um novo em `auth.rs` | `headers.get("authorization")…strip_prefix("Bearer ")` |
| Um pedido HTTP de saída | `state.outbound` (`net_guard`): `tenant()` + `check_tenant_url` se o URL vem do cliente, `operator()` + `check_operator_url` se vem do operador; OIDC via `outbound.oidc()` | `reqwest::Client::builder()` |
| sha256, token aleatório, comparação em tempo constante, argon2 | a função que já existe (`auth::hash_password`/`verify_password`, `auth::hash_refresh_token`, `apikeys::ct_eq`); **a próxima necessidade é a extracção para `crypto.rs`** | uma cópia local |
| Uma regra usada pela BFF E pela v1 | uma função partilhada no módulo do domínio, chamada pelas duas | dois handlers com a mesma validação |
| Um erro de unicidade | `ApiError::Conflict` a partir de um helper; se não existir, cria `ApiError::from_unique` | o 15.º `match db.is_unique_violation()` à mão |
| Tipos de mensagem WebSocket | junto dos outros `ClientMsg`/`ServerMsg` — destino: crate `protocol` | um módulo que importe `signaling` só pelos tipos |
| Uma tarefa de fundo | com `CancellationToken`/`JoinSet`, parada no shutdown | `tokio::spawn` solto em `main.rs` |
| Uma função para outro módulo usar | `pub(crate) fn nome_real` | `pub fn nome_real_pub` |
| Identificadores novos | inglês | `inscrever`, `Emissao` (os existentes ficam até mudarem de crate) |

## A catraca da arquitectura

`scripts/check-arquitectura-catraca.sh` corre no `make fitness` e no CI. Conta sete padrões:

| Medida | Referência 2026-09-16 | O que conta |
|---|---|---|
| `pertenca_org_fora_de_org_rs` | 28 | `org_members` fora de `org.rs` |
| `authorization_lido_a_mao` | 3 | `strip_prefix("Bearer ` |
| `clientes_reqwest` | 4 | `reqwest::Client::builder()`/`new()` |
| `primitivas_cripto_espalhadas` | 17 | `Sha256::digest`, `Argon2::default()`, `fill_bytes`, `thread_rng().fill` fora de `crypto.rs` |
| `funcoes_sufixo_pub` | 7 | `fn …_pub` |
| `respostas_ok_true` | 28 | `"ok": true` |
| `rotas_v1_com_sessao` | 4 | handlers em `/api/v1` que extraem `AuthUser` |

- **Subiu:** a cópia nova sai. Não se edita a referência para a deixar entrar.
- **Desceu:** óptimo. `BLESS=1 bash scripts/check-arquitectura-catraca.sh` grava a fasquia
  nova. O `BLESS` recusa gravar subidas.
- **O limite:** a catraca conta padrões, não semântica. Um helper com outro nome que
  faça a mesma coisa escapa-lhe. Não confies nela em vez de ler o diff.

## Segurança — o que foi fechado e o que continua aberto

**Fechadas no #76 (R121), provadas ao vivo antes e depois.** Não se reabrem:

- **S1 — administrador da plataforma** é `config.platform_admin_user_ids`
  (`PLATFORM_ADMIN_USER_IDS`, UUIDs, fail-closed), verificado em
  `storage::require_platform_admin`. **Nunca** se deriva de `org_members`: o registo
  cria sempre um admin. Falta de papel é `ApiError::Forbidden` (`403`), não `401`.
- **S2 — sincronização de directório** passa sempre por `odoo_sso::upsert_member`
  (regra R25). As contas recusadas saem em `skipped` com a razão, sem falhar o lote.
- **S3 — «colega» e «quem pede» são membros ACTIVOS** (`archived_at IS NULL`). O
  SUJEITO não se filtra quando o dado é da organização: a gravação de quem saiu continua
  descarregável pelo admin activo, e a auditoria e a retenção continuam a contá-lo.

**Continuam abertos** — quem tocar nestes caminhos fecha-os ou nomeia-os no relatório:

- ~~`org::add_employee`~~ **fechado no #78 (R122)**: recusa (`409`) uma conta que já seja
  membro activo de outra org, a mesma regra `ForeignOrg`. A guarda só era alcançável numa
  org legada (`email_domain` vazio); o portão `web/e2e/captura-empregado.mjs` ataca a base
  directamente para lá chegar.
- ~~`odoo::list_users` devolve arquivados~~ **fechado (R143)**.
- ~~`meetings_v1::resolve_org_user` junta contas órfãs~~ **fechado (R151)**: só dentro do
  domínio da organização.
- **S4:** SSRF no `odoo_url`, no WebDAV e na descoberta OIDC — **continua aberta**.
- ~~**S5:** segredos de integração em claro~~ **fechado para webhooks, SSO e WebDAV (R160)**:
  quem escreve ou lê `org_webhooks.secret`, `org_sso_configs.client_secret` ou
  `platform_storage.webdav_password` passa por `secrets_at_rest::{seal,open}` (aad
  `<tabela>.<coluna>:<id>`); sem chaves a escrita é `422`, o herdado em claro lê-se, e
  `reseal_legacy` cifra-o no arranque e de hora a hora.
- ~~**S6:** chaves de API sem escopos~~ **fechado (R170)**: `key.require(Scope::…)?` na
  primeira linha de cada handler v1.
- A cópia única `users::provision_by_email` (ADR-0004 §6 passo 4) continua por fazer:
  o #76 fechou a cópia que estava errada, não juntou as seis.

## A organização-alvo (ADR-0004 §3) e a ordem

```
core ◄─ protocol        core ◄─ store        core,store ◄─ identity
core,store,identity ◄─ integrations          core,protocol ◄─ media (único com webrtc)
core,protocol,store,media ◄─ realtime        todos ◄─ api ◄─ server
```

**Nunca saltes passos.** Cada um depende do anterior (ADR-0004 §6):

| # | Passo | Porquê não antes |
|---|---|---|
| 0 | ~~Fechar S1–S3~~ — feito no #76 (R121) | — |
| 1 | `src/lib.rs`, e `sfu_e2e` passa para `tests/` | sem lib não há testes de integração |
| 2 | `#[sqlx::test]` em org/meetings/recordings + job com Postgres | sem isto, mover SQL parte queries em silêncio (302 queries de runtime, sem `query!`) |
| 3 | Extrair `crypto`, `auth::extract`, `org::membership`, `net_guard`, `protocol` | parte o ciclo de 18 módulos |
| 4 | Serviços partilhados BFF/v1 (`meetings::service`, `users::provision_by_email`) | as cópias divergentes juntam-se sobre testes |
| 5 | Separar a v1 + OpenAPI | → `delonix-meet-api` |
| 6 | Workspace crate a crate: `core`, `protocol`, … | com ciclo, não compila |
| 7 | gRPC voz/IVR e transcrição | → `delonix-meet-api` |

**Como medir o ciclo antes de declarar o passo 3 feito:** constrói o grafo a partir de
`grep -o 'crate::[a-z_]*' server/src/<mod>.rs` (sem os blocos `#[cfg(test)]`) e calcula
as componentes fortemente ligadas. O passo está feito quando nenhuma componente tem
mais de um módulo.

## Armadilhas medidas

- **O SQL é de runtime.** Um nome de coluna errado compila e só falha em produção.
  Qualquer mudança de esquema ou de query exige o teste que percorre esse caminho.
- **`AppState` é usado em 231 sítios.** Não o partas num PR com outras coisas.
- **`redis_state.rs` faz ler-alterar-gravar sem atomicidade**, e votos simultâneos em
  nós diferentes perdem-se. Um script Lua ou um `WATCH` resolvem; mais uma cópia do
  padrão não.
- **Há três caminhos para fechar uma sondagem** (`signaling.rs:1468`, `:2625`,
  `room_tools.rs:105`), e o último não é alcançável pelo WebSocket. Antes de mexer nas
  regras das sondagens, junta os três.
- **`mls.rs` está todo marcado `#![allow(dead_code)]`** e não está montado. O
  `check-route-auth.sh` impede que volte a sê-lo por acaso. Não o documentes como activo.
- **A catraca do clippy (31) conta avisos de `sfu.rs` e `recorder.rs`.** Não os limpes
  em bloco à pressa: é o caminho do RTP e o da gravação.
