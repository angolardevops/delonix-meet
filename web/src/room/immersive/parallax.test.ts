import { describe, expect, it } from 'vitest'
import {
  BG_TRAVEL,
  chooseTiltSource,
  computeParallax,
  confidenceToAlpha,
  immersiveBlock,
  maskCoverage,
  microMotion,
  orientationToTilt,
  pointerToTilt,
  sceneTransition,
  smoothTilt,
  speechEnvelope,
} from './parallax'

const rect = { left: 100, top: 50, width: 800, height: 400 }

describe('entradas de inclinação', () => {
  it('rato: centro 0, cantos ±1, fora do palco corta', () => {
    expect(pointerToTilt(500, 250, rect)).toEqual({ x: 0, y: 0 })
    expect(pointerToTilt(100, 50, rect)).toEqual({ x: -1, y: -1 })
    expect(pointerToTilt(900, 450, rect)).toEqual({ x: 1, y: 1 })
    expect(pointerToTilt(5000, -99, rect)).toEqual({ x: 1, y: -1 })
    expect(pointerToTilt(1, 1, { ...rect, width: 0 })).toEqual({ x: 0, y: 0 })
  })
  it('orientação: relativa à posição inicial, e nula sem sensor', () => {
    expect(orientationToTilt(40, 0, { beta: 40, gamma: 0 })).toEqual({ x: 0, y: 0 })
    expect(orientationToTilt(50, -10, { beta: 40, gamma: 0 })).toEqual({ x: -0.5, y: 0.5 })
    expect(orientationToTilt(90, 90, { beta: 40, gamma: 0 })).toEqual({ x: 1, y: 1 })
    expect(orientationToTilt(null, 3, { beta: 0, gamma: 0 })).toBeNull()
  })
  it('suavização independente dos fps', () => {
    const alvo = { x: 1, y: 0 }
    // Meio-tempo inteiro de uma vez ou em dois passos dá o mesmo.
    const um = smoothTilt({ x: 0, y: 0 }, alvo, 110, 110)
    const dois = smoothTilt(smoothTilt({ x: 0, y: 0 }, alvo, 55, 110), alvo, 55, 110)
    expect(um.x).toBeCloseTo(0.5, 6)
    expect(dois.x).toBeCloseTo(0.5, 6)
    expect(smoothTilt({ x: 0.3, y: 0 }, alvo, 0)).toEqual({ x: 0.3, y: 0 })
  })
  it('a cabeça só manda enquanto detecta; depois orientação; depois rato', () => {
    expect(chooseTiltSource(5000, 4500, 4900)).toBe('head')
    expect(chooseTiltSource(5000, 3000, 4900)).toBe('orientation')
    expect(chooseTiltSource(5000, 3000, 3000)).toBe('pointer')
    expect(chooseTiltSource(5000, null, null)).toBe('pointer')
  })
})

describe('computeParallax — profundidade entre camadas', () => {
  it('parado ao centro: nada se desloca', () => {
    const p = computeParallax({ x: 0, y: 0 })
    expect(p.bg).toEqual({ x: 0, y: 0 })
    expect(Math.abs(p.fg.x) + Math.abs(p.fg.y)).toBe(0)
  })
  it('fundo e pessoa andam em sentidos opostos, e o fundo anda mais', () => {
    const p = computeParallax({ x: 1, y: -1 })
    expect(p.bg.x).toBeGreaterThan(0)
    expect(p.fg.x).toBeLessThan(0)
    expect(p.bg.y).toBeLessThan(0)
    expect(p.fg.y).toBeGreaterThan(0)
    expect(Math.abs(p.bg.x)).toBeGreaterThan(Math.abs(p.fg.x))
    expect(p.bg.x).toBeCloseTo(BG_TRAVEL.x, 10)
  })
  it('a ampliação do fundo cobre SEMPRE o deslocamento (nunca se vê a borda)', () => {
    for (const x of [-1, -0.4, 0, 0.7, 1]) {
      for (const d of [0.5, 1, 1.5]) {
        const p = computeParallax({ x, y: x }, d)
        const margem = (1 - 1 / p.overscan) / 2
        expect(margem).toBeGreaterThanOrEqual(Math.max(Math.abs(p.bg.x), Math.abs(p.bg.y)))
      }
    }
  })
  it('entradas fora do intervalo não fazem o fundo fugir', () => {
    expect(computeParallax({ x: 40, y: Number.NaN }).bg).toEqual(computeParallax({ x: 1, y: 0 }).bg)
  })
  it('movimento reduzido: zero deslocamento e zero micro-movimento', () => {
    const p = computeParallax({ x: 1, y: 1 }, 1, true)
    expect(p.bg).toEqual({ x: 0, y: 0 })
    expect(p.fg).toEqual({ x: 0, y: 0 })
    expect(p.overscan).toBe(1)
    expect(microMotion(1, 3, true)).toEqual({ scale: 1, lift: 0 })
  })
})

describe('quarta dimensão — tempo', () => {
  it('envelope da fala: sobe depressa, desce devagar', () => {
    const subida = speechEnvelope(0, true, 100)
    const descida = 1 - speechEnvelope(1, false, 100)
    expect(subida).toBeGreaterThan(descida)
    let e = 0
    for (let i = 0; i < 30; i++) e = speechEnvelope(e, true, 33)
    expect(e).toBeGreaterThan(0.99)
    expect(speechEnvelope(0.5, false, -5)).toBe(0.5)
  })
  it('micro-movimento subtil: no máximo 1,2 % de escala', () => {
    expect(microMotion(0, 1)).toEqual({ scale: 1, lift: -0 })
    const m = microMotion(1, 0.3)
    expect(m.scale).toBeCloseTo(1.012, 6)
    expect(Math.abs(m.lift)).toBeLessThanOrEqual(0.004)
  })
  it('transição de cena assenta em escala 1 e sem escurecer', () => {
    expect(sceneTransition(0)).toMatchObject({ progress: 0, scale: 1.05, fade: 0.4 })
    expect(sceneTransition(700)).toEqual({ progress: 1, scale: 1, fade: 0 })
    expect(sceneTransition(350).scale).toBeLessThan(1.05)
    expect(sceneTransition(10, 0).progress).toBe(1)
  })
})

describe('immersiveBlock — acessibilidade e bateria antes do efeito', () => {
  const ok = { reducedMotion: false, saveData: false, batteryLow: false, webgl2: true }
  it('sem impedimentos corre', () => expect(immersiveBlock(ok)).toBeNull())
  it('movimento reduzido ganha a tudo', () => {
    expect(immersiveBlock({ ...ok, reducedMotion: true, webgl2: false, batteryLow: true })).toBe('reducedMotion')
  })
  it('cada impedimento tem o seu motivo', () => {
    expect(immersiveBlock({ ...ok, webgl2: false })).toBe('noWebgl')
    expect(immersiveBlock({ ...ok, saveData: true })).toBe('saveData')
    expect(immersiveBlock({ ...ok, batteryLow: true })).toBe('battery')
    expect(immersiveBlock({ ...ok, cpuLimited: true })).toBe('cpu')
  })
})

describe('máscara', () => {
  it('mesma rampa do BackgroundEffect: 0,30 → 0, 0,62 → 255, suave no meio', () => {
    const out = confidenceToAlpha(new Float32Array([0, 0.3, 0.46, 0.62, 1]), new Uint8Array(5))
    expect([...out]).toEqual([0, 0, 128, 255, 255])
  })
  it('mistura temporal com a máscara anterior', () => {
    const out = confidenceToAlpha(new Float32Array([1]), new Uint8Array(1), new Uint8Array([0]), 0.5)
    expect(out[0]).toBe(128)
  })
  it('cobertura', () => {
    expect(maskCoverage(new Uint8Array([0, 200, 255, 10]))).toBe(0.5)
    expect(maskCoverage(new Uint8Array(0))).toBe(0)
  })
})
