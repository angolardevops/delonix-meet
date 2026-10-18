/**
 * A MOLDURA DE TRANSMISSÃO — a geometria de uma cena com marca, do género que
 * uma conferência põe no ar: uma faixa a rolar no topo, uma coluna lateral com
 * o logótipo em cima, as câmaras ao meio e a assinatura em baixo, e o conteúdo
 * (o ecrã partilhado, normalmente) grande ao lado.
 *
 * Aqui não há canvas, nem `MediaStream`, nem DOM. Só rectângulos e um
 * deslocamento — sem jsdom neste repo, o que não for puro não se testa. O
 * desenho fica para um irmão `desenhoDaMoldura.ts`, pela mesma separação que o
 * `studio/tv/mesa.ts` e o `desenhoDaMesa.ts` já usam.
 *
 * ESTADO, a 2026-10-18. O desenho está no irmão `desenhoDaMoldura.ts`, e o
 * `rectsDaMoldura` tem QUATRO chamadores — contados, porque eu já escrevi aqui
 * «dois» e estava errado:
 *   1. `room/compositor.ts`, cena `moldura` — passa a lista INTEIRA de câmaras,
 *      logo é o caminho onde o `omitidas` conta a verdade;
 *   2. `studio/compositor.ts`, layout `moldura` — idem, com as peças do palco;
 *   3. `studio/palco.ts`, dentro do `rectsDoLayout` — este CORTA o `n` antes
 *      (`Math.min(n, MAXIMO_POR_LAYOUT.moldura)`) e devolve só rectângulos, pelo
 *      que o `omitidas` nasce sempre a zero deste lado;
 *   4. `desenhoDaMoldura.ts`, no `desenharMoldura` completo.
 *
 * O (3) IMPORTA e não está resolvido: quem desenha a partir do `rectsDoLayout`
 * — hoje a mesa de corte do Estúdio (`studio/tv/desenhoDaMesa.ts`, que faz
 * `rects.forEach` sobre a lista já cortada) — continua a largar fontes em
 * silêncio, em qualquer layout e não só neste. O aviso desta fatia cobre o (1) e
 * o (2). Fechar o (3) é fazer o `rectsDoLayout` devolver também o que cortou, e
 * é trabalho de outra fatia; fica escrito para não se confundir «a moldura
 * anuncia» com «o palco anuncia».
 *
 * PORQUE É UM MÓDULO À PARTE, e não mais uma função no `studio/palco.ts`: a
 * cena vai ser desenhada pelos dois compositores, e `room/` e `studio/` já se
 * importam NOS DOIS SENTIDOS (`studio/palco.ts` traz o `calcularRects` da sala;
 * `room/useMulticam.ts` traz o `directo` do estúdio). Pô-la numa dessas pastas
 * só acrescentava arestas a um emaranhado que já existe. Esta é folha: não
 * importa de nenhuma outra pasta de `src/`, e por isso qualquer uma a pode
 * importar sem fechar ciclo. Quem o impõe é a regra `no-restricted-imports`
 * para `src/cena/**` no `web/eslint.config.js` — e não um `grep`, que já se
 * mostrou cego a importações multi-linha e a ciclos transitivos.
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
 * Lê e SANEIA as opções, como o `sanearSobreposicoes` do `studio/palco.ts` faz
 * para as suas — um JSON velho, mexido à mão, ou um `localStorage` corrompido
 * não pode dar tipos errados à geometria.
 *
 * E não basta `{ ...OPCOES_INICIAIS, ...opcoes }`: o spread deixa uma chave
 * explicitamente `undefined` DESTRUIR o valor por omissão em vez de cair nele.
 * Como o `exactOptionalPropertyTypes` está desligado no `tsconfig`, um
 * `{ lado: cfg?.lado }` ou um `JSON.parse` de armazenamento estragado passa no
 * compilador e produz exactamente isso. Medido: `{ lado: undefined }` mandava a
 * coluna para o lado errado, e `{ faixa: undefined }` fazia a faixa desaparecer
 * e a cena saltar 62 px a meio de uma emissão.
 */
export function sanearOpcoes(v: unknown): OpcoesDaMoldura {
  const o = (v && typeof v === 'object' ? v : {}) as Record<string, unknown>
  const bool = (k: keyof OpcoesDaMoldura) =>
    typeof o[k] === 'boolean' ? (o[k] as boolean) : (OPCOES_INICIAIS[k] as boolean)
  return {
    lado: o.lado === 'esquerda' || o.lado === 'direita' ? o.lado : OPCOES_INICIAIS.lado,
    faixa: bool('faixa'),
    logo: bool('logo'),
    assinatura: bool('assinatura'),
  }
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
  /**
   * Quantas câmaras ficaram DE FORA por não caberem na coluna.
   *
   * Sem este número, quem chama não consegue distinguir três convidados de
   * nove: a coluna fica igualmente bem desenhada nos dois casos, e os que
   * sobram não aparecem em frame nenhum — com o áudio deles a continuar
   * misturado, portanto a falar fora do ecrã durante a sessão inteira. O
   * `calcularRects` da sala dá rectângulo a toda a gente; aqui há um tecto, e
   * um tecto que não se anuncia é uma perda silenciosa.
   *
   * QUEM O LÊ: o `desenharAvisoDeOmitidas` do `desenhoDaMoldura.ts`, que o põe
   * como ficha no canto do último painel da coluna. Por isso é um número e não
   * um booleano — e por isso o tecto deixou de ser silencioso.
   */
  omitidas: number
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
  const o = sanearOpcoes({ ...OPCOES_INICIAIS, ...opcoes })

  // Dimensões saneadas ANTES de qualquer conta. Um `canvas.width` lido antes do
  // `loadedmetadata`, ou um `video.videoWidth` de uma pista que ainda não
  // chegou, dá `NaN`/`undefined` — e um `NaN` que chegue ao `drawImage` ou ao
  // `fillRect` apaga o desenho TODO sem erro nenhum. Mais vale um frame com
  // rectângulos de área zero (nada desenhado, recuperável no frame seguinte)
  // do que a tela em branco e silêncio.
  const W = Number.isFinite(largura) && largura > 0 ? largura : 0
  const H = Number.isFinite(altura) && altura > 0 ? altura : 0

  // A margem é da altura, mas não pode comer a largura num canvas muito
  // estreito (uma captura vertical, 100×1920): sem o tecto, `wHeroi` saía
  // NEGATIVO e as peças trocavam de lado.
  const margem = Math.min(H * FRACCAO_DA_MARGEM, W / 10)

  const faixa: Rect | null = o.faixa ? { x: 0, y: 0, w: W, h: H * FRACCAO_DA_FAIXA } : null

  const topoDoConteudo = (faixa?.h ?? 0) + margem
  const alturaDoConteudo = Math.max(0, H - topoDoConteudo - margem)
  const larguraUtil = Math.max(0, W - margem * 2)
  const wColuna = Math.max(0, (larguraUtil - margem) * FRACCAO_DA_COLUNA)
  const wHeroi = Math.max(0, larguraUtil - margem - wColuna)

  const xColuna = o.lado === 'esquerda' ? margem : margem + wHeroi + margem
  const xHeroi = o.lado === 'esquerda' ? margem + wColuna + margem : margem

  const heroi: Rect = { x: xHeroi, y: topoDoConteudo, w: wHeroi, h: alturaDoConteudo }

  const pedidas = Number.isFinite(n) ? Math.max(0, Math.floor(n)) : 0
  const k = Math.min(pedidas, MAXIMO_DE_CAMARAS)

  // As fracções do logótipo e da assinatura, com o espaço das câmaras
  // redistribuído quando não há câmaras nenhumas.
  //
  // SEM ISTO havia um buraco: com `k = 0` o logótipo ficava colado ao topo e a
  // assinatura ancorada em baixo, e no meio sobravam 59% da altura da coluna
  // por pintar — 564 px num 1080p. O comentário desta função prometia «não um
  // buraco preto» e o código fazia exactamente um. Passou despercebido porque o
  // teste de contiguidade só corria com três câmaras.
  const fLogo0 = o.logo ? FRACCAO_DO_LOGO : 0
  const fAssinatura0 = o.assinatura ? FRACCAO_DA_ASSINATURA : 0
  const somaDasMarcas = fLogo0 + fAssinatura0
  const escala = k === 0 && somaDasMarcas > 0 ? 1 / somaDasMarcas : 1
  const hLogo = alturaDoConteudo * fLogo0 * escala
  const hAssinatura = alturaDoConteudo * fAssinatura0 * escala

  const logo: Rect | null = o.logo ? { x: xColuna, y: topoDoConteudo, w: wColuna, h: hLogo } : null
  const assinatura: Rect | null = o.assinatura
    ? { x: xColuna, y: topoDoConteudo + alturaDoConteudo - hAssinatura, w: wColuna, h: hAssinatura }
    : null

  const yCamaras = topoDoConteudo + hLogo
  const hCamaras = Math.max(0, alturaDoConteudo - hLogo - hAssinatura)
  const camaras: Rect[] = Array.from({ length: k }, (_, i) => ({
    x: xColuna,
    y: yCamaras + (hCamaras / k) * i,
    w: wColuna,
    h: hCamaras / k,
  }))

  return { faixa, heroi, logo, camaras, assinatura, omitidas: pedidas - k }
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
  // `Number.isFinite` nos TRÊS, e não só o `passo > 0`: um `NaN` passa o
  // `> 0` como falso e portanto saía 0 por acidente, mas um `passo` negativo
  // devolvia um resultado FORA de `[0, passo)` — e um `passo` NaN vindo de uma
  // imagem de padrão ainda por carregar (`larguraDoPadrao * escala`) levava o
  // NaN até ao `translate`, que apaga o frame inteiro sem erro nenhum.
  if (!Number.isFinite(passo) || passo <= 0) return 0
  if (!Number.isFinite(instanteMs) || !Number.isFinite(largura)) return 0
  const px = (instanteMs / 1000) * largura * VELOCIDADE_DA_FAIXA
  const resto = px % passo
  // O `%` do JS devolve negativo para entradas negativas; um instante antes da
  // origem não pode atirar a faixa para fora do ecrã.
  return resto < 0 ? resto + passo : resto
}
