/**
 * Correcção de imagem por câmara — a matemática, sem WebGL.
 *
 * O shader (`media/correccaoGl.ts`) faz, por píxel e por esta ordem:
 *
 *   cor · multiplicadores → contraste à volta do cinzento médio → saturação
 *
 * e tudo o que é escolha da pessoa (exposição em EV, temperatura da luz em
 * kelvin, matiz verde↔magenta, contraste, saturação) vira aqui uniformes.
 *
 * A TEMPERATURA é a da LUZ da cena, como numa câmara: dizer «a luz está a
 * 3 200 K» compensa o laranja do tungsténio e a pele fica neutra. 6 500 K é o
 * neutro (nada muda).
 */

export interface CorreccaoDeImagem {
  /** −2 a +2 EV. */
  exposicao: number
  /** 2 500 a 10 000 K. */
  temperatura: number
  /** −100 (verde) a +100 (magenta). */
  matiz: number
  /** 0,5 a 2 (1 = sem mudança). */
  contraste: number
  /** 0 a 2 (1 = sem mudança). */
  saturacao: number
  /** Multiplicadores do equilíbrio automático entre câmaras (cinzento médio). */
  equilibrio: [number, number, number]
}

export const CORRECCAO_NEUTRA: CorreccaoDeImagem = {
  exposicao: 0,
  temperatura: 6500,
  matiz: 0,
  contraste: 1,
  saturacao: 1,
  equilibrio: [1, 1, 1],
}

export const LIMITES = {
  exposicao: [-2, 2],
  temperatura: [2500, 10000],
  matiz: [-100, 100],
  contraste: [0.5, 2],
  saturacao: [0, 2],
} as const

const limitar = (v: number, [a, b]: readonly [number, number], omissao: number) =>
  Number.isFinite(v) ? Math.min(b, Math.max(a, v)) : omissao

export function sanearCorreccao(v: unknown): CorreccaoDeImagem {
  const o = (v && typeof v === 'object' ? v : {}) as Record<string, unknown>
  const eq = Array.isArray(o.equilibrio) && o.equilibrio.length === 3 ? (o.equilibrio as unknown[]).map(Number) : [1, 1, 1]
  return {
    exposicao: limitar(Number(o.exposicao), LIMITES.exposicao, 0),
    temperatura: limitar(Number(o.temperatura), LIMITES.temperatura, 6500),
    matiz: limitar(Number(o.matiz), LIMITES.matiz, 0),
    contraste: limitar(Number(o.contraste), LIMITES.contraste, 1),
    saturacao: limitar(Number(o.saturacao), LIMITES.saturacao, 1),
    equilibrio: eq.map((x) => limitar(x, [0.5, 2], 1)) as [number, number, number],
  }
}

export function ehNeutra(c: CorreccaoDeImagem): boolean {
  return (
    c.exposicao === 0 &&
    c.temperatura === 6500 &&
    c.matiz === 0 &&
    c.contraste === 1 &&
    c.saturacao === 1 &&
    c.equilibrio.every((x) => x === 1)
  )
}

/**
 * Cor (0–1) de um corpo negro a `k` kelvin — a aproximação de Tanner Helland,
 * boa a ±1 % entre 1 000 e 40 000 K, que é o que se precisa para luz de estúdio.
 */
export function corDoKelvin(k: number): [number, number, number] {
  const t = k / 100
  let r: number, g: number, b: number
  if (t <= 66) {
    r = 255
    g = 99.4708025861 * Math.log(t) - 161.1195681661
    b = t <= 19 ? 0 : 138.5177312231 * Math.log(t - 10) - 305.0447927307
  } else {
    r = 329.698727446 * Math.pow(t - 60, -0.1332047592)
    g = 288.1221695283 * Math.pow(t - 60, -0.0755148492)
    b = 255
  }
  const c = (x: number) => Math.min(255, Math.max(0, x)) / 255
  return [c(r), c(g), c(b)]
}

/** Os multiplicadores RGB finais (exposição · temperatura · matiz · equilíbrio). */
export function multiplicadores(c: CorreccaoDeImagem): [number, number, number] {
  const ganho = Math.pow(2, c.exposicao)
  const luz = corDoKelvin(c.temperatura)
  const neutro = corDoKelvin(6500)
  // Compensar a luz: dividir pela cor dela, relativa ao neutro.
  const wb = [neutro[0] / Math.max(1e-3, luz[0]), neutro[1] / Math.max(1e-3, luz[1]), neutro[2] / Math.max(1e-3, luz[2])]
  // Normaliza pela luminância para a temperatura não mexer no brilho.
  const lum = 0.2126 * wb[0] + 0.7152 * wb[1] + 0.0722 * wb[2]
  const matiz = c.matiz / 100
  const tint = [1 + matiz * 0.1, 1 - matiz * 0.15, 1 + matiz * 0.1]
  return [0, 1, 2].map((i) => (ganho * wb[i] * tint[i] * c.equilibrio[i]) / lum) as [number, number, number]
}

/** Aplica a correcção a UM píxel (0–1) — a referência que o shader segue e que os testes medem. */
export function corrigirPixel(rgb: [number, number, number], c: CorreccaoDeImagem): [number, number, number] {
  const m = multiplicadores(c)
  let p = rgb.map((v, i) => v * m[i]) as [number, number, number]
  p = p.map((v) => (v - 0.5) * c.contraste + 0.5) as [number, number, number]
  const l = 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]
  p = p.map((v) => l + (v - l) * c.saturacao) as [number, number, number]
  return p.map((v) => Math.min(1, Math.max(0, v))) as [number, number, number]
}

/** Luminância média (0–255) de pixéis RGBA — a prova de que a exposição mexeu. */
export function luminanciaMedia(dados: ArrayLike<number>, passo = 4): number {
  let soma = 0
  let n = 0
  for (let i = 0; i + 2 < dados.length; i += 4 * passo) {
    soma += 0.2126 * dados[i] + 0.7152 * dados[i + 1] + 0.0722 * dados[i + 2]
    n++
  }
  return n ? soma / n : 0
}

/** Média RGB (0–255) de pixéis RGBA. */
export function mediaRgb(dados: ArrayLike<number>, passo = 4): [number, number, number] {
  const s = [0, 0, 0]
  let n = 0
  for (let i = 0; i + 2 < dados.length; i += 4 * passo) {
    s[0] += dados[i]
    s[1] += dados[i + 1]
    s[2] += dados[i + 2]
    n++
  }
  return n ? [s[0] / n, s[1] / n, s[2] / n] : [0, 0, 0]
}

/**
 * Equilíbrio entre câmaras pelo «mundo cinzento»: multiplicadores que levam a
 * média de cada canal à média dos três. Aplicado a todas as câmaras, as
 * brancas ficam iguais entre elas, que é o que o corte entre planos pede.
 */
export function equilibrioCinzento(media: [number, number, number]): [number, number, number] {
  const [r, g, b] = media
  if (r < 8 || g < 8 || b < 8) return [1, 1, 1] // imagem quase preta: não há nada para medir
  const cinza = (r + g + b) / 3
  const lim = (x: number) => Math.min(2, Math.max(0.5, x))
  return [lim(cinza / r), lim(cinza / g), lim(cinza / b)]
}
