import { describe, expect, it } from 'vitest'
import { parseJson, toJson } from './exporters'
import { contentBox, lassoPick, moveSelection, pointInPolygon } from './geometry'
import { emptyDoc, makeNode, PALETTES, Stroke } from './model'
import { quickShapePath, strokeD } from './shapes'

const square = [0, 0, 100, 0, 100, 100, 0, 100]

describe('livre · laço, mover e formas rápidas', () => {
  it('ponto dentro de um polígono', () => {
    expect(pointInPolygon({ x: 50, y: 50 }, square)).toBe(true)
    expect(pointInPolygon({ x: 150, y: 50 }, square)).toBe(false)
  })

  it('o laço apanha traços com a maioria dos pontos dentro e elementos pelo centro', () => {
    const doc = {
      nodes: [makeNode('sticky', 10, 10, '', {}, 'n1'), makeNode('sticky', 300, 300, '', {}, 'n2')],
      strokes: [
        { id: 'a', points: [10, 10, 20, 20, 30, 30], color: '#000', width: 2 },
        { id: 'b', points: [90, 90, 200, 200, 300, 300], color: '#000', width: 2 },
      ] as Stroke[],
    }
    // O post-it tem 150×120: centro em (85, 70), dentro de um quadrado 0..200.
    expect(lassoPick(doc, [0, 0, 200, 0, 200, 200, 0, 200])).toEqual({ nodes: ['n1'], strokes: ['a'] })
    expect(lassoPick(doc, [0, 0, 1, 1])).toEqual({ nodes: [], strokes: [] })
  })

  it('mover desloca elementos e todos os pontos dos traços, sem tocar no resto', () => {
    const doc = { ...emptyDoc('d', 't', 'free'), nodes: [makeNode('sticky', 0, 0, '', {}, 'n1'), makeNode('text', 5, 5, 'x', {}, 'n2')], strokes: [{ id: 's', points: [1, 2, 3, 4], color: '#000', width: 2 }] }
    const moved = moveSelection(doc, { nodes: ['n1'], strokes: ['s'] }, 10, -4)
    expect(moved.nodes[0]).toMatchObject({ x: 10, y: -4 })
    expect(moved.nodes[1]).toMatchObject({ x: 5, y: 5 })
    expect(moved.strokes[0].points).toEqual([11, -2, 13, 0])
  })

  it('formas rápidas: rectângulo normalizado, elipse fechada, seta com ponta', () => {
    expect(quickShapePath({ shape: 'rect', points: [100, 80, 20, 10] })).toBe('M20 10h80v70h-80Z')
    expect(quickShapePath({ shape: 'ellipse', points: [0, 0, 40, 20] })).toMatch(/^M0 10a20 10 0 1 0 40 0a20 10 0 1 0 -40 0Z$/)
    expect(quickShapePath({ shape: 'arrow', points: [0, 0, 100, 0] }).match(/L100 0/g)).toHaveLength(2)
    expect(strokeD({ points: [0, 0, 5, 5] })).toBe('M0 0L5 5')
  })

  it('traços com opacidade e forma sobrevivem ao modelo exportado e entram no enquadramento', () => {
    const doc = { ...emptyDoc('d', 't', 'free'), strokes: [{ id: 's', points: [0, 0, 50, 30], color: '#ad1017', width: 16, opacity: 0.35, shape: 'rect' as const }] }
    const back = parseJson(toJson(doc))!
    expect(back.strokes[0]).toMatchObject({ opacity: 0.35, shape: 'rect', width: 16 })
    expect(contentBox(back)).toEqual({ x: 0, y: 0, w: 50, h: 30 })
  })

  it('a paleta Livre tem marcador, laço, quatro formas e post-its de cores', () => {
    const items = PALETTES.free.flatMap((g) => g.items)
    expect(items.map((i) => i.kind)).toEqual(expect.arrayContaining(['pen', 'eraser', 'marker', 'lasso', 'shape']))
    expect(items.filter((i) => i.kind === 'shape')).toHaveLength(4)
    expect(items.filter((i) => i.kind === 'node' && i.type === 'sticky')).toHaveLength(5)
  })
})
