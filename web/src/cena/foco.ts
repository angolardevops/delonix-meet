/**
 * O FOCO DA CENA: quem o realizador escolheu vem primeiro.
 *
 * Uma função, pura, sem canvas nem `MediaStream` — como o `moldura.ts` ao lado,
 * e pela mesma razão: isto é a decisão de QUEM ocupa os lugares, e a geometria
 * ao lado é a de ONDE eles ficam. Juntas fazem a cena de marca.
 *
 * PORQUE VIVE AQUI, e não no `room/compositor.ts` onde nasceu: a coluna da
 * moldura é desenhada pelos DOIS compositores, e `room/` e `studio/` já se
 * importam nos dois sentidos (`studio/palco.ts` traz o `calcularRects` da sala;
 * `room/useMulticam.ts` traz o `directo` do estúdio). O Estúdio chamá-la de
 * `room/` fechava mais uma aresta nesse emaranhado; esta pasta é FOLHA — não
 * importa de nenhuma outra de `src/` — e por isso qualquer uma a pode importar
 * sem ciclo. Quem o impõe é a regra `no-restricted-imports` para `src/cena/**`
 * no `web/eslint.config.js`, que corre no `check-frontend-lint.sh` do CI.
 *
 * A sala RE-EXPORTA-A, como já faz com o `Rect`, para que nada a jusante tenha
 * de mudar de import por causa de uma mudança que não lhe diz respeito.
 */

/**
 * A lista TODA, reordenada para que quem está em foco venha primeiro.
 *
 * NÃO CORTA NADA, e é essa a razão de ser de uma função separada do
 * `participantesVisiveis` da sala, que corta ao número de lugares da cena. A
 * moldura precisa da lista inteira para o `rectsDaMoldura` saber quantas
 * câmaras ficaram de fora — se esta função devolvesse só três, o `omitidas`
 * vinha sempre a zero e o aviso no canto nunca aparecia. Quem corta é a
 * geometria, no sítio onde também conta o que sobra.
 *
 * O FOCO É ORDEM, não filtro: quem o realizador escolhe sobe aos primeiros
 * lugares, e os restantes seguem pela ordem em que estavam. Assim uma escolha
 * não expulsa ninguém da emissão por si — expulsa-o o tecto da coluna, que é
 * quem o anuncia.
 *
 * «Realizador» é o anfitrião na Sala (as fichas de foco do `MulticamPanel`) e o
 * operador no Estúdio (o `programaId`, o botão «Tornar programa»). São dois
 * nomes para a mesma escolha, e por isso uma função só.
 */
export function ordenarPorFoco<T>(todos: T[], focoIds: string[], idDe: (t: T) => string): T[] {
  const vistos = new Set<string>()
  const escolhidos: T[] = []
  for (const id of focoIds) {
    // `vistos` ANTES do `find`: um `focoIds` com o mesmo id duas vezes (o
    // `toggleFoco` não o produz hoje, mas é estado que vem de fora) punha a
    // mesma câmara em dois lugares da coluna e empurrava uma terceira pessoa
    // para fora do ar — a falha exacta que esta função existe para impedir.
    if (vistos.has(id)) continue
    const t = todos.find((x) => idDe(x) === id)
    if (!t) continue
    vistos.add(id)
    escolhidos.push(t)
  }
  return [...escolhidos, ...todos.filter((t) => !vistos.has(idDe(t)))]
}
