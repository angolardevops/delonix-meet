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

Tudo o que ele exporta, medido a 2026-09-30:

`Button` · `IconButton` · `Card` · `SectionHead` · `Tag` · `StatusBadge` · `Field` ·
`TextInput` · `TextArea` · `Select` · `Checkbox` · `Toggle` · `Segmented` · `Tabs` ·
`Avatar` · `AvatarStack` (+ `avatarTone`, `initials`) · `Meter` · `Empty` · `Alert` ·
`Spinner` · `Skeleton` · `Dialog` · `cx`.

Ícones em `web/src/ui/icons.tsx`: `<Icon name=… />` e `<DelonixSymbol />`. Um ícone novo
entra **imediatamente antes do `}` que fecha o mapa `P`**, debaixo de uma linha `// <área>`
— os conflitos resolvem-se na integração.

Classes utilitárias em `ui/base.css`: `dx-num` (mono tabular), `dx-eyebrow`, `dx-kv`,
`dx-chips`/`dx-chip`, `dx-toasts`/`dx-toast`, `dx-table`/`dx-table-wrap`.

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
