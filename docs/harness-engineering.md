# Harness Engineering — Delonix Meet

Este documento explica como o harness de desenvolvimento AI está estruturado e como usá-lo.

## Ficheiros do harness

| Ficheiro | Audiência | Propósito |
|---|---|---|
| `HARNESS.md` | agentes de IA (CLI + API) | Harness primário — contexto completo da plataforma |
| `AGENTS.md` | OpenAI Codex CLI + convenção universal de agentes | Resumo operacional partilhado (identidade, stack, invariantes, workflow, revisores) |
| `GEMINI.md` | Gemini CLI / Gemini API | Contexto equivalente ao HARNESS.md em inglês |
| `.github/copilot-instructions.md` | GitHub Copilot (VS Code) | Instruções inline carregadas automaticamente pelo Copilot |
| `.cursorrules` | Cursor | Padrões de código + contexto para autocompleção |
| `.claude/agents/delonix-meet-*.md` | agentes de IA (subagentes) | Oito revisores versionados: architecture, api, security, rust, webrtc, frontend, devops, product |
| `.claude/skills/delonix-meet*/SKILL.md` | agentes de IA (skills) | Cinco: `delonix-meet` (entrada e encaminhamento), `delonix-meet-backend`, `delonix-meet-api`, `delonix-meet-telefonia`, `delonix-meet-voip` |
| `docs/adr/0004-organizacao-alvo-do-backend.md` | Todos | Organização-alvo do backend e regras para código novo |
| `scripts/check-arquitectura-catraca.sh` | CI + `make fitness` | Catraca: nenhuma cópia nova de regra (ADR-0004 §5) |
| `docs/reference/architecture.md` | Todos | **Referência estável** do sistema — base de conhecimento para o crescimento |
| `docs/competitive-positioning.md` | Todos | Análise Zoom/Teams/Meet — o que copiamos, o que superamos |
| `docs/ai-reviewers.md` | Todos | Painel de revisores especializados com personas de expertise |

> **Coerência:** `HARNESS.md`, `AGENTS.md` e `GEMINI.md` cobrem o mesmo núcleo (identidade, stack, invariantes, workflow, revisores). Ao mudar um invariante ou uma decisão de arquitetura, atualizar os três + `docs/reference/architecture.md`.

> **O que o `check-docs-drift.sh` impõe, e o que não impõe.** Ele garante que nenhum
> ficheiro do harness cita um `delonix-meet-*` que não exista, que o `name:` de cada
> skill e agente bate com o caminho, e que todas as ligações resolvem. **Não** garante
> que o que lá está escrito ainda é verdade: a 2026-09-30, com 491 commits desde a
> última revisão, o agente de frontend mandava ir buscar controlos a um
> `components/ui.tsx` que a reescrita da consola apagou, e o de segurança dava por
> abertas quatro falhas já fechadas (S4 na R180, S5 na R160, S6 nas R170/R171). Um
> revisor que segue instruções falsas é pior do que revisor nenhum — **ao fechar uma
> entrega grande, relê a skill e o agente da área que tocaste.**

## Como os modelos carregam o contexto

### agentes de IA (CLI)
Carrega `HARNESS.md` automaticamente a partir da raiz do repositório. Também carrega `HARNESS.md` em sub-diretórios quando trabalhando nesses diretórios. A memória persistida em `~/agents/projects/.../memory/` complementa com estado de sessão.

```bash
# Verificar que está a ler o HARNESS.md
claude "resume o estado atual do projeto"
```

### GitHub Copilot
`.github/copilot-instructions.md` é carregado automaticamente no VS Code com a extensão Copilot (versão ≥ 1.26). Aparece como contexto em todas as sugestões inline e no chat.

### Cursor
`.cursorrules` é carregado automaticamente pelo Cursor Editor em todas as janelas do projeto. Inclui regras de geração de código para Rust e TypeScript.

### Gemini CLI / Gemini API
`GEMINI.md` pode ser passado como contexto de sistema. Para o Gemini CLI:
```bash
gemini --system-prompt @GEMINI.md "adiciona suporte a X"
```

### Codex (OpenAI)
`.cursorrules` é compatível com o Codex CLI e com ferramentas que leem `.cursorrules`.

---

## Painel de revisores — como usar

O ficheiro `docs/ai-reviewers.md` define 8 personas de revisor baseadas em engenheiros reais. Para invocar:

### Exemplo 1 — Revisão de segurança
```
Assume o papel de Adam Langley (Google BoringSSL) e revê o módulo auth.rs.
Identifica fraquezas no modelo de cookie, JWT e Argon2.
```

### Exemplo 2 — Performance Rust
```
Como Graydon Hoare, criador do Rust, revisaria o hot path de fan-out RTP
em sfu.rs? Há alocações desnecessárias no loop de forward?
```

### Exemplo 3 — Deploy e K8s
```
Com o chapéu de Brendan Burns (co-criador do Kubernetes), define um
Helm chart mínimo para Delonix Meet com: backend deployment, postgres
StatefulSet, redis deployment, coturn DaemonSet e NetworkPolicy para
isolamento inter-pod.
```

### Exemplo 4 — WebRTC correctness
```
Justin Uberti está a fazer code review do recorder.rs.
O IVFWriter usa PTS em ms reais do RTP em vez do contador de frames da lib.
Esta abordagem é correta para VP8 → IVF? Há edge cases com B-frames?
```

### Exemplo 5 — Análise competitiva de feature
```
Compara a implementação de breakout rooms do Delonix Meet com a do Zoom
usando o perfil do Zoom Platform Architect. O que falta para atingir paridade?
```

---

## Contexto competitivo para prompts

Quando pedir novas features, incluir contexto competitivo ajuda o modelo a gerar sugestões de produto:

```
Quero implementar X no Delonix Meet.
O Zoom faz assim: [...]
O Teams faz assim: [...]
O Google Meet faz assim: [...]
O que faltam é: [...] (ver docs/competitive-positioning.md)
Implementa uma versão que seja melhor nos seguintes aspetos: [...]
```

---

## Manter o harness atualizado

O harness é tão útil quanto está atualizado. Atualizar após:

1. **Nova feature completada** → atualizar secção "Feature inventory" no `HARNESS.md` e `GEMINI.md`
2. **Nova decisão de arquitetura** → adicionar a "Architecture — non-obvious decisions"
3. **Novo gotcha descoberto** → adicionar a "Known gotchas"
4. **Dependência de versão fixada** → atualizar tabela de stack
5. **Concorrente lança feature relevante** → atualizar `competitive-positioning.md`

### Quem atualiza
O agente agentes de IA atualiza automaticamente a memória persistida (`~/agents/projects/.../memory/`).
Os ficheiros de harness no repositório devem ser atualizados manualmente ou por PR.

---

## Roadmap do harness

- [ ] `server/HARNESS.md` específico do backend com exemplos de handlers e queries
- [ ] `web/HARNESS.md` específico do frontend com exemplos de componentes e hooks
- [ ] Testes automáticos que validam que HARNESS.md não está desatualizado (lint das features marcadas ✅)
- [ ] Integration com GitHub Actions: comentário automático de revisão usando personas do painel
- [ ] Prompt templates como skills em `.claude/skills/` para operações comuns (ex.: `/review-security`, `/add-feature`)
- [x] **Portão de sintaxe de Lua** (R223, 2026-09-30). O `dialin_ivr.lua` e o
      `ramais_dial.lua` estão no caminho do cliente e nenhum portão os lia.
      `scripts/check-lua-sintaxe.sh` compila-os com o `luac5.2` (o Lua do `mod_lua`) no
      `make fitness` e no CI, e falha — não salta — se não houver `luac`. Verifica a
      **sintaxe**, não o comportamento do IVR.
- [x] **Portão do XML do FreeSWITCH** (R226, 2026-10-03). `scripts/check-fs-xml.sh` lê
      todo o `*.xml` e `*.xml.inc` de `voice/` no `make fitness` e no CI: XML bem
      formado, nenhuma directiva `X-PRE-PROCESS` dentro de um comentário (o FreeSWITCH
      executa-a lá), e nenhum `$${NOME_EM_MAIÚSCULAS}` (lê uma variável global, não o
      ambiente), e desde a R228 nenhum perfil de conferência a pedir um grupo de controlos que
      o ficheiro não define. É estático: o que a configuração **faz** em chamada continua a ser do
      `scripts/softphone-prova.sh srtp-real`.
- [x] **As provas de voz com chamadas correm no CI** (2026-10-04). O workflow «Imagem
      FreeSWITCH» já construía a imagem; passa a correr contra ela o
      `softphone-prova.sh selftest` (PIN por DTMF, tons nos dois sentidos), o `srtp-real` e
      o `srtp-cluster` (R226, R227: a configuração que o arranque monta com os ficheiros do
      `compose.yaml` e com os do cluster — a chamada em claro leva `488`, e o segredo de
      voz e o PIN não ficam no log), e dispara com qualquer mudança em `voice/freeswitch/`,
      `voice/cluster/`, no `compose.yaml`, no `cluster-voice.sh` ou no script. Um softphone
      e um FreeSWITCH numa rede docker sem saída — não prova a operadora, o Kamailio nem o
      browser.
- [x] Revisores e skills versionados em `.claude/` e verificados pelo `check-docs-drift.sh` (2026-09-16)
