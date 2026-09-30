# Template navegável v5 («Navegavel5»)

O desenho de origem dos ecrãs do Delonix Meet, geração v5 — a do Estúdio-TV. **Entrou no
repositório a 2026-09-30 porque não existia em mais lado nenhum:** viveu catorze dias numa
pasta temporária (`.worktrees/delonix-meet/notas-ui-template/`), e ao contrário das
gerações anteriores **não há um `Navegavel5.html` consolidado** em nenhum arquivo. Uma
limpeza distraída levava-o.

## O que está aqui

| Pasta | O quê | Estado |
|---|---|---|
| `origem/` | os 38 HTML de autoria — 36 ecrãs + `DelonixNav` e `DelonixListControls`, que são componentes e não ecrãs | a fonte |
| `render/` | 36 capturas PNG dos mesmos ecrãs | **a única renderização fiel que sobrevive** — ver o aviso abaixo |
| `texto/` | 36 extractos do texto visível de cada ecrã | útil para conteúdo e i18n |

## O aviso: nenhum dos dois abre hoje tal e qual

Os ficheiros de `origem/` carregam um `./support.js` que **não existe** — nem aqui, nem no
arquivo de onde vieram. É ele que resolve os elementos próprios do formato (`<x-dc>`,
`<helmet>`, `<dc-import name="…">`). Sem ele, um browser mostra o conteúdo — a marcação
e os estilos estão todos em linha — mas **não injecta os componentes importados**: onde
devia estar a barra lateral, fica um `<dc-import>` vazio. Os componentes existem à parte,
em `origem/DelonixNav.html` e `origem/DelonixListControls.html`.

A captura que a pasta original tinha do mesmo HTML *com* o carregador injectado não veio:
trazia URLs `blob:http://127.0.0.1:41121/…` de um servidor local que já não existe, o que
a torna mais quebrada do que a origem, não menos.

**É por isso que os PNG entram apesar dos 4,8 MB.** Não são um derivado que se regenera
abrindo o HTML — hoje, são a única forma de ver o ecrã como foi desenhado.

## Para que serve

É a referência de **fidelidade**: um ecrã construído compara-se com o desenho. O sistema
que dele saiu está em [`../../reference/design-system.md`](../../reference/design-system.md)
— e os valores batem certo: o `--accent:#ad1017` que os estilos em linha destes ficheiros
usam é o mesmo do `web/src/ui/tokens.css`.

Duas regras que continuam a valer, e que estão no design system:

- **o template mostra coisas que não têm backend** (PSTN, dobragem, exportações, motores
  de IA, multistream para cinco destinos…). Desenhar um ecrã para um endpoint que não
  existe é inventar capacidade — mede em `api.ts`/`server/src` antes;
- **números no ecrã vêm do servidor**, nunca do template.

## Se voltar a aparecer um `support.js`

Põe-no em `origem/` e apaga este parágrafo. Enquanto não aparecer, é honesto dizer que o
template não se navega — só se lê.
