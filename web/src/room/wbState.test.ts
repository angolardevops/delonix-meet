import { describe, expect, it } from 'vitest'
import { aEditar, comObjecto, comTexto, daPagina, movido, objectoEm, semObjecto, type WbObject } from './wbState'

const traco = (id: string, pts: [number, number][], page = 0): WbObject => ({ id, pts, c: '#000', w: 3, kind: 'stroke', page })

describe('quadro como dados', () => {
  it('um id repetido (eco, snapshot) não duplica o objecto', () => {
    let l = comObjecto([], traco('a', [[0, 0], [1, 1]]))
    l = comObjecto(l, traco('a', [[0, 0], [1, 1]]))
    expect(l).toHaveLength(1)
  })

  it('apagar, mover e editar texto actuam só sobre o id', () => {
    let l: WbObject[] = [traco('a', [[0.1, 0.1], [0.2, 0.2]]), { id: 'n', kind: 'note', pts: [[0.5, 0.5]], text: 'x', c: '#000', w: 1 }]
    l = movido(l, 'a', 0.1, -0.05)
    expect(l[0].pts[0][0]).toBeCloseTo(0.2)
    expect(l[0].pts[1][1]).toBeCloseTo(0.15)
    l = comTexto(l, 'n', 'novo')
    expect(l[1].text).toBe('novo')
    l = semObjecto(l, 'a')
    expect(l.map((o) => o.id)).toEqual(['n'])
  })

  it('cada página só mostra os seus objectos (sem página = primeira)', () => {
    const l = [traco('a', [[0, 0], [1, 1]], 0), traco('b', [[0, 0], [1, 1]], 2), { ...traco('c', [[0, 0], [1, 1]]), page: undefined }]
    expect(daPagina(l, 0).map((o) => o.id)).toEqual(['a', 'c'])
    expect(daPagina(l, 2).map((o) => o.id)).toEqual(['b'])
  })

  it('apanha o objecto de CIMA sob o cursor, com tolerância em píxeis', () => {
    const l = [traco('baixo', [[0.1, 0.5], [0.9, 0.5]]), traco('cima', [[0.5, 0.1], [0.5, 0.9]])]
    expect(objectoEm(l, [0.5, 0.5], 1000, 1000)?.id).toBe('cima')
    expect(objectoEm(l, [0.3, 0.505], 1000, 1000)?.id).toBe('baixo')
    expect(objectoEm(l, [0.3, 0.7], 1000, 1000)).toBeNull()
  })

  it('«A editar» mostra quem mexeu nos últimos 20 s, o mais recente primeiro', () => {
    expect(aEditar({ Ana: 1000, Rui: 25_000, Zé: 30_000 }, 40_000)).toEqual(['Zé', 'Rui'])
  })
})
