/**
 * O quadro como DADOS: objectos (traço, texto, nota, forma) em coordenadas
 * normalizadas 0..1, por página, com autor e id. Puro, para se testar sem
 * canvas nem socket — o `useWhiteboard` liga isto às mensagens e o
 * `Whiteboard` desenha.
 */
import type { WbStroke } from '../signaling'

export type WbObject = WbStroke

/** Traço vindo do servidor (ou meu): um id repetido não entra duas vezes. */
export function comObjecto(lista: WbObject[], o: WbObject): WbObject[] {
  if (o.id && lista.some((x) => x.id === o.id)) return lista
  return [...lista, o]
}

export function semObjecto(lista: WbObject[], id: string): WbObject[] {
  return lista.some((x) => x.id === id) ? lista.filter((x) => x.id !== id) : lista
}

/** `wb-transform`: desloca TODOS os pontos (o servidor fez o mesmo). */
export function movido(lista: WbObject[], id: string, dx: number, dy: number): WbObject[] {
  return lista.map((x) => (x.id === id ? { ...x, pts: x.pts.map(([px, py]) => [px + dx, py + dy] as [number, number]) } : x))
}

export function comTexto(lista: WbObject[], id: string, text: string): WbObject[] {
  return lista.map((x) => (x.id === id && (x.kind === 'text' || x.kind === 'note') ? { ...x, text } : x))
}

export const daPagina = (lista: WbObject[], pagina: number) => lista.filter((o) => (o.page ?? 0) === pagina)

/** Caixa (normalizada) de um objecto; texto e nota têm tamanho aproximado dado por quem desenha. */
export function caixa(o: WbObject, texto: { w: number; h: number } = { w: 0.2, h: 0.08 }): { x0: number; y0: number; x1: number; y1: number } {
  if (o.kind === 'text' || o.kind === 'note') {
    const [x, y] = o.pts[0] ?? [0, 0]
    return { x0: x, y0: y, x1: x + texto.w, y1: y + texto.h }
  }
  const xs = o.pts.map((p) => p[0])
  const ys = o.pts.map((p) => p[1])
  return { x0: Math.min(...xs), y0: Math.min(...ys), x1: Math.max(...xs), y1: Math.max(...ys) }
}

/** Distância de um ponto a um segmento (em píxeis, com o aspecto do quadro). */
function distSegmento(p: [number, number], a: [number, number], b: [number, number]): number {
  const [px, py] = p
  const [ax, ay] = a
  const [bx, by] = b
  const dx = bx - ax
  const dy = by - ay
  const l2 = dx * dx + dy * dy
  const t = l2 ? Math.max(0, Math.min(1, ((px - ax) * dx + (py - ay) * dy) / l2)) : 0
  return Math.hypot(px - (ax + t * dx), py - (ay + t * dy))
}

/**
 * O objecto de cima que está sob o ponto (`W`×`H` em píxeis para medir a
 * tolerância como a pessoa a vê). Traços pela distância à linha; formas pela
 * caixa (e pelo contorno, para rectângulos grandes); texto e nota pela caixa.
 */
export function objectoEm(
  lista: WbObject[],
  ponto: [number, number],
  W: number,
  H: number,
  caixasDeTexto: Record<string, { w: number; h: number }> = {},
  tolerancia = 8,
): WbObject | null {
  const P: [number, number] = [ponto[0] * W, ponto[1] * H]
  for (let i = lista.length - 1; i >= 0; i--) {
    const o = lista[i]
    if (o.kind === 'text' || o.kind === 'note') {
      const c = caixa(o, (o.id && caixasDeTexto[o.id]) || undefined)
      if (ponto[0] >= c.x0 && ponto[0] <= c.x1 && ponto[1] >= c.y0 && ponto[1] <= c.y1) return o
      continue
    }
    const pts = o.pts.map(([x, y]) => [x * W, y * H] as [number, number])
    if (o.kind === 'shape' && pts.length === 2) {
      const [a, b] = pts
      if (o.shape === 'line' || o.shape === 'arrow') {
        if (distSegmento(P, a, b) <= tolerancia + o.w) return o
        continue
      }
      const x0 = Math.min(a[0], b[0]) - tolerancia
      const x1 = Math.max(a[0], b[0]) + tolerancia
      const y0 = Math.min(a[1], b[1]) - tolerancia
      const y1 = Math.max(a[1], b[1]) + tolerancia
      if (P[0] >= x0 && P[0] <= x1 && P[1] >= y0 && P[1] <= y1) return o
      continue
    }
    for (let k = 1; k < pts.length; k++) if (distSegmento(P, pts[k - 1], pts[k]) <= tolerancia + o.w) return o
    if (pts.length === 1 && Math.hypot(P[0] - pts[0][0], P[1] - pts[0][1]) <= tolerancia) return o
  }
  return null
}

/** Tamanho da letra de um texto a partir da espessura escolhida (as três de base). */
export function tamanhoDoTexto(w: number): number {
  return w <= 2 ? 13 : w >= 8 ? 32 : 22
}

/**
 * Desenha um objecto de canvas (traço ou forma). Texto e nota são HTML por
 * cima do canvas — aqui só se pintam quando `comTexto` (exportar PNG, palco).
 */
export function desenharObjecto(ctx: CanvasRenderingContext2D, s: WbObject, W: number, H: number, escala: number, comTextos = false) {
  ctx.strokeStyle = s.c
  ctx.fillStyle = s.c
  ctx.lineCap = 'round'
  ctx.lineJoin = 'round'
  if (s.kind === 'text' || s.kind === 'note') {
    if (!comTextos || !s.text || !s.pts[0]) return
    const [x, y] = [s.pts[0][0] * W, s.pts[0][1] * H]
    const tamanho = (s.kind === 'text' ? tamanhoDoTexto(s.w) : 11.5) * escala
    if (s.kind === 'note') {
      const linhas = s.text.split('\n')
      const largura = Math.max(...linhas.map((l) => l.length)) * tamanho * 0.55 + 26 * escala
      ctx.save()
      ctx.fillStyle = '#fffbe8'
      ctx.strokeStyle = '#e3d9a8'
      ctx.lineWidth = 1
      ctx.fillRect(x, y, largura, linhas.length * tamanho * 1.5 + 22 * escala)
      ctx.strokeRect(x, y, largura, linhas.length * tamanho * 1.5 + 22 * escala)
      ctx.restore()
      ctx.fillStyle = '#4a4326'
    }
    ctx.font = `${s.kind === 'text' ? 700 : 500} ${tamanho}px Archivo, sans-serif`
    s.text.split('\n').forEach((l, i) => ctx.fillText(l, x + (s.kind === 'note' ? 13 * escala : 0), y + tamanho * (1.1 + i * 1.5) + (s.kind === 'note' ? 11 * escala : 0)))
    return
  }
  if (s.kind === 'shape' && s.pts.length === 2) {
    const [[ax, ay], [bx, by]] = s.pts.map(([x, y]) => [x * W, y * H])
    ctx.lineWidth = s.w * escala
    ctx.beginPath()
    if (s.shape === 'rect') ctx.rect(Math.min(ax, bx), Math.min(ay, by), Math.abs(bx - ax), Math.abs(by - ay))
    else if (s.shape === 'ellipse') ctx.ellipse((ax + bx) / 2, (ay + by) / 2, Math.abs(bx - ax) / 2, Math.abs(by - ay) / 2, 0, 0, Math.PI * 2)
    else {
      ctx.moveTo(ax, ay)
      ctx.lineTo(bx, by)
      if (s.shape === 'arrow') {
        const ang = Math.atan2(by - ay, bx - ax)
        const cab = Math.min(18 * escala, Math.hypot(bx - ax, by - ay) * 0.35)
        ctx.moveTo(bx, by)
        ctx.lineTo(bx - cab * Math.cos(ang - Math.PI / 7), by - cab * Math.sin(ang - Math.PI / 7))
        ctx.moveTo(bx, by)
        ctx.lineTo(bx - cab * Math.cos(ang + Math.PI / 7), by - cab * Math.sin(ang + Math.PI / 7))
      }
    }
    ctx.stroke()
    return
  }
  if (s.pts.length < 2) return
  // Pressão por ponto (B7): a espessura acompanha a caneta segmento a segmento.
  if (s.p && s.p.length === s.pts.length) {
    for (let i = 1; i < s.pts.length; i++) {
      const pr = (s.p[i - 1] + s.p[i]) / 2
      ctx.lineWidth = Math.max(0.5, s.w * (0.35 + pr * 1.3)) * escala
      ctx.beginPath()
      ctx.moveTo(s.pts[i - 1][0] * W, s.pts[i - 1][1] * H)
      ctx.lineTo(s.pts[i][0] * W, s.pts[i][1] * H)
      ctx.stroke()
    }
    return
  }
  ctx.lineWidth = s.w * escala
  ctx.beginPath()
  ctx.moveTo(s.pts[0][0] * W, s.pts[0][1] * H)
  for (const [x, y] of s.pts.slice(1)) ctx.lineTo(x * W, y * H)
  ctx.stroke()
}

/** Quem está a editar: autores de objectos recentes e cursores em movimento. */
export function aEditar(actividade: Record<string, number>, agora: number, janelaMs = 20_000): string[] {
  return Object.entries(actividade)
    .filter(([, t]) => agora - t <= janelaMs)
    .sort((a, b) => b[1] - a[1])
    .map(([nome]) => nome)
}
