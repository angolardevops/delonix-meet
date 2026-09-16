/**
 * Regras de apresentação da biblioteca e do leitor: estado visível,
 * resolução, categoria, capítulo activo, orador de um segmento. Tudo puro e
 * testado, e só sobre `RecordingView` — nenhum campo da API é lido aqui (isso
 * é da `recordingView.ts`).
 *
 * Nada aqui inventa números: um campo `null` dá `null`, e o ecrã mostra «—»
 * ou esconde o elemento.
 */
import type { RecordingView, SessionCategory } from './recordingView'

export type LibraryFilter = 'all' | 'mine' | 'shared' | 'training' | 'broadcast' | 'meeting' | '4k' | 'processing' | 'failed'

/** Estado visível de uma gravação, pela ordem em que manda. */
export type VisibleState =
  | { kind: 'failed' }
  | { kind: 'processing'; pct: number | null }
  | { kind: 'transcribing'; pct: number | null }
  | { kind: 'published' }
  | { kind: 'retained'; days: number }
  | { kind: 'ready' }

const DAY_MS = 24 * 3600 * 1000

type StateInput = Pick<RecordingView, 'pipeline' | 'progressPct' | 'transcriptRunning' | 'createdAt'>

/**
 * `retentionDays` é a política da organização (`organizations.retention_days`,
 * 0 = sem retenção). Uma gravação pronta e NÃO publicada numa organização com
 * retenção mostra quantos dias faltam até a varredura a apagar
 * (`recorder.rs::retention_sweep`), arredondado para cima.
 */
export function visibleState(r: StateInput, retentionDays = 0, now = Date.now()): VisibleState {
  if (r.pipeline === 'failed') return { kind: 'failed' }
  if (r.pipeline === 'processing') return { kind: 'processing', pct: r.progressPct }
  if (r.pipeline === 'transcribing') return { kind: 'transcribing', pct: r.progressPct }
  if (r.pipeline === 'ready' && r.transcriptRunning) return { kind: 'transcribing', pct: null }
  if (r.pipeline === 'published') return { kind: 'published' }
  if (retentionDays > 0) {
    const created = new Date(r.createdAt).getTime()
    if (Number.isFinite(created)) {
      const left = Math.ceil((created + retentionDays * DAY_MS - now) / DAY_MS)
      return { kind: 'retained', days: Math.max(0, left) }
    }
  }
  return { kind: 'ready' }
}

/** Rótulo curto de uma resolução: «4K», «1080p», «720p»… ou `null`. */
export function resolutionLabel(size: { width: number | null; height: number | null } | null): string | null {
  const h = size?.height ?? 0
  const w = size?.width ?? 0
  if (!h || !w) return null
  if (h >= 2160 || w >= 3840) return '4K'
  return `${Math.min(h, w)}p`
}

export const is4k = (r: Pick<RecordingView, 'width' | 'height'>) => resolutionLabel(r) === '4K'

export function isInProgress(r: StateInput): boolean {
  const k = visibleState(r).kind
  return k === 'processing' || k === 'transcribing'
}

/**
 * Categoria do template: Videoaulas (formação; a híbrida é uma videoaula com
 * público presencial), Emissões, Reuniões. `null` sem dado.
 */
export function kindGroup(kind: SessionCategory | null): 'training' | 'broadcast' | 'meeting' | null {
  if (kind === 'training' || kind === 'hybrid') return 'training'
  if (kind === 'broadcast') return 'broadcast'
  if (kind === 'meeting') return 'meeting'
  return null
}

export function matchesFilter(r: RecordingView, f: LibraryFilter): boolean {
  switch (f) {
    case 'all':
      return true
    case 'mine':
      return r.owned
    case 'shared':
      return !r.owned
    case 'failed':
      return r.failed
    case 'processing':
      return isInProgress(r)
    case '4k':
      return is4k(r)
    default:
      return !r.failed && kindGroup(r.category) === f
  }
}

export function filterCounts(items: RecordingView[]): Record<LibraryFilter, number> {
  const out: Record<LibraryFilter, number> = { all: 0, mine: 0, shared: 0, training: 0, broadcast: 0, meeting: 0, '4k': 0, processing: 0, failed: 0 }
  for (const r of items) for (const f of Object.keys(out) as LibraryFilter[]) if (matchesFilter(r, f)) out[f]++
  return out
}

/**
 * Chips que a biblioteca mostra. Um chip só aparece se o dado por trás existe:
 * sem categoria no servidor, «Videoaulas» filtraria sempre para zero — seria
 * um botão inerte. Enquanto não há categorias ficam os filtros que os dados de
 * hoje sustentam (minhas / partilhadas comigo), no mesmo sítio.
 */
export function visibleFilters(items: RecordingView[]): LibraryFilter[] {
  const counts = filterCounts(items)
  const out: LibraryFilter[] = ['all']
  if (items.some((r) => r.category !== null)) out.push('training', 'broadcast', 'meeting')
  else out.push('mine', 'shared')
  if (counts['4k'] > 0) out.push('4k')
  if (counts.processing > 0) out.push('processing')
  if (counts.failed > 0) out.push('failed')
  return out
}

/**
 * Relógio do vídeo a partir de milissegundos: «00:00», «08:41», «48:12»,
 * «1:02:55». Minutos sempre com dois algarismos abaixo da hora, como o template.
 */
export function formatClock(ms: number | null | undefined): string {
  if (ms == null || !Number.isFinite(ms)) return '—'
  const s = Math.max(0, Math.floor(ms / 1000))
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const pad = (n: number) => String(n).padStart(2, '0')
  return h > 0 ? `${h}:${pad(m)}:${pad(s % 60)}` : `${pad(m)}:${pad(s % 60)}`
}

/** Índice do capítulo em curso no instante `tMs` (o último que já começou), ou -1. */
export function chapterAt(chapters: { tMs: number }[], tMs: number): number {
  let idx = -1
  for (let i = 0; i < chapters.length; i++) if (chapters[i].tMs <= tMs) idx = i
  return idx
}

/** Índice do segmento que contém `tMs` (ou o último que já começou), ou -1. */
export function segmentAt(segments: { startMs: number }[], tMs: number): number {
  let idx = -1
  for (let i = 0; i < segments.length; i++) {
    if (segments[i].startMs <= tMs) idx = i
    else break
  }
  return idx
}

/** «Ana Mbala: O failover…» → orador e texto. Sem orador reconhecível devolve só o texto. */
export function splitSpeaker(text: string): { speaker: string | null; text: string } {
  const m = /^([^:\n]{1,48}):\s+(.+)$/s.exec(text.trim())
  if (!m || /\d{1,2}$/.test(m[1])) return { speaker: null, text: text.trim() }
  return { speaker: m[1].trim(), text: m[2].trim() }
}

/** Posição (0–100) de um instante na barra, ou `null` sem duração. */
export function percentOf(tMs: number, durationMs: number | null | undefined): number | null {
  if (!durationMs || durationMs <= 0) return null
  return Math.max(0, Math.min(100, (tMs / durationMs) * 100))
}

/** Uma etiqueta digitada («#Formação, voz») → lista limpa, sem «#» nem repetidas. */
export function parseTags(raw: string): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const part of raw.split(/[,\s]+/)) {
    const tag = part.replace(/^#+/, '').trim()
    if (!tag || seen.has(tag.toLowerCase())) continue
    seen.add(tag.toLowerCase())
    out.push(tag)
  }
  return out.slice(0, 20)
}
