# Auditoria do backend — organização, duplicação e contrato de API

> **Data:** 2026-09-16 · **Árvore medida:** `origin/main` em `f2c4628`
> **Método:** leitura do código com `grep`/`rg`, grafo de módulos construído a partir de
> `crate::`, `cargo clippy --all-targets -- -D warnings` e `cargo fmt --check` corridos
> num target à parte. **Nada foi corrido contra um servidor ou uma base de dados.**
> **Pedido:** revisão completa — crates e nomes, boas práticas, código duplicado, REST e gRPC.

É a fonte de evidência do [ADR-0004](adr/0004-organizacao-alvo-do-backend.md) e das skills
`delonix-meet-*` em `.claude/skills/`. Quando um número daqui mudar, a catraca
(`scripts/check-arquitectura-catraca.sh`) é o sítio onde isso se prova.

---

## 1. Linha de base

| Medida | Valor | Como se obteve |
|---|---|---|
| Crates | **1** (`delonix-server`, sem `[workspace]`) | `server/Cargo.toml` |
| Módulos | 34 ficheiros lado a lado em `server/src/`, 23 087 linhas | `wc -l server/src/*.rs` |
| Maiores | `signaling.rs` 3 994 · `sfu.rs` 2 181 · `meetings.rs` 1 230 · `org.rs` 1 216 | idem |
| `src/lib.rs` / `server/tests/` | **não existem** | `ls` |
| Ciclo de módulos | **1 componente com 18 módulos** | grafo de `crate::x`, Tarjan |
| Módulos que importam `crate::AppState` | 23 (231 referências) | `grep` |
| Chamadas `sqlx::query*` | **302**, zero macros `query!` | `grep` |
| `tenant_tx` vs `state.db` | 4 vs 362 | `grep` |
| Tabelas com `FORCE ROW LEVEL SECURITY` | 1 (`employee_groups`) | `migrations/0024` |
| Testes | 95 `#[test]` + 55 `#[tokio::test]`, nenhum toca na base | `grep` |
| `cargo fmt --check` | passa | medido |
| `cargo clippy -D warnings` | 31 avisos, igual a `scripts/clippy-baseline.txt` | medido (toolchain 1.98) |
| Dependências duplicadas | 74 crates em várias versões | `cargo tree -d -e normal` |
| Peso do `webrtc` | 287 dos 424 crates do lock | `cargo tree` |
| Rotas HTTP/WS | 105 `.route(` | `grep` |
| gRPC | **nenhum** (sem `tonic`, `prost`, `*.proto`, `build.rs`) | `find`, `git grep` |
| OpenAPI / testes de contrato v1 | **nenhum** | `git grep utoipa\|openapi` |

---

## 2. O que ficou provado

### 2.1 Segurança — resolver primeiro

Os três primeiros foram confirmados lendo o código, mas **nenhum foi explorado**.

| # | Achado | Prova |
|---|---|---|
| S1 | **Qualquer registo passa a «admin da plataforma».** O `register` é público e insere o autor como `admin` da org nova; o `require_platform_admin` só exige ser admin de UMA org qualquer. Protege a configuração global de armazenamento e um `PROPFIND` a URL à escolha (SSRF). | `auth.rs:322`, `storage.rs:277-292`, `storage.rs:187` |
| S2 | **Captura de conta de outra org por email.** O `odoo::provision` aceita chave `dlx_`, procura o utilizador só por email, reescreve-o e junta-o à org de quem chama (até como `admin`). A guarda `ForeignOrg` existe em 2 das 6 cópias deste fluxo e falta nesta. Contraria o invariante 10 do `HARNESS.md` (R25). | `odoo.rs:92`, `odoo.rs:284-340`; guarda em `odoo_sso.rs:313`, `meetings_v1.rs:236` |
| S3 | **Funcionário arquivado mantém acesso.** 17 verificações de pertença escritas à mão não filtram `archived_at`, incluindo o `org_mate` dentro de `room_access` e o download de gravações. O helper canónico `org::role_in_org` filtra. | `rooms.rs:184`, `recordings.rs:199,237`, `users.rs:113`, e mais 13 |
| S4 | SSRF: `validate_public_url` só é usado pelos webhooks. Falta no `odoo_url` (recebe passwords), no WebDAV e na descoberta OIDC (redirects até 5, sem timeout). | `webhooks.rs:76`; `odoo.rs:161`, `storage.rs:187`, `auth.rs:666,771` |
| S5 | Segredos de integrações em claro: `webdav_password`, `client_secret` do SSO, segredo dos webhooks. | `storage.rs:106`, `org.rs:1144`, `webhooks.rs:268` |
| S6 | Chaves de API sem escopos nem expiração — o `HARNESS.md` dizia «hash + scopes». | `apikeys.rs:29-69` |

### 2.2 Duplicação

| O que se repete | Cópias | Divergência já existente |
|---|---|---|
| Criar/ligar utilizador por email | 6 | só 2 recusam conta de outra org (S2) |
| Pertença à org à mão (`FROM org_members` fora de `org.rs`) | 29 linhas em 12 módulos | 17 esquecem `archived_at` (S3) |
| Criação de reunião (BFF vs v1) | 2 | título 140/rejeita vs 120/corta; duração mín. 5 vs 1; a v1 salta `max_meetings` e conflitos |
| Emissão do token de sala | 2 | — |
| `SELECT … FROM rooms WHERE code=$1` | 4 + 5 parciais | a v1 não passa o código a minúsculas |
| Leitura do `Bearer` | 3 | — |
| Consulta da chave `dlx_` | 2 | a cópia do Odoo não actualiza `last_used_at` |
| Token de WebSocket verificado à mão | 3 | — |
| `sha256_hex` / tokens aleatórios / tempo constante | 4 / 7 / 3 | — |
| Argon2 reimplementado | 3 | — |
| Credencial TURN HMAC-SHA1 | 2 | uma devolve erro, a outra omite o TURN |
| `is_unique_violation` mapeado à mão | 14 | — |
| Publicar no Redis + entregar local | 4 | — |
| Ler-alterar-gravar estado no Redis | 4 | sem atomicidade: votos simultâneos perdem-se |
| Regras das sondagens | 3 caminhos de fecho | o voto recuperado do Redis ignora `ends_at` |

### 2.3 Organização e boas práticas

- **Ciclo de 18 módulos**, fechado sobretudo por utilitários e tipos no sítio errado:
  `signaling → apikeys` (`ct_eq`), `apikeys → odoo` (`gen_token_pub`),
  `odoo_sso → meetings_v1` (`is_usable_email`), `pubsub/redis_state → signaling` (só tipos),
  `presence ↔ rooms`, `recorder → webhooks/org`, `config → sfu` (constantes).
  **Enquanto existir, não é possível dividir em crates.**
- **Sem camadas:** os handlers fazem o SQL; não há repositório nem serviço; `AppState` tem 17 campos.
- **Nomes que enganam:** `apikeys.rs` contém a API pública v1 inteira; `odoo.rs` serve
  `/api/public/settings`; `meetings_v1.rs` tem a versão num módulo de domínio;
  `broadcast.rs` colide com `tokio::sync::broadcast`; `recorder.rs` vs `recordings.rs`;
  `room_tools.rs`/`actions.rs` vagos; `voice.rs` é telefonia PSTN; 7 funções com sufixo
  `_pub` em vez de `pub(crate)`; `mls.rs` inteiro com `#![allow(dead_code)]`.
- **Língua dos identificadores:** `mfa::{inscrever,activar,desactivar}`,
  `broadcast::{Destino,Emissao,ws_directo}`, `AppState.directos`. A regra de fronteira de
  2026-09-03 (código NOVO em inglês; o existente fica) não estava escrita no harness —
  só num comentário de `scripts/check-capability-claims.sh`.
- **Tarefas de fundo:** 5 ciclos em `main.rs:569-634` sem `JoinHandle` nem cancelamento.
- **`unwrap` em caminho quente:** `redis_state.rs` (7× `to_string(..).unwrap()`).

### 2.4 REST e gRPC

- A fronteira BFF (`/api/…`) vs pública (`/api/v1`) está escrita e há portão de
  autenticação por rota (`check-route-auth.sh`). **Isto está bem.**
- A v1 **mistura três públicos e quatro autenticações**: chave `dlx_` (inquilino), token
  `dlxo_` (Odoo), segredo de plataforma (`/admin/orgs`) e JWT de sessão
  (`/platform/storage*`, que devolve YAML de Kubernetes).
- **Códigos de estado planos:** nenhuma rota montada devolve 201/204/202; 28
  `{"ok": true}`; erro `{"error": "<texto PT>"}` sem código estável.
- **Paginação:** nenhuma. Limites fixos silenciosos (100/200/500); `meetings::list` e
  `recordings::library` sem limite; `v1/meetings?since=` corta aos 500.
- **Idempotência:** sem `Idempotency-Key`, `ETag`, `If-Match`.
- **Recursos incompletos:** `/meetings/{id}` não tem GET nem na BFF nem na v1.
- **Semântica:** `POST /orgs/{id}/settings` para actualizar; `/share` singular para
  colecção; `/api/action-items/{id}` fora da reunião; `/api/voice/ivr/*`
  (máquina-a-máquina) na árvore pública.
- **gRPC não existe.** Onde faz sentido e onde não faz está decidido no ADR-0004 §4.

### 2.5 O próprio harness

- Os seis revisores da pasta `agents` na raiz que `HARNESS.md` e `AGENTS.md` mandavam invocar
  **nunca existiram no git** (`git log --all -- agents` vazio).
- `.cursorrules` e `.github/copilot-instructions.md` apontam para `docs/architecture/`,
  que não existe neste repo.
- `HARNESS.md` afirmava: chaves com «scopes» (falso), `mls.rs` activo (morto),
  isolamento «em todos os endpoints» (S3), «118 chamadas» sqlx (302),
  «Pool via `Extension<PgPool>`» (é `State<Arc<AppState>>`).

---

## 3. O que NÃO foi validado

- **Nada correu contra servidor nem base.** S1–S3 são leitura confirmada, não exploração.
  Não se verificou se algum proxy fecha `/api/auth/register` antes do handler.
- Não se confirmou, query a query, que as 302 filtram por organização.
- Não se verificou se o ingress expõe `/api/voice/ivr/*` para fora.
- Não se mediu o ganho de compilação da divisão em crates (287 crates é contagem, não tempo),
  nem o custo dos 217 `.clone()` em `sfu.rs`/`signaling.rs`.
- O grafo de módulos vem de regex, não do compilador.
- O clippy correu com a toolchain local 1.98; o `stable` do CI pode contar outro número.
- A rejeição do extractor `Json` do axum 0.8 (400 vs 422, texto vs JSON) não foi testada ao vivo.
