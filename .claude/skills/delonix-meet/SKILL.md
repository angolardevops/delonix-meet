---
name: delonix-meet
description: Ponto de entrada do Delonix Meet (videoconferência self-hosted — backend Rust em `server/`, web React em `web/`). Usa-a SEMPRE que o trabalho for neste repo e ainda não souberes que skill ou revisor chamar, quando o pedido for «revê», «audita», «onde é que isto se põe», «que agente uso», ou quando fores tocar em mais de uma camada. Encaminha para `delonix-meet-backend` (organização do código Rust, camadas, crates, duplicação) e `delonix-meet-api` (REST, v1, OpenAPI, gRPC), e diz que agente `delonix-meet-*` revê cada área. NÃO a uses para o motor `delonix-runtime` nem para o PaaS — as `delonix-*` sem `-meet` são do MOTOR.
---

# Delonix Meet — ponto de entrada

**Repo:** `delonix-meet/` (remote `angolardevops/delonix-meet`).
**Contexto completo:** [`HARNESS.md`](../../../HARNESS.md). **Resumo partilhado:** [`AGENTS.md`](../../../AGENTS.md).
**Destino da arquitectura:** [ADR-0004](../../../docs/adr/0004-organizacao-alvo-do-backend.md) (estado: **Proposto**).
**Evidência de onde tudo isto sai:** [auditoria de 2026-09-16](../../../docs/auditoria-2026-09-16-backend.md).

## Playbook — sempre que fores chamado

1. **Mede contra a `origin/main`, nunca contra a árvore local.** A árvore partilhada já
   esteve 133 commits atrás. Mede com `git show origin/main:<path>` ou abre um worktree.
2. **Um worktree por tarefa**, a partir de `origin/main`, em
   `<workspace>/.worktrees/delonix-meet/<tarefa>` — **nunca em `/tmp`**, que esta máquina
   esvazia a cada arranque (a 2026-09-16 levou um commit a meio e um build de cinco
   minutos). Da raiz do workspace:
   `git -C delonix-meet worktree add -b <skill>/<tarefa> "$PWD/.worktrees/delonix-meet/<tarefa>" origin/main`.
   `git add <ficheiro>`, nunca `-A`. Confirma `git branch --show-current` antes de cada commit.
   Faz commit assim que um lote passar, e o `cargo target` fica dentro do worktree ou em
   `~/.cache` — o que não estiver em commit, um reinício leva.
3. **Encaminha** pela tabela abaixo. Um pedido que toque em duas áreas começa pela de cima.
4. **Antes de dizer «feito»**, corre o portão da área (tabela abaixo) na árvore de
   integração limpa. «Compila» não fecha nada.
5. **Reporta as duas metades:** o que ficou provado e o que não foi validado. Os casos
   típicos são «não corri contra Postgres» e «não testei no browser».
6. **Destrói o worktree** no fim. Uma tarefa com worktree vivo não está terminada.

## Encaminhamento

| Se o pedido for sobre… | Skill | Revisor (`.claude/agents/`) |
|---|---|---|
| Onde fica código novo; camadas; crates; nomes; duplicação; ciclo de módulos | `delonix-meet-backend` | `delonix-meet-architecture` |
| Rota nova; `/api/v1`; códigos de estado; erro; paginação; OpenAPI; gRPC | `delonix-meet-api` | `delonix-meet-api` |
| Autenticação, isolamento entre orgs, SSRF, segredos, E2EE, DLP, auditoria | `delonix-meet-backend` §Segurança | `delonix-meet-security` |
| Rust em profundidade: async, locks, hot path, `unwrap`, tarefas de fundo | `delonix-meet-backend` | `delonix-meet-rust` |
| SFU, ICE, simulcast, gravação, media num só sentido | `docs/reference/regressions.md` | `delonix-meet-webrtc` |
| Telefone, PSTN, dial-in, SIP, FreeSWITCH, SRTP, troncos, CDR | `delonix-meet-telefonia` | `delonix-meet-webrtc` (media) + `delonix-meet-security` (socket e chaves) |
| `web/src/**`, sala, design system, i18n | `HARNESS.md` §5 | `delonix-meet-frontend` |
| `deploy/`, K8s, afinidade por sala, coturn, imagens | `HARNESS.md` §7 e §11 | `delonix-meet-devops` |
| Prioridade de roadmap, paridade com Zoom/Teams/Meet | `docs/competitive-positioning.md` | `delonix-meet-product` |

## Portões por área

| Área | Portão |
|---|---|
| Qualquer mudança | `make fitness` — corre onze dos quinze `scripts/check-*.sh` |
| Backend | **o que o CI corre, por esta ordem:** `cargo fmt --manifest-path server/Cargo.toml --check` · `bash scripts/check-clippy-ratchet.sh` · `cargo test --release --workspace -- --test-threads=4` · `bash scripts/check-openapi.sh`. O `fmt` é o primeiro e é o que mais vezes trava um push apressado |
| Rota nova ou alterada | `check-route-auth.sh` + `check-isolamento-cobertura.sh` + `node web/e2e/isolamento.mjs` contra servidor real |
| Frontend | `cd web && npx tsc --noEmit && npx vitest run` + o e2e do ecrã |
| Media | `cargo test --release sfu_e2e` + `node web/e2e/reuniao.mjs` |
| Telefone e PSTN | `delonix-meet-telefonia` §Portões — inclui a prova contra um FreeSWITCH real, que **não corre no CI** |

Os quinze portões: `arquitectura-catraca`, `browser-antes-do-e2e`, `capability-claims`,
`clippy-ratchet`, `crate-deps`, `dep-audit`, `docs-drift`, `isolamento-cobertura`,
`k8s-render`, `openapi`, `proto`, `repo-hygiene`, `room-affinity`, `route-auth`,
`tenant-rls`.

## O estado real (2026-09-30, `main` `1fd4750`) — não o redescubras

Números medidos, não lembrados: **165 regressões** no catálogo, **30 binários** em
`server/tests/`, **15 portões** `scripts/check-*.sh`, catraca do clippy em **13**,
`rotas_sem_openapi=0`. Zero PRs abertas.

- **Workspace em transição** (ADR-0006 §1). O monólito `delonix-server` continua a ser a
  raiz, com `src/lib.rs` (o `main.rs` só chama `run()`), mais os crates sem IO
  `delonix-meet-core`, `delonix-meet-domain` e `delonix-meet-protocol`;
  `scripts/check-crate-deps.sh` impõe a regra da dependência. Os módulos do monólito
  **ainda não** estão em `store`/`api`/`media`/`realtime` — ADR-0006 §«Ordem» D e G.
- **OpenAPI 3.1 gerado** e commitado (`docs/reference/openapi/{bff,v1}.json`), com catraca
  a zero. **gRPC interno** com mTLS; **nunca** gRPC para o browser.
- **Testes de integração contra Postgres real** — `cargo test --release --workspace --
  --test-threads=4` precisa de `DATABASE_URL`. Sem ela, os testes que usam `#[sqlx::test]`
  falham com «DATABASE_URL must be set», que parece um defeito e não é.
- **Telefone ligado à sala** desde o #130 (ADR-0010, R221/R222): quem entra por telefone é
  um participante da sala. A telefonia completa — troncos, plano de marcação, CDR — e os
  canais da sala **ainda não estão portados**: ver `delonix-meet-telefonia`.
- **Segurança:** S1–S4 fechadas — a S4 (SSRF para fora: `odoo_url`, WebDAV, OIDC) fechou
  na **R180**, e o harness dava-a por aberta até 30-09. **Continua aberto** o registo sem
  verificação de email, que é decisão de produto e não defeito.
- **Os revisores estão em `.claude/agents/delonix-meet-*.md`.**

### Dois hábitos desta máquina que custaram tempo

- **O host é partilhado.** Já se mediu carga de 40 a 68 com outras sessões a compilar. Um
  teste com prazo de relógio falha aí e passa em série — antes de lhe chamar defeito,
  repete-o sozinho e regista a carga. O Docker chega a não conseguir arrancar contentores.
- **Postgres e Redis aparecem pausados** (`docker unpause wt-merge-postgres-1
  wt-merge-redis-1`). Os testes falham dentro do `sqlx testing/mod.rs`, não numa asserção
  — não é o teu código.

## Três regras que valem em tudo

1. **Código novo não copia uma regra: chama-a.** A catraca
   (`scripts/check-arquitectura-catraca.sh`) conta sete padrões de cópia e falha se
   algum subir. Não a contornes com um nome diferente — a revisão apanha isso.
2. **Uma capacidade anunciada tem código por trás.** Isto vale também para o harness: a
   documentação que afirma escopos, isolamento total ou revisores que não existem é o
   mesmo relato desonesto que o `check-capability-claims.sh` persegue.
3. **Identificadores novos em inglês**; comentários, documentação e mensagens em
   português (regra de fronteira de 2026-09-03). O código existente não se renomeia por
   isso.

## Ao fechar uma tarefa

Propõe um a três pedidos seguintes, por raio de dano. Cada um nomeia o alvo, a skill, a
prova a medir e o que fica de fora. Hoje, por ordem de valor:

1. «Porta a frente dos canais da sala (1 197 linhas em
   `origin/delonix-meet-backend/v3-canais`) — ela traz o consumidor do silenciar e do pôr
   a palco uma perna de telefone, que saíram do #130 por não terem quem os chamasse
   (`delonix-meet-telefonia`, revisor `delonix-meet-webrtc`). Prova: um caso em `sfu_e2e`
   que silencia uma perna e mede que o tom deixa de chegar. Fora: a telefonia (frente C).»
2. «Porta a frente C da telefonia (9 772 linhas, ADR-0009, 17 rotas)
   (`delonix-meet-telefonia`, revisores `delonix-meet-api` e `delonix-meet-security`).
   Prova: `tests/telephony.rs` contra Postgres real. Fora: o WhatsApp Business.»
3. «Põe um verificador de sintaxe de Lua no `make fitness` e no CI, e mete o
   `voice/freeswitch/scripts/dialin_ivr.lua` debaixo dele — está no caminho do cliente e
   hoje não tem portão nenhum. Prova: controlo negativo, partir o ficheiro e ver falhar.
   Fora: testar o comportamento do IVR.»
