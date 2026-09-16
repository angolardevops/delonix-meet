/**
 * Regras de apresentação da biblioteca com metadados (`frontend/b1-gravacoes`):
 * estado visível, resolução, categoria, capítulo activo, orador de um segmento.
 * Tudo puro e testado — os componentes só desenham o que isto decide.
 *
 * Nada aqui inventa números: um campo que o servidor devolve `null` (duração
 * ou resolução que o ffprobe não mediu) dá `null`, e o ecrã mostra «—».
 */
import type { RecordingChapter, RecordingLibraryItem, SessionKind } from '../../api'

export type LibraryFilter = 'all' | 'training' | 'broadcast' | 'meeting' | '4k' | 'processing' | 'failed'

/** Estado visível de uma gravação, pela ordem em que manda. */
export type VisibleState =
  | { kind: 'failed' }
  | { kind: 'processing'; pct: number | null }
  | { kind: 'transcribing'; pct: number | null }
  | { kind: 'published' }
  | { kind: 'retained'; days: number }
  | { kind: 'ready' }

const DAY_MS = 24 * 3600 * 1000

/**
 * `retentionDays` é a política da organização (`organizations.retention_days`,
 * 0 = sem retenção). Uma gravação pronta e NÃO publicada numa organização com
 * retenção mostra quantos dias faltam até a varredura a apagar
 * (`recorder.rs::retention_sweep`), arredondado para cima.
 */
export function visibleState(
  r: Pick<RecordingLibraryItem, 'status' | 'state' | 'progress_pct' | 'transcript_status' | 'created_at'>,
  retentionDays = 0,
  now = Date.now(),
): VisibleState {
  if (r.status === 'failed' || r.state === 'failed') return { kind: 'failed' }
  if (r.state === 'processing') return { kind: 'processing', pct: r.progress_pct ?? null }
  if (r.state === 'transcribing' || (r.state === 'ready' && r.transcript_status === 'transcribing')) {
    return { kind: 'transcribing', pct: r.state === 'transcribing' ? (r.progress_pct ?? null) : null }
  }
  if (r.state === 'published') return { kind: 'published' }
  if (retentionDays > 0) {
    const created = new Date(r.created_at).getTime()
    if (Number.isFinite(created)) {
      const left = Math.ceil((created + retentionDays * DAY_MS - now) / DAY_MS)
      return { kind: 'retained', days: Math.max(0, left) }
    }
  }
  return { kind: 'ready' }
}

/** Rótulo curto da resolução medida: «4K», «1080p», «720p»… ou `null`. */
export function resolutionLabel(r: Pick<RecordingLibraryItem, 'width' | 'height'>): string | null {
  const h = r.height ?? 0
  const w = r.width ?? 0
  if (!h || !w) return null
  if (h >= 2160 || w >= 3840) return '4K'
  return `${Math.min(h, w)}p`
}

export const is4k = (r: Pick<RecordingLibraryItem, 'width' | 'height'>) => resolutionLabel(r) === '4K'

export const isInProgress = (r: Pick<RecordingLibraryItem, 'state' | 'status' | 'transcript_status'>) => {
  const k = visibleState({ ...r, progress_pct: null, created_at: '' }).kind
  return k === 'processing' || k === 'transcribing'
}

/**
 * Categoria do template: Videoaulas (formação; a híbrida é uma videoaula com
 * público presencial), Emissões, Reuniões.
 */
export function kindGroup(kind: SessionKind | string): 'training' | 'broadcast' | 'meeting' {
  if (kind === 'training' || kind === 'hybrid') return 'training'
  if (kind === 'broadcast') return 'broadcast'
  return 'meeting'
}

export function matchesFilter(r: RecordingLibraryItem, f: LibraryFilter): boolean {
  switch (f) {
    case 'all':
      return true
    case 'failed':
      return visibleState(r).kind === 'failed'
    case 'processing':
      return isInProgress(r)
    case '4k':
      return is4k(r)
    default:
      return visibleState(r).kind !== 'failed' && kindGroup(r.kind) === f
  }
}

export function filterCounts(items: RecordingLibraryItem[]): Record<LibraryFilter, number> {
  const out: Record<LibraryFilter, number> = { all: 0, training: 0, broadcast: 0, meeting: 0, '4k': 0, processing: 0, failed: 0 }
  for (const r of items) for (const f of Object.keys(out) as LibraryFilter[]) if (matchesFilter(r, f)) out[f]++
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
export function chapterAt(chapters: Pick<RecordingChapter, 't_ms'>[], tMs: number): number {
  let idx = -1
  for (let i = 0; i < chapters.length; i++) if (chapters[i].t_ms <= tMs) idx = i
  return idx
}

/** Índice do segmento que contém `tMs` (ou o último que já começou), ou -1. */
export function segmentAt(segments: { start_ms: number; end_ms: number }[], tMs: number): number {
  let idx = -1
  for (let i = 0; i < segments.length; i++) {
    if (segments[i].start_ms <= tMs) idx = i
    else break
  }
  return idx
}

/**
 * «Ana Mbala: O failover…» → orador e texto. A transcrição do ai-worker põe o
 * orador à frente quando o sabe; sem ele devolve só o texto.
 */
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
