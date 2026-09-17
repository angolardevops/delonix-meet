/**
 * Grupos de selecção dos diagramas (template v5: ⌘G / ⇧⌘G). Funções puras
 * sobre o documento — o Canvas e a página só as chamam, e o histórico
 * (desfazer) vem de graça porque cada operação devolve um documento novo.
 *
 * Um grupo é plano: agrupar coisas que já estão noutros grupos tira-as de lá
 * (um grupo que fique com menos de dois membros desaparece). Não há grupos
 * dentro de grupos — nem no template, nem nos formatos para onde se exporta.
 */
import { alignDeltas, distributeDeltas, unionBox, type AlignMode, type Axis, type UnitBox } from '../../ui/arrange'
import { Box, moveSelection, nodeBox } from './geometry'
import { DiagramDoc, SelectionGroup, uid } from './model'

/** Uma selecção: ids de elementos e de traços (a mesma forma do `Multi` do Canvas). */
export interface Items {
  nodes: string[]
  strokes: string[]
}

type GDoc = { nodes: { id: string }[]; strokes: { id: string }[]; groups?: SelectionGroup[] }

export const groupsOf = (doc: GDoc): SelectionGroup[] => doc.groups ?? []

export const pickSize = (p: Items | null | undefined) => (p ? p.nodes.length + p.strokes.length : 0)

export function groupOfItem(doc: GDoc, kind: 'node' | 'stroke', id: string): SelectionGroup | undefined {
  return groupsOf(doc).find((g) => (kind === 'node' ? g.nodes : g.strokes).includes(id))
}

const touches = (g: SelectionGroup, p: Items) => g.nodes.some((n) => p.nodes.includes(n)) || g.strokes.some((s) => p.strokes.includes(s))

/** Os grupos que a selecção toca. */
export function groupsInPick(doc: GDoc, p: Items): SelectionGroup[] {
  return groupsOf(doc).filter((g) => touches(g, p))
}

/** A selecção alargada aos grupos inteiros que toca — escolher um membro escolhe o grupo. */
export function expandPick(doc: GDoc, p: Items): Items {
  const nodes = new Set(p.nodes)
  const strokes = new Set(p.strokes)
  for (const g of groupsInPick(doc, p)) {
    g.nodes.forEach((n) => nodes.add(n))
    g.strokes.forEach((s) => strokes.add(s))
  }
  return { nodes: [...nodes], strokes: [...strokes] }
}

/** Junta ou tira (⇧clique) um item — ou o grupo dele inteiro — de uma selecção. */
export function togglePick(doc: GDoc, p: Items | null, kind: 'node' | 'stroke', id: string): Items {
  const base = p ?? { nodes: [], strokes: [] }
  const unit = expandPick(doc, kind === 'node' ? { nodes: [id], strokes: [] } : { nodes: [], strokes: [id] })
  const has = (kind === 'node' ? base.nodes : base.strokes).includes(id)
  if (has) {
    return { nodes: base.nodes.filter((n) => !unit.nodes.includes(n)), strokes: base.strokes.filter((s) => !unit.strokes.includes(s)) }
  }
  return { nodes: [...new Set([...base.nodes, ...unit.nodes])], strokes: [...new Set([...base.strokes, ...unit.strokes])] }
}

/** A selecção é exactamente um grupo (e nada mais)? */
export function pickIsOneGroup(doc: GDoc, p: Items): SelectionGroup | undefined {
  const gs = groupsInPick(doc, p)
  if (gs.length !== 1) return undefined
  const g = gs[0]
  const same = (a: string[], b: string[]) => a.length === b.length && a.every((x) => b.includes(x))
  return same(g.nodes, p.nodes) && same(g.strokes, p.strokes) ? g : undefined
}

/** Tira do documento as referências a itens que já não existem (e os grupos que ficam com menos de dois). */
export function cleanGroups<T extends GDoc>(doc: T): T {
  if (!doc.groups) return doc
  const ns = new Set(doc.nodes.map((n) => n.id))
  const ss = new Set(doc.strokes.map((s) => s.id))
  const groups = doc.groups
    .map((g) => ({ ...g, nodes: g.nodes.filter((n) => ns.has(n)), strokes: g.strokes.filter((s) => ss.has(s)) }))
    .filter((g) => g.nodes.length + g.strokes.length >= 2)
  const same = groups.length === doc.groups.length && groups.every((g, i) => g.nodes.length === doc.groups![i].nodes.length && g.strokes.length === doc.groups![i].strokes.length)
  return same ? doc : { ...doc, groups }
}

/**
 * Agrupa a selecção. Precisa de dois ou mais itens e de não ser já, ela toda,
 * um único grupo. `name` dá nome ao grupo novo.
 */
export function groupPick<T extends GDoc>(doc: T, p: Items, name: string, id = uid('g')): { doc: T; group: SelectionGroup } | null {
  if (pickSize(p) < 2 || pickIsOneGroup(doc, p)) return null
  const group: SelectionGroup = { id, name, nodes: [...p.nodes], strokes: [...p.strokes] }
  const rest = groupsOf(doc).map((g) => ({ ...g, nodes: g.nodes.filter((n) => !p.nodes.includes(n)), strokes: g.strokes.filter((s) => !p.strokes.includes(s)) }))
  const groups = [...rest.filter((g) => g.nodes.length + g.strokes.length >= 2), group]
  return { doc: { ...doc, groups }, group }
}

/** Desfaz os grupos que a selecção toca. `null` se não havia nenhum. */
export function ungroupPick<T extends GDoc>(doc: T, p: Items): T | null {
  const hit = groupsInPick(doc, p)
  if (hit.length === 0) return null
  const groups = groupsOf(doc).filter((g) => !hit.includes(g))
  return { ...doc, groups }
}

export function renameGroup<T extends GDoc>(doc: T, id: string, name: string): T {
  return { ...doc, groups: groupsOf(doc).map((g) => (g.id === id ? { ...g, name } : g)) }
}

/** Nome livre para um grupo novo: «Grupo 1», «Grupo 2»… (`label(n)` vem da língua). */
export function nextGroupName(doc: GDoc, label: (n: number) => string): string {
  const taken = new Set(groupsOf(doc).map((g) => g.name))
  for (let i = 1; ; i++) if (!taken.has(label(i))) return label(i)
}

// ---------------------------------------------------------------------------
//  Caixas e unidades (alinhar e distribuir tratam cada grupo como um bloco)
// ---------------------------------------------------------------------------

export function strokeBox(points: number[]): Box | null {
  let x0 = Infinity
  let y0 = Infinity
  let x1 = -Infinity
  let y1 = -Infinity
  for (let i = 0; i + 1 < points.length; i += 2) {
    x0 = Math.min(x0, points[i])
    y0 = Math.min(y0, points[i + 1])
    x1 = Math.max(x1, points[i])
    y1 = Math.max(y1, points[i + 1])
  }
  return Number.isFinite(x0) ? { x: x0, y: y0, w: x1 - x0, h: y1 - y0 } : null
}

/** Caixa de uma selecção (elementos e traços). */
export function pickBox(doc: DiagramDoc, p: Items): Box | null {
  const boxes: Box[] = []
  for (const n of doc.nodes) if (p.nodes.includes(n.id)) boxes.push(nodeBox(n))
  for (const s of doc.strokes) {
    if (!p.strokes.includes(s.id)) continue
    const b = strokeBox(s.points)
    if (b) boxes.push(b)
  }
  return unionBox(boxes)
}

/** As unidades de uma selecção: cada grupo inteiro é uma, cada item solto é outra. */
export function unitsOf(doc: DiagramDoc, p: Items): { id: string; pick: Items }[] {
  const units: { id: string; pick: Items }[] = []
  const seen = new Set<string>()
  for (const g of groupsInPick(doc, p)) {
    units.push({ id: `g:${g.id}`, pick: { nodes: g.nodes.filter((n) => p.nodes.includes(n)), strokes: g.strokes.filter((s) => p.strokes.includes(s)) } })
    g.nodes.forEach((n) => seen.add(`n:${n}`))
    g.strokes.forEach((s) => seen.add(`s:${s}`))
  }
  for (const n of p.nodes) if (!seen.has(`n:${n}`)) units.push({ id: `n:${n}`, pick: { nodes: [n], strokes: [] } })
  for (const s of p.strokes) if (!seen.has(`s:${s}`)) units.push({ id: `s:${s}`, pick: { nodes: [], strokes: [s] } })
  return units
}

function arrange(doc: DiagramDoc, p: Items, deltas: (boxes: UnitBox[]) => Map<string, { dx: number; dy: number }>): DiagramDoc {
  const units = unitsOf(doc, p)
  const boxes: UnitBox[] = []
  for (const u of units) {
    const b = pickBox(doc, u.pick)
    if (b) boxes.push({ id: u.id, ...b })
  }
  const d = deltas(boxes)
  let next = doc
  for (const u of units) {
    const m = d.get(u.id)
    if (m) next = moveSelection(next, u.pick, Math.round(m.dx), Math.round(m.dy))
  }
  return next
}

export const alignPick = (doc: DiagramDoc, p: Items, mode: AlignMode) => arrange(doc, p, (b) => alignDeltas(b, mode))
export const distributePick = (doc: DiagramDoc, p: Items, axis: Axis) => arrange(doc, p, (b) => distributeDeltas(b, axis))
