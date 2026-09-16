/**
 * Tinta do quadro, no cliente. O traço que viaja pela sinalização continua a
 * ser `{ pts, c, w }` (uma espessura por traço — pressão por ponto precisa de
 * servidor, B7). O que se faz aqui é decidir BEM esses três valores antes de
 * enviar: suavizar, endireitar formas, e ler pressão e inclinação da caneta
 * para a espessura do traço.
 */
export type Pt = [number, number]

/** O servidor recusa traços com mais pontos (`room_tools.rs`). */
export const MAX_PONTOS = 2000

/** Cor com transparência para o marcador — cabe nos 24 caracteres do servidor. */
export function corDeMarcador(hex: string, alfa = 0.35): string {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex)
  if (!m) return hex
  const n = parseInt(m[1], 16)
  return `rgba(${(n >> 16) & 255},${(n >> 8) & 255},${n & 255},${alfa})`
}

/**
 * Suavização exponencial (0 = crua, 1 = muito suave). As pontas mantêm-se: o
 * traço começa e acaba onde a caneta tocou.
 */
export function suavizar(pts: Pt[], quanto: number): Pt[] {
  const k = Math.max(0, Math.min(1, quanto))
  if (k === 0 || pts.length < 3) return pts
  const alfa = 1 - k * 0.85
  const out: Pt[] = [pts[0]]
  let [x, y] = pts[0]
  for (let i = 1; i < pts.length - 1; i++) {
    x = x + alfa * (pts[i][0] - x)
    y = y + alfa * (pts[i][1] - y)
    out.push([x, y])
  }
  out.push(pts[pts.length - 1])
  return out
}

/** Reduz a `MAX_PONTOS` mantendo as pontas. */
export function limitarPontos(pts: Pt[], max = MAX_PONTOS): Pt[] {
  if (pts.length <= max) return pts
  const passo = (pts.length - 1) / (max - 1)
  return Array.from({ length: max }, (_, i) => pts[Math.round(i * passo)])
}

export interface AmostraCaneta {
  /** 0..1 como o `PointerEvent.pressure` (rato carregado = 0,5). */
  pressao: number
  /** Graus (0 = caneta a direito). */
  inclinacao: number
}

/**
 * Espessura de UM traço a partir das amostras da caneta. `sensibilidade`
 * (0..1) decide quanto a pressão média afasta a espessura da base; com
 * `inclinacaoEngrossa`, a caneta deitada escreve mais grosso (como um lápis).
 */
export function espessura(base: number, amostras: AmostraCaneta[], sensibilidade: number, inclinacaoEngrossa: boolean): number {
  if (amostras.length === 0) return base
  const media = (f: (a: AmostraCaneta) => number) => amostras.reduce((s, a) => s + f(a), 0) / amostras.length
  let fator = 1 + (media((a) => a.pressao) - 0.5) * 2 * Math.max(0, Math.min(1, sensibilidade))
  if (inclinacaoEngrossa) fator *= 1 + (Math.min(90, media((a) => a.inclinacao)) / 90) * 1.5
  return Math.round(Math.max(base * 0.4, Math.min(base * 3, base * fator)) * 10) / 10
}

/** Inclinação da caneta em graus a partir de `tiltX`/`tiltY`. */
export function grausDeInclinacao(tiltX: number, tiltY: number): number {
  return Math.min(90, Math.hypot(tiltX || 0, tiltY || 0))
}

export type Forma = 'rect' | 'elipse' | 'seta' | 'linha'

/** Pontos de uma forma desenhada por arrasto de `a` a `b`. */
export function pontosDaForma(forma: Forma, a: Pt, b: Pt, aspecto = 1): Pt[] {
  const [x0, y0] = a
  const [x1, y1] = b
  if (forma === 'linha') return [a, b]
  if (forma === 'rect') return [[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]]
  if (forma === 'elipse') {
    const cx = (x0 + x1) / 2
    const cy = (y0 + y1) / 2
    const rx = Math.abs(x1 - x0) / 2
    const ry = Math.abs(y1 - y0) / 2
    return Array.from({ length: 49 }, (_, i) => {
      const t = (i / 48) * Math.PI * 2
      return [cx + rx * Math.cos(t), cy + ry * Math.sin(t)] as Pt
    })
  }
  // Seta: a cabeça calcula-se em espaço de píxeis (aspecto), senão sai torta.
  const dx = (x1 - x0) * aspecto
  const dy = y1 - y0
  const len = Math.hypot(dx, dy)
  if (len === 0) return [a, b]
  const cab = Math.min(0.04, len * 0.3)
  const ang = Math.atan2(dy, dx)
  const ponta = (d: number): Pt => [x1 - (Math.cos(ang + d) * cab) / aspecto, y1 - Math.sin(ang + d) * cab]
  return [a, b, ponta(Math.PI / 7), b, ponta(-Math.PI / 7)]
}

/**
 * «Endireitar formas»: um traço à mão que é quase uma linha, um rectângulo ou
 * uma elipse vira essa forma. Devolve `null` quando não reconhece — o traço
 * fica como foi desenhado. Tudo em espaço de píxeis (`aspecto` = largura/altura).
 */
export function endireitar(pts: Pt[], aspecto = 1): { forma: Forma; pts: Pt[] } | null {
  if (pts.length < 6) return null
  const P = pts.map(([x, y]) => [x * aspecto, y] as Pt)
  const [ax, ay] = P[0]
  const [bx, by] = P[P.length - 1]
  const xs = P.map((p) => p[0])
  const ys = P.map((p) => p[1])
  const minX = Math.min(...xs)
  const maxX = Math.max(...xs)
  const minY = Math.min(...ys)
  const maxY = Math.max(...ys)
  const diag = Math.hypot(maxX - minX, maxY - minY)
  if (diag < 0.02) return null
  const fechado = Math.hypot(bx - ax, by - ay) < diag * 0.2

  if (!fechado) {
    const len = Math.hypot(bx - ax, by - ay)
    if (len < 0.03) return null
    const desvio = Math.max(...P.map(([x, y]) => Math.abs((by - ay) * x - (bx - ax) * y + bx * ay - by * ax) / len))
    return desvio < len * 0.06 ? { forma: 'linha', pts: [pts[0], pts[pts.length - 1]] } : null
  }

  const w = maxX - minX
  const h = maxY - minY
  if (w < 0.02 || h < 0.02) return null
  // Erro médio a um rectângulo (distância ao lado mais próximo) e a uma elipse
  // (afastamento do raio normalizado), relativos ao tamanho.
  const erroRect =
    P.reduce((s, [x, y]) => s + Math.min(Math.abs(x - minX), Math.abs(x - maxX), Math.abs(y - minY), Math.abs(y - maxY)), 0) /
    P.length /
    Math.min(w, h)
  const cx = (minX + maxX) / 2
  const cy = (minY + maxY) / 2
  const erroElipse =
    P.reduce((s, [x, y]) => s + Math.abs(Math.hypot((x - cx) / (w / 2), (y - cy) / (h / 2)) - 1), 0) / P.length
  const a: Pt = [minX / aspecto, minY]
  const b: Pt = [maxX / aspecto, maxY]
  if (erroRect < 0.08 && erroRect <= erroElipse) return { forma: 'rect', pts: pontosDaForma('rect', a, b) }
  if (erroElipse < 0.15) return { forma: 'elipse', pts: pontosDaForma('elipse', a, b) }
  return null
}
