import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'
import { describe, expect, it } from 'vitest'
import {
  budgetVerdict,
  fitCanvasSize,
  IMMERSIVE_BUDGET,
  kernel3x3,
  MAX_AMOUNT,
  SHARPEN_BUDGET,
  summarizeCost,
  unsharpParams,
  type FrameSample,
} from './sharpen'

describe('kernel3x3 — o desfocado não muda o brilho', () => {
  for (const r of [0.5, 1, 1.7, 3]) {
    it(`raio ${r}: pesos somam 1 e decrescem com a distância`, () => {
      const k = kernel3x3(r)
      expect(k.center + 4 * k.edge + 4 * k.corner).toBeCloseTo(1, 10)
      expect(k.center).toBeGreaterThan(k.edge)
      expect(k.edge).toBeGreaterThan(k.corner)
      expect(k.corner).toBeGreaterThan(0)
    })
  }
  it('a forma não depende da escala (σ proporcional ao raio)', () => {
    expect(kernel3x3(1)).toEqual(kernel3x3(2))
  })
})

describe('unsharpParams', () => {
  it('intensidade 0 = sem efeito; 1 = máximo; fora do intervalo corta', () => {
    expect(unsharpParams(0, 720, 720).amount).toBe(0)
    expect(unsharpParams(1, 720, 720).amount).toBe(MAX_AMOUNT)
    expect(unsharpParams(7, 720, 720).amount).toBe(MAX_AMOUNT)
    expect(unsharpParams(-1, 720, 720).amount).toBe(0)
    expect(unsharpParams(Number.NaN, 720, 720).amount).toBe(0)
  })
  it('o raio segue a ampliação, entre 1 e 3 píxeis de saída', () => {
    expect(unsharpParams(0.5, 720, 720).radius).toBe(1)
    expect(unsharpParams(0.5, 360, 720).radius).toBe(2)
    expect(unsharpParams(0.5, 180, 1440).radius).toBe(3)
    expect(unsharpParams(0.5, 1080, 540).radius).toBe(1)
  })
  it('ao ampliar muito, o limiar sobe (blocos de compressão ficam maiores)', () => {
    expect(unsharpParams(0.5, 360, 720).threshold).toBeGreaterThan(unsharpParams(0.5, 720, 720).threshold)
  })
  it('dimensões desconhecidas não rebentam', () => {
    expect(unsharpParams(0.5, 0, 0).radius).toBe(1)
  })
})

describe('fitCanvasSize — o canvas cobre o vídeo a 1:1 com o ecrã', () => {
  it('cover: escala pelo lado que preenche', () => {
    expect(fitCanvasSize(1280, 720, 800, 800, 1, 'cover')).toEqual({ width: 1422, height: 800 })
  })
  it('contain: escala pelo lado que cabe', () => {
    expect(fitCanvasSize(1280, 720, 800, 800, 1, 'contain')).toEqual({ width: 800, height: 450 })
  })
  it('mantém a proporção do vídeo e aplica o dpr', () => {
    const s = fitCanvasSize(640, 480, 400, 300, 2, 'contain')
    expect(s).toEqual({ width: 800, height: 600 })
  })
  it('respeita o tecto de píxeis sem mudar a proporção', () => {
    const s = fitCanvasSize(1920, 1080, 3840, 2160, 2, 'cover', 1920 * 1080)
    expect(s.width * s.height).toBeLessThanOrEqual(1920 * 1080 + 4000)
    expect(s.width / s.height).toBeCloseTo(16 / 9, 2)
  })
  it('sem dimensões: 1×1, nunca 0', () => {
    expect(fitCanvasSize(0, 0, 100, 100, 1, 'cover')).toEqual({ width: 1, height: 1 })
  })
})

describe('summarizeCost e budgetVerdict — desligar quando o dispositivo não aguenta', () => {
  const frames = (n: number, ms: number | null, fps = 30, t0 = 0): FrameSample[] =>
    Array.from({ length: n }, (_, i) => ({ t: t0 + (i * 1000) / fps, ms }))

  it('fps e percentis na janela', () => {
    const s = summarizeCost(frames(60, 4), 1990)
    expect(s.fps).toBe(30)
    expect(s.p50Ms).toBe(4)
    expect(s.timed).toBe(60)
  })
  it('dentro do orçamento: nada acontece', () => {
    const v = budgetVerdict({ fps: 30, p50Ms: 2, p95Ms: 3, timed: 10 }, 30, null, 0, SHARPEN_BUDGET)
    expect(v).toEqual({ overSince: null, disable: false, why: null })
  })
  it('lento mas por pouco tempo: marca, não desliga', () => {
    const v = budgetVerdict({ fps: 30, p50Ms: 9, p95Ms: 12, timed: 10 }, 30, null, 1000, SHARPEN_BUDGET)
    expect(v.overSince).toBe(1000)
    expect(v.disable).toBe(false)
    expect(v.why).toBe('slow')
  })
  it('lento durante o período de graça: desliga', () => {
    const v = budgetVerdict({ fps: 30, p50Ms: 9, p95Ms: 12, timed: 10 }, 30, 1000, 1000 + SHARPEN_BUDGET.graceMs, SHARPEN_BUDGET)
    expect(v.disable).toBe(true)
  })
  it('não acompanha os fps da fonte: também conta', () => {
    const v = budgetVerdict({ fps: 12, p50Ms: 1, p95Ms: 2, timed: 10 }, 30, 0, 5000, SHARPEN_BUDGET)
    expect(v).toMatchObject({ disable: true, why: 'fps' })
  })
  it('sem fps da fonte ou sem frames cronometrados, não se julga às cegas', () => {
    expect(budgetVerdict({ fps: 0, p50Ms: 0, p95Ms: 0, timed: 0 }, 0, null, 0, SHARPEN_BUDGET).overSince).toBeNull()
    expect(budgetVerdict({ fps: 30, p50Ms: 50, p95Ms: 50, timed: 2 }, 30, null, 0, SHARPEN_BUDGET).overSince).toBeNull()
  })
  it('o palco imersivo tem orçamento maior (inclui o recorte)', () => {
    expect(IMMERSIVE_BUDGET.maxP95Ms).toBeGreaterThan(SHARPEN_BUDGET.maxP95Ms)
  })
})

describe('invariante: o realce é só de ecrã — nunca no caminho de gravação nem de envio', () => {
  // O filtro desenha por cima de um <video> deste ecrã. Se alguém o ligar à
  // gravação, ao compositor ou ao envio, os outros passariam a receber (ou a
  // gravação a guardar) um vídeo alterado sem o saberem.
  const src = join(__dirname, '..')
  const files: string[] = []
  const walk = (d: string) => {
    for (const f of readdirSync(d)) {
      const p = join(d, f)
      if (statSync(p).isDirectory()) walk(p)
      else if (/\.(ts|tsx)$/.test(f) && !/\.test\.tsx?$/.test(f)) files.push(p)
    }
  }
  walk(src)
  const proibidos = /(record|Recorder|compositor|webrtc|signaling|studio\/|Studio|matte|multicam|Multicam)/i
  it('nenhum módulo de gravação, composição ou envio importa o realce ou o palco imersivo', () => {
    const maus = files
      .filter((f) => proibidos.test(relative(src, f)))
      .filter((f) => /from ['"][^'"]*(media\/sharpen|sharpenGl|immersive\/)/.test(readFileSync(f, 'utf8')))
      .map((f) => relative(src, f))
    expect(maus).toEqual([])
  })
})
