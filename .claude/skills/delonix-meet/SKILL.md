---
name: delonix-meet
description: >-
  Ponto de entrada do Delonix Meet (videoconferência self-hosted — backend Rust em
  `server/`, web React em `web/`, voz em `voice/`): encaminha para a skill e o
  revisor certos, fixa os portões por área e os números medidos do repo.
when_to_use: >-
  SEMPRE que o trabalho for neste repo e ainda não souberes que skill ou revisor
  chamar; quando o pedido for «revê», «audita», «onde é que isto se põe», «que
  agente uso»; ou quando fores tocar em mais de uma camada. NÃO a uses para o
  motor `delonix-runtime` nem para o PaaS — as `delonix-*` sem `-meet` são do
  MOTOR e da plataforma.
---

# Delonix Meet — ponto de entrada

**Repo:** `delonix-meet/` (remote `angolardevops/delonix-meet`).
**Contexto completo:** [`HARNESS.md`](../../../HARNESS.md). **Resumo partilhado:** [`AGENTS.md`](../../../AGENTS.md).
**Destino da arquitectura:** [ADR-0004](../../../docs/adr/0004-organizacao-alvo-do-backend.md)
(**Aceite** a 2026-09-16; o §3 foi sucedido pelo
[ADR-0006](../../../docs/adr/0006-backend-enterprise-contextos-edicoes-e-entrega.md), também Aceite).
**Evidência de onde tudo isto sai:** [auditoria de 2026-09-16](../../../docs/auditoria-2026-09-16-backend.md).

## Fronteira

- **`delonix-meet-backend`** — onde o código Rust vive, camadas, crates, catraca, e o
  **estado da segurança** (o que fechou, o que está aberto). Esse estado escreve-se lá e
  só lá.
- **`delonix-meet-api`** — superfícies, checklist de rota nova, OpenAPI, gRPC. Os números
  do OpenAPI escrevem-se lá.
- **`delonix-meet-telefonia`** — telefone, PSTN, FreeSWITCH, troncos, CDR. O que está
  ligado e o que falta portar escreve-se lá.
- **`delonix-meet-voip`** — como um PBX de cliente (Issabel, FreePBX) ou uma
  operadora (tronco SIP, GSM, eSIM) se liga a nós, e as boas práticas de SIP/VoIP de um
  tronco. O que é medido e o que é só regra de casa escreve-se lá.
- **`paridade`** — «o que falta para empatar com o Zoom, o Teams e o Meet», «por onde
  começo», «podemos anunciar X». A fila está em `.claude/paridade/` (uma lacuna, um
  ficheiro, um prompt que outra sessão corre tal e qual) e a tabela medida em
  `docs/paridade-medida-2026-10-18.md`. **Um documento de comparação NÃO é fonte**: o
  `docs/plano-paridade-2026-10-13.md` estava errado em dois pontos **cinco dias** depois
  de ser escrito — dizia SCIM = zero (há 899 linhas com rotas registadas e dez testes) e
  que o correio não tinha parte `text/calendar` (tem, `mail.rs:412`). Lê-se para
  intenção; os números remedem-se.
- **Esta skill** só encaminha, lista os portões e guarda os números transversais do repo.
  Não repete o conteúdo das quatro de cima: aponta.
- **Fora deste repo:** o Meet é um produto separado da cloud. A 2026-10-03 não há
  `.claude/settings.json` no repo, por isso **nenhum plugin do harness do workspace
  carrega aqui** — as skills `ngolacloud-*` e as `delonix-*` do motor não existem numa
  sessão aberta neste repo. Frontend, deploy, media e produto **não têm skill**: o
  conhecimento está no `HARNESS.md`, no catálogo de regressões e no revisor da área.

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
| SFU, ICE, simulcast, gravação, media num só sentido | — (`docs/reference/regressions.md`) | `delonix-meet-webrtc` |
| Interligar um PBX de cliente (Issabel, FreePBX, Asterisk) ou uma operadora (tronco, GSM, eSIM); NAT, DTMF, codecs, fraude de tarifação | `delonix-meet-voip` | `delonix-meet-security` (allowlist, credenciais) + `delonix-meet-devops` (portas, NAT, imagem) |
| Telefone, PSTN, dial-in, SIP, FreeSWITCH, SRTP, troncos, plano de marcação, CDR | `delonix-meet-telefonia` | `delonix-meet-webrtc` (media) + `delonix-meet-security` (socket, chaves, credenciais SIP) + `delonix-meet-api` (rotas `/telephony`) |
| `web/src/**`, sala, design system, i18n | — (`HARNESS.md` §5 e §8) | `delonix-meet-frontend` |
| `deploy/`, K8s, afinidade por sala, coturn, imagens, CI | — (`HARNESS.md` §7 e §11) | `delonix-meet-devops` |
| Prioridade de roadmap, paridade com Zoom/Teams/Meet | — (`docs/competitive-positioning.md`) | `delonix-meet-product` |

Oito revisores para cinco skills, de propósito: `architecture` e `rust` partilham a
skill do backend com perguntas diferentes (onde vive o código / o que faz a uma sala
sob carga); `security` usa a secção de segurança dela; a telefonia não tem revisor
próprio e reparte-se por três.

## Portões por área

| Área | Portão |
|---|---|
| Qualquer mudança | `make fitness` — `cargo fmt --check` mais os `scripts/check-*.sh`. **Medido a 2026-10-08: 27 scripts**, todos no `make fitness` menos dois, que só correm no CI: `check-isolamento-cobertura.sh` e `check-browser-antes-do-e2e.sh` |
| Backend | **o que o CI corre, por esta ordem** (`ci.yml:57-86`): `cargo fmt --manifest-path server/Cargo.toml --check` · `bash scripts/check-clippy-ratchet.sh` · `cargo test --manifest-path server/Cargo.toml --release --workspace -- --test-threads=4` · `cargo build --release` · `bash scripts/check-openapi.sh`. O `fmt` é o primeiro e é o que mais vezes trava um push apressado |
| Rota nova ou alterada | `delonix-meet-api` §Portões |
| Frontend | `cd web && npx tsc --noEmit && npx vitest run` + o e2e do ecrã |
| Media | `cargo test --release sfu_e2e` + `node web/e2e/reuniao.mjs` |
| Telefone e PSTN | `delonix-meet-telefonia` §Portões — inclui as provas contra um FreeSWITCH real, que **não correm no CI** |

Os portões (`ls scripts/check-*.sh` é a lista a sério; esta envelhece): `arquitectura-catraca`, `browser-antes-do-e2e`, `capability-claims`,
`clippy-ratchet`, `crate-deps`, `dep-audit`, `docs-drift`, `ffmpeg-licenca`, **`filas-reivindicacao`**, `fs-xml`,
`isolamento-cobertura`, `k8s-render`, `lua-sintaxe`, `openapi`, `proto`, `repo-hygiene`, `room-affinity`,
`route-auth`, `tenant-rls`. O `check-tenant-rls.sh` exige um cluster vivo e **salta sem
ele** — verde aí não prova nada.

## O estado real (2026-10-18, `develop` `e48658f06`) — não o redescubras

**A linha activa é a `develop`, não a `main`.** Os PR vão todos para lá. Esta secção dizia
`main` até 2026-10-18, e a `main` está centenas de commits atrás — construir contra ela é
reimplementar o que já existe. (Já aconteceu: três commits de uma funcionalidade de
convidado que a `develop` já tinha, descartados.)

Números **contados hoje**, cada um pelo comando ao lado. A coluna «dizia» é o que esta
secção afirmava a 2026-10-03, para se ver a que velocidade isto envelhece:

| O quê | Hoje | Dizia | Comando |
|---|---|---|---|
| Regressões no catálogo | **231** (a última é a R311) | 173, R225 | `grep -cE '^### R[0-9]+' docs/reference/regressions.md` |
| Binários em `server/tests/` | **83** | 31 | `ls server/tests/*.rs \| wc -l` |
| Portões `scripts/check-*.sh` | **31** | 27 | `ls scripts/check-*.sh \| wc -l` |
| Catraca do clippy | **11** | 13 | `cat scripts/clippy-baseline.txt` |
| Migrações | **125** (a última é a `0125`) | 73 | `ls server/migrations/*.sql \| wc -l` |
| Rotas no router | **335** | — | `grep -cE '^\s*\.route\(' server/src/lib.rs` |
| ADR | **31** | — | `ls docs/adr/*.md \| wc -l` |
| Ficheiros de teste do frontend | **111** | — | `find web/src -name '*.test.ts*' \| wc -l` |
| `rotas_sem_openapi` | **0** | 0 | `scripts/openapi-baseline.txt` |

Dois avisos sobre estes números. O do frontend **muda com o ramo** — um ramo com testes
novos dá mais, e é por isso que o commit está no cabeçalho. E PRs abertos não se citam de
memória: `gh pr list` antes de desenhar, sempre.

### O que está medido como AUSENTE (2026-10-18) — não o proponhas como se existisse, nem o procures outra vez

Dez capacidades a zero por grep, com o id da fila. O detalhe e o prompt estão em
`.claude/paridade/`; isto é só para não se perder meia hora a redescobrir.

| id | A zero | Grep que o prova |
|---|---|---|
| `A-03` | push com a app fechada | `vapid\|pushManager\|web-push` = 0, **em todo o repo**; o `sw.js` não tem handler de `push` |
| `A-05` | calendário nos dois sentidos | `graph.microsoft\|googleapis\|caldav` = 0 em código |
| `A-06` | canais persistentes de equipa | uma única rota de chat: `lib.rs:801` |
| `A-08` | legal hold / eDiscovery | `legal_hold\|ediscovery` = 0 |
| `A-09` | modo webinar | `webinar` = 4 acertos, **nenhum de produto**; `broadcast` e `hybrid` guardam-se e não fazem nada |
| `A-11` | SAML 2.0 | `saml\|AuthnRequest` = 0 (o OIDC existe e é bom) |
| `B-02` | assistente de IA **ao vivo** | a acta na sala é regex (`useTranscription.ts:24`); o LLM corre depois |
| `B-03` | pesquisa semântica | `pgvector\|embedding` = 0 (o FTS é bom, mas é lexical) |
| `C-01` | PBX: filas, voicemail, atendedor, supervisão | `call_queue\|voicemail\|auto_attendant` = 0; transferência **desligada à força** |
| `C-04` | salas de hardware / SIP de vídeo | `h.323\|cisco` = 0 — e **o bloqueio não é o codec**: o SFU já registra H.264 |

**Duas armadilhas de medição**, ambas já cometidas aqui:

- `grep -i poly` casa «polyfill» e fez o H.323 parecer existente. Palavras curtas medem-se
  com fronteira, ou não se medem.
- Uma capacidade pode existir **sem a palavra**: as peças do webinar estão todas no código
  (sala de espera, `Role::Attendee`, palco que fixa áudio no SFU, Q&A, RTMP) e o grep de
  `webinar` não as vê. Grepa o código **e** confirma no router — uma função sem rota não é
  uma capacidade.

E três coisas que a fila chama **afirmação falsa latente**, porque a base de dados ou o
ecrã prometem o que nenhum código serve: os dois formatos de sala inertes (`A-09`), e o
telefone que entra numa sala E2EE porque o caminho de entrada não verifica o que o de
saída recusa com razão escrita (`A-01`).

- **Workspace em transição** (ADR-0006 §1). O monólito `delonix-server` continua a ser a
  raiz, com `src/lib.rs` (o `main.rs` tem seis linhas e só chama `run()`), mais quatro
  crates em `server/crates/`: `delonix-meet-core`, `-domain`, `-protocol` e `-store`.
  O resto do mapa e a regra da dependência: `delonix-meet-backend`.
- **Testes de integração contra Postgres real** — `cargo test --release --workspace --
  --test-threads=4` precisa de `DATABASE_URL`. Sem ela, os testes que usam `#[sqlx::test]`
  falham com «DATABASE_URL must be set», que parece um defeito e não é.
- **OpenAPI, superfícies e gRPC:** `delonix-meet-api`.
- **Telefone:** a ponte telefone↔sala (#130), o telefone no censo da sala (#135) e a
  telefonia de troncos, plano de marcação e CDR (#136) estão na `main`. O que ficou de
  fora: `delonix-meet-telefonia`.
- **Palco:** o `Spotlight` do anfitrião fixa o áudio do destacado no SFU pela porta
  `signaling::StageControl` (R225) — o selector de oradores já não o pode suprimir.
- **Segurança:** S1–S6 fechadas; o único aberto é decisão de produto. A lista, com a
  regressão que guarda cada uma: `delonix-meet-backend` §Segurança.
- **Os revisores estão em `.claude/agents/delonix-meet-*.md`.**

### Dois hábitos desta máquina que custaram tempo

- **O host é partilhado.** Já se mediu carga de 40 a 68 com outras sessões a compilar. Um
  teste com prazo de relógio falha aí e passa em série — antes de lhe chamar defeito,
  repete-o sozinho e regista a carga. O Docker chega a não conseguir arrancar contentores.
- **Postgres e Redis aparecem pausados** (`docker unpause wt-merge-postgres-1
  wt-merge-redis-1`). Os testes falham dentro do `sqlx testing/mod.rs`, não numa asserção
  — não é o teu código.

### Retomar noutra máquina

O que NÃO vem com o `git clone`, e por isso se refaz:

- **Os plugins do workspace** registam-se uma vez por máquina, a partir da raiz do
  `ngolacloud` (`claude plugin marketplace add <raiz>`); as skills e os revisores deste
  repo vêm no próprio repo (`.claude/`) e não precisam de nada.
- **Worktrees e ramos locais não viajam.** Só existe o que está em `origin`: começa por
  `git fetch origin` e abre o worktree a partir de `origin/main`, nunca de uma `main` local.
- **A infra dos testes** sobe com `make infra` (Postgres, Redis, coturn) e os testes de
  integração precisam de `DATABASE_URL` exportada.
- **O toolchain é `stable` flutuante**, no CI e nas imagens: uma versão nova do Rust pode
  trazer um aviso que sobe a catraca do clippy sem ninguém ter mexido no código (foi o
  `fetch_update` depreciado no 1.99). Lista os avisos antes de procurar o defeito no diff.

## Três regras que valem em tudo

1. **Código novo não copia uma regra: chama-a.** A catraca
   (`scripts/check-arquitectura-catraca.sh`) conta oito padrões de cópia e falha se
   algum subir. Não a contornes com um nome diferente — a revisão apanha isso.
2. **Uma capacidade anunciada tem código por trás.** Isto vale também para o harness: a
   documentação que afirma escopos, isolamento total ou revisores que não existem é o
   mesmo relato desonesto que o `check-capability-claims.sh` persegue.
3. **Identificadores novos em inglês**; comentários, documentação e mensagens em
   português (regra de fronteira de 2026-09-03). O código existente não se renomeia por
   isso.

## Ao fechar uma tarefa

Propõe um a três pedidos seguintes, por raio de dano. Cada um nomeia o alvo, a skill, a
prova a medir e o que fica de fora. Os pedidos de telefonia estão no fim da
`delonix-meet-telefonia`; os transversais, hoje, por ordem de valor:

1. «Põe o `HARNESS.md` e o `AGENTS.md` a par da `main`: o `AGENTS.md:64` lista três
   skills e são quatro; o `HARNESS.md:364` e o `AGENTS.md:58` mandam declarar módulos e
   rotas no `main.rs`, que tem seis linhas (o router está em `server/src/lib.rs:232`); o
   `HARNESS.md:368` fala do kit `ui.tsx`, que já não existe (`web/src/ui/kit.tsx`). Prova:
   `bash scripts/check-docs-drift.sh` verde e um controlo negativo que mostre se o portão
   vê este tipo de deriva. Fora: reescrever as secções de produto.»
2. «Passa o `sfu_e2e` de `server/src/sfu_e2e.rs` (`lib.rs:54`) para `server/tests/` — é a
   metade por fazer do passo 1 do ADR-0004 §6 (`delonix-meet-backend`, revisores
   `delonix-meet-architecture` e `delonix-meet-webrtc`). Prova: `cargo test --release
   sfu_e2e` com os mesmos casos e a mesma contagem. Fora: mexer na lógica do SFU.»
3. «Decide o estado do ADR-0009: o código da telefonia está na `main` desde o #136 e o
   ADR continua **Proposto** (`docs/adr/0009-…md:3`). É decisão do dono, não do agente.»
