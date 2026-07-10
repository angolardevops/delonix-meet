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
| `agents/*.md` | agentes de IA (subagentes) | Revisores autónomos invocáveis (rust-perf, webrtc-sfu, k8s-scale, security, competitive) |
| `agents/commands/*.md` | agentes de IA (slash) | Comandos de revisão em linha (`/review-rust`, `/review-webrtc`, …) |
| `docs/reference/architecture.md` | Todos | **Referência estável** do sistema — base de conhecimento para o crescimento |
| `docs/competitive-positioning.md` | Todos | Análise Zoom/Teams/Meet — o que copiamos, o que superamos |
| `docs/ai-reviewers.md` | Todos | Painel de revisores especializados com personas de expertise |

> **Coerência:** `HARNESS.md`, `AGENTS.md` e `GEMINI.md` cobrem o mesmo núcleo (identidade, stack, invariantes, workflow, revisores). Ao mudar um invariante ou uma decisão de arquitetura, atualizar os três + `docs/reference/architecture.md`.

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
- [ ] Prompt templates em `agents/commands/` para operações comuns (ex: `/review-security`, `/add-feature`)
