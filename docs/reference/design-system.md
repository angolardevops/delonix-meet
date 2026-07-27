# Design System — Delonix Meet

> Fonte de verdade para UI. Regra de ouro: **um controlo novo nunca inventa
> tamanho, raio ou cor** — usa o kit (`web/src/components/ui.tsx`) e os tokens.
> Qualquer exceção é uma alteração AO SISTEMA (aqui + tokens + kit), não à página.

## 1. Tokens

Duas fontes, sempre coerentes entre si:

| Onde | O quê |
|---|---|
| `web/src/styles/tokens.scss` (`$base` + temas) | cores por tema, fontes, `radius` base — emitidos como CSS vars |
| `styles.scss` bloco ":root — Design system" | escala aditiva: `--space-*`, `--radius-sm/md/lg`, `--shadow-*`, `--ctl-h` |

Valores atuais (27/07/2026):

```
--radius-sm: 4px   controlos (botões, inputs, selects, chips)
--radius-md: 6px   superfícies (cartões, painéis, modais, toasts)
--radius-lg: 8px   destaques (auth-card, hero)
--ctl-h:    30px   altura única dos controlos pequenos
html font-size: 15px   ← botão ÚNICO de densidade (tudo dimensiona em rem)
```

### 1.1 Cor: AÇÃO ≠ MARCA

| Papel | Tokens | Onde |
|---|---|---|
| **Ação** (índigo) | `--accent`, `--accent-hi`, `--accent-text`, `--accent-soft` | botões primários, foco, links, nav ativo, chips de estado |
| **Marca** (vermelho + dourado) | `--brand`, `--brand-hi`, `--grad-brand`, `--wordmark`, `--accent-2` | logo, quadrado da sidebar, «Meet» do wordmark, landing |

Nunca trocar os papéis: vermelho em navegação lê-se como destrutivo, e índigo
no logo apaga a identidade. Feedback tem tokens próprios (`--ok`, `--warn`,
`--danger`) — não reaproveitar a marca para estados.

### 1.2 Barra lateral: escura nos dois temas

Os tokens `--sb-bg` / `--sb-text` / `--sb-line` / `--sb-hover` são
deliberadamente escuros também no tema claro (navy `#1e2a45`). **Não** os fazer
seguir o tema: o rail é âncora de identidade e, escuro, deixa de competir com o
conteúdo. Tudo o que viva dentro de `.shell-nav` herda destes tokens — um
componente novo no rail usa `--sb-*`, nunca `--surface`/`--text`.

### 1.3 Separação por linha, não por sombra

`--shadow` é 1px. A hierarquia entre superfícies vem da **luminância**
(`--bg` → `--surface` → `--surface-3`) e de uma borda de 1px. Dentro de um
cartão, os separadores usam `--border-soft` (mais leve que `--border`) — é o
que impede que uma lista densa pareça uma grelha.

## 2. Os 3 tiers (camada "SISTEMA DE CONTROLO ÚNICO", fim de `styles.scss`)

A camada vive de propósito no FIM do ficheiro — à mesma especificidade vence os
valores hardcoded históricos. **Não adicionar novos `border-radius`/alturas
hardcoded**: se um elemento novo não cair num tier, é o tier que se estende.

1. **Ação** — `.btn-sm` (+ `ghost|danger|success`), `.chip-btn`, `.seg-btn`,
   `.admit-accept/.admit-deny`, `.integ-tab`, `.auth-tab`… altura `--ctl-h`,
   padding 0 12px, fonte 0.82rem, raio `--radius-sm`.
2. **Botão-ícone** — `.icon-btn`, `.lobby-deny`, `.poll-correct-pick`:
   quadrado `--ctl-h`×`--ctl-h`.
3. **Superfícies** — `.dash-card`, `.kpi-card`, `.rec-*`, `.poll-card`,
   `.admit-card`, `.modal`, `.side-panel`, `.toast`… raio `--radius-md`.

## 2.1 Camada CONSOLA (27/07/2026) — a ÚLTIMA do ficheiro

Vem **depois** do bloco de controlo único; à mesma especificidade, ganha. É
onde vive o alinhamento com o template de consola. Se um valor desta camada
entrar em conflito com um mais acima, a correção é aqui — não duplicar a regra
no meio do ficheiro.

O que a camada define:

- **Densidade** — `html { font-size: 15px }`. A app dimensiona quase toda em
  `rem`; a raiz é o botão único. Não apertar tamanhos página a página.
- **Rail** — 224px, tokens `--sb-*`, item ativo com `box-shadow: inset 2px 0 0`
  (sem pseudo-elemento nem glow) e **sem** `translateX` no hover: num rail
  denso o deslize lê-se como instabilidade.
- **`.app-bar`** — barra de aplicação no topo do conteúdo, montada em
  `Shell.tsx`: data, tema, «Nova reunião» e campo de código. Estas ações
  **saíram da Home**; não voltar a duplicá-las lá.
- **Estrutura do Shell** — `.shell-main` (flex column, `overflow: hidden`) →
  `.app-bar` + `.shell-body` (o elemento que faz scroll). Uma página de altura
  total dentro do Shell usa `height: 100%`, **nunca `100vh`**: a barra já ocupa
  ~46px e a página ficaria com scroll parasita.
- **Cartões** — cabeçalho `10px 14px` com borda inferior, linhas `9px 14px`
  separadas por `--border-soft`, `padding: 0` no cartão (o cabeçalho e as linhas
  trazem o seu).
- **Sala** — ver §4.1.

## 3. Kit de componentes (`web/src/components/ui.tsx`)

Wrappers finos sobre o CSS (zero estilo próprio → zero divergência):

```tsx
import { Btn, IconBtn, Card, Field, TextInput, SelectCtl, Switch } from '../components/ui'

<Btn onClick={…}>Guardar</Btn>                 // primário (accent)
<Btn variant="ghost">Cancelar</Btn>             // secundário
<Btn variant="danger">Apagar</Btn>              // destrutivo
<Btn variant="success">Admitir</Btn>            // positivo (verde)
<IconBtn title="Fechar"><CloseIcon /></IconBtn> // quadrado 30×30
<Card title="Membros" actions={<Btn…/>}>…</Card>
<Field label="Microfone" hint="opcional"><SelectCtl>…</SelectCtl></Field>
<Switch checked={on} onChange={setOn} ariaLabel="Bloquear reunião" />
```

**Migração**: código novo usa SEMPRE o kit. Código existente migra
oportunisticamente — quando tocares num bloco, converte os botões dele
(referência: painel Ferramentas em `Room.tsx`). Variante nova = classe no CSS
(+tier) e entrada no `BTN_CLASS` do kit — nunca um `className` ad-hoc na página.

## 4. Temas

Temas são **mapas de tokens**, nunca overrides espalhados:

1. `styles/tokens.scss`: cria o mapa (`$meu-tema`) a partir dos primitivos
   (`_primitives.scss`) — cores semânticas (`bg`, `surface*`, `text`, `accent*`,
   `ok`, `danger`…). Copia a estrutura de `$dark`/NgolaCloud.
2. Emite-o sob `[data-theme='meu-tema']` como os existentes.
3. Regista no `ThemePicker` (Shell/Room Definições) + persiste em `dx_theme`.
4. **A sala ignora temas claros**: `.room-page`/`.waiting-page` reafirmam os
   tokens dark com `!important` no fim de `styles.scss` — NÃO contornar (regra
   de marca: a sala é sempre escura). Ver §4.1 para o chrome atual.
5. Testar SEMPRE: Landing, Login, Home, sala (deve ficar escura), Analytics,
   modais — nos temas claro E escuro (o tema claro a "vazar" dark já foi uma
   regressão inteira, tarefa #67).

### 4.1 Chrome da sala (27/07/2026)

A sala mantém-se sempre escura, mas deixou o cinza-Meet (`#202124`, controlos
em círculos de 50px) e passou ao cinza frio da consola:

```text
fundo         #0d0f14      barras (topo/controlos)  #12141a   linha  #20242e
palco         linear-gradient(160deg, #1b2030, #12141c)
tiles         linear-gradient(150deg, #242a3d, #161a26)
painel        #14161d · 320px · encostado (sem margem flutuante)
```

- **Controlos**: quadrados de 38px, raio `--radius-sm`, agrupados em
  `.ctrl-group` — um grupo DISPOSITIVOS (mic/câmara) e um grupo SESSÃO (CC,
  reações, partilha, mão, gravar, mais); o botão de terminar fica solto, a
  52×38. O agrupamento é só markup em `Room.tsx` — **nenhuma lógica de media
  depende dele**.
- **Chevron de dispositivo**: caret de 15px no canto inferior direito do botão
  (era metade de uma pill). Continua a abrir a lista de microfones/câmaras.
- **Avatares** (`.avatar-circle`, `.tile-avatar`): gradientes frios. O
  vermelho/dourado é da marca — num tile de vídeo só criava ruído quente.
- **Terminar**: vermelho chapado, sem gradiente nem halo — a cor já é o sinal e
  o glow competia com o estado «a falar» do microfone.

## 5. Checklist de revisão UI (para humanos e agentes)

- [ ] Botão/input/cartão novo usa o kit ou classes de tier — sem CSS de tamanho/raio na página
- [ ] Zero `border-radius`/`height` hardcoded novos em `styles.scss` fora da camada de sistema
- [ ] Cores só via tokens (`var(--…)`) — nunca hex na página
- [ ] Índigo = ação, vermelho/dourado = marca (§1.1) — sem trocas
- [ ] Componente no rail usa `--sb-*`, não `--surface`/`--text` (§1.2)
- [ ] Página de altura total dentro do Shell usa `height: 100%`, não `100vh`
- [ ] Ecrãs verificados nos 4 temas + a sala continua escura
- [ ] i18n PT/EN/FR para strings novas
