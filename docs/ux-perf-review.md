# Delonix Meet — Levantamento de UI/UX: performance e layout

> Segunda ronda. A [primeira](ux-review.md) tratou de **arquitetura de informação**
> (secções na nav, Cmd-K, centro de notificações, menu de conta, `PageHeader`,
> `EmptyState`) e está fechada. Esta trata do que ficou por medir: **quanto custa
> carregar e usar a consola**, e **se o layout aguenta um ecrã que não seja o do
> portátil de quem a escreveu**.
>
> Medido a **2026-08-25** contra `feat/console-ui-template` (`762e98e`), com
> `npx vite build` e leitura estática de `web/src/` e `deploy/`.
> Prioridades: **P0** bloqueante · **P1** alto valor · **P2** polish.

---

## Estado

| Lote | Achados | Estado |
|---|---|---|
| **1 · Config e cortes** | 1.1 · 1.2 · 1.3 · 4.3 (+1.6 de borla) | **FECHADO** — `ui/lote-1-carregamento`, medido abaixo |
| **2 · Layout** | 3.1.1 · 3.1.3 · 3.1.4 · 4.1 · 4.2 · 3.2.5 | **FECHADO** — `ui/lote-2-layout`, medido abaixo |
| **3 · Sala e higiene** | perfil → 2.1–2.3 · 3.2.1 · 3.2.4 | **FECHADO** — `ui/lote-3-sala`, medido abaixo |

### Lote 1 — o que mudou, medido

| Medida | Antes | Depois |
|---|---|---|
| Chunk de entrada (cru) | 648,82 KB | **347,40 KB** |
| Chunk de entrada (gzip) | 194,55 KB | **110,84 KB** |
| **Bytes no fio, 1.º acesso** (JS+CSS, via nginx real) | **826 447** | **143 381** |
| Idiomas no arranque | 3 (98,9 KB) | **1** (EN/FR sob `import()`) |
| Ficheiros JS na landing pública | 1 (tudo) | **2** (entrada + idioma, se trocado) |
| `Room` no chunk de entrada | sim | **não** (135,47 KB à parte) |

O número do fio foi medido a servir o `dist` real pela imagem
`nginxinc/nginx-unprivileged:1.27-alpine` com o `nginx.conf` deste repo:
`347 395 → 110 413` bytes de JS e `178 237 → 32 968` de CSS, com
`Vary: Accept-Encoding` e os cabeçalhos COOP/COEP intactos.

Os quatro invariantes viraram testes (`web/src/lote1.invariantes.test.ts`, 17
casos) e cada um foi visto a ficar **vermelho** com o invariante partido e verde
com ele reposto. Lições em `reference/regressions.md` (R46, R47).

**A lacuna do lote 1 ficou fechada no lote 2** — ver o bloco do lote 2 abaixo:
o anel de foco está confirmado num Chromium real, por comparação de pixéis, nos
controlos que estavam cegos. Ver R46.

### Lote 2 — o que mudou, medido

Verificado a **375×812** contra o `dist` construído, servido por um servidor com
a API simulada em quatro modos (ok · vazio · falha · lento).

| Medida | Antes | Depois |
|---|---|---|
| Largura roubada ao conteúdo pelo rail, a 375px | **224 px** (60% do ecrã) | **0** — gaveta sobreposta |
| Entrar por código no telemóvel | não existia (`display: none`) | **na gaveta**, campo focável de 29px |
| Estados de carregamento na app | **0** | 3 listas de esqueleto, anunciadas `aria-live` |
| Falha de API na Home | dashboard vazio, indistinguível de «não há» | **erro + mensagem do servidor + «Tentar outra vez»** |
| Sessão perdida num 5xx | sim | **não** — só 401/403 terminam sessão |
| Emoji como controlo na consola | 15 | **0** |
| `100vh` sem `dvh` | 14 | **0** |

Ciclo completo confirmado no browser: API a devolver **503** → três cartões em
erro, **sem perder a sessão** → servidor recupera → «Tentar outra vez» → dados.

### Práticas de estado, alinhadas com o `delonix-portal`

O `delonix-portal` já tinha pago por três armadilhas; passam a valer aqui, com
teste (R48, R49 e R50 no catálogo):

- **`ApiError` carrega o `status`.** Sem ele, quem apanha o erro adivinha pela
  mensagem — e é dessa adivinha que nasce o resto.
- **`isAuthFailure`: só 401/403 terminam a sessão.** O `refreshSession` deslogava
  em qualquer resposta não-OK; um gateway a reiniciar tirava a pessoa de onde
  estava para resolver um problema que não era dela.
- **`isAbort` em todo o `.catch()` de um pedido com sinal.** O duplo-efeito do
  StrictMode aborta o primeiro pedido; sem a guarda, pinta-se um erro em cada
  montagem. No portal faltava em onze sítios.
- **Estado de servidor numa máquina de três estados** (`loading | ready | error`),
  por secção, com `AbortController` por efeito — não num booleano nem num store
  global. Estado de UI (gaveta, tema, colapso) fica separado e só o que é
  preferência persiste.

**A lacuna dos lotes 1 e 2, fechada.** Ficou escrito nos dois PRs que o painel de
browser do agente não resolve `transform` nem `outline`, e que faltava uma
passagem num motor a sério. Foi feita: `web/e2e/layout-consola.mjs` corre um
Chromium real contra o `dist` construído e verifica as duas coisas que estavam
por confirmar.

```
3.1.1 · a gaveta em ecrã estreito (375×812)
  ok  o rail está FORA do ecrã com a gaveta fechada        x=-268 largura=268
  ok  o conteúdo ocupa a largura toda (antes perdia 224px) x=0 largura=375
  ok  a gaveta desliza para dentro do ecrã                 x=0 largura=268
  ok  o transform computado é `none` com a gaveta aberta
  ok  o backdrop cobre o ecrã · aria-expanded acompanha
  ok  entrar por código está alcançável na gaveta, e aceita escrita
  ok  Escape fecha a gaveta · sem scroll horizontal

4.3 · o anel nos controlos que ESTAVAM cegos
  ok  o campo de código da barra mostra foco   (comparação de pixéis)
  ok  o campo do Cmd-K mostra foco             (comparação de pixéis)
  ok  o contentor casa :focus-within, e ganha o anel do sistema
```

Duas notas sobre o próprio teste, porque as duas custaram uma repetição:

1. **O primeiro teste de foco apontava ao sítio errado.** Media um `.land-link`,
   que nunca teve `outline: none` e por isso sempre teve o anel do browser — o
   teste passava sem provar nada. Os sujeitos certos são os **seis controlos que
   estavam cegos**, e a leitura é por **comparação de pixéis**, que não depende
   de como o Chromium serializa o `outline` dele próprio.
2. **A primeira tentativa de o ver falhar não falhou** — o `sed` que devia
   partir a regra da gaveta tinha um `^` e a regra está indentada dentro da
   media query, por isso não mudou nada e o «vermelho» foi um verde disfarçado.
   Repetido a sério: regra da gaveta trocada → 2 falhas; anéis removidos →
   2 falhas.

O arnês **não está ligado ao CI**: o Playwright obrigaria o `npm ci` de todos os
jobs a descarregá-lo mais o Chromium, e isso é custo de build para toda a gente —
decisão de quem mantém o repo, não efeito lateral de uma correcção de layout. A
nota de como o ligar está no fim do ficheiro.


### Lote 3 — o perfil primeiro, depois a correcção

O plano dizia que este lote **começa por tirar o perfil**, e começou. Não é
possível conduzir a sala inteira sem SFU, media e pares reais, por isso o perfil
mede o que a correcção muda: o custo de a raiz voltar a renderizar sem que os
mosaicos tenham mudado — que é exactamente o que os três relógios de 1 Hz
provocavam. Instrumento: o `<Profiler>` do React (`actualDuration`, tempo de
commit real), sobre o mosaico **do produto**, em `web/e2e/bench/`.

| pares | sem `memo` | com `memo` | fator |
|---:|---:|---:|---:|
| 4 | 1,058 ms/tique | 0,042 ms/tique | **25×** |
| 12 | 2,352 ms/tique | 0,038 ms/tique | **62×** |
| 25 | 2,670 ms/tique | 0,072 ms/tique | **37×** |

A leitura que interessa não é o fator: é a **forma da curva**. Sem `memo` o custo
cresce com o número de pessoas na sala; com `memo` é **plano** (~0,04–0,07 ms
seja com 4 ou com 25). Uma sala grande era o pior caso e passa a não ser caso.

**Aviso sobre o primeiro instrumento.** A primeira versão do banco contava
renders num invólucro à volta do mosaico — e o invólucro renderiza sempre, com
ou sem `memo`. Deu exactamente o mesmo número nas duas colunas e teria sido
publicado como «o memo não faz diferença». Contar renders de um invólucro mede
o invólucro; o que se quer é tempo de commit da subárvore.

| Medida | Antes | Depois |
|---|---|---|
| `setState` de tempo na raiz da sala | 3 × 1 Hz + 1 × 30 s | **0** |
| Reconciliações/dia da sala só por relógios | ~262 000 | **0** |
| `memo(` em `Room.tsx` | 0 | mosaico extraído e memoizado |
| Callbacks por render passados aos mosaicos | 3 × N (novos) | 3 (estáveis) |
| `.dash-card` / `.dash-grid` / `.dash-card-head` no topo | 2 cada | **1 cada** |
| Marca em hexadecimal solto | 4 | **0** |

A fusão do 3.2.1 foi verificada por **diferença de estilo computado** num
Chromium real, sobre oito selectores do dashboard, antes e depois: **zero
diferenças**. Apagar a regra velha em vez de a sobrepor não mudou um pixel.

**Correcção ao próprio relatório.** O achado 3.2.4 dizia «15× `#eda33b`, 9×
`#c8201d`». Estava inflacionado: 22 dessas ocorrências são *fallbacks* de
`var(--token, #hex)`, que são defensivos e não violam a regra dos tokens. Os
hardcoded a sério eram **4**, e são esses que foram convertidos — incluindo um
`status-badge` que usava o dourado da MARCA para dizer «em curso», onde o design
system manda usar `--warn`.

**O que o lote 3 NÃO fez:** os 74 `#fff` do SCSS. Quase todos são texto sobre
superfície escura (o rail, botões de acção), onde o token certo depende do sítio
— é uma passagem caso a caso com risco visual real, e não se faz de enfiada no
fim de um lote. Fica como dívida com dono. E o `useGridLayout` (achado 2.4)
também fica: a troca por `grid` + `aspect-ratio` mexe no layout de vídeo, que é
o sítio onde uma regressão se paga mais caro, e merece o seu próprio lote com
verificação visual.


---

## As duas metades

**O que ficou provado.** Todos os números abaixo saem de um build real e de
contagens sobre o código e a configuração de servir. O `dist/` foi reconstruído
nesta branch — não se herdou o de 5 de Agosto.

**O que NÃO foi validado.** Nada disto foi corrido num browser: não há Lighthouse,
não há perfil do React DevTools, não há medição em dispositivo real nem em rede
degradada, e não há teste com leitor de ecrã. Os achados de runtime (§2) são
deduzidos da estrutura do código e **descrevem trabalho que o React vai fazer**,
não latência observada. Antes de otimizar a §2, há um perfil a tirar.

---

## Resumo executivo

A base visual é sólida e a doutrina do design system está escrita
([design-system.md](reference/design-system.md)) — o problema não é gosto, é
**física e disciplina de camadas**.

1. **A app é servida sem compressão.** Nem `deploy/k8s/nginx.conf` nem
   `deploy/nginx-delonix.conf` ligam `gzip`, e a imagem base do nginx traz a
   diretiva comentada. O primeiro carregamento transfere **~826 KB** em vez dos
   **~227 KB** que os mesmos ficheiros dão comprimidos. É uma linha de config
   com o maior retorno do relatório inteiro.
2. **Tudo vive num chunk só.** 648,82 KB de JS e 177,63 KB de CSS num par de
   ficheiros: a landing pública, a sala de 4 254 linhas, o calendário, as
   análises e os três idiomas carregam **antes de se ver o dashboard**.
3. **O rail de 224px nunca colapsa sozinho.** Nenhuma media query toca em
   `.shell-nav`. Num telemóvel de 375px a navegação ocupa 60% da largura, e o
   único modo de a recolher é um botão que o utilizador tem de descobrir.
4. **O CSS é uma pilha de overrides por posição.** `.dash-card` está definido
   duas vezes — linha 2181 e linha 4392 — e é a segunda que ganha *por estar
   mais abaixo no ficheiro*. Isso está documentado como técnica deliberada; o
   custo é que 177 KB carregam declarações que nunca pintam.

---

## 1. Carregamento — o que se transfere antes do primeiro pixel

| # | Achado | Medida | Recomendação | Pri |
|---|---|---|---|---|
| 1.1 | **Sem gzip/brotli** em qualquer um dos dois nginx | JS 648,82 KB → 194,55 KB gz · CSS 177,63 KB → 32,78 KB gz | `gzip on` + tipos, ou `brotli` no `nginx.conf` do pod e no de produção | **P0** |
| 1.2 | **Zero code-splitting por rota** — `App.tsx:3-17` importa as 15 páginas estaticamente | 1 chunk de 648,82 KB | `React.lazy` por rota + `<Suspense>`; a `Room` (185 KB de fonte) e a `Landing` (13,5 KB) nunca coexistem no mesmo ecrã | **P0** |
| 1.3 | **Três idiomas no arranque** — `i18n.ts` traz `pt`, `en` e `fr` inline | 98,9 KB de fonte (pt 32,8 · en 31,1 · fr 34,4); dois nunca são lidos numa sessão | Um ficheiro por locale + `import()` no `setLanguage`; só o ativo entra no chunk | **P0** |
| 1.4 | **CSS único e bloqueante** para todas as vistas | 177,63 KB · 4 603 linhas · 1 866 blocos de regra | Separar o CSS da sala (o maior bloco) e o da landing do CSS da consola; segue o mesmo corte do 1.2 | **P1** |
| 1.5 | **Seis pesos de fonte, nenhum pré-carregado** — `main.tsx:5-10` | Sans 400/500/600/700 + Mono 400/500 | `<link rel="preload">` ao Sans 400 e 600 latinos; largar o 500 ou o 600 (a consola não usa quatro pesos de peso próximo) | **P1** |
| 1.6 | **`index.html` não tem `<meta name="description">` nem `lang` dinâmico** | `lang="pt"` fixo com i18n PT/EN/FR | Sincronizar `documentElement.lang` com o locale ativo — é a11y, não SEO | **P2** |

> **Ordem de execução.** 1.1 é config e vale ~600 KB por visita. 1.2 e 1.3 são a
> mesma refatoração e valem mais um corte grande no chunk de entrada. Fazer 1.1
> primeiro, medir, e só depois decidir se 1.4 ainda se justifica.

---

## 2. Runtime — a sala, onde a UI tem de aguentar 60 fps

`Room.tsx` são **4 254 linhas num único componente** com **114 `useState`**,
**38 `useEffect`** e **6 `setInterval`** — três deles a **1 Hz** (`pollNow:402`,
`now:475`, `elapsed:571`).

| # | Achado | Porque custa | Recomendação | Pri |
|---|---|---|---|---|
| 2.1 | **Três timers de 1 s a fazer `setState` na raiz da sala** | Cada tique reconcilia a árvore inteira — incluindo **todos os `<RemoteTile>`** e os seus `<video>` | Isolar cada relógio no componente que o mostra (`<Elapsed/>`, `<PollCountdown/>`); a raiz deixa de saber as horas | **P0** |
| 2.2 | **Nenhum tile é memoizado** — `memo(` aparece 0 vezes em `Room.tsx` | `RemoteTile:4179`, `PresentationTile:3950` e `Ctrl:3731` re-renderizam a cada tique de 2.1 | `React.memo` nos três + `useCallback` nos `onPin`/`onMute`/`onKick` | **P0** |
| 2.3 | **`style` inline com identidade nova a cada render** — `RemoteTile` recebe `style={style}` de `useGridLayout` | Anula qualquer memoização que se acrescente em 2.2 se não for estabilizada | Passar `w`/`h` como números e montar o `style` com `useMemo` dentro do tile | **P1** |
| 2.4 | **A grelha de vídeo é calculada em JS** — `useGridLayout:83` faz `ResizeObserver` + varrimento de colunas + `width`/`height` inline | Reflow a cada resize; a grelha nativa faz isto sem JS | `display:grid` + `grid-template-columns: repeat(auto-fit, minmax(…))` + `aspect-ratio:16/9`; o JS fica só para escolher o nº de colunas | **P1** |
| 2.5 | **`transition: all` em `a`, `input` e `button` globais** (`styles.scss:33,48,64`, 11 ocorrências) | Transiciona propriedades de layout, não só de pintura | Enumerar: `transition: background-color, border-color, color` | **P1** |
| 2.6 | **`button:active { transform: scale(0.97) }` global** (`styles.scss:67`) | Toca em **todos** os botões, incluindo os itens do rail que a camada CONSOLA declarou explicitamente sem movimento | Remover do global; se se quiser o toque, é numa classe do tier de ação | **P2** |
| 2.7 | **57 `catch` vazios nas páginas** (Room 27 · Calendar 10 · Whiteboards 4 · Home 4 · …) | Não é performance — é diagnóstico: quando a sala degrada, não fica rasto | Pelo menos `console.warn` com contexto nos caminhos de media e de sinalização | **P1** |

---

## 3. Layout — profissional, simples, moderno

### 3.1 Responsivo

| # | Achado | Medida | Recomendação | Pri |
|---|---|---|---|---|
| 3.1.1 | **O rail não tem comportamento móvel.** Nenhuma media query altera `.shell-nav`; a largura é 224px em qualquer viewport | 224px de 375px = **60% do ecrã** | Abaixo de 900px: rail vira gaveta sobreposta (`position:fixed` + backdrop), com um botão no `.app-bar`. O `collapsed` do localStorage é preferência de desktop, não substituto disto | **P0** |
| 3.1.2 | **Nove breakpoints sem escala**: 640, 720, 760, 800, 860, 900, 980, 1000, 1100 | 21 media queries no total | Três degraus (`640` · `900` · `1200`) em variáveis SCSS; converter caso a caso ao tocar em cada bloco | **P1** |
| 3.1.3 | **14 usos de `100vh`** — `.shell`, `.auth-page`, `.lobby-page`, `.prejoin-page`, `#root` | Em iOS/Android a barra do browser corta o rodapé; o `#root { height:100vh }` colide com o `.shell-main { overflow:hidden }` | `100dvh` com fallback `100vh` | **P1** |
| 3.1.4 | **A `.app-bar` esconde a data e o campo de código abaixo de 860px** (`styles.scss:4378`) | Entrar por código deixa de existir em telemóvel | Mover para a gaveta do 3.1.1 em vez de `display:none` | **P1** |

### 3.2 Coerência do sistema

| # | Achado | Medida | Recomendação | Pri |
|---|---|---|---|---|
| 3.2.1 | **Regras duplicadas que só se resolvem por posição no ficheiro** | `.dash-card` em 2181 **e** 4392 · `.dash-grid` em 2178 **e** 4391 · `.dash-greet` em 2177 **e** 4387 | Apagar a versão histórica em vez de a sobrepor. A camada CONSOLA é o destino, não um patch | **P1** |
| 3.2.2 | **54 `!important`** | — | Cada um é uma cascata que se perdeu; converter ao tocar no bloco | **P2** |
| 3.2.3 | **22 valores de `z-index` sem escala**: 0…4, 10, 15, 20, 22, 25, 30, 35, 40, 60, 100, 200, 210, 220, 300, 4000, 4001, 4200 | — | Cinco degraus tokenizados (`--z-base/dropdown/overlay/modal/toast`) | **P2** |
| 3.2.4 | **Cores fora dos tokens**: 74× `#fff`, 15× `#eda33b` (dourado da marca), 9× `#c8201d` (vermelho da marca) no SCSS; 13 hex em TSX | O DS proíbe isto explicitamente | `#fff` sobre superfície escura → `--sb-text`/`--text`; os hex de marca → `--wordmark`/`--brand` | **P1** |
| 3.2.5 | **170 emojis usados como ícones** (📌 ✋ 📅 ▶ ⌕ ◐ 🔒 …) a par do conjunto SVG de `icons.tsx` | Renderizam diferente por SO, não herdam `currentColor`, não escalam com o `--ctl-h` | Ampliar `icons.tsx` e trocar os de UI. Emoji fica onde é **conteúdo** (reações), não onde é **controlo** | **P1** |
| 3.2.6 | **Duas superfícies concorrentes para o tema**: `◐` na `.app-bar` e o `<select>` no drawer de definições | Duas fontes de verdade para a mesma preferência | Uma só — o toggle na barra, e o drawer aponta para lá | **P2** |

---

## 4. UX — o que o utilizador sente sem saber porquê

| # | Achado | Evidência | Recomendação | Pri |
|---|---|---|---|---|
| 4.1 | **Não existe um único estado de carregamento.** Zero ocorrências de `skeleton`/`shimmer` no codebase | `Home.tsx:37-60` dispara 3 pedidos e rende `dash-empty` até chegarem | Skeleton nas linhas do `dash-card`; um cartão vazio tem de significar «não há», nunca «ainda não sei» | **P0** |
| 4.2 | **Falha de API é indistinguível de lista vazia.** Os 3 `try/catch` da Home engolem o erro por desenho | Se a API cair, o dashboard aparece completo e vazio | Terceiro estado: carregando · vazio · falhou-com-retry. O `EmptyState` já aceita `action` | **P0** |
| 4.3 | **11 `outline: none`**, vários sem anel de substituição | `.join-box input:874`, `.cmd-search input:3815`, `.app-bar-join input:4370`, `:1218`, `:3071`, `:3544` | Anel tokenizado (`--ring`) em todos; navegação por teclado não pode ficar cega no Cmd-K | **P0** |
| 4.4 | **Três `aria-live`/`role="status"` na app inteira** | `EmptyState`, `cc-overlay`, `rec-start-toast` | O «✓ Guardado» do drawer, o erro da `.app-bar` e os toasts de presença precisam de região viva | **P1** |
| 4.5 | **Strings PT fixas dentro de componentes já traduzidos** | `Shell.tsx:74-75` (opções do tema), `:231-232`, `:254`, «Nome de utilizador», «Guardar alterações», «Marca» | Em EN/FR o drawer de definições fica meio traduzido | **P1** |
| 4.6 | **A `.app-bar` mostra data e hora estáticas** — `Shell.tsx` calcula `new Date()` no render, sem timer | Uma sessão longa mostra a hora de quando se entrou | Ou atualiza de minuto a minuto, ou mostra só a data | **P2** |

---

## Plano sugerido — três lotes

**Lote 1 · config e cortes (dias, sem risco de UI)** — ✅ FECHADO
1.1 gzip · 1.3 idiomas sob `import()` · 1.2 rotas em `React.lazy` · 4.3 anel de foco.
Fecha o maior custo de carregamento e a lacuna de a11y mais grave sem tocar em pixels.

**Lote 2 · layout (a parte visível)** — ✅ FECHADO
3.1.1 gaveta móvel · 3.1.3 `dvh` · 3.1.4 ações na gaveta · 4.1/4.2 skeleton e estado de falha ·
3.2.5 troca dos emojis de controlo. É este lote que muda a leitura de «simples e moderno».

**Lote 3 · sala e higiene** — ✅ FECHADO
Perfil real da `Room` primeiro (o que a §2 não tem), depois 2.1 → 2.4 pela ordem que
o perfil indicar · 3.2.1 duplicados · 3.2.4 cores.

---

## Invariantes a fixar depois de corrigir

Para o [catálogo de regressões](reference/regressions.md), quando os lotes fecharem:

- O chunk de entrada não passa de **250 KB comprimidos** (fitness function no CI).
- Nenhuma media query nova fora dos três degraus declarados.
- `.shell-nav` tem comportamento definido abaixo de 900px — um teste que meça a
  largura do rail a 375px.
- Zero `outline: none` sem `box-shadow` de foco na mesma regra.
