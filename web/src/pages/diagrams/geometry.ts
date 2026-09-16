/**
 * Geometria dos diagramas: caixas, pontos de ancoragem das setas, pertença a
 * piscinas e pistas, e a caixa do conteúdo (para «ajustar à página» e para
 * exportar). Puro — testável sem DOM.
 */
import { CLASSIFIERS, DEdge, DiagramDoc, DNode, Lane, nodeById } from './model'

export interface Box {
  x: number
  y: number
  w: number
  h: number
}

export interface Pt {
  x: number
  y: number
}

/** Métricas do texto das classes — iguais no ecrã e na exportação. */
export const CLASS = {
  stereo: 13,
  name: 20,
  pad: 7,
  line: 14,
}

export const POOL_HEADER = 30
export const LANE_HEADER = 26

/** Altura de um classificador: cabeça, atributos e operações. */
export function classifierHeight(n: DNode): number {
  const attrs = n.props.attributes?.length ?? 0
  const ops = n.props.operations?.length ?? 0
  const head = CLASS.pad * 2 + CLASS.name + (n.props.stereotype ? CLASS.stereo : 0)
  const attrBox = CLASS.pad * 2 + Math.max(1, attrs) * CLASS.line
  const opBox = n.type === 'enum' && ops === 0 ? 0 : CLASS.pad * 2 + Math.max(1, ops) * CLASS.line
  return head + attrBox + opBox
}

/** Caixa que o elemento ocupa (para seleccionar, ligar e enquadrar). */
export function nodeBox(n: DNode): Box {
  if (CLASSIFIERS.has(n.type)) return { x: n.x, y: n.y, w: n.w, h: classifierHeight(n) }
  if (n.type === 'lifeline') return { x: n.x, y: n.y, w: n.w, h: n.h + (n.props.length ?? 240) }
  if (n.type === 'pool') return { x: n.x, y: n.y, w: n.w, h: poolHeight(n) }
  return { x: n.x, y: n.y, w: n.w, h: n.h }
}

export function center(b: Box): Pt {
  return { x: b.x + b.w / 2, y: b.y + b.h / 2 }
}

export function poolHeight(pool: DNode): number {
  const lanes = pool.props.lanes ?? []
  if (lanes.length === 0) return pool.h
  return lanes.reduce((s, l) => s + l.size, 0)
}

type Shape = 'rect' | 'ellipse' | 'diamond'

function shapeOf(n: DNode): Shape {
  if (n.type === 'usecase' || n.type === 'startEvent' || n.type === 'intermediateEvent' || n.type === 'endEvent') return 'ellipse'
  if (n.type === 'gateway' || n.type === 'decision') return 'diamond'
  return 'rect'
}

/**
 * Onde uma recta que sai do centro de `n` em direcção a `toward` corta o
 * contorno do elemento.
 */
export function anchorPoint(n: DNode, toward: Pt): Pt {
  // A linha de vida liga-se pela cabeça (as mensagens têm o seu próprio y).
  const b = n.type === 'lifeline' ? { x: n.x, y: n.y, w: n.w, h: n.h } : nodeBox(n)
  const c = center(b)
  const dx = toward.x - c.x
  const dy = toward.y - c.y
  if (dx === 0 && dy === 0) return c
  const hw = b.w / 2
  const hh = b.h / 2
  const shape = shapeOf(n)
  let t: number
  if (shape === 'ellipse') {
    t = 1 / Math.sqrt((dx * dx) / (hw * hw) + (dy * dy) / (hh * hh))
  } else if (shape === 'diamond') {
    t = 1 / (Math.abs(dx) / hw + Math.abs(dy) / hh)
  } else {
    const tx = dx === 0 ? Infinity : hw / Math.abs(dx)
    const ty = dy === 0 ? Infinity : hh / Math.abs(dy)
    t = Math.min(tx, ty)
  }
  return { x: c.x + dx * t, y: c.y + dy * t }
}

/** Segmento de uma aresta, de contorno a contorno. `null` se falta um extremo. */
export function edgeSegment(doc: Pick<DiagramDoc, 'nodes'>, e: DEdge): { a: Pt; b: Pt } | null {
  const from = nodeById(doc, e.from)
  const to = nodeById(doc, e.to)
  if (!from || !to) return null
  if ((e.type === 'message' || e.type === 'reply') && from.type === 'lifeline' && to.type === 'lifeline') {
    const y = Math.min(from.y, to.y) + from.h + (e.offset ?? 40)
    const ax = from.x + from.w / 2
    const bx = to.x + to.w / 2
    if (from.id === to.id) return { a: { x: ax, y }, b: { x: ax, y: y + 24 } }
    return { a: { x: ax, y }, b: { x: bx, y } }
  }
  if (from.id === to.id) {
    const b = nodeBox(from)
    return { a: { x: b.x + b.w, y: b.y + b.h / 3 }, b: { x: b.x + b.w, y: b.y + (b.h * 2) / 3 } }
  }
  const cb = center(nodeBox(to))
  const ca = center(nodeBox(from))
  return { a: anchorPoint(from, cb), b: anchorPoint(to, ca) }
}

export function contains(outer: Box, p: Pt): boolean {
  return p.x >= outer.x && p.x <= outer.x + outer.w && p.y >= outer.y && p.y <= outer.y + outer.h
}

/** Piscina que contém o centro do elemento (a de cima, se houver várias). */
export function poolOf(doc: Pick<DiagramDoc, 'nodes'>, n: DNode): DNode | null {
  if (n.type === 'pool') return null
  const c = center(nodeBox(n))
  let found: DNode | null = null
  for (const p of doc.nodes) if (p.type === 'pool' && contains(nodeBox(p), c)) found = p
  return found
}

/** Pista em que o centro do elemento cai, dentro da sua piscina. */
export function laneOf(doc: Pick<DiagramDoc, 'nodes'>, n: DNode): { pool: DNode; lane: Lane; top: number } | null {
  const pool = poolOf(doc, n)
  if (!pool) return null
  const lanes = pool.props.lanes ?? []
  const cy = center(nodeBox(n)).y
  let top = pool.y
  for (const lane of lanes) {
    if (cy >= top && cy < top + lane.size) return { pool, lane, top }
    top += lane.size
  }
  return null
}

/** Topo de uma pista dentro da piscina. */
export function laneTop(pool: DNode, laneId: string): number | null {
  let top = pool.y
  for (const l of pool.props.lanes ?? []) {
    if (l.id === laneId) return top
    top += l.size
  }
  return null
}

/** Caixa de tudo o que está desenhado (elementos, arestas e traços). */
export function contentBox(doc: Pick<DiagramDoc, 'nodes' | 'strokes'>): Box | null {
  let x0 = Infinity
  let y0 = Infinity
  let x1 = -Infinity
  let y1 = -Infinity
  for (const n of doc.nodes) {
    const b = nodeBox(n)
    x0 = Math.min(x0, b.x)
    y0 = Math.min(y0, b.y)
    x1 = Math.max(x1, b.x + b.w)
    y1 = Math.max(y1, b.y + b.h + (n.type === 'actor' || n.type.endsWith('Event') || n.type === 'gateway' || n.type === 'dataObject' ? 18 : 0))
  }
  for (const s of doc.strokes) {
    for (let i = 0; i < s.points.length; i += 2) {
      x0 = Math.min(x0, s.points[i])
      y0 = Math.min(y0, s.points[i + 1])
      x1 = Math.max(x1, s.points[i])
      y1 = Math.max(y1, s.points[i + 1])
    }
  }
  if (!Number.isFinite(x0)) return null
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

/** Vista (deslocamento e escala) que enquadra `box` numa área `vw × vh`. */
export function fitView(box: Box | null, vw: number, vh: number, pad = 32): { x: number; y: number; k: number } {
  if (!box || box.w <= 0 || box.h <= 0) return { x: pad, y: pad, k: 1 }
  const k = clampZoom(Math.min((vw - pad * 2) / box.w, (vh - pad * 2) / box.h, 1.5))
  return { x: (vw - box.w * k) / 2 - box.x * k, y: (vh - box.h * k) / 2 - box.y * k, k }
}

export const ZOOM_MIN = 0.2
export const ZOOM_MAX = 3

export function clampZoom(k: number): number {
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, k))
}

/** Distância de um ponto a um segmento (para apagar traços e tocar arestas). */
export function distToSegment(p: Pt, a: Pt, b: Pt): number {
  const dx = b.x - a.x
  const dy = b.y - a.y
  const len = dx * dx + dy * dy
  const t = len === 0 ? 0 : Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / len))
  return Math.hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy))
}

/** Mensagens dentro do intervalo vertical de um fragmento combinado. */
export function messageY(doc: Pick<DiagramDoc, 'nodes'>, e: DEdge): number | null {
  const seg = edgeSegment(doc, e)
  return seg ? seg.a.y : null
}
