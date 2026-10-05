# Design System — Delonix Meet

> **Fonte de verdade para UI.** Regra de ouro: **um controlo novo nunca inventa tamanho,
> raio ou cor** — usa o kit (`web/src/ui/kit.tsx`) e os tokens (`web/src/ui/tokens.css`).
> Qualquer excepção é uma alteração AO SISTEMA (aqui + tokens + kit), não à página.

> **Reescrito a 2026-09-30.** A versão anterior descrevia a UI que a reescrita da consola
> (#119/#120) apagou: mandava ir buscar controlos a `web/src/components/ui.tsx` e tokens a
> `styles/tokens.scss` e `styles.scss` — **os três ficheiros já não existem**. Pior, a
> regra central estava invertida: dizia «acção = índigo, marca = vermelho», e hoje o
> `--accent` É vermelho (`#ad1017`), da mesma família da marca. Tudo o que está abaixo foi
> medido na árvore, não recordado.

## 0. O desenho de origem

O template navegável v5 está em [`../templates/v5/`](../templates/v5/README.md): os HTML de
autoria, as capturas de cada ecrã e os extractos de texto. É a referência de **fidelidade**
— um ecrã construído compara-se com ele. Os estilos em linha desses ficheiros usam os
mesmos valores que os tokens abaixo (`--accent:#ad1017`), o que é a prova de que os dois
não andaram à parte.

## 1. Tokens — `web/src/ui/tokens.css`

Uma só fonte, 70 tokens, em três blocos:

| Selector | Para quê |
|---|---|
| `:root, [data-theme='light']` | tema claro (omissão) |
| `[data-theme='dark'], .dx-stage` | tema escuro **e** as superfícies que são sempre escuras |
| `:root:lang(zh)` | ajustes tipográficos do chinês |

**`.dx-stage` reafirma o escuro qualquer que seja o tema** — sala, pré-entrada e estúdio
de emissão. Não se «clareia» um palco.

```
--r-2: 2px · --r-3: 4px · --r-8: 8px      raios
--ctl-h: 32px · --ctl-h-lg: 42px          altura única dos controlos
--accent / --accent-strong / --accent-pressed    acção (vermelho, #ad1017 no claro)
--brand / --brand-ink                     marca
--warning · --danger · --stage            estados e palco
```

Os contrastes estão medidos e escritos no topo do ficheiro (texto 15,4:1, esbatido 5,2:1,
acento 7,7:1 no claro; 15,9 / 6,7 / 5,8 no escuro). **Um valor novo mede-se antes de
entrar.**

## 2. Kit — `web/src/ui/kit.tsx`

Tudo o que ele exporta, medido a 2026-10-05:

`Button` · `IconButton` · `Card` · `SectionHead` · `Tag` · `StatusBadge` · `Field` ·
`TextInput` · `TextArea` · `Select` · `Checkbox` · `Toggle` · `Segmented` · `Tabs` ·
`Avatar` · `AvatarStack` (+ `avatarTone`, `initials`) · `Meter` · `Empty` · `Alert` ·
`Spinner` · `Skeleton` · `Dialog` · `cx`.

O `Segmented` é o selector de VISTAS da app (o do Estúdio, o do editor, o das fontes):
leva `className` para a pele de cada área, `dataKey` (escreve `data-<chave>="<valor>"` em
cada segmento, que é por onde o e2e agarra uma vista sem depender da língua), `title` e
`disabled` por opção, e **as setas andam pelos segmentos** — num grupo de botões o Tab
salta para fora, e quem navega por teclado não tinha como percorrer as vistas.

**Menu de contexto — `web/src/ui/Menu.tsx`:** `useMenuDeContexto()` e `<Menu>`, mais a
função pura `posicaoDoMenu` (dobra junto às bordas, encosta quando nem dobrado cabe).
O botão direito do rato é a única forma de chegar às acções de uma linha num ecrã táctil,
onde não há `hover`. Está ligado nos retratos da sala, nas duas vistas da biblioteca e na
lista de contactos; o portão vive em `ui/Menu.test.ts`.

Ícones em `web/src/ui/icons.tsx`: `<Icon name=… />` e `<DelonixSymbol />`. Um ícone novo
entra **imediatamente antes do `}` que fecha o mapa `P`**, debaixo de uma linha `// <área>`
— os conflitos resolvem-se na integração.

Classes utilitárias em `ui/base.css`: `dx-num` (mono tabular), `dx-eyebrow`, `dx-kv`,
`dx-chips`/`dx-chip`, `dx-toasts`/`dx-toast`, `dx-table`/`dx-table-wrap`,
`dx-kbd`/`dx-keys` (folha de atalhos), `dx-menu` (menu de contexto).

## 2.1 Atalhos de teclado — `web/src/ui/atalhos.ts`

**Um atalho novo declara-se no CATÁLOGO, nunca num `addEventListener` de uma página.**
Antes havia quinze ouvintes de `keydown` espalhados e nenhum ecrã sabia dizer que teclas
tinha: as cinco vistas do Estúdio só se alcançavam com o rato, e duas delas só de dentro
do editor.

- `CATALOGO_DE_ATALHOS` — `id`, escopo (`global`, `sala`, `estudio`, `mesa`, `quadro`),
  combinação (`mod+shift+2`, onde `mod` é o Ctrl ou o ⌘) e a chave i18n do rótulo. Onde já
  existe um rótulo neutro do botão que o atalho aciona, aponta-se ESSA chave.
- `combina(spec, evento)` e `escreverAtalho(spec)` são puras: os dígitos lêem-se do `code`
  (`Digit1`), nunca da `key`, porque com ⇧ a `key` de «1» é «!» ou «+» segundo o teclado.
- `useAtalhos(escopo, accoes)` liga-os num ecrã (um ouvinte por escopo, acções por `id`) e
  `useDicaDeAtalho()` dá a dica do `title` — «Edição · ⌘⇧2».
- A folha do «?» (`components/AtalhosDialog.tsx`) LÊ o catálogo e não tem lista própria.
- O portão (`ui/atalhos.test.ts`) prova que a gramática fecha, que ninguém pisa ninguém
  dentro de um escopo nem rouba um atalho global, que cada rótulo existe nas quatro
  línguas, e que **a mesa de corte ignora as vistas do Estúdio** — é por isso que se pode
  mudar de vista com a mesa no ar.

## 2.2 As peças por área — não voltes a escrever a caixa à mão

Três famílias de marcação estavam copiadas em cento e vinte sítios. As classes são as
mesmas; o que mudou é que a caixa agora é um componente.

| Peças | Onde | O que substituem |
|---|---|---|
| `room/Bloco.tsx` | painéis da sala | 13 × `<section className="rm-block" aria-labelledby="rm-xx-h">` + `<h3 id=…>` escrito à mão (o `useId` acaba com dois `id` iguais quando dois painéis estão montados) |
| `pages/studio/tv/pecas.tsx` | os cinco ecrãs de TV | `Cartao` (25), `Cabeca` (22) e `BotaoTv` (22), com as variantes do template em propriedades |
| `studio/pecasDoEditor.tsx` | linha de tempo, legendas, exportações | `BotaoEd` (22), `CartaoEd` (13), `TituloEd`, `LigacaoEd` — o estado é `variante={ligado && 'on'}`, não um `cx()` na página |

O editor não usa o `dx-btn` do kit de propósito: é denso (26 px de altura contra 32) e
vive sobre a coluna `raised` do template.

## 3. Fundação — usa, não dupliques

| Ficheiro | O que dá |
|---|---|
| `components/AsyncSection.tsx` | `useAsync(load(signal), deps)` → `{state, reload, mutate}` e `<AsyncSection state onRetry>`. **Todo o carregamento passa por aqui** — abortável, três estados |
| `components/PageBar.tsx` | barra de topo de cada página da consola (`title`, `meta`, acções), inclui o botão da gaveta |
| `components/shellContext.ts` | `useShell()`: `user`, `org`, `orgs`, `isAdmin`, `navigate`, `enterRoom`, `openPalette`, `openSettings` |
| `components/BrandMark.tsx` | `BrandMark`, `BrandLockup` — **nunca** escrevas o nome ou `/logo.svg` à mão |
| `components/PresenceProvider.tsx` | `usePresence()`: `online`, `isOnline`, `startCall`, `missed`, `ackMissed`, `callBack` |
| `roomCode.ts` | `parseRoomCode(raw)` |

Corpo de página: `<div className="page">…</div>`.

## 4. Regras de código

1. **Ficheiros teus:** a página em `pages/`, sub-componentes em `pages/<área>/`, `room/`
   ou `studio/`, a folha `ui/<área>.css` **importada pela própria página** (fica no chunk
   lazy dela), e `locales/{pt,en,fr,zh}/<área>.ts`.
2. **Não edites** `ui/kit.tsx`, `ui/base.css`, `ui/tokens.css`, `ui/shell.css`,
   `components/*`, `App.tsx`, `main.tsx`, `i18n.ts`. Se o kit não chega, faz um componente
   local e di-lo no relatório.
3. **Sem cor, raio ou altura de controlo escritos à mão** fora dos tokens. Nenhuma cor de
   marca alheia.
4. **i18n:** zero texto visível fora do `t()` — JSX, `title`, `placeholder`, `aria-*`,
   template literals, `setStatus('…')`. Chaves `área.subgrupo.chave`, **cada valor numa só
   linha**. **As quatro línguas com exactamente as mesmas chaves** (`pt` é a origem, mais
   `en`, `fr`, `zh`); plurais com `_one`/`_other`. **Nenhum emoji** em JSX, atributos
   visíveis ou locales — usa `<Icon>`; emoji escolhidos pelo utilizador em runtime são
   permitidos.
5. **Pedidos:** `.catch` com `isAbort`; erros por `apiErrorMessage(e, t('…'))`.
6. **O frontend não decide política.** Um botão de anfitrião ou de admin escondido no
   cliente **não é autorização**: mostra o que o servidor devolve e trata o `403`.
7. **Acessibilidade e ecrã estreito:** tudo operável por teclado, `aria-label` em botões só
   de ícone, foco visível, layout funcional a **375 px** (grelhas colapsam para uma coluna,
   tabelas dentro de `dx-table-wrap`). `100vh` sempre seguido de `100dvh`.
8. **Não desenhes ecrãs para endpoints que não existem.** Mede em `api.ts` /
   `signaling.ts` / `server/src`: se não existe, **não aparece** — nem como botão inerte
   nem como número inventado. Números no ecrã vêm do servidor, nunca de um template. Lista
   o que ficou de fora no relatório.
9. **Identificadores novos em inglês**; comentários e textos em português europeu.

## 5. Portões

```bash
cd web && npx tsc --noEmit && npx vitest run
```

Nenhum teste da tua área pode ficar vermelho, e não podes pôr vermelho nenhum que estava
verde. **Os testes de invariantes reescrevem-se para o código novo mantendo o comportamento
protegido** — nunca se apagam nem se afrouxam; se um deixar de fazer sentido no desenho
novo, substitui-se por um equivalente e diz-se porquê no commit. Os e2e em `web/e2e/*.mjs`
que usam selectores da tua área actualizam-se para os novos.

**Layout e comportamento só se provam com browser real.** Arranca o Vite
(`NO_HTTPS=1 npx vite --port <porta> --strictPort --host 127.0.0.1`) e abre com o Playwright
do projecto a 1440×900 e a 375×812. Sem backend os pedidos falham — isso prova layout e
estados de erro/vazio, **não dados**, e diz-se isso no relatório.
