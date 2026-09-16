// Palco imersivo — a parte pura: inclinação, paralaxe, micro-movimento,
// transições e a decisão de poder correr.
//
// «4D» é o nome de produto e é SIMULADO: não há profundidade medida. Há duas
// camadas 2D (fundo e pessoa) recortadas pelo segmentador, deslocadas a
// velocidades diferentes conforme o movimento de quem vê — é a paralaxe que o
// cérebro lê como profundidade —, mais desfoque no fundo, luz e sombra. A
// «quarta dimensão» é o tempo: a pessoa mexe-se ligeiramente quando fala e a
// cena entra com uma transição quando o orador muda.

export interface Tilt {
  /** −1 (esquerda) … 1 (direita). */
  x: number
  /** −1 (cima) … 1 (baixo). */
  y: number
}

const clamp = (v: number, lo = -1, hi = 1) => Math.min(hi, Math.max(lo, Number.isFinite(v) ? v : 0))

export function clampTilt(t: Tilt): Tilt {
  return { x: clamp(t.x), y: clamp(t.y) }
}

/** Posição do rato sobre o palco → inclinação. Centro = 0. */
export function pointerToTilt(clientX: number, clientY: number, rect: { left: number; top: number; width: number; height: number }): Tilt {
  if (rect.width <= 0 || rect.height <= 0) return { x: 0, y: 0 }
  return clampTilt({
    x: ((clientX - rect.left) / rect.width - 0.5) * 2,
    y: ((clientY - rect.top) / rect.height - 0.5) * 2,
  })
}

/**
 * Orientação do telemóvel → inclinação, relativa à posição em que estava
 * quando o efeito ligou (`base`). Ninguém segura o telemóvel na vertical
 * perfeita; medir contra o zero absoluto punha o fundo encostado a um canto.
 * `gamma` é o rolar esquerda/direita, `beta` o inclinar para a frente/trás.
 */
export function orientationToTilt(
  beta: number | null,
  gamma: number | null,
  base: { beta: number; gamma: number },
  rangeDeg = 20,
): Tilt | null {
  if (beta == null || gamma == null || !Number.isFinite(beta) || !Number.isFinite(gamma)) return null
  return clampTilt({ x: (gamma - base.gamma) / rangeDeg, y: (beta - base.beta) / rangeDeg })
}

/**
 * Suavização exponencial independente dos fps: `halfLifeMs` é o tempo que
 * leva a percorrer metade da distância. A 30 ou a 120 fps sente-se igual.
 */
export function smoothTilt(prev: Tilt, target: Tilt, dtMs: number, halfLifeMs = 110): Tilt {
  if (dtMs <= 0) return prev
  const k = 1 - Math.pow(0.5, dtMs / Math.max(1, halfLifeMs))
  return { x: prev.x + (target.x - prev.x) * k, y: prev.y + (target.y - prev.y) * k }
}

export interface Vec {
  x: number
  y: number
}

export interface LayerOffsets {
  /** Deslocamento do fundo, em fracção da largura/altura (UV). */
  bg: Vec
  /** Deslocamento da pessoa, em UV. Sentido OPOSTO ao do fundo. */
  fg: Vec
  /** Sombra projectada da pessoa no fundo, em UV. */
  shadow: Vec
  /** Ampliação do fundo para os deslocamentos nunca mostrarem a borda. */
  overscan: number
}

/** Amplitudes máximas, em UV. Pequenas de propósito: é profundidade, não enjoo. */
export const BG_TRAVEL = { x: 0.035, y: 0.025 }
export const FG_TRAVEL = { x: 0.012, y: 0.008 }

/**
 * Paralaxe entre camadas.
 *
 * Um palco visto por uma janela: o que está LONGE (o fundo) acompanha o
 * movimento de quem vê, o que está PERTO (a pessoa, à frente do plano do ecrã)
 * desloca-se no sentido contrário. A diferença entre os dois é o que se lê como
 * profundidade. A sombra cai para baixo e para o lado oposto à luz, e desliza
 * com a pessoa.
 *
 * Com movimento reduzido pedido não há deslocamento nenhum — e por isso o
 * efeito inteiro desliga-se a montante (`immersiveBlock`); isto é a segunda
 * rede.
 */
export function computeParallax(tilt: Tilt, depth = 1, reducedMotion = false): LayerOffsets {
  if (reducedMotion) return { bg: { x: 0, y: 0 }, fg: { x: 0, y: 0 }, shadow: { x: 0.012, y: 0.02 }, overscan: 1 }
  const t = clampTilt(tilt)
  const d = Math.min(1.5, Math.max(0, depth))
  const bg = { x: t.x * BG_TRAVEL.x * d, y: t.y * BG_TRAVEL.y * d }
  const fg = { x: -t.x * FG_TRAVEL.x * d, y: -t.y * FG_TRAVEL.y * d }
  const shadow = { x: 0.012 - t.x * 0.006 * d, y: 0.02 - t.y * 0.004 * d }
  // A ampliação cobre o maior deslocamento dos dois lados: (1 − 1/s)/2 ≥ |bg|.
  const reach = Math.max(Math.abs(bg.x), Math.abs(bg.y))
  const overscan = 1 / (1 - 2 * reach - 0.004)
  return { bg, fg, shadow, overscan }
}

/**
 * Envelope da fala, 0–1, a partir do «está a falar» que a sala já mede
 * (`LevelWatcher`). Sobe depressa e desce devagar, como um VU: o
 * micro-movimento não pode acompanhar cada sílaba, senão treme.
 */
export function speechEnvelope(prev: number, speaking: boolean, dtMs: number, attackMs = 90, releaseMs = 480): number {
  const target = speaking ? 1 : 0
  const tau = speaking ? attackMs : releaseMs
  const k = 1 - Math.exp(-Math.max(0, dtMs) / Math.max(1, tau))
  return Math.min(1, Math.max(0, prev + (target - prev) * k))
}

/**
 * Micro-movimento de quem fala: aproxima-se um pouco (escala) e respira
 * (elevação lenta). Amplitudes de 1 % e 0,4 % — perceptível de canto de olho,
 * nunca ao ponto de o texto de um quadro por trás dançar.
 */
export function microMotion(envelope: number, tSec: number, reducedMotion = false): { scale: number; lift: number } {
  if (reducedMotion) return { scale: 1, lift: 0 }
  const e = Math.min(1, Math.max(0, envelope))
  return { scale: 1 + 0.012 * e, lift: -0.004 * e * (0.5 + 0.5 * Math.sin(tSec * Math.PI * 2 * 0.7)) }
}

/**
 * Transição de cena (orador novo, ou o efeito acabou de ligar): a imagem entra
 * ligeiramente ampliada e escurecida e assenta. Curva ease-out cúbica.
 */
export function sceneTransition(elapsedMs: number, durationMs = 700): { progress: number; scale: number; fade: number } {
  const p = Math.min(1, Math.max(0, durationMs > 0 ? elapsedMs / durationMs : 1))
  const e = 1 - Math.pow(1 - p, 3)
  return { progress: p, scale: 1.05 - 0.05 * e, fade: 0.4 * (1 - e) }
}

export type TiltSource = 'head' | 'orientation' | 'pointer'

/**
 * Que sinal guia a paralaxe. A cabeça ganha — é a única que dá a sensação de
 * janela —, mas só enquanto está mesmo a detectar uma cara: uma câmara ligada
 * a apontar para o tecto não pode congelar o efeito. Depois a orientação do
 * telemóvel, se está a chegar. O rato é o que sobra.
 */
export function chooseTiltSource(now: number, lastHeadAt: number | null, lastOrientationAt: number | null, freshMs = 1000): TiltSource {
  if (lastHeadAt != null && now - lastHeadAt <= freshMs) return 'head'
  if (lastOrientationAt != null && now - lastOrientationAt <= freshMs) return 'orientation'
  return 'pointer'
}

export interface ImmersiveEnv {
  reducedMotion: boolean
  saveData: boolean
  batteryLow: boolean
  webgl2: boolean
  cpuLimited?: boolean
}

export type ImmersiveBlock = 'reducedMotion' | 'saveData' | 'battery' | 'noWebgl' | 'cpu'

/**
 * Pode correr? Pura. Movimento reduzido primeiro: é uma preferência de
 * acessibilidade — há quem fique maldisposto com paralaxe — e não se negocia.
 */
export function immersiveBlock(env: ImmersiveEnv): ImmersiveBlock | null {
  if (env.reducedMotion) return 'reducedMotion'
  if (!env.webgl2) return 'noWebgl'
  if (env.saveData) return 'saveData'
  if (env.batteryLow) return 'battery'
  if (env.cpuLimited) return 'cpu'
  return null
}

/**
 * Confiança do segmentador → alfa (0–255), com a MESMA rampa do
 * `BackgroundEffect` (0,30…0,62 com smoothstep): inclusiva no limite, para o
 * cabelo — a zona de menor confiança do modelo leve — não ficar de fora.
 * `prev` mistura com a máscara anterior: sem isso a borda cintila com a
 * paralaxe a ampliar cada hesitação do modelo.
 */
export function confidenceToAlpha(conf: ArrayLike<number>, out: Uint8Array, prev: Uint8Array | null = null, keep = 0.35): Uint8Array {
  const len = Math.min(conf.length, out.length)
  for (let i = 0; i < len; i++) {
    const a = Math.min(1, Math.max(0, (conf[i] - 0.3) / 0.32))
    let v = a * a * (3 - 2 * a) * 255
    if (prev) v = prev[i] * keep + v * (1 - keep)
    out[i] = Math.round(v)
  }
  return out
}

/** Fracção da máscara ocupada pela pessoa (alfa > 50 %). */
export function maskCoverage(mask: Uint8Array): number {
  if (mask.length === 0) return 0
  let c = 0
  for (let i = 0; i < mask.length; i++) if (mask[i] > 127) c++
  return c / mask.length
}
