/**
 * A geometria de uma fonte: o que se guarda, e o que o desenho faz com ela.
 *
 * O caso que motivou isto: um telemóvel em pé entra como 1080×1920 e, rodado
 * um quarto de volta, tem de CABER deitado — a escala calcula-se com os lados
 * trocados. Antes de haver rotação no desenho, endireitar obrigava a relançar
 * a fonte, e o browser largava o dispositivo.
 */
import { beforeAll, describe, expect, it, vi } from 'vitest'
import { desenharImagem } from './desenhoDaMesa'
import { ehDireita, GEOMETRIA_NEUTRA, rodar, sanearGeometria, trocaOsLados } from './geometria'

// Esta bateria corre SEM DOM: o `HTMLVideoElement` do `desenharImagem` não
// existe e o `instanceof` rebenta. Um duplo vazio chega — as fontes do teste
// são canvas, por isso o `instanceof` dá falso, como em produção.
beforeAll(() => {
  if (typeof globalThis.HTMLVideoElement === 'undefined') {
    vi.stubGlobal('HTMLVideoElement', class {})
  }
})

describe('geometria da fonte', () => {
  it('saneia o que vem do armazenamento', () => {
    expect(sanearGeometria(null)).toEqual(GEOMETRIA_NEUTRA)
    expect(sanearGeometria({ rotacao: 90, espelho: true })).toEqual({ rotacao: 90, espelho: true })
    // Um valor que não é um quarto de volta não roda nada — nem rebenta.
    expect(sanearGeometria({ rotacao: 45 }).rotacao).toBe(0)
    expect(sanearGeometria({ rotacao: '180' }).rotacao).toBe(180)
    expect(sanearGeometria({ espelho: 'sim' }).espelho).toBe(false)
  })

  it('roda em ciclo nos dois sentidos', () => {
    expect(rodar(GEOMETRIA_NEUTRA).rotacao).toBe(90)
    expect(rodar({ rotacao: 270, espelho: false }).rotacao).toBe(0)
    expect(rodar(GEOMETRIA_NEUTRA, -1).rotacao).toBe(270)
    // O espelho não se perde ao rodar.
    expect(rodar({ rotacao: 0, espelho: true }).espelho).toBe(true)
  })

  it('só os quartos de volta trocam os lados', () => {
    expect(trocaOsLados({ rotacao: 90, espelho: false })).toBe(true)
    expect(trocaOsLados({ rotacao: 270, espelho: false })).toBe(true)
    expect(trocaOsLados({ rotacao: 180, espelho: false })).toBe(false)
    expect(ehDireita({ rotacao: 0, espelho: true })).toBe(false)
  })
})

/** Um contexto 2D de mentira que regista o que lhe mandam fazer. */
function contextoFalso() {
  const c = {
    fillStyle: '',
    fillRect: vi.fn(),
    save: vi.fn(),
    restore: vi.fn(),
    beginPath: vi.fn(),
    rect: vi.fn(),
    clip: vi.fn(),
    translate: vi.fn(),
    rotate: vi.fn(),
    scale: vi.fn(),
    drawImage: vi.fn(),
  }
  return c as unknown as CanvasRenderingContext2D & typeof c
}

const retrato = () => ({ width: 1080, height: 1920 }) as HTMLCanvasElement
const R = { x: 0, y: 0, w: 1920, h: 1080 }

describe('desenharImagem com geometria', () => {
  it('sem geometria, desenha centrado e não roda', () => {
    const c = contextoFalso()
    desenharImagem(c, retrato(), R, 'cover')
    expect(c.rotate).not.toHaveBeenCalled()
    expect(c.scale).not.toHaveBeenCalled()
    expect(c.drawImage).toHaveBeenCalledTimes(1)
  })

  it('um retrato rodado 90° cobre o rectângulo pelo lado certo', () => {
    const c = contextoFalso()
    desenharImagem(c, retrato(), R, 'cover', { rotacao: 90, espelho: false })
    expect(c.rotate).toHaveBeenCalledWith(Math.PI / 2)
    // `cover` com os lados trocados: a escala sai de 1920/1920 e 1080/1080 — 1.
    // Sem a troca, sairia max(1920/1080, 1080/1920) ≈ 1,78 e cortava.
    const [, , , dw, dh] = c.drawImage.mock.calls[0] as unknown as [unknown, number, number, number, number]
    expect(dw).toBeCloseTo(1080)
    expect(dh).toBeCloseTo(1920)
  })

  it('o espelho vem antes da volta', () => {
    const c = contextoFalso()
    desenharImagem(c, retrato(), R, 'cover', { rotacao: 90, espelho: true })
    const ordem = [c.scale.mock.invocationCallOrder[0], c.rotate.mock.invocationCallOrder[0]]
    expect(ordem[0]).toBeLessThan(ordem[1])
    expect(c.scale).toHaveBeenCalledWith(-1, 1)
  })

  it('180° não troca os lados, só vira', () => {
    const c = contextoFalso()
    desenharImagem(c, retrato(), R, 'contain', { rotacao: 180, espelho: false })
    expect(c.rotate).toHaveBeenCalledWith(Math.PI)
    const [, , , dw] = c.drawImage.mock.calls[0] as unknown as [unknown, number, number, number, number]
    // `contain` de um retrato num rectângulo deitado: limita a ALTURA.
    expect(dw).toBeCloseTo((1080 * 1080) / 1920)
  })
})
