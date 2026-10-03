---
name: delonix-meet-backend
description: >-
  Organização do backend Rust do Delonix Meet (`server/`) — onde fica cada coisa, as
  camadas http→service→store, os helpers canónicos que NÃO se copiam (pertença à
  org, extractors de auth, cripto, pedidos de saída), a catraca da arquitectura, os
  crates `delonix-meet-*` e a ordem para lá chegar, e o estado da segurança (S1–S6
  fechadas, e o que continua aberto).
when_to_use: >-
  Quando fores escrever ou rever código em `server/src/` ou `server/crates/`, criar
  um módulo, mover código, propor crates, ou quando o pedido falar em «duplicado»,
  «refactor», «camada», «crate», «workspace», «nome do módulo». NÃO a uses para o
  desenho das rotas e do contrato (`delonix-meet-api`), nem para o que o telefone e
  o FreeSWITCH fazem (`delonix-meet-telefonia`), nem para o motor `delonix-runtime`.
---

# Backend do Delonix Meet — organização e regras

**Autoridade:** [ADR-0004](../../../docs/adr/0004-organizacao-alvo-do-backend.md)
(**Aceite** a 2026-09-16) e, para a lista de crates e a ordem de entrega, o
[ADR-0006](../../../docs/adr/0006-backend-enterprise-contextos-edicoes-e-entrega.md)
(Aceite — sucede o §3 do ADR-0004).
**Evidência:** [auditoria de 2026-09-16](../../../docs/auditoria-2026-09-16-backend.md).
**Precedência:** ADR aceite > esta skill > hábito do módulo que estás a editar.

## Fronteira

- **`delonix-meet-api`** — a forma da rota: superfície, verbos, estados, erro, paginação,
  OpenAPI, gRPC. Esta skill diz onde fica o handler e que helper ele chama.
- **`delonix-meet-telefonia`** — o domínio do telefone (`phone_bridge/`, `voice.rs`,
  `telephony_*.rs`, `crates/delonix-meet-domain/src/telephony/`). As regras de camada e a
  catraca daqui valem lá; o que o FreeSWITCH faz e as provas reais são de lá.
- **`delonix-meet`** — encaminhamento, portões por área e os números transversais.
- **Esta skill é o único sítio onde se escreve o estado da segurança** (§Segurança). O
  revisor `delonix-meet-security` e a skill de entrada apontam para aqui.
- Três revisores assentam nesta skill com perguntas diferentes: `delonix-meet-architecture`
  (onde vive o código), `delonix-meet-rust` (concorrência e hot path) e
  `delonix-meet-security` (§Segurança).

## Regra 0 — descobrir antes de mexer

Antes de mover, extrair ou renomear, segue quatro passos:

1. **Mapeia quem usa o que vais tocar:** `grep -n 'crate::<mod>' server/src/*.rs`.
2. **Classifica cada cópia:** é igual, ou já divergiu?
3. **Escolhe a versão certa.** Se as cópias divergiram, a certa é a mais restritiva.
   Numa regra de acesso, **nunca** se escolhe a mais permissiva por ser a mais usada.
4. **Planeia de modo a que cada commit deixe a árvore verde.**

Uma proposta que comece por apagar código que funciona é recusada na revisão.

## Onde fica código novo

O workspace (`server/Cargo.toml`) tem o monólito `delonix-server` na raiz e quatro
crates em `server/crates/`. **Nenhuma fatia nova volta a entrar no monólito** se já
houver crate para ela (`server/Cargo.toml:1-4`).

| Estás a escrever… | Vai para | Não vai para |
|---|---|---|
| Uma regra de domínio sem IO (custo, plano de marcação, validação) | `delonix-meet-domain`, no contexto certo (`conferencing`, `content`, `identity`, `integration`, `notification`, `operations`, `organization`, `telephony`) | um handler |
| Uma verificação «é membro/admin da org?» | chamar `org::role_in_org` / `org::require_member_pub` / `org::require_admin_pub` | um `SELECT … FROM org_members` no teu módulo |
| Uma verificação «este papel pode fazer isto?» | `org::require_capability` / `require_capability_for` (`org.rs:2149`, ADR-0008 §4) | comparar `role == "admin"` no teu módulo — a catraca conta-o |
| Autenticação de um pedido | um extractor existente (`auth::AuthUser`, `apikeys::ApiKeyAuth`, `odoo::OdooTokenAuth`), ou um novo em `auth.rs` | `headers.get("authorization")…strip_prefix("Bearer ")` |
| Um pedido HTTP de saída | `state.outbound` (`net_guard`): `tenant()` + `check_tenant_url` se o URL vem do cliente (`check_tenant_config_url` ao gravá-lo: `400` com razão), `operator()` + `check_operator_url` se vem do operador; OIDC via `outbound.oidc()` | `reqwest::Client::builder()` |
| sha256, token aleatório, comparação em tempo constante, argon2 | `delonix_meet_core::crypto` (`sha256_hex`, `ct_eq`, `hash_password`, `verify_password`); no monólito, `crate::crypto` e `auth::hash_password` delegam nele | uma cópia local |
| Um segredo guardado na base | `secrets_at_rest::{seal,open}` (aad `<tabela>.<coluna>:<id>`), sobre `delonix_meet_core::secret_box` | uma coluna em claro |
| Uma regra usada pela BFF E pela v1 | uma função partilhada no módulo do domínio, chamada pelas duas | dois handlers com a mesma validação |
| Um erro de unicidade | `ApiError::from_unique` (`error.rs:79`) | mais um `match db.is_unique_violation()` à mão |
| Uma listagem paginada | `delonix_meet_core::page` | `LIMIT` fixo |
| Tipos de mensagem WebSocket | junto dos outros `ClientMsg`/`ServerMsg` em `signaling.rs` (o crate `protocol` só tem, hoje, os tipos gRPC) | um módulo que importe `signaling` só pelos tipos |
| Uma tarefa de fundo | com `CancellationToken`/`JoinSet`, parada no shutdown (modelo: o `quarantine_sweeper`, `lib.rs:1307`) | mais um `tokio::spawn` solto em `run()` — já lá estão catorze |
| Uma função para outro módulo usar | `pub(crate) fn nome_real` | `pub fn nome_real_pub` |
| Identificadores novos | inglês | `inscrever`, `Emissao` (os existentes ficam até mudarem de crate) |

## A catraca da arquitectura

`scripts/check-arquitectura-catraca.sh` corre no `make fitness` e no CI. Conta oito
padrões. **A fasquia vive em `scripts/arquitectura-baseline.txt`** — lê-a lá; a coluna
abaixo é a leitura de 2026-10-03, para veres o caminho feito:

| Medida | 2026-09-16 | 2026-10-03 | O que conta |
|---|---|---|---|
| `pertenca_org_fora_de_org_rs` | 28 | 19 | `org_members` fora de `org.rs` |
| `authorization_lido_a_mao` | 3 | 1 | `strip_prefix("Bearer ` |
| `clientes_reqwest` | 4 | 0 | `reqwest::Client::builder()`/`new()` |
| `primitivas_cripto_espalhadas` | 17 | 0 | `Sha256::digest`, `Argon2::default()`, `fill_bytes`, `thread_rng().fill` fora do dono |
| `funcoes_sufixo_pub` | 7 | 4 | `fn …_pub` |
| `respostas_ok_true` | 28 | 0 | `"ok": true` |
| `verificacoes_papel_por_string_fora_de_org_rs` | — | 2 | papel comparado por string fora de `org.rs` (medida nova, ADR-0008) |
| `rotas_v1_com_sessao` | 4 | 0 | handlers em `/api/v1` que extraem `AuthUser` |

- **Subiu:** a cópia nova sai. Não se edita a referência para a deixar entrar.
- **Desceu:** óptimo. `BLESS=1 bash scripts/check-arquitectura-catraca.sh` grava a fasquia
  nova. O `BLESS` recusa gravar subidas.
- **O limite:** a catraca conta padrões, não semântica. Um helper com outro nome que
  faça a mesma coisa escapa-lhe. Não confies nela em vez de ler o diff.

## Segurança — o que foi fechado e o que continua aberto

**Fechadas, cada uma com a regressão que a guarda** (`docs/reference/regressions.md`).
Não se reabrem nem se revêem como se estivessem em aberto:

| # | Falha | Fechada com | Regressão |
|---|---|---|---|
| S1 | Qualquer registo era admin da plataforma | `config.platform_admin_user_ids` (`PLATFORM_ADMIN_USER_IDS`, UUIDs, fail-closed), verificado em `storage::require_platform_admin`. **Nunca** se deriva de `org_members`. Falta de papel é `403`, não `401` | R121 (#76) |
| S2 | `odoo::provision` capturava contas de outra org por email | passa sempre por `odoo_sso::upsert_member` (regra R25); as recusadas saem em `skipped` com a razão | R121 |
| S3 | Um membro arquivado mantinha acesso | «colega» e «quem pede» são membros ACTIVOS (`archived_at IS NULL`). O SUJEITO não se filtra quando o dado é da organização: a gravação de quem saiu continua descarregável pelo admin activo | R121 |
| — | `org::add_employee` capturava conta de outra org | recusa (`409`, `ForeignOrg`) uma conta já membro activo de outra org; portão `web/e2e/captura-empregado.mjs` | R122 (#78) |
| — | `odoo::list_users` devolvia arquivados | filtra `archived_at IS NULL` | R143 |
| — | `meetings_v1::resolve_org_user` juntava contas órfãs | só dentro do domínio da organização | R151 |
| S4 | SSRF para fora (`odoo_url`, WebDAV, descoberta OIDC) | tudo o que sai passa por `state.outbound` (`net_guard`) | R180 |
| S5 | Segredos de integração em claro (webhooks, SSO, WebDAV) | `secrets_at_rest::{seal,open}`; sem chaves a escrita é `422`, o herdado em claro lê-se, e `reseal_legacy` cifra-o no arranque e de hora a hora | R160 |
| S6 | Chaves `dlx_` sem escopos nem expiração | `key.require(Scope::…)?` na primeira linha de cada handler v1 | R170, R171 |
| — | Tronco SIP para endereço interno (SSRF por SIP); credenciais SIP legíveis sem reautenticação | ver `delonix-meet-telefonia` | R213, R214 |
| — | O segredo de voz ia no **URL do `mod_xml_curl`**, e o FreeSWITCH escrevia esse URL no log | o `mod_xml_curl` envia-o por HTTP Basic (`gateway-credentials`); `ivr_directory` e `ivr_dialplan_did` só chamam o `check_media_secret` — o `?secret=` deixou de autenticar. **Só este caminho**: ver «Continuam abertos» | R227 |

**Continuam abertos** — quem tocar nestes caminhos fecha-os ou nomeia-os no relatório:

- **O registo não verifica o email.** É decisão de produto, não defeito técnico.
- **A cópia única `users::provision_by_email`** (ADR-0004 §6 passo 4) continua por fazer
  — a função não existe (`grep`, 2026-10-03): o #76 fechou a cópia que estava errada, não
  juntou as seis.
- **O segredo de voz ainda chega ao disco do FreeSWITCH** (R227, «por corrigir»): os dois
  Lua passam-no nos argumentos de `session:execute("curl", …)`, que o FreeSWITCH escreve
  no log a cada chamada (e o `mod_curl` outra vez, a DEBUG), e o `freeswitch.xml.fsxml` do
  directório de logs traz a configuração expandida. Rodar o segredo não o tira de lá.
- **Superfície nova a vigiar:** o socket SIP da ponte telefone↔sala. As barras
  fail-closed estão em `delonix-meet-telefonia` §O que está ligado.

## A organização-alvo e a ordem

**A lista de crates e a regra da dependência vivem em `scripts/check-crate-deps.sh`**
(tabela `REGRAS`): um crate `delonix-meet-*` só depende de crates de camada menor, e
cada um tem dependências externas proibidas. Um crate novo sem linha na tabela falha.

```
0 core ◄ 1 protocol ◄ 2 domain ◄ 3 store · integrations · media (único com webrtc)
                               ◄ 4 realtime ◄ 5 api ◄ 6 server
```

Existem a 2026-10-03: `core` (`crypto`, `secret_box`, `page`, `error`, `egress`,
`edition`), `protocol` (tipos gRPC), `domain` (oito contextos) e `store` (só
`identity`). `integrations`, `media`, `realtime` e `api` **ainda não existem** — o
código deles está no monólito.

**Nunca saltes passos.** A ordem é a do ADR-0004 §6, estendida pelo ADR-0006 §Ordem
(entregas A–G; o estado que o ADR regista é de 2026-09-16). Medido a 2026-10-03:

| # | Passo (ADR-0004 §6) | Estado | Porquê não antes |
|---|---|---|---|
| 0 | Fechar S1–S3 | feito (#76, R121) | — |
| 1 | `src/lib.rs`, e `sfu_e2e` passa para `tests/` | **meio feito**: `lib.rs` existe; o `sfu_e2e` continua em `server/src/sfu_e2e.rs` (`lib.rs:54`) | sem lib não há testes de integração |
| 2 | `#[sqlx::test]` + job com Postgres | feito: 31 binários em `server/tests/`, CI com `DATABASE_URL` (`ci.yml:68-78`) | sem isto, mover SQL parte queries em silêncio |
| 3 | Extrair `crypto`, `auth::extract`, `org::membership`, `net_guard`, `protocol` | parcial: `crypto` no `core` e `net_guard` como módulo; `auth::extract`, `org::membership` e os tipos WS no `protocol` por fazer | parte o ciclo de módulos |
| 4 | Serviços partilhados BFF/v1 (`meetings::service`, `users::provision_by_email`) | por fazer | as cópias divergentes juntam-se sobre testes |
| 5 | Separar a v1 + OpenAPI | feito → `delonix-meet-api` | — |
| 6 | Workspace crate a crate | em curso: quatro de nove | com ciclo, não compila |
| 7 | gRPC voz/IVR e transcrição | feito → `delonix-meet-api` | — |

**Como medir o ciclo antes de declarar o passo 3 feito:** constrói o grafo a partir de
`grep -o 'crate::[a-z_]*' server/src/<mod>.rs` (sem os blocos `#[cfg(test)]`) e calcula
as componentes fortemente ligadas. O passo está feito quando nenhuma componente tem
mais de um módulo. **Não foi re-medido a 2026-10-03.**

## Armadilhas medidas

- **O SQL é de runtime.** Zero macros `query!`; mais de seiscentas chamadas
  `sqlx::query*` (624 por `grep` a 2026-10-03). Um nome de coluna errado compila e só
  falha em execução. Qualquer mudança de esquema ou de query exige o teste que percorre
  esse caminho.
- **`AppState` aparece em cerca de seiscentas linhas** de `server/src/` (601 a
  2026-10-03). Não o partas num PR com outras coisas.
- **`redis_state.rs` faz ler-alterar-gravar sem atomicidade** (nenhum `WATCH`, `MULTI`
  ou script Lua no ficheiro a 2026-10-03), e votos simultâneos em nós diferentes
  perdem-se. Um script Lua ou um `WATCH` resolvem; mais uma cópia do padrão não.
- **Há mais de um caminho para fechar uma sondagem:** `signaling.rs:4241`, que passa por
  `hub.close_poll` (`:2433`) e valida o anfitrião, e `room_tools.rs:123`, que mexe em
  `room.polls` directamente. Antes de mexer nas regras das sondagens, junta-os.
- **`mls.rs` está todo marcado `#![allow(dead_code)]`** (`mls.rs:17`) e não está montado
  (`lib.rs:344`). O `check-route-auth.sh` impede que volte a sê-lo por acaso. Não o
  documentes como activo.
- **A telefonia trouxe adaptadores sem consumidor**, marcados `#[allow(dead_code)]` com a
  nota «consumidor: frente D» (`telephony_service.rs:287,301,630,658,682`). Não os tomes
  por caminho vivo, e não acrescentes outros: uma porta sem quem a chame não se prova.
- **A catraca do clippy (13, `scripts/clippy-baseline.txt`) conta avisos de `sfu.rs` e
  `recorder.rs`.** Não os limpes em bloco à pressa: é o caminho do RTP e o da gravação.
