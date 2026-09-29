/**
 * Selecção múltipla e grupos do quadro da sala (template v5: Ponteiro, Laço,
 * «N seleccionados», ⌘G / ⇧⌘G). Puro — sem canvas nem socket.
 *
 * O protocolo do quadro (PR #92) conhece OBJECTOS: `wb-stroke`, `wb-transform`,
 * `wb-update`, `wb-erase`, com autoria e permissões no servidor. Não conhece
 * «grupo», e este trabalho não inventa mensagens. Por isso:
 *  - o grupo vive neste browser (quem agrupou vê o grupo; os outros vêem os
 *    objectos mexerem-se juntos, porque cada mover é um `wb-transform` por
 *    objecto);
 *  - só entram na selecção objectos que ESTA pessoa pode editar (autor ou
 *    anfitrião — a mesma regra do `wb_can_edit` do servidor): alinhar metade
 *    e ver a outra metade recusada em silêncio seria pior do que não deixar;
 *  - desfazer é desta pessoa e manda as mensagens inversas (mover ao
 *    contrário, voltar a pôr o que se apagou com o mesmo id).
 */
import { alignDeltas, distributeDeltas, unionBox, type AlignMode, type Axis, type Delta, type UnitBox } from '../ui/arrange'
import type { WbStroke } from '../signaling'
import { caixa, type WbObject } from './wbState'

export type Pt = [number, number]
export interface NBox {
  x: number
  y: number
  w: number
  h: number
}
export interface WbGroup {
  id: string
  ids: string[]
}
export type TextBoxes = Record<string, { w: number; h: number }>

/** Caixa normalizada de um objecto (texto e nota com a caixa medida no DOM). */
export function boxOf(o: WbObject, textos: TextBoxes = {}): NBox {
  const c = caixa(o, (o.id && textos[o.id]) || undefined)
  return { x: c.x0, y: c.y0, w: c.x1 - c.x0, h: c.y1 - c.y0 }
}

export function boxOfIds(lista: WbObject[], ids: string[], textos: TextBoxes = {}): NBox | null {
  return unionBox(lista.filter((o) => o.id && ids.includes(o.id)).map((o) => boxOf(o, textos)))
}

const centro = (b: NBox): Pt => [b.x + b.w / 2, b.y + b.h / 2]

function dentroDoPoligono([x, y]: Pt, poly: Pt[]): boolean {
  let dentro = false
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const [xi, yi] = poly[i]
    const [xj, yj] = poly[j]
    if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi || 1e-12) + xi) dentro = !dentro
  }
  return dentro
}

/** Caixa de selecção: entra o que tem o CENTRO lá dentro. */
export function idsInRect(lista: WbObject[], a: Pt, b: Pt, textos: TextBoxes = {}): string[] {
  const x0 = Math.min(a[0], b[0])
  const x1 = Math.max(a[0], b[0])
  const y0 = Math.min(a[1], b[1])
  const y1 = Math.max(a[1], b[1])
  return lista
    .filter((o) => {
      if (!o.id) return false
      const [cx, cy] = centro(boxOf(o, textos))
      return cx >= x0 && cx <= x1 && cy >= y0 && cy <= y1
    })
    .map((o) => o.id!)
}

/** Laço: entra o que tem o centro dentro do polígono desenhado. */
export function idsInLasso(lista: WbObject[], poly: Pt[], textos: TextBoxes = {}): string[] {
  if (poly.length < 3) return []
  return lista.filter((o) => o.id && dentroDoPoligono(centro(boxOf(o, textos)), poly)).map((o) => o.id!)
}

/** O mesmo que o `wb_can_edit` do servidor: autor ou anfitrião. */
export const canEdit = (o: WbObject, me: string | undefined, isHost: boolean) => isHost || (!!me && o.by === me)

// ── grupos ──────────────────────────────────────────────────────────────────

export const groupOf = (grupos: WbGroup[], id: string) => grupos.find((g) => g.ids.includes(id))

/** Alarga a selecção aos grupos inteiros que toca. */
export function expandToGroups(ids: string[], grupos: WbGroup[]): string[] {
  const out = new Set(ids)
  for (const g of grupos) if (g.ids.some((i) => out.has(i))) g.ids.forEach((i) => out.add(i))
  return [...out]
}

/** ⇧clique: junta ou tira um objecto (ou o grupo dele inteiro). */
export function toggleInSelection(sel: string[], id: string, grupos: WbGroup[]): string[] {
  const unidade = expandToGroups([id], grupos)
  return sel.includes(id) ? sel.filter((i) => !unidade.includes(i)) : [...new Set([...sel, ...unidade])]
}

export function isOneGroup(sel: string[], grupos: WbGroup[]): WbGroup | undefined {
  const g = grupos.find((x) => x.ids.some((i) => sel.includes(i)))
  return g && g.ids.length === sel.length && g.ids.every((i) => sel.includes(i)) ? g : undefined
}

export function groupSelection(grupos: WbGroup[], sel: string[], id: string): WbGroup[] | null {
  if (sel.length < 2 || isOneGroup(sel, grupos)) return null
  const resto = grupos.map((g) => ({ ...g, ids: g.ids.filter((i) => !sel.includes(i)) })).filter((g) => g.ids.length >= 2)
  return [...resto, { id, ids: [...sel] }]
}

export function ungroupSelection(grupos: WbGroup[], sel: string[]): WbGroup[] | null {
  const fica = grupos.filter((g) => !g.ids.some((i) => sel.includes(i)))
  return fica.length === grupos.length ? null : fica
}

/** Tira dos grupos o que já não existe (apagado por alguém, página limpa). */
export function pruneGroups(grupos: WbGroup[], existentes: Set<string>): WbGroup[] {
  const n = grupos.map((g) => ({ ...g, ids: g.ids.filter((i) => existentes.has(i)) })).filter((g) => g.ids.length >= 2)
  return n.length === grupos.length && n.every((g, i) => g.ids.length === grupos[i].ids.length) ? grupos : n
}

// ── alinhar e distribuir: deslocamento POR OBJECTO (o grupo move-se como bloco) ──

function unidades(sel: string[], grupos: WbGroup[]): { id: string; ids: string[] }[] {
  const vistos = new Set<string>()
  const out: { id: string; ids: string[] }[] = []
  for (const g of grupos) {
    const dentro = g.ids.filter((i) => sel.includes(i))
    if (dentro.length === 0) continue
    out.push({ id: `g:${g.id}`, ids: dentro })
    dentro.forEach((i) => vistos.add(i))
  }
  for (const i of sel) if (!vistos.has(i)) out.push({ id: `o:${i}`, ids: [i] })
  return out
}

export const unitCount = (sel: string[], grupos: WbGroup[]) => unidades(sel, grupos).length

function arrumar(lista: WbObject[], sel: string[], grupos: WbGroup[], textos: TextBoxes, calc: (b: UnitBox[]) => Map<string, Delta>): { id: string; dx: number; dy: number }[] {
  const us = unidades(sel, grupos)
  const caixas: UnitBox[] = []
  for (const u of us) {
    const b = boxOfIds(lista, u.ids, textos)
    if (b) caixas.push({ id: u.id, ...b })
  }
  const d = calc(caixas)
  const out: { id: string; dx: number; dy: number }[] = []
  for (const u of us) {
    const m = d.get(u.id)
    if (m && (Math.abs(m.dx) > 1e-4 || Math.abs(m.dy) > 1e-4)) for (const id of u.ids) out.push({ id, dx: m.dx, dy: m.dy })
  }
  return out
}

export const alignMoves = (lista: WbObject[], sel: string[], grupos: WbGroup[], mode: AlignMode, textos: TextBoxes = {}) => arrumar(lista, sel, grupos, textos, (b) => alignDeltas(b, mode))
export const distributeMoves = (lista: WbObject[], sel: string[], grupos: WbGroup[], axis: Axis, textos: TextBoxes = {}) =>
  arrumar(lista, sel, grupos, textos, (b) => distributeDeltas(b, axis))

// ── desfazer ────────────────────────────────────────────────────────────────

export type WbUndo =
  | { t: 'moves'; moves: { id: string; dx: number; dy: number }[] }
  | { t: 'groups'; before: WbGroup[]; after: WbGroup[] }
  | { t: 'erase'; objs: WbStroke[]; groupsBefore: WbGroup[] }

export const inverseMoves = (moves: { id: string; dx: number; dy: number }[]) => moves.map((m) => ({ id: m.id, dx: -m.dx, dy: -m.dy }))
