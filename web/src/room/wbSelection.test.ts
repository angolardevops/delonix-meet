import { describe, expect, it } from 'vitest'
import type { WbObject } from './wbState'
import {
  alignMoves,
  canEdit,
  distributeMoves,
  expandToGroups,
  groupSelection,
  idsInLasso,
  idsInRect,
  inverseMoves,
  isOneGroup,
  pruneGroups,
  toggleInSelection,
  ungroupSelection,
  unitCount,
} from './wbSelection'

const rect = (id: string, x: number, y: number, w = 0.1, h = 0.1, by = 'ana'): WbObject => ({ id, kind: 'shape', shape: 'rect', pts: [[x, y], [x + w, y + h]], c: '#000', w: 2, by })

const lista = [rect('a', 0.1, 0.1), rect('b', 0.4, 0.15), rect('c', 0.7, 0.3, 0.1, 0.1, 'joao'), rect('d', 0.1, 0.7)]

describe('quadro · selecção múltipla', () => {
  it('caixa: entra o que tem o centro dentro, em qualquer sentido do arrasto', () => {
    expect(idsInRect(lista, [0.05, 0.05], [0.55, 0.3])).toEqual(['a', 'b'])
    expect(idsInRect(lista, [0.55, 0.3], [0.05, 0.05])).toEqual(['a', 'b'])
  })

  it('laço: polígono à volta de a, b e c', () => {
    expect(idsInLasso(lista, [[0.05, 0.05], [0.9, 0.05], [0.9, 0.45], [0.05, 0.45]])).toEqual(['a', 'b', 'c'])
    expect(idsInLasso(lista, [[0, 0], [1, 1]])).toEqual([])
  })

  it('só se escolhe o que o servidor deixa editar (autor ou anfitrião)', () => {
    expect(canEdit(lista[2], 'ana', false)).toBe(false)
    expect(canEdit(lista[2], 'ana', true)).toBe(true)
    expect(canEdit(lista[0], 'ana', false)).toBe(true)
  })
})

describe('quadro · grupos (locais) e alinhar', () => {
  it('agrupar, ⇧clique no grupo e desagrupar', () => {
    const g = groupSelection([], ['a', 'b'], 'G')!
    expect(g).toEqual([{ id: 'G', ids: ['a', 'b'] }])
    expect(groupSelection(g, ['b', 'a'], 'H')).toBeNull()
    expect(isOneGroup(['b', 'a'], g)?.id).toBe('G')
    expect(expandToGroups(['a'], g)).toEqual(['a', 'b'])
    expect(toggleInSelection(['d'], 'b', g)).toEqual(['d', 'b', 'a'])
    expect(toggleInSelection(['d', 'b', 'a'], 'a', g)).toEqual(['d'])
    expect(ungroupSelection(g, ['a'])).toEqual([])
    expect(ungroupSelection(g, ['d'])).toBeNull()
  })

  it('um objecto apagado por alguém sai do grupo; com um só, o grupo acaba', () => {
    const g = [{ id: 'G', ids: ['a', 'b', 'x'] }]
    expect(pruneGroups(g, new Set(['a', 'b']))).toEqual([{ id: 'G', ids: ['a', 'b'] }])
    expect(pruneGroups(g, new Set(['a']))).toEqual([])
  })

  it('alinhar à esquerda move cada objecto — e o grupo como um bloco', () => {
    const g = [{ id: 'G', ids: ['b', 'c'] }]
    const m = alignMoves(lista, ['a', 'b', 'c'], g, 'left')
    // a já está à esquerda; o bloco b+c (x=0.4) vai para 0.1: os dois com o mesmo dx.
    expect(m.map((x) => x.id)).toEqual(['b', 'c'])
    expect(m[0].dx).toBeCloseTo(-0.3)
    expect(m[1].dx).toBeCloseTo(-0.3)
    expect(unitCount(['a', 'b', 'c'], g)).toBe(2)
  })

  it('distribuir na horizontal e o inverso para desfazer', () => {
    const m = distributeMoves(lista, ['a', 'b', 'c'], [], 'x')
    // a: 0.1–0.2, c: 0.7–0.8 → vão 0.5 - 0.1 de b = 0.4 → 0.2 de cada lado → b em 0.4 (já está).
    expect(m).toEqual([])
    const m2 = distributeMoves([rect('a', 0, 0), rect('b', 0.15, 0), rect('c', 0.9, 0)], ['a', 'b', 'c'], [], 'x')
    expect(m2).toHaveLength(1)
    expect(m2[0].dx).toBeCloseTo(0.3)
    expect(inverseMoves(m2)[0].dx).toBeCloseTo(-0.3)
  })
})
