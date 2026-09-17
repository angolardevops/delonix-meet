/**
 * Matemática de sinal do editor — sonoridade (LUFS), picos, onda sonora e
 * trechos fora do microfone. Puro, sem Web Audio: recebe `Float32Array` e
 * devolve números, por isso testa-se em Node com sinais fabricados.
 */

// ---------------------------------------------------------------------------
//  Sonoridade integrada (ITU-R BS.1770-4 / EBU R 128, a recomendação de sonoridade)
// ---------------------------------------------------------------------------

export interface Biquad {
  b0: number
  b1: number
  b2: number
  a1: number
  a2: number
}

/**
 * Coeficientes do filtro K para qualquer taxa de amostragem — as tabelas da
 * norma são só para 48 kHz, e a gravação pode vir a 44,1 kHz. Derivação de
 * Brecht De Man (a do `pyloudnorm`), que reproduz a tabela da norma a 48 kHz.
 *
 * Nota medida: a forma «RBJ» dos mesmos parâmetros NÃO a reproduz — dá 0,26 dB
 * a menos a 1 kHz, e um seno de referência media −3,27 em vez de −3,01 LUFS.
 */
export function filtrosK(sr: number): [Biquad, Biquad] {
  // Prateleira alta (efeito da cabeça).
  const G = 3.999843853973347
  const Q = 0.7071752369554196
  const K = Math.tan((Math.PI * 1681.974450955533) / sr)
  const Vh = Math.pow(10, G / 20)
  const Vb = Math.pow(Vh, 0.4996667741545416)
  const a0 = 1 + K / Q + K * K
  const shelf: Biquad = {
    b0: (Vh + (Vb * K) / Q + K * K) / a0,
    b1: (2 * (K * K - Vh)) / a0,
    b2: (Vh - (Vb * K) / Q + K * K) / a0,
    a1: (2 * (K * K - 1)) / a0,
    a2: (1 - K / Q + K * K) / a0,
  }
  // Passa-altos RLB: numerador [1, −2, 1], sem normalizar — como na norma.
  const Qh = 0.5003270373238773
  const Kh = Math.tan((Math.PI * 38.13547087602444) / sr)
  const ah = 1 + Kh / Qh + Kh * Kh
  const hp: Biquad = { b0: 1, b1: -2, b2: 1, a1: (2 * (Kh * Kh - 1)) / ah, a2: (1 - Kh / Qh + Kh * Kh) / ah }
  return [shelf, hp]
}

function filtrar(x: Float32Array, f: Biquad): Float32Array {
  const y = new Float32Array(x.length)
  let x1 = 0
  let x2 = 0
  let y1 = 0
  let y2 = 0
  for (let i = 0; i < x.length; i++) {
    const v = f.b0 * x[i] + f.b1 * x1 + f.b2 * x2 - f.a1 * y1 - f.a2 * y2
    x2 = x1
    x1 = x[i]
    y2 = y1
    y1 = v
    y[i] = v
  }
  return y
}

/**
 * Sonoridade integrada em LUFS. `-Infinity` para silêncio.
 * Blocos de 400 ms com 75% de sobreposição, porta absoluta a −70 LUFS e porta
 * relativa a −10 LU — sem as portas, uma aula com pausas longas media-se mais
 * baixa do que soa e o ganho aplicado estourava a fala.
 */
export function lufsIntegrado(canais: Float32Array[], sr: number): number {
  if (!canais.length || !canais[0].length) return -Infinity
  const [shelf, hp] = filtrosK(sr)
  const k = canais.map((c) => filtrar(filtrar(c, shelf), hp))
  const bloco = Math.round(0.4 * sr)
  const passo = Math.round(0.1 * sr)
  const n = k[0].length
  const energias: number[] = []
  for (let ini = 0; ini + bloco <= n; ini += passo) {
    let soma = 0
    for (const c of k) {
      let s = 0
      for (let i = ini; i < ini + bloco; i++) s += c[i] * c[i]
      soma += s / bloco // pesos G = 1 para L/R/C
    }
    energias.push(soma)
  }
  if (!energias.length) return -Infinity
  const paraLufs = (z: number) => -0.691 + 10 * Math.log10(z)
  const absolutos = energias.filter((z) => paraLufs(z) > -70)
  if (!absolutos.length) return -Infinity
  const media = (xs: number[]) => xs.reduce((a, b) => a + b, 0) / xs.length
  const relativo = paraLufs(media(absolutos)) - 10
  const finais = absolutos.filter((z) => paraLufs(z) > relativo)
  return finais.length ? paraLufs(media(finais)) : -Infinity
}

export function pico(canais: Float32Array[]): number {
  let m = 0
  for (const c of canais) for (let i = 0; i < c.length; i++) m = Math.max(m, Math.abs(c[i]))
  return m
}

export const dbParaGanho = (db: number) => Math.pow(10, db / 20)
export const ganhoParaDb = (g: number) => 20 * Math.log10(g)

/**
 * Ganho (em dB) para levar o material ao alvo de sonoridade, limitado a
 * ±24 dB — acima disso o problema é a gravação, e amplificar ruído não o
 * resolve.
 */
export function ganhoParaAlvo(lufs: number, alvo: number): number {
  if (!Number.isFinite(lufs)) return 0
  return Math.max(-24, Math.min(24, alvo - lufs))
}

/**
 * Limitador de picos simples com ataque instantâneo e libertação exponencial.
 * Aplica-se DEPOIS do ganho de sonoridade: sem ele, subir uma aula baixa para
 * −14 LUFS cortava os picos em onda quadrada. Altera `canais` no sítio.
 */
export function limitar(canais: Float32Array[], sr: number, tectoDb = -1, libertacao = 0.08): void {
  const tecto = dbParaGanho(tectoDb)
  const coef = Math.exp(-1 / (libertacao * sr))
  const n = canais[0]?.length ?? 0
  let g = 1
  for (let i = 0; i < n; i++) {
    let m = 0
    for (const c of canais) m = Math.max(m, Math.abs(c[i]))
    const preciso = m > tecto ? tecto / m : 1
    // Desce logo que é preciso; sobe devagar, para o ganho não «bombear».
    g = preciso < g ? preciso : Math.min(preciso, 1 - coef * (1 - g))
    for (const c of canais) c[i] *= g
  }
}

/** Multiplica por um ganho linear. Altera no sítio. */
export function aplicarGanho(canais: Float32Array[], ganho: number): void {
  if (ganho === 1) return
  for (const c of canais) for (let i = 0; i < c.length; i++) c[i] *= ganho
}

/** Normalização de pico a `tectoDb`. Devolve o ganho aplicado. */
export function normalizarPico(canais: Float32Array[], tectoDb = -1): number {
  const p = pico(canais)
  if (p <= 1e-6) return 1
  const g = dbParaGanho(tectoDb) / p
  aplicarGanho(canais, g)
  return g
}

// ---------------------------------------------------------------------------
//  Onda sonora
// ---------------------------------------------------------------------------

/** Pico absoluto por balde, misturando canais — o que a faixa A1 desenha. */
export function picos(canais: Float32Array[], baldes: number): Float32Array {
  const out = new Float32Array(Math.max(0, baldes))
  const n = canais[0]?.length ?? 0
  if (!n || !baldes) return out
  const porBalde = n / baldes
  for (let b = 0; b < baldes; b++) {
    const de = Math.floor(b * porBalde)
    const ate = Math.min(n, Math.max(de + 1, Math.floor((b + 1) * porBalde)))
    let m = 0
    for (const c of canais) for (let i = de; i < ate; i++) m = Math.max(m, Math.abs(c[i]))
    out[b] = m
  }
  return out
}

/** RMS por janela (mistura de canais). */
export function rmsPorJanela(canais: Float32Array[], sr: number, janela = 0.05): Float32Array {
  const porJanela = Math.max(1, Math.round(janela * sr))
  const n = Math.floor((canais[0]?.length ?? 0) / porJanela)
  const out = new Float32Array(n)
  for (let j = 0; j < n; j++) {
    let s = 0
    for (let i = j * porJanela; i < (j + 1) * porJanela; i++) {
      let v = 0
      for (const c of canais) v += c[i]
      v /= canais.length
      s += v * v
    }
    out[j] = Math.sqrt(s / porJanela)
  }
  return out
}

// ---------------------------------------------------------------------------
//  Fala fora do microfone
// ---------------------------------------------------------------------------

export interface TrechoFraco {
  inicio: number
  fim: number
  /** Ganho sugerido para o trecho chegar ao nível da fala normal, em dB. */
  ganhoDb: number
}

/**
 * Trechos em que HÁ fala mas muito abaixo do nível normal — a oradora afastou-se
 * do microfone. Não é silêncio (isso é o `analise.ts`): é som entre o chão de
 * ruído e ~35% da mediana da fala, durante pelo menos `minimo` segundos.
 */
export function trechosForaDoMicrofone(rms: Float32Array, janela: number, minimo = 1.5): TrechoFraco[] {
  const comSom = Array.from(rms).filter((v) => v > 1e-4).sort((a, b) => a - b)
  if (comSom.length < 10) return []
  // Mediana da metade de cima: é a fala; a de baixo mistura pausas e ruído.
  const referencia = comSom[Math.floor(comSom.length * 0.75)]
  const chao = Math.max(referencia * 0.06, 1e-4)
  const tecto = referencia * 0.35
  const out: TrechoFraco[] = []
  let ini: number | null = null
  let soma = 0
  let n = 0
  // Toleram-se buracos curtos (respiração) dentro de um trecho fraco.
  let buraco = 0
  const fecha = (fimJanela: number) => {
    if (ini !== null && (fimJanela - ini) * janela >= minimo && n > 0) {
      const medio = soma / n
      out.push({ inicio: ini * janela, fim: fimJanela * janela, ganhoDb: Math.min(12, Math.round(ganhoParaDb(referencia / medio) * 10) / 10) })
    }
    ini = null
    soma = 0
    n = 0
    buraco = 0
  }
  for (let j = 0; j < rms.length; j++) {
    const v = rms[j]
    const fraco = v > chao && v < tecto
    if (fraco) {
      if (ini === null) ini = j
      soma += v
      n++
      buraco = 0
    } else if (ini !== null) {
      buraco++
      if (v >= tecto || buraco * janela > 0.4) fecha(j - buraco + 1)
    }
  }
  if (ini !== null) fecha(rms.length - buraco)
  return out
}
