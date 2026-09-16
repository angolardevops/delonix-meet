# N'GolaCloud — regras para agentes no Delonix Meet

Estás no repo `delonix-meet` (videoconferência self-hosted; backend Rust em `server/`, web React em `web/`).

1. **Contexto:** lê `AGENTS.md` (resumo) e `HARNESS.md` (completo) antes de qualquer tarefa.
2. **Arquitectura:** o destino do backend está em `docs/adr/0004-organizacao-alvo-do-backend.md`; as regras para código NOVO (§5) valem já e são contadas por `scripts/check-arquitectura-catraca.sh` — chamar a regra existente, nunca copiá-la.
3. **Contrato de API:** `docs/reference/api-contract.md` + a checklist em `.claude/skills/delonix-meet-api/SKILL.md`. gRPC só máquina-a-máquina; browser→servidor é REST + WS.
4. **Revisores:** `.claude/agents/delonix-meet-*.md` — em ferramentas sem subagentes, cola o ficheiro do revisor como contexto.
5. **Regressões a não reintroduzir:** `docs/reference/regressions.md`.
6. **Workspace:** as regras globais do N'GolaCloud estão no `CLAUDE.md` da raiz do workspace (worktree por tarefa, prova medida, as duas metades no relatório).
