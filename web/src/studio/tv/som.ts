/**
 * A matemática da mesa de som, sem Web Audio: a lei do fader, níveis em dBFS,
 * a sonoridade AO VIVO (momentânea, curta e integrada, BS.1770-4 / EBU R 128)
 * e o pico real (dBTP) por sobre-amostragem ×4.
 *
 * A sonoridade reutiliza os filtros K do editor (`edit/sinal.ts`), que já
 * estão medidos contra a tabela da norma. O que muda aqui é que o sinal chega
 * aos BOCADOS (100 ms de cada vez, vindos de um AudioWorklet) e o estado dos
 * filtros tem de atravessar os bocados — reiniciá-lo a cada bloco dava um
 * degrau no passa-altos a cada 100 ms e uma leitura inventada.
 */
import { type Biquad, filtrosK } from '../edit/sinal'

// ------------------------------------------------------------------ fader

/**
 * Lei do fader: posição (0–1) → dB. Os pontos são os de uma mesa real — 0 dB
 * a três quartos, +10 dB no topo, e a metade de baixo a descer depressa até
 * ao silêncio. Entre pontos, linear em dB.
 */
const LEI: [number, number][] = [
  [0, -Infinity],
  [0.05, -60],
  [0.25, -30],
  [0.5, -10],
  [0.75, 0],
  [1, 10],
]

export function faderParaDb(pos: number): number {
  const p = Math.min(1, Math.max(0, Number.isFinite(pos) ? pos : 0))
  if (p <= 0) return -Infinity
  for (let i = 1; i < LEI.length; i++) {
    const [p1, d1] = LEI[i]
    const [p0, d0] = LEI[i - 1]
    if (p <= p1) {
      if (!Number.isFinite(d0)) return d1 + (Math.log10(p / p1) * 20) // a cauda desce em log até −∞
      return d0 + ((p - p0) / (p1 - p0)) * (d1 - d0)
    }
  }
  return 10
}

export function dbParaFader(db: number): number {
  if (!Number.isFinite(db) || db <= -120) return 0
  if (db >= 10) return 1
  for (let i = LEI.length - 1; i > 0; i--) {
    const [p1, d1] = LEI[i]
    const [p0, d0] = LEI[i - 1]
    if (db >= (Number.isFinite(d0) ? d0 : -Infinity)) {
      if (!Number.isFinite(d0)) return Math.max(0, p1 * Math.pow(10, (db - d1) / 20))
      return p0 + ((db - d0) / (d1 - d0)) * (p1 - p0)
    }
  }
  return 0
}

export const dbParaGanho = (db: number) => (Number.isFinite(db) ? Math.pow(10, db / 20) : 0)
export const ganhoParaDb = (g: number) => (g > 0 ? 20 * Math.log10(g) : -Infinity)

/** Pico e RMS de um bloco, em dBFS. */
export function nivelDoBloco(x: Float32Array): { picoDb: number; rmsDb: number } {
  let pico = 0
  let soma = 0
  for (let i = 0; i < x.length; i++) {
    const v = x[i]
    const a = Math.abs(v)
    if (a > pico) pico = a
    soma += v * v
  }
  return { picoDb: ganhoParaDb(pico), rmsDb: ganhoParaDb(Math.sqrt(soma / Math.max(1, x.length))) }
}

/**
 * Altura de um medidor (0–1) para um nível em dBFS: −60 dB em baixo, 0 em
 * cima, linear em dB — o que se lê numa régua de mesa.
 */
export const alturaDoMedidor = (db: number, piso = -60) => (Number.isFinite(db) ? Math.min(1, Math.max(0, (db - piso) / -piso)) : 0)

// ------------------------------------------------------------------ sonoridade ao vivo

interface EstadoBiquad {
  x1: number
  x2: number
  y1: number
  y2: number
}

function filtrarNoSitio(x: Float32Array, f: Biquad, s: EstadoBiquad, out: Float32Array): void {
  let { x1, x2, y1, y2 } = s
  for (let i = 0; i < x.length; i++) {
    const xi = x[i]
    const v = f.b0 * xi + f.b1 * x1 + f.b2 * x2 - f.a1 * y1 - f.a2 * y2
    x2 = x1
    x1 = xi
    y2 = y1
    y1 = v
    out[i] = v
  }
  s.x1 = x1
  s.x2 = x2
  s.y1 = y1
  s.y2 = y2
}

/**
 * Filtro de sobre-amostragem ×4 para o pico real: passa-baixo em seno
 * janelado (Blackman), 48 coeficientes (12 por fase) — a mesma ordem do
 * filtro do anexo 2 da BS.1770-4. Erro medido < 0,3 dB até 0,45·fs.
 */
const FASES = 4
const POR_FASE = 12
const COEFS: Float64Array[] = (() => {
  const n = FASES * POR_FASE
  const h = new Float64Array(n)
  const centro = (n - 1) / 2
  for (let i = 0; i < n; i++) {
    const t = (i - centro) / FASES
    const sinc = t === 0 ? 1 : Math.sin(Math.PI * t) / (Math.PI * t)
    const w = 0.42 - 0.5 * Math.cos((2 * Math.PI * i) / (n - 1)) + 0.08 * Math.cos((4 * Math.PI * i) / (n - 1))
    h[i] = sinc * w
  }
  const fases: Float64Array[] = []
  for (let f = 0; f < FASES; f++) {
    const c = new Float64Array(POR_FASE)
    let soma = 0
    for (let k = 0; k < POR_FASE; k++) {
      c[k] = h[k * FASES + f]
      soma += c[k]
    }
    // Cada fase com ganho DC 1: um sinal constante não pode «ganhar» pico.
    for (let k = 0; k < POR_FASE; k++) c[k] /= soma
    fases.push(c)
  }
  return fases
})()

class PicoReal {
  private hist = new Float64Array(POR_FASE)
  private pos = 0
  /** Maior valor absoluto sobre-amostrado deste bloco. */
  bloco(x: Float32Array): number {
    let m = 0
    const h = this.hist
    for (let i = 0; i < x.length; i++) {
      h[this.pos] = x[i]
      this.pos = (this.pos + 1) % POR_FASE
      for (let f = 0; f < FASES; f++) {
        const c = COEFS[f]
        let v = 0
        // O mais recente fica no fim da convolução.
        for (let k = 0; k < POR_FASE; k++) v += c[k] * h[(this.pos + k) % POR_FASE]
        const a = Math.abs(v)
        if (a > m) m = a
      }
    }
    return m
  }
}

const paraLufs = (z: number) => (z > 0 ? -0.691 + 10 * Math.log10(z) : -Infinity)

export interface LeituraDeSonoridade {
  /** 400 ms. */
  momentanea: number
  /** 3 s. */
  curta: number
  /** Desde o início (ou o último `reiniciar`), com as portas absoluta e relativa. */
  integrada: number
  /** Pico real, dBTP, desde o início. */
  picoReal: number
  /** Pico real dos últimos 3 s. */
  picoRealRecente: number
}

/**
 * Medidor contínuo. Alimenta-se com blocos CONSECUTIVOS de 100 ms (um array
 * por canal); pesos G = 1 (L/R). Guarda uma energia por bloco de 100 ms: a
 * momentânea é a média dos últimos 4, a curta dos últimos 30 e a integrada
 * usa janelas de 400 ms a passo de 100 ms (75 % de sobreposição), como a norma.
 */
export class MedidorDeSonoridade {
  private filtros: [Biquad, Biquad]
  private estados: EstadoBiquad[][] = []
  private picos: PicoReal[] = []
  private tmp = new Float32Array(0)
  private energias: number[] = []
  private janelas: number[] = []
  private picoTotal = 0
  private picosRecentes: number[] = []
  /** Limite das janelas guardadas para a integrada: 4 h a 10 por segundo. */
  private static MAX_JANELAS = 4 * 3600 * 10

  constructor(readonly taxa: number) {
    this.filtros = filtrosK(taxa)
  }

  get amostrasPorBloco(): number {
    return Math.round(this.taxa * 0.1)
  }

  reiniciar(): void {
    this.energias = []
    this.janelas = []
    this.picoTotal = 0
    this.picosRecentes = []
  }

  /** Um bloco de ~100 ms, um `Float32Array` por canal. */
  alimentar(canais: Float32Array[]): void {
    if (!canais.length) return
    const n = canais[0].length
    if (this.tmp.length !== n) this.tmp = new Float32Array(n)
    let energia = 0
    let picoBloco = 0
    for (let c = 0; c < canais.length; c++) {
      this.estados[c] ??= [
        { x1: 0, x2: 0, y1: 0, y2: 0 },
        { x1: 0, x2: 0, y1: 0, y2: 0 },
      ]
      this.picos[c] ??= new PicoReal()
      const [e0, e1] = this.estados[c]
      filtrarNoSitio(canais[c], this.filtros[0], e0, this.tmp)
      filtrarNoSitio(this.tmp, this.filtros[1], e1, this.tmp)
      let s = 0
      for (let i = 0; i < n; i++) s += this.tmp[i] * this.tmp[i]
      energia += s / Math.max(1, n)
      picoBloco = Math.max(picoBloco, this.picos[c].bloco(canais[c]))
    }
    this.energias.push(energia)
    if (this.energias.length > 30) this.energias.shift()
    this.picoTotal = Math.max(this.picoTotal, picoBloco)
    this.picosRecentes.push(picoBloco)
    if (this.picosRecentes.length > 30) this.picosRecentes.shift()
    if (this.energias.length >= 4) {
      const ult = this.energias.slice(-4)
      this.janelas.push(ult.reduce((a, b) => a + b, 0) / 4)
      if (this.janelas.length > MedidorDeSonoridade.MAX_JANELAS) this.janelas.shift()
    }
  }

  ler(): LeituraDeSonoridade {
    const media = (xs: number[]) => (xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : 0)
    const momentanea = this.energias.length >= 4 ? paraLufs(media(this.energias.slice(-4))) : -Infinity
    const curta = this.energias.length >= 30 ? paraLufs(media(this.energias)) : -Infinity
    return {
      momentanea,
      curta,
      integrada: integrar(this.janelas),
      picoReal: ganhoParaDb(this.picoTotal),
      picoRealRecente: ganhoParaDb(Math.max(0, ...this.picosRecentes)),
    }
  }
}

/** As duas portas da norma: absoluta a −70 LUFS, relativa a −10 LU. */
export function integrar(janelas: readonly number[]): number {
  const absolutas = janelas.filter((z) => paraLufs(z) > -70)
  if (!absolutas.length) return -Infinity
  const media = (xs: number[]) => xs.reduce((a, b) => a + b, 0) / xs.length
  const relativo = paraLufs(media(absolutas)) - 10
  const finais = absolutas.filter((z) => paraLufs(z) > relativo)
  return finais.length ? paraLufs(media(finais)) : -Infinity
}

// ------------------------------------------------------------------ equalizador

export type TipoDeBanda = 'lowshelf' | 'peaking' | 'highshelf'

export interface Banda {
  tipo: TipoDeBanda
  freq: number
  ganhoDb: number
  q: number
}

/** As quatro bandas do template: grave 80 Hz, médio− 420 Hz, médio+ 2,4 kHz, agudo 8 kHz. */
export const EQ_INICIAL: readonly Banda[] = [
  { tipo: 'lowshelf', freq: 80, ganhoDb: 0, q: 0.7 },
  { tipo: 'peaking', freq: 420, ganhoDb: 0, q: 1 },
  { tipo: 'peaking', freq: 2400, ganhoDb: 0, q: 1 },
  { tipo: 'highshelf', freq: 8000, ganhoDb: 0, q: 0.7 },
]

export const EQ_GANHO_MAXIMO = 12

/**
 * Resposta em dB de uma banda à frequência `f` — a curva desenhada no ecrã.
 * Fórmulas RBJ (as que o `BiquadFilterNode` usa), avaliadas em |H(e^jw)|.
 */
export function respostaDaBanda(b: Banda, f: number, taxa = 48000): number {
  if (b.ganhoDb === 0) return 0
  const A = Math.pow(10, b.ganhoDb / 40)
  const w0 = (2 * Math.PI * b.freq) / taxa
  const cos = Math.cos(w0)
  const sin = Math.sin(w0)
  let b0: number, b1: number, b2: number, a0: number, a1: number, a2: number
  if (b.tipo === 'peaking') {
    const alpha = sin / (2 * b.q)
    b0 = 1 + alpha * A
    b1 = -2 * cos
    b2 = 1 - alpha * A
    a0 = 1 + alpha / A
    a1 = -2 * cos
    a2 = 1 - alpha / A
  } else {
    // Prateleiras com S = 1, como o BiquadFilterNode.
    const alpha = (sin / 2) * Math.sqrt(2)
    const k = 2 * Math.sqrt(A) * alpha
    if (b.tipo === 'lowshelf') {
      b0 = A * (A + 1 - (A - 1) * cos + k)
      b1 = 2 * A * (A - 1 - (A + 1) * cos)
      b2 = A * (A + 1 - (A - 1) * cos - k)
      a0 = A + 1 + (A - 1) * cos + k
      a1 = -2 * (A - 1 + (A + 1) * cos)
      a2 = A + 1 + (A - 1) * cos - k
    } else {
      b0 = A * (A + 1 + (A - 1) * cos + k)
      b1 = -2 * A * (A - 1 + (A + 1) * cos)
      b2 = A * (A + 1 + (A - 1) * cos - k)
      a0 = A + 1 - (A - 1) * cos + k
      a1 = 2 * (A - 1 - (A + 1) * cos)
      a2 = A + 1 - (A - 1) * cos - k
    }
  }
  const w = (2 * Math.PI * f) / taxa
  const re = (c0: number, c1: number, c2: number) => c0 + c1 * Math.cos(w) + c2 * Math.cos(2 * w)
  const im = (c1: number, c2: number) => -(c1 * Math.sin(w) + c2 * Math.sin(2 * w))
  const num = Math.hypot(re(b0, b1, b2), im(b1, b2))
  const den = Math.hypot(re(a0, a1, a2), im(a1, a2))
  return 20 * Math.log10(num / den)
}

/** A curva do EQ inteiro em `n` pontos de 20 Hz a 20 kHz (escala logarítmica). */
export function curvaDoEq(bandas: readonly Banda[], n = 64, taxa = 48000): { f: number; db: number }[] {
  const out: { f: number; db: number }[] = []
  for (let i = 0; i < n; i++) {
    const f = 20 * Math.pow(1000, i / (n - 1))
    out.push({ f, db: bandas.reduce((s, b) => s + respostaDaBanda(b, f, taxa), 0) })
  }
  return out
}

// ------------------------------------------------------------------ formatos

/** «−15,8» com o sinal tipográfico e a vírgula da língua; «−∞» para silêncio. */
export function formatarDb(db: number, locale = 'pt-AO', casas = 1): string {
  if (!Number.isFinite(db)) return db > 0 ? '+∞' : '−∞'
  const v = Math.abs(db).toLocaleString(locale, { minimumFractionDigits: casas, maximumFractionDigits: casas })
  if (Number(v.replace(',', '.')) === 0) return v
  return `${db < 0 ? '−' : '+'}${v}`
}
