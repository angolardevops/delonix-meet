/**
 * A MOLDURA DE TRANSMISSÃO — a geometria de uma cena com marca, do género que
 * uma conferência põe no ar: uma faixa a rolar no topo, uma coluna lateral com
 * o logótipo em cima, as câmaras ao meio e a assinatura em baixo, e o conteúdo
 * (o ecrã partilhado, normalmente) grande ao lado.
 *
 * Aqui não há canvas, nem `MediaStream`, nem DOM. Só rectângulos e um
 * deslocamento. O desenho vive no irmão `desenhoDaMoldura.ts` — a mesma
 * separação que o `studio/tv/mesa.ts` e o `desenhoDaMesa.ts` já usam, e pela
 * mesma razão: sem jsdom neste repo, o que não for puro não se testa.
 *
 * PORQUE É UM MÓDULO À PARTE, e não mais uma função no `studio/palco.ts`:
 * esta cena serve os DOIS compositores — o do Estúdio (`studio/compositor.ts`)
 * e o da Sala (`room/compositor.ts`). O `studio/palco.ts` já importa o
 * `calcularRects` da sala, logo a dependência existente é estúdio → sala. Pôr a
 * cena no estúdio e chamá-la da sala fechava um CICLO. Esta pasta é folha: não
 * importa de `room/` nem de `studio/`, e por isso ambos a podem importar.
 *
 * É também daqui que sai o `Rect`, que antes nascia no `room/compositor.ts` —
 * a sala re-exporta-o para que nada a jusante tenha de mudar de import.
 */

export interface Rect {
  x: number
  y: number
  w: number
  h: number
}

// ------------------------------------------------------------------ proporções
//
// As fracções são da referência que deu origem à cena (uma transmissão de
// conferência em 16:9). São de IMAGEM, não de interface: vão para o vídeo como
// pixéis, e por isso não são tokens de tema — o modo escuro não pode mexer
// numa moldura já gravada. Tudo em FRACÇÃO, nunca em píxeis, para o 2160p sair
// idêntico ao 1080p.

/** Altura da faixa do topo, em fracção da altura do canvas. */
export const FRACCAO_DA_FAIXA = 0.058
/** Largura da coluna lateral, em fracção da largura útil. */
export const FRACCAO_DA_COLUNA = 0.28
/** Margem preta à volta do conteúdo, em fracção da ALTURA (para ser igual nos dois eixos). */
export const FRACCAO_DA_MARGEM = 0.028
/** Altura do logótipo no cimo da coluna, em fracção da altura da coluna. */
export const FRACCAO_DO_LOGO = 0.26
/** Altura da assinatura na base da coluna, em fracção da altura da coluna. */
export const FRACCAO_DA_ASSINATURA = 0.15
/** Quantas câmaras a coluna mostra antes de deixar de as repartir. */
export const MAXIMO_DE_CAMARAS = 3
/** Velocidade da faixa, em fracção da LARGURA do canvas por segundo. */
export const VELOCIDADE_DA_FAIXA = 0.05

export interface OpcoesDaMoldura {
  /** De que lado fica a coluna. A referência tem-na à esquerda. */
  lado: 'esquerda' | 'direita'
  /** Desligar a faixa do topo devolve o seu espaço ao conteúdo. */
  faixa: boolean
  /** Desligar o logótipo ou a assinatura dá o espaço às câmaras. */
  logo: boolean
  assinatura: boolean
}

export const OPCOES_INICIAIS: OpcoesDaMoldura = {
  lado: 'esquerda',
  faixa: true,
  logo: true,
  assinatura: true,
}

/**
 * Os rectângulos de uma moldura. O que estiver desligado vem a `null`, e quem
 * desenha testa o `null` em vez de adivinhar por tamanho zero. As `camaras`
 * podem vir em lista vazia: uma aula sem convidados é uma coluna só com marca,
 * não um buraco preto.
 */
export interface RectsDaMoldura {
  faixa: Rect | null
  heroi: Rect
  logo: Rect | null
  camaras: Rect[]
  assinatura: Rect | null
}

/**
 * A geometria da cena para `n` câmaras num canvas de `largura` × `altura`.
 *
 * A ordem em que o espaço se reparte, e porquê:
 *   1. a FAIXA corta do topo e não leva margem — é uma barra de ponta a ponta,
 *      como na referência, não um cartão;
 *   2. o resto leva margem igual nos quatro lados, calculada sobre a ALTURA —
 *      sobre a largura, um 21:9 ficaria com margens laterais gordas e topo
 *      fino;
 *   3. o que sobra divide-se em COLUNA e HERÓI, com um vão entre eles do
 *      tamanho da margem — sem vão, o ecrã encostava ao retrato;
 *   4. dentro da coluna empilham-se LOGO, CÂMARAS e ASSINATURA.
 *
 * Com `n <= 0` a coluna fica só com a marca e o herói NÃO cresce para cima
 * dela: a cena tem de ser estável quando alguém sai do directo, não saltar de
 * geometria a meio da emissão.
 */
export function rectsDaMoldura(
  n: number,
  largura: number,
  altura: number,
  opcoes: Partial<OpcoesDaMoldura> = {},
): RectsDaMoldura {
  const o = { ...OPCOES_INICIAIS, ...opcoes }
  const margem = altura * FRACCAO_DA_MARGEM

  const faixa: Rect | null = o.faixa ? { x: 0, y: 0, w: largura, h: altura * FRACCAO_DA_FAIXA } : null

  const topoDoConteudo = (faixa?.h ?? 0) + margem
  const alturaDoConteudo = altura - topoDoConteudo - margem
  const larguraUtil = largura - margem * 2
  const wColuna = (larguraUtil - margem) * FRACCAO_DA_COLUNA
  const wHeroi = larguraUtil - margem - wColuna

  const xColuna = o.lado === 'esquerda' ? margem : margem + wHeroi + margem
  const xHeroi = o.lado === 'esquerda' ? margem + wColuna + margem : margem

  const heroi: Rect = { x: xHeroi, y: topoDoConteudo, w: wHeroi, h: alturaDoConteudo }

  const hLogo = o.logo ? alturaDoConteudo * FRACCAO_DO_LOGO : 0
  const hAssinatura = o.assinatura ? alturaDoConteudo * FRACCAO_DA_ASSINATURA : 0
  const logo: Rect | null = o.logo ? { x: xColuna, y: topoDoConteudo, w: wColuna, h: hLogo } : null
  const assinatura: Rect | null = o.assinatura
    ? { x: xColuna, y: topoDoConteudo + alturaDoConteudo - hAssinatura, w: wColuna, h: hAssinatura }
    : null

  const k = Math.max(0, Math.min(Math.floor(n), MAXIMO_DE_CAMARAS))
  const yCamaras = topoDoConteudo + hLogo
  const hCamaras = alturaDoConteudo - hLogo - hAssinatura
  const camaras: Rect[] =
    k > 0
      ? Array.from({ length: k }, (_, i) => ({
          x: xColuna,
          y: yCamaras + (hCamaras / k) * i,
          w: wColuna,
          h: hCamaras / k,
        }))
      : []

  return { faixa, heroi, logo, camaras, assinatura }
}

/**
 * Quanto é que a faixa já andou, em píxeis, no instante dado.
 *
 * O INSTANTE É ARGUMENTO, e é o ponto todo: se esta função lesse o `Date.now()`
 * por dentro, dois frames do mesmo instante dariam valores diferentes, não
 * havia como a testar, e não havia como voltar a desenhar um frame igual. Quem
 * anima passa o instante do seu `requestAnimationFrame`.
 *
 * O resultado está sempre em `[0, passo)`, onde `passo` é a largura de uma
 * repetição do padrão — é o que permite desenhar o padrão em mosaico e
 * deslocá-lo sem acumular, ao fim de horas de directo, um número grande o
 * suficiente para perder precisão.
 */
export function deslocamentoDaFaixa(instanteMs: number, passo: number, largura: number): number {
  if (!(passo > 0) || !Number.isFinite(instanteMs) || !Number.isFinite(largura)) return 0
  const px = (instanteMs / 1000) * largura * VELOCIDADE_DA_FAIXA
  const resto = px % passo
  // O `%` do JS devolve negativo para entradas negativas; um instante antes da
  // origem não pode atirar a faixa para fora do ecrã.
  return resto < 0 ? resto + passo : resto
}
