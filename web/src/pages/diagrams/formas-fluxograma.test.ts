import { describe, expect, it } from 'vitest'
import pt from '../../locales/pt/diagrams'
import { example } from './examples'
import { canConnect, DEdge, DiagramDoc, DNode, emptyDoc, FLOW_NODES, makeNode, NodeProps, NodeType, PALETTES } from './model'
import { validate } from './validate'

const N = (id: string, type: NodeType, x: number, y: number, name = id, props: NodeProps = {}): DNode => makeNode(type, x, y, name, props, id)
const E = (id: string, type: DEdge['type'], from: string, to: string, extra: Partial<DEdge> = {}): DEdge => ({ id, type, from, to, label: '', ...extra })
const doc = (nodes: DNode[], edges: DEdge[] = []): DiagramDoc => ({ ...emptyDoc('d1', 'F', 'flow', '', '2026-09-17T00:00:00Z'), nodes, edges })
const codes = (d: DiagramDoc) => validate(d).map((i) => i.code).sort()
const tx = (k: string) => k.split('.').reduce<unknown>((o, p) => (o as Record<string, unknown>)?.[p], (pt as { exemplos: unknown }).exemplos) as string

describe('fluxograma · formas ISO 5807', () => {
  it('a paleta tem as formas pedidas e todas ligam por fluxo', () => {
    const keys = PALETTES.flow.flatMap((g) => g.items.map((i) => i.key))
    for (const k of ['predefinedProcess', 'manualInput', 'manualOperation', 'preparation', 'delay', 'merge', 'connector', 'offPageConnector', 'flowDatabase', 'storedData', 'display', 'multiDocument', 'loopLimit', 'flowAnnotation', 'sequentialStorage', 'directAccessStorage']) {
      expect(keys).toContain(k)
    }
    const p = N('p', 'process', 0, 0)
    for (const t of FLOW_NODES) expect(canConnect('flow', N('x', t, 0, 0), p)).toBe(true)
    // A anotação não entra no fluxo: liga-se por ligação de nota.
    const a = N('a', 'flowAnnotation', 0, 0)
    expect(canConnect('flow', a, p)).toBe(false)
    expect(canConnect('flowNote', a, p)).toBe(true)
    expect(canConnect('flowNote', p, N('q', 'process', 0, 0))).toBe(false)
  })

  it('o exemplo é um fluxograma válido', () => {
    const ex = example('flow', tx)!
    expect(validate(doc(ex.nodes, ex.edges))).toEqual([])
  })

  it('conector fora da página sem entrada conta como início; conector na página pede par', () => {
    const d = doc([N('c', 'offPageConnector', 0, 0, 'P1'), N('p', 'process', 0, 100, 'Faz'), N('k', 'connector', 0, 200, 'A')], [E('1', 'flow', 'c', 'p'), E('2', 'flow', 'p', 'k')])
    expect(codes(d)).toEqual(['flowConectorSemPar'])
    d.nodes.push(N('k2', 'connector', 300, 0, 'A'), N('q', 'process', 300, 100, 'Continua'))
    d.edges.push(E('3', 'flow', 'k2', 'q'))
    expect(codes(d)).toEqual([])
  })

  it('anotação solta não é «inalcançável» nem conta como forma do fluxo', () => {
    const d = doc([N('s', 'terminator', 0, 0, 'Início'), N('p', 'process', 0, 100, 'Faz'), N('n', 'flowAnnotation', 200, 0, '', { text: 'nota' })], [E('1', 'flow', 's', 'p')])
    expect(codes(d)).toEqual([])
  })
})
