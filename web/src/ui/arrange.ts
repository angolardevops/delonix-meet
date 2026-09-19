/**
 * Selecção múltipla — a parte que o quadro branco e os diagramas partilham
 * (template v5: «N seleccionados», Agrupar ⌘G, Desagrupar ⇧⌘G, alinhar e
 * distribuir). Funções puras sobre caixas; cada editor diz o que é uma caixa
 * (coordenadas normalizadas no quadro, unidades do mundo nos diagramas) e
 * aplica os deslocamentos à sua maneira.
 *
 * Uma «unidade» é o que se alinha como um todo: um objecto solto, ou um grupo
 * inteiro — alinhar à esquerda não desmancha um grupo.
 */

export interface UnitBox {
  id: string
  x: number
  y: number
  w: number
  h: number
}

export type AlignMode = 'left' | 'centerX' | 'top'
export type Axis = 'x' | 'y'
export type Delta = { dx: number; dy: number }

/** Caixa que envolve várias caixas (ou `null` sem nenhuma). */
export function unionBox(boxes: { x: number; y: number; w: number; h: number }[]): { x: number; y: number; w: number; h: number } | null {
  if (boxes.length === 0) return null
  let x0 = Infinity
  let y0 = Infinity
  let x1 = -Infinity
  let y1 = -Infinity
  for (const b of boxes) {
    x0 = Math.min(x0, b.x)
    y0 = Math.min(y0, b.y)
    x1 = Math.max(x1, b.x + b.w)
    y1 = Math.max(y1, b.y + b.h)
  }
  return Number.isFinite(x0) ? { x: x0, y: y0, w: x1 - x0, h: y1 - y0 } : null
}

/**
 * Alinhar: à esquerda (o `x` mais pequeno), ao centro na horizontal (o centro
 * da caixa que envolve todas) ou ao topo. Só entram no resultado as unidades
 * que mexem.
 */
export function alignDeltas(boxes: UnitBox[], mode: AlignMode): Map<string, Delta> {
  const out = new Map<string, Delta>()
  if (boxes.length < 2) return out
  const all = unionBox(boxes)!
  for (const b of boxes) {
    const dx = mode === 'left' ? all.x - b.x : mode === 'centerX' ? all.x + all.w / 2 - (b.x + b.w / 2) : 0
    const dy = mode === 'top' ? all.y - b.y : 0
    if (Math.abs(dx) > 1e-9 || Math.abs(dy) > 1e-9) out.set(b.id, { dx, dy })
  }
  return out
}

/**
 * Distribuir: as unidades das pontas ficam onde estão e as do meio passam a ter
 * o MESMO espaço entre si (espaço entre bordas, não entre centros — é o que se
 * vê). Precisa de três ou mais.
 */
export function distributeDeltas(boxes: UnitBox[], axis: Axis): Map<string, Delta> {
  const out = new Map<string, Delta>()
  if (boxes.length < 3) return out
  const pos = (b: UnitBox) => (axis === 'x' ? b.x : b.y)
  const size = (b: UnitBox) => (axis === 'x' ? b.w : b.h)
  const sorted = [...boxes].sort((a, b) => pos(a) - pos(b) || a.id.localeCompare(b.id))
  const first = sorted[0]
  const last = sorted[sorted.length - 1]
  const span = pos(last) + size(last) - pos(first)
  const occupied = sorted.reduce((s, b) => s + size(b), 0)
  const gap = (span - occupied) / (sorted.length - 1)
  let cursor = pos(first) + size(first) + gap
  for (const b of sorted.slice(1, -1)) {
    const d = cursor - pos(b)
    if (Math.abs(d) > 1e-9) out.set(b.id, axis === 'x' ? { dx: d, dy: 0 } : { dx: 0, dy: d })
    cursor += size(b) + gap
  }
  return out
}

/** Sistema da Apple? Decide o símbolo do atalho (⌘) — a tecla aceite é sempre ⌘ ou Ctrl. */
export function isApplePlatform(nav: { platform?: string; userAgent?: string } | undefined = typeof navigator === 'undefined' ? undefined : navigator): boolean {
  if (!nav) return false
  return /Mac|iPhone|iPad|iPod/i.test(nav.platform || '') || /Mac OS X|iPhone|iPad/i.test(nav.userAgent || '')
}

export interface ShortcutProbe {
  key: string
  ctrlKey: boolean
  metaKey: boolean
  altKey: boolean
  shiftKey: boolean
}

/**
 * ⌘G / Ctrl+G agrupa; com ⇧ desagrupa. Alt nunca (é outro atalho no sistema).
 * Não colide com o Ctrl+K da pesquisa, e quem chama já recusou o evento se a
 * pessoa estava a escrever num campo.
 */
export function groupShortcut(e: ShortcutProbe): 'group' | 'ungroup' | null {
  if (!(e.ctrlKey || e.metaKey) || e.altKey) return null
  if (e.key.toLowerCase() !== 'g') return null
  return e.shiftKey ? 'ungroup' : 'group'
}
