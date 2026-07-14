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

Valores atuais (14/07/2026):

```
--radius-sm: 4px   controlos (botões, inputs, selects, chips)
--radius-md: 6px   superfícies (cartões, painéis, modais, toasts)
--radius-lg: 8px   destaques (auth-card, hero)
--ctl-h:    30px   altura única dos controlos pequenos
```

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
   de marca: a sala é sempre escura, estilo Meet).
5. Testar SEMPRE: Landing, Login, Home, sala (deve ficar escura), Analytics,
   modais — nos temas claro E escuro (o tema claro a "vazar" dark já foi uma
   regressão inteira, tarefa #67).

## 5. Checklist de revisão UI (para humanos e agentes)

- [ ] Botão/input/cartão novo usa o kit ou classes de tier — sem CSS de tamanho/raio na página
- [ ] Zero `border-radius`/`height` hardcoded novos em `styles.scss` fora da camada de sistema
- [ ] Cores só via tokens (`var(--…)`) — nunca hex na página
- [ ] Ecrãs verificados nos 4 temas + a sala continua escura
- [ ] i18n PT/EN/FR para strings novas
