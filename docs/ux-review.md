# Delonix Meet — Revisão de UI/UX (nível enterprise, vs MS Teams & Zoom)

> Revisão da UI completa do frontend (`web/src/`) com foco em **navegação/sidebar**,
> **exposição e consistência dos componentes**, **design system** e **UX da sala**.
> Objetivo: elevar a solução a um nível que compita de frente com Teams e Zoom.
> Achados priorizados **P0** (fundamental) · **P1** (alto valor) · **P2** (polish).

---

## Resumo executivo

A base é forte: SFU próprio, E2EE, temas, i18n, e um shell limpo com sidebar
colapsável ([Shell.tsx](../web/src/components/Shell.tsx)). O maior gap vs Teams/Zoom
**não é funcionalidade — é arquitetura de informação e consistência visual**:

1. A **sidebar é uma lista plana de 7 itens sem agrupamento** nem separação
   pessoal/admin ([Shell.tsx:12-21](../web/src/components/Shell.tsx)) — não escala e
   não comunica hierarquia enterprise.
2. Falta a **espinha dorsal de produtividade** que Teams/Zoom têm: **pesquisa global
   / command palette (Cmd-K)**, **centro de notificações**, e um **menu de conta**
   (o bloco do utilizador no rodapé nem é clicável — [Shell.tsx:315](../web/src/components/Shell.tsx)).
3. **Inconsistência de sistema**: tokens fragmentados (17 custom properties soltas em
   `styles.scss` + o bloco principal em [tokens.scss:109](../web/src/styles/tokens.scss)),
   **sprawl de classes de botão** (`btn`, `btn-sm`, `btn-new`, `btn-ghost`, `btn-meet-now`,
   `btn-today`), **cores hex hardcoded** em JSX (viola a regra de tokens), e **cabeçalhos
   de página não uniformes** (Home/Calendar sem `.page-head`).

Fechar estes três eixos dá o salto de "boa app" para "produto enterprise credível".

---

## Achados priorizados

| # | Área | Problema concreto (ficheiro) | Impacto | Recomendação | Pri |
|---|---|---|---|---|---|
| 1 | Sidebar / IA | Lista plana de 7 itens sem grupos nem secção admin ([Shell.tsx:12](../web/src/components/Shell.tsx)) | Não comunica hierarquia; não escala | Agrupar em secções (ver §2) com rótulos e separadores | **P0** |
| 2 | Conta | `.nav-user` no rodapé não é clicável ([Shell.tsx:315](../web/src/components/Shell.tsx)) | Padrão esperado (avatar→menu) ausente | Avatar → menu (perfil, definições, tema, logout, estado presença) | **P0** |
| 3 | Navegação | Sem **pesquisa global / Cmd-K** (nenhuma ocorrência no código) | Teams/Zoom têm; descoberta lenta | Command palette (salas, pessoas, gravações, ações) | **P1** |
| 4 | Notificações | Só toasts de presença ([PresenceProvider.tsx](../web/src/components/PresenceProvider.tsx)); sem histórico | Utilizador perde eventos (convites, chamadas perdidas, gravação pronta) | Sino + centro de notificações persistente | **P1** |
| 5 | Design system | Tokens divididos: 17 vars em `styles.scss` + `tokens.scss` | Inconsistência, difícil manter | Consolidar tudo em `styles/` (primitivos→semânticos) | **P1** |
| 6 | Botões | 6+ variantes de classe (`btn-new`, `btn-meet-now`, `btn-today`…) | Inconsistência visual e de código | Um sistema: `.btn` + modificadores (`--primary/-ghost/-sm/-danger`) | **P1** |
| 7 | Cores | Hex hardcoded em JSX (`#C8201D`, `#EDA33B`, `#202124`…) | Quebra temas; regra de tokens violada | Trocar por `var(--token)` | **P1** |
| 8 | Cabeçalhos | `.page-head` inconsistente (Home/Calendar não usam) | Falta de ritmo/consistência entre páginas | Componente `<PageHeader title actions breadcrumb>` reutilizado | **P1** |
| 9 | Densidade | Espaçamentos ad-hoc; sem escala de espaçamento tokenizada | Ruído visual, "não profissional" | Escala de spacing (4/8/12/16/24/32) + aplicar | **P2** |
| 10 | Vazios/erros | Estados vazios e de erro pouco trabalhados nas páginas | Percepção de imaturidade | `<EmptyState>` e `<ErrorState>` com ilustração + CTA | **P2** |
| 11 | Onboarding | (Resolvido) tour guiado adicionado ([OnboardingTour.tsx](../web/src/components/OnboardingTour.tsx)) | — | Estender a coach-marks contextuais na 1ª sala | **P2** |
| 12 | Acessibilidade | Falta foco visível consistente e `aria-current` na nav | Enterprise exige a11y (WCAG) | `:focus-visible` tokenizado + `aria-current="page"` | **P1** |

---

## Sidebar & Menu — proposta de reorganização

**IA atual** (flat, [Shell.tsx:12-21](../web/src/components/Shell.tsx)):
```
Início · Organização · Calendário · Gravações · Quadros · Análises · [Roadmap dev]
[Definições]  [avatar (não clicável)]
```

**IA proposta** (agrupada, com secção admin condicional ao papel):
```
┌ TRABALHO
│  ⌂ Início
│  ▦ Calendário
│  ◉ Reunir agora        ← ação primária destacada (novo)
├ BIBLIOTECA
│  ▷ Gravações
│  ▤ Quadros
├ ORGANIZAÇÃO
│  ♟ Diretório
│  ▤ Salas presenciais    ← separar de Diretório (hoje misturado)
├ ADMIN  (só admin da org)
│  ▤ Análises
│  ⚙ Webhooks & API
│  ⚑ Definições da org
└──────────────
[⌕ Pesquisar (Cmd-K)]     ← topo da sidebar (novo)
[🔔 Notificações]          ← rodapé (novo)
[avatar ▾  → menu conta]  ← clicável (perfil, tema, idioma, estado, sair)
```

Princípios: **secções rotuladas** (Teams usa isto), **ação primária "Reunir agora"**
sempre visível (Zoom), **secção Admin condicional ao papel** (`org_member.role`),
**pesquisa no topo** e **notificações + conta no rodapé**. Manter o colapso a 64px
(já existe, [styles.scss:669](../web/src/styles.scss)) mostrando só ícones com tooltip.

---

## Quick wins (≤1 dia cada)

1. **Tornar o avatar clicável** → menu de conta ([Shell.tsx:315](../web/src/components/Shell.tsx)). Fecha um padrão esperado.
2. **Agrupar a sidebar** com rótulos de secção — só reestrutura o array `NAV` + CSS de secção ([Shell.tsx:12](../web/src/components/Shell.tsx)).
3. **`aria-current="page"`** no item ativo + `:focus-visible` tokenizado (a11y barata, alto sinal enterprise).
4. **Consolidar botões**: alias das variantes órfãs (`btn-new`→`btn --primary`, etc.) num só sistema; começa pela folha de estilos sem tocar em muitos JSX.
5. **Substituir os ~10 hex hardcoded** em JSX por `var(--token)` (grep já os lista).
6. **Botão "Reunir agora"** fixo no topo do conteúdo/da sidebar (reusa o fluxo de criação de sala já existente).

---

## Design system — gaps concretos

- **Fonte única de tokens**: mover as 17 custom properties soltas de `styles.scss` para
  `styles/tokens.scss`/`_semantic.scss` (hierarquia primitivos→semânticos→componentes já
  iniciada). Hoje há duas fontes → drift.
- **Escala de espaçamento** tokenizada (`--space-1..8`) e **escala tipográfica** explícita
  (display/title/body/caption) — hoje tamanhos ad-hoc por componente.
- **Sistema de botões** único: `.btn` base + `--primary | --ghost | --danger | --sm | --icon`.
  Remove `btn-new`, `btn-meet-now`, `btn-today`, `btns`.
- **Componentes partilhados em falta**: `<PageHeader>`, `<EmptyState>`, `<ErrorState>`,
  `<Card>`, `<Badge>`, `<Menu>` (dropdown acessível). Reduz duplicação e unifica o look.
- **Elevação/sombra e raios** tokenizados (`--shadow-1/2`, `--radius-sm/md`) aplicados
  de forma consistente (cards, modais, drawer de definições, tour).
- **Foco/hover/estado ativo** consistentes e visíveis (a nav já tem `.active`; falta o
  resto ser sistemático).

---

## Room UX — vs Teams/Zoom

A sala ([Room.tsx](../web/src/pages/Room.tsx)) já tem grelha↔palco, controlos estilo Meet,
breakouts, host controls, whiteboard, polls/Q&A, CC, gravação — **muito completo**. Gaps
para paridade/liderança:

- **Barra de controlo com rótulos + agrupamento** (Teams agrupa mic/câmara | partilha |
  pessoas/chat | reações | mais). Hoje a pill é forte mas os itens secundários podem
  esconder-se num "Mais ⋯" mais claro.
- **Painel unificado à direita** com abas (Pessoas · Chat · Q&A · Notas · Breakouts) em vez
  de painéis separados — reduz troca de contexto (padrão Teams/Zoom).
- **Pre-join / green room mais rico**: preview de câmara, seleção de dispositivos, efeitos
  e teste de áudio ANTES de entrar (Zoom faz isto muito bem). Verificar cobertura atual no
  lobby ([Lobby.tsx](../web/src/pages/Lobby.tsx)).
- **Layouts explícitos** (Galeria / Orador / Lado-a-lado com partilha) num seletor visível,
  não só automático.
- **Reações + "levantar mão" persistentes na barra** com contadores, e **raise-hand queue**
  visível ao host.
- **Indicadores de rede/qualidade por participante** (já há getStats em [webrtc.ts](../web/src/webrtc.ts)) —
  expor um selo de qualidade discreto por tile.
- **Estado de gravação/CC/E2EE** sempre visível no topo (selos), como Teams.

---

## Sequência recomendada

1. **PR-1 (P0, meio-dia):** avatar→menu de conta + `aria-current` + `:focus-visible`.
2. **PR-2 (P0/P1, 1 dia):** reorganizar a sidebar em secções + "Reunir agora" + secção Admin por papel.
3. **PR-3 (P1, 1-2 dias):** consolidar design system (tokens numa fonte, sistema de botões, `<PageHeader>`/`<EmptyState>`), trocar hex hardcoded.
4. **PR-4 (P1, 2 dias):** command palette (Cmd-K) + centro de notificações.
5. **PR-5 (P1/P2):** painel unificado da sala (abas) + pre-join rico + seletor de layout.

---

## Estado de implementação

| Item | Estado |
|---|---|
| PR-1 — sidebar agrupada + avatar→menu + `aria-current`/`:focus-visible` | ✅ feito |
| PR-2 — command palette (Cmd-K) + role-gating da secção Admin | ✅ feito |
| Centro de notificações (sino + painel: chamadas perdidas + convites) | ✅ feito |
| Design system — `<PageHeader>` + `<EmptyState>` + tokens de escala + foco | ✅ primitivos criados e adotados (Recordings, notificações); adoção nas restantes páginas é incremental |
| Onboarding — tour guiado animado com skip | ✅ feito ([OnboardingTour.tsx](../web/src/components/OnboardingTour.tsx)) |
| PR-5 — painel unificado da sala (abas) + pre-join rico + seletor de layout | ⬜ pendente |

**Correção honesta à revisão inicial:** a leitura do código revelou que dois "achados"
estavam sobre-avaliados — (a) os **hex hardcoded em JSX** são, na maioria, legítimos
(paleta do quadro branco em canvas, cores de categoria de gráficos), não violações de
tokens; (b) os **overrides de tokens da sala** e as **variantes de botão** são
**intencionais**, não drift — pelo que a "consolidação" agressiva foi deliberadamente
evitada para não regredir. O ganho real de design system são os **primitivos partilhados**
(acima), adotados incrementalmente.

---

*Relatório de revisão. Ver [onboarding.md](onboarding.md) para o mapa da UI e [HARNESS.md](../HARNESS.md) §5 para o design system atual.*
