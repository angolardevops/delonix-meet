# Design System — Delonix Meet

A UI actual foi **reconstruída de raiz a partir do template navegável** «Delonix Meet
Navegavel2.html» (23 ecrãs: sistema de design, entrada, início, agenda, pré-entrada, sala em
grelha, orador e chat, estúdio de emissão, quadro, chamadas, gravações, integrações,
moderação, administração, telemóvel, linha de tempo, legendas, leitor, UML, BPMN, quadro no
palco, exportações, inteligência e idiomas). Nada da UI anterior foi reaproveitado; a lógica
sem UI (`api.ts`, `webrtc.ts`, `signaling.ts`, `media.ts`, `e2ee.ts`, `studio/*.ts`, …) ficou.

Resumo dos tokens e regras: [`HARNESS.md` §5](../../HARNESS.md). Este ficheiro diz **onde
está cada peça** e **como se prova a fidelidade**.

## 1. Peças

| Peça | Ficheiro |
|---|---|
| Tokens claro/escuro, `.dx-stage` sempre escuro, raios, espaçamento, tipografia | `web/src/ui/tokens.css` |
| Reset, foco, kit em CSS (`dx-btn`, `dx-card`, `dx-badge`, `dx-field`, `dx-table`, `dx-dialog`, …) | `web/src/ui/base.css` |
| Kit em React | `web/src/ui/kit.tsx` |
| Ícones SVG e símbolo Delonix | `web/src/ui/icons.tsx` |
| Marca (respeita o nome da aplicação: `isMarcaDeOrigem`) | `web/src/components/BrandMark.tsx` |
| Consola: rail, gaveta, barra de página, paleta, definições, chamadas a entrar | `components/Shell.tsx`, `PageBar.tsx`, `CommandPalette.tsx`, `SettingsDialog.tsx`, `PresenceProvider.tsx`, `ui/shell.css` |
| Dados do servidor em três estados | `components/AsyncSection.tsx` |
| Folhas por área (no chunk da página) | `web/src/ui/<área>.css` |
| Dicionários por área | `web/src/locales/<pt\|en\|fr>/<área>.ts` |

## 2. Temas

- **Consola** (entrada, início, agenda, gravações, gestão): claro por omissão, escuro opcional
  (`data-theme` no `<html>`, `web/src/theme.ts`).
- **Palco** (sala, pré-entrada, estúdio de emissão, editor, legendas, leitor, diagramas):
  `.dx-stage`, escuro em qualquer tema.

## 3. Fidelidade ao template

A referência de cada ecrã é uma captura a 1440×900 do template (e o telemóvel a 390×844),
comparada lado a lado com a app com dados reais. Diferenças aceites: dados reais; elementos
cujo backend não existe (não se simulam); o rail de navegação da consola. Tudo o resto é
defeito.

Armadilha medida: aberto como `file://`, o Chromium recusa os `blob:` do runtime do template e
os ciclos (`sc-for`, `{{ }}`) ficam por preencher — as referências têm de ser capturadas com o
ficheiro servido por HTTP local.

## 4. Checklist de revisão UI

- Controlos saem do kit; nenhuma cor/raio/altura à mão; vermelho da marca só como token.
- Badges de estado com forma/ícone + texto.
- Teclado: tudo operável, `aria-label` em botões só de ícone, foco visível.
- 375 px: sem scroll horizontal; grelhas colapsam; tabelas em `dx-table-wrap`.
- i18n: zero texto fora do `t()`; pt/en/fr com as mesmas chaves; sem emoji.
- Pedidos abortáveis, erros por `apiErrorMessage`.
- Nada no ecrã que o servidor não faça.
