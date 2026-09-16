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
| `web/src/**`, sala, design system, i18n | `HARNESS.md` §5 | `delonix-meet-frontend` |
| `deploy/`, K8s, afinidade por sala, coturn, imagens | `HARNESS.md` §7 e §11 | `delonix-meet-devops` |
| Prioridade de roadmap, paridade com Zoom/Teams/Meet | `docs/competitive-positioning.md` | `delonix-meet-product` |

## Portões por área

| Área | Portão |
|---|---|
| Qualquer mudança | `make fitness` (inclui a catraca da arquitectura e a do clippy) |
| Backend | `cd server && cargo fmt --check && cargo test --release` + `bash scripts/check-clippy-ratchet.sh` |
| Rota nova ou alterada | `bash scripts/check-route-auth.sh` + `bash scripts/check-isolamento-cobertura.sh` + `node web/e2e/isolamento.mjs` contra servidor real |
| Frontend | `cd web && npx tsc --noEmit && npx vitest run` + o e2e do ecrã |
| Media | `cargo test --release sfu_e2e` + `node web/e2e/reuniao.mjs` |

## O estado real (2026-09-16) — não o redescubras

- **Um crate só** (`delonix-server`), com 34 módulos planos. **Não há workspace nem
  crates.** A divisão está desenhada no ADR-0004 §3 e **só é possível depois de partir o
  ciclo de 18 módulos** (§6 passo 3).
- **Não há gRPC nem OpenAPI.** O desenho de onde entram está no ADR-0004 §4. Não
  proponhas gRPC entre o browser e o servidor.
- **S1–S3 fechadas no #76** (R121): administrador da plataforma declarado em
  `PLATFORM_ADMIN_USER_IDS`, sincronização Odoo pela regra R25, membro arquivado sem
  acesso. **Continuam abertos** `add_employee` por email, S4–S6 e o registo sem
  verificação de email — ver `delonix-meet-backend` §Segurança. Uma tarefa nesses
  caminhos fecha-os ou nomeia-os no relatório.
- **Os revisores estão em `.claude/agents/delonix-meet-*.md`.** A pasta `agents` na raiz, que o
  harness citava, nunca existiu no git.

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
prova a medir e o que fica de fora. Os três que a auditoria deixou em aberto, por
ordem (S1–S3 já fechadas no #76):

1. «Fecha a ligação de conta existente por email no `org::add_employee`
   (`delonix-meet-backend`, revisor `delonix-meet-security`). Prova: caso negativo em
   `web/e2e/isolamento.mjs`, contra servidor e Postgres reais, com o controlo positivo
   antes. Fora: verificação de email no registo (decisão de produto).»
2. «ADR-0004 §6 passos 1–2: `src/lib.rs`, `sfu_e2e` para `tests/`, `#[sqlx::test]` em
   org/meetings/recordings, e um job com Postgres no CI. Fora: mover SQL.»
3. «ADR-0004 §6 passo 5, só a separação da v1 em inquilino/operador/Odoo, com OpenAPI
   `utoipa` e um portão spec-gerado = spec-commitado (`delonix-meet-api`). Fora: gRPC.»
