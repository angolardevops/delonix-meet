// Realce de nitidez na RECEPÇÃO — a parte pura (parâmetros, medida, decisão).
// O desenho em WebGL2 vive em `sharpenGl.ts`.
//
// O que é, dito sem vender: uma máscara de nitidez (unsharp mask). Subtrai-se
// ao frame uma versão desfocada dele, e a diferença — as arestas — soma-se de
// volta, amplificada. Aumenta o contraste LOCAL das arestas que existem, e é
// isso que o olho lê como «mais nítido». Não inventa detalhe: o que a
// compressão deitou fora não volta, e em blocos de compressão o filtro
// realçaria o próprio artefacto — daí o limiar, que deixa passar sem mexer as
// diferenças pequenas (ruído e blocos) e só actua em arestas a sério.
//
// Onde NÃO corre, por desenho: nunca na gravação nem no servidor. O filtro
// desenha num `<canvas>` por cima do `<video>` deste ecrã e em mais sítio
// nenhum; a gravação compõe a partir das tracks (ver `room/compositor.ts`) e o
// servidor nunca vê pixels descodificados. Um teste de invariante guarda isto.

/**
 * Pesos gaussianos do núcleo 3×3 amostrado a `radius` píxeis de saída:
 * centro, os quatro vizinhos em cruz e os quatro em diagonal (a √2·radius).
 * Normalizados — somam 1 —, senão o «desfocado» mudava o brilho e a diferença
 * que se soma de volta deixava de ser só aresta.
 */
export function kernel3x3(radius: number): { center: number; edge: number; corner: number } {
  const r = Math.max(0.5, radius)
  // σ proporcional ao raio: o núcleo mantém a mesma forma a qualquer escala.
  const sigma = r * 0.85
  const g = (d: number) => Math.exp(-(d * d) / (2 * sigma * sigma))
  const c = 1
  const e = g(r)
  const k = g(r * Math.SQRT2)
  const sum = c + 4 * e + 4 * k
  return { center: c / sum, edge: e / sum, corner: k / sum }
}

export interface UnsharpParams {
  /** Quanto da aresta se soma de volta. 0 = filtro sem efeito. */
  amount: number
  /** Raio do desfoque, em píxeis de SAÍDA. */
  radius: number
  /** Diferença de luma abaixo da qual não se realça (ruído, blocos). */
  threshold: number
}

export const MAX_AMOUNT = 1.5

/**
 * Parâmetros a partir da intensidade escolhida (0–1) e da escala a que o vídeo
 * está a ser desenhado.
 *
 * O raio é em píxeis de saída porque o filtro corre à resolução do ecrã: um
 * vídeo de 640×360 ampliado para um palco de 1280×720 tem arestas com 2 píxeis
 * de largura, e um raio de 1 realçaria só o meio delas. Ao ampliar sobe também o
 * limiar — a ampliação torna os blocos de compressão maiores e mais visíveis, e
 * é exactamente onde o filtro faria pior figura.
 */
export function unsharpParams(strength: number, srcHeight: number, dstHeight: number): UnsharpParams {
  const s = Math.min(1, Math.max(0, Number.isFinite(strength) ? strength : 0))
  const scale = srcHeight > 0 && dstHeight > 0 ? dstHeight / srcHeight : 1
  const radius = Math.min(3, Math.max(1, scale))
  const threshold = scale > 1.5 ? 0.03 : 0.015
  return { amount: s * MAX_AMOUNT, radius, threshold }
}

export type ObjectFit = 'cover' | 'contain'

/**
 * Tamanho do canvas para desenhar o vídeo a 1:1 com o ecrã.
 *
 * O canvas mantém a PROPORÇÃO do vídeo e leva o mesmo `object-fit` que o
 * `<video>` por baixo — assim os dois ocupam exactamente o mesmo sítio, com o
 * mesmo corte ou as mesmas barras, sem se recalcular o enquadramento à mão. O
 * tecto de píxeis protege um palco em ecrã inteiro num monitor 4K: correr o
 * filtro a 8 milhões de píxeis por frame não se justifica.
 */
export function fitCanvasSize(
  videoW: number,
  videoH: number,
  boxW: number,
  boxH: number,
  dpr: number,
  fit: ObjectFit,
  maxPixels = 2560 * 1440,
): { width: number; height: number } {
  if (videoW <= 0 || videoH <= 0 || boxW <= 0 || boxH <= 0) return { width: 1, height: 1 }
  const scale = fit === 'cover' ? Math.max(boxW / videoW, boxH / videoH) : Math.min(boxW / videoW, boxH / videoH)
  let w = videoW * scale * Math.max(1, dpr)
  let h = videoH * scale * Math.max(1, dpr)
  const px = w * h
  if (px > maxPixels) {
    const k = Math.sqrt(maxPixels / px)
    w *= k
    h *= k
  }
  return { width: Math.max(1, Math.round(w)), height: Math.max(1, Math.round(h)) }
}

/* ------------------------------------------------------------------ */
/* Custo medido e desligamento automático                              */
/* ------------------------------------------------------------------ */

export interface FrameSample {
  /** Instante do frame, ms. */
  t: number
  /** Tempo de trabalho do frame, ms. `null` = frame não cronometrado. */
  ms: number | null
}

export interface CostSummary {
  /** Frames desenhados por segundo na janela. */
  fps: number
  /** Mediana e p95 do tempo por frame, ms. */
  p50Ms: number
  p95Ms: number
  /** Frames cronometrados na janela. */
  timed: number
}

/** Resume a janela `[now - windowMs, now]`. */
export function summarizeCost(samples: FrameSample[], now: number, windowMs = 2000): CostSummary {
  const inWin = samples.filter((s) => s.t > now - windowMs && s.t <= now)
  const times = inWin.map((s) => s.ms).filter((v): v is number => v != null).sort((a, b) => a - b)
  const pick = (q: number) => (times.length ? times[Math.min(times.length - 1, Math.floor(q * times.length))] : 0)
  return {
    fps: Math.round((inWin.length * 1000) / windowMs),
    p50Ms: Math.round(pick(0.5) * 10) / 10,
    p95Ms: Math.round(pick(0.95) * 10) / 10,
    timed: times.length,
  }
}

export interface Budget {
  /** p95 do tempo por frame acima do qual o efeito custa demais, ms. */
  maxP95Ms: number
  /** Fracção mínima dos fps da fonte que o efeito tem de acompanhar. */
  minFpsRatio: number
  /** Quanto tempo seguido acima do orçamento antes de desligar, ms. */
  graceMs: number
}

/**
 * Orçamento do realce: a 30 fps um frame tem 33 ms, e um filtro de nitidez
 * não pode levar mais de um quarto disso sem roubar ao descodificador.
 */
export const SHARPEN_BUDGET: Budget = { maxP95Ms: 8, minFpsRatio: 0.75, graceMs: 3000 }

/** O palco imersivo inclui o recorte da pessoa: orçamento maior, mesma regra. */
export const IMMERSIVE_BUDGET: Budget = { maxP95Ms: 20, minFpsRatio: 0.6, graceMs: 4000 }

export type BudgetVerdict = { overSince: number | null; disable: boolean; why: 'slow' | 'fps' | null }

/**
 * Decide se o dispositivo aguenta. Pura.
 *
 * Duas formas de não aguentar, e qualquer delas conta: cada frame demora
 * demais, ou os frames nem chegam a ser desenhados ao ritmo a que o vídeo os
 * entrega. Um pico não desliga nada — tem de durar `graceMs`. Sem frames
 * cronometrados não se julga o tempo; sem fps da fonte não se julga o ritmo.
 */
export function budgetVerdict(
  cost: CostSummary,
  sourceFps: number,
  prevOverSince: number | null,
  now: number,
  budget: Budget,
): BudgetVerdict {
  const slow = cost.timed >= 3 && cost.p95Ms > budget.maxP95Ms
  const lagging = sourceFps >= 5 && cost.fps < sourceFps * budget.minFpsRatio
  if (!slow && !lagging) return { overSince: null, disable: false, why: null }
  const overSince = prevOverSince ?? now
  return { overSince, disable: now - overSince >= budget.graceMs, why: slow ? 'slow' : 'fps' }
}
