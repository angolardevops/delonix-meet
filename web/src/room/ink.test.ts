import { describe, expect, it } from 'vitest'
import { corDeMarcador, endireitar, espessura, grausDeInclinacao, limitarPontos, pontosDaForma, suavizar, type Pt } from './ink'

describe('tinta do quadro (cliente)', () => {
  it('o marcador cabe no limite de 24 caracteres da cor', () => {
    for (const hex of ['#0b0b0c', '#ad1017', '#ffffff']) {
      const c = corDeMarcador(hex)
      expect(c).toMatch(/^rgba\(\d+,\d+,\d+,0\.35\)$/)
      expect(c.length).toBeLessThanOrEqual(24)
    }
  })

  it('suavizar mantém as pontas e aproxima os pontos do meio', () => {
    const zig: Pt[] = [[0, 0], [0.1, 0.1], [0.2, 0], [0.3, 0.1], [0.4, 0]]
    const s = suavizar(zig, 1)
    expect(s[0]).toEqual([0, 0])
    expect(s[4]).toEqual([0.4, 0])
    expect(Math.abs(s[1][1])).toBeLessThan(0.1)
    expect(suavizar(zig, 0)).toBe(zig)
  })

  it('nunca envia mais pontos do que o servidor aceita', () => {
    const muitos: Pt[] = Array.from({ length: 5000 }, (_, i) => [i / 5000, 0])
    const r = limitarPontos(muitos)
    expect(r).toHaveLength(2000)
    expect(r[0]).toEqual([0, 0])
    expect(r[1999]).toEqual(muitos[4999])
  })

  it('pressão e inclinação decidem a espessura do traço, com tecto', () => {
    expect(espessura(4, [], 1, false)).toBe(4)
    expect(espessura(4, [{ pressao: 0.5, inclinacao: 0 }], 1, false)).toBe(4) // rato
    expect(espessura(4, [{ pressao: 1, inclinacao: 0 }], 1, false)).toBe(8)
    expect(espessura(4, [{ pressao: 1, inclinacao: 0 }], 0, false)).toBe(4) // sem sensibilidade
    expect(espessura(4, [{ pressao: 0, inclinacao: 0 }], 1, false)).toBe(1.6) // mínimo 40 %
    expect(espessura(4, [{ pressao: 1, inclinacao: 90 }], 1, true)).toBe(12) // tecto 3×
    expect(grausDeInclinacao(30, 40)).toBe(50)
  })

  it('endireita uma linha tremida', () => {
    const tremida: Pt[] = Array.from({ length: 20 }, (_, i) => [0.1 + i * 0.02, 0.5 + (i % 2 ? 0.004 : -0.004)])
    expect(endireitar(tremida)?.forma).toBe('linha')
    expect(endireitar(tremida)?.pts).toHaveLength(2)
  })

  it('reconhece rectângulo e elipse fechados, e deixa o resto como está', () => {
    const rect: Pt[] = [
      ...Array.from({ length: 10 }, (_, i) => [0.2 + i * 0.04, 0.2] as Pt),
      ...Array.from({ length: 10 }, (_, i) => [0.6, 0.2 + i * 0.03] as Pt),
      ...Array.from({ length: 10 }, (_, i) => [0.6 - i * 0.04, 0.5] as Pt),
      ...Array.from({ length: 10 }, (_, i) => [0.2, 0.5 - i * 0.03] as Pt),
    ]
    expect(endireitar(rect)?.forma).toBe('rect')
    const circ: Pt[] = Array.from({ length: 40 }, (_, i) => {
      const t = (i / 39) * Math.PI * 2
      return [0.5 + 0.2 * Math.cos(t), 0.5 + 0.2 * Math.sin(t)]
    })
    expect(endireitar(circ)?.forma).toBe('elipse')
    const rabisco: Pt[] = [[0.1, 0.1], [0.5, 0.4], [0.2, 0.6], [0.7, 0.2], [0.3, 0.9], [0.6, 0.6], [0.15, 0.3]]
    expect(endireitar(rabisco)).toBeNull()
  })

  it('formas por arrasto', () => {
    expect(pontosDaForma('rect', [0, 0], [1, 1])).toHaveLength(5)
    expect(pontosDaForma('linha', [0, 0], [1, 1])).toHaveLength(2)
    expect(pontosDaForma('seta', [0, 0], [1, 0])).toHaveLength(5)
    const e = pontosDaForma('elipse', [0, 0], [1, 1])
    expect(e[0][0]).toBeCloseTo(1)
    expect(e[48][0]).toBeCloseTo(1)
  })
})
