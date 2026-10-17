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

  /**
   * ONDE A IMAGEM ATERRA, e não em que ordem as chamadas saíram.
   *
   * O teste que aqui estava afirmava `scale` antes de `rotate` na ordem de
   * invocação — que é exactamente o que NÃO determina a geometria, porque no
   * canvas as transformações pós-multiplicam e a última a ser chamada é a
   * primeira a actuar. Com isso, fixava o defeito: compunha-se `S·R(θ)`, que é
   * `R(−θ)·S`, e com «espelhar» ligado rodar à direita rodava à esquerda.
   *
   * Agora compõe-se a matriz do que foi chamado e segue-se um ponto conhecido.
   * O discriminador é o meio da ARESTA DIREITA da imagem com espelho e um
   * quarto de volta: com o espelho a actuar primeiro vai para CIMA do centro;
   * com a ordem trocada vai para BAIXO. Um sinal, e distingue as duas.
   */
  const pontoDaArestaDireita = (c: ReturnType<typeof contextoFalso>) => {
    // Matriz do canvas: [a,b,c,d,e,f]; x' = a·x + c·y + e, y' = b·x + d·y + f.
    let m = [1, 0, 0, 1, 0, 0]
    const mul = (n: number[]) => {
      m = [
        m[0] * n[0] + m[2] * n[1],
        m[1] * n[0] + m[3] * n[1],
        m[0] * n[2] + m[2] * n[3],
        m[1] * n[2] + m[3] * n[3],
        m[0] * n[4] + m[2] * n[5] + m[4],
        m[1] * n[4] + m[3] * n[5] + m[5],
      ]
    }
    // Pela ordem em que foram chamadas.
    const chamadas: { ordem: number; f: () => void }[] = [
      ...c.translate.mock.calls.map((a, i) => ({
        ordem: c.translate.mock.invocationCallOrder[i],
        f: () => mul([1, 0, 0, 1, a[0] as number, a[1] as number]),
      })),
      ...c.rotate.mock.calls.map((a, i) => ({
        ordem: c.rotate.mock.invocationCallOrder[i],
        f: () => {
          const t = a[0] as number
          mul([Math.cos(t), Math.sin(t), -Math.sin(t), Math.cos(t), 0, 0])
        },
      })),
      ...c.scale.mock.calls.map((a, i) => ({
        ordem: c.scale.mock.invocationCallOrder[i],
        f: () => mul([a[0] as number, 0, 0, a[1] as number, 0, 0]),
      })),
    ].sort((x, y) => x.ordem - y.ordem)
    for (const { f } of chamadas) f()
    const [, , , dw] = c.drawImage.mock.calls[0] as unknown as [unknown, number, number, number, number]
    // Meio da aresta direita, em coordenadas da imagem centrada na origem.
    const [x, y] = [dw / 2, 0]
    return { x: m[0] * x + m[2] * y + m[4], y: m[1] * x + m[3] * y + m[5] }
  }

  it('com espelho, um quarto de volta leva a aresta direita para CIMA', () => {
    const c = contextoFalso()
    desenharImagem(c, retrato(), R, 'cover', { rotacao: 90, espelho: true })
    const p = pontoDaArestaDireita(c)
    // Centro do rectângulo: (960, 540). Acima é y MENOR.
    expect(p.y).toBeLessThan(540)
    expect(p.x).toBeCloseTo(960)
  })

  it('e sem espelho leva-a para BAIXO — é o espelho que inverte o sentido', () => {
    // O controlo que torna a asserção de cima uma medição: sem ele, um `y`
    // menor que 540 podia ser de qualquer composição.
    const c = contextoFalso()
    desenharImagem(c, retrato(), R, 'cover', { rotacao: 90, espelho: false })
    const p = pontoDaArestaDireita(c)
    expect(p.y).toBeGreaterThan(540)
  })

  it('o espelho continua a ser um espelho horizontal', () => {
    const c = contextoFalso()
    desenharImagem(c, retrato(), R, 'cover', { rotacao: 0, espelho: true })
    expect(c.scale).toHaveBeenCalledWith(-1, 1)
    // Sem volta, a aresta direita vai para a ESQUERDA do centro.
    expect(pontoDaArestaDireita(c).x).toBeLessThan(960)
  })

  /**
   * O CASO QUE FALTAVA. O único `cover` rodado que aqui havia era degenerado:
   * um retrato 1080×1920 num rectângulo 1920×1080 tem os lados trocados
   * exactamente iguais ao destino, logo `min` e `max` dão ambos 1 — e o teste
   * passava igual se se trocasse `Math.max` por `Math.min` no desenho.
   *
   * Um destino que NÃO é 16:9 separa os dois: com os lados trocados
   * (1920×1080) num 1000×800, `contain` limita pela largura (0,5208) e `cover`
   * pela altura (0,7407).
   */
  const RETANGULO_NAO_16_9 = { x: 0, y: 0, w: 1000, h: 800 }

  it('cover e contain divergem num destino que não é 16:9 — e com os lados trocados', () => {
    const contain = contextoFalso()
    desenharImagem(contain, retrato(), RETANGULO_NAO_16_9, 'contain', { rotacao: 90, espelho: false })
    const [, , , dwContain] = contain.drawImage.mock.calls[0] as unknown as [unknown, number, number, number, number]
    expect(dwContain).toBeCloseTo(1080 * (1000 / 1920), 1) // 562,5 — cabe inteira

    const cover = contextoFalso()
    desenharImagem(cover, retrato(), RETANGULO_NAO_16_9, 'cover', { rotacao: 90, espelho: false })
    const [, , , dwCover] = cover.drawImage.mock.calls[0] as unknown as [unknown, number, number, number, number]
    expect(dwCover).toBeCloseTo(1080 * (800 / 1080), 1) // 800 — enche e corta

    // E são DIFERENTES: é isto que falha se o `max`/`min` for trocado.
    expect(dwCover).toBeGreaterThan(dwContain)
  })

  it('270° troca os lados como 90°', () => {
    const c = contextoFalso()
    desenharImagem(c, retrato(), RETANGULO_NAO_16_9, 'contain', { rotacao: 270, espelho: false })
    expect(c.rotate).toHaveBeenCalledWith((270 * Math.PI) / 180)
    const [, , , dw] = c.drawImage.mock.calls[0] as unknown as [unknown, number, number, number, number]
    expect(dw).toBeCloseTo(1080 * (1000 / 1920), 1)
  })

  it('espelho com meia volta: a aresta direita volta à direita', () => {
    // 180° leva a aresta direita para a esquerda; o espelho devolve-a. Sem a
    // composição certa, este caso e o de 0°+espelho não se distinguiriam.
    const c = contextoFalso()
    desenharImagem(c, retrato(), R, 'cover', { rotacao: 180, espelho: true })
    expect(pontoDaArestaDireita(c).x).toBeGreaterThan(960)
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
