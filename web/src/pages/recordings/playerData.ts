/**
 * Dados do leitor calculados do que o servidor já devolve — sem pedidos
 * novos e sem números inventados.
 */
import type { Meeting, RecordingItem } from '../../api'
import { isFailed } from './format'

/**
 * «A seguir»: as gravações depois desta na ordem da biblioteca (a do
 * servidor, mais recentes primeiro), e a seguir as de antes. Nunca a própria,
 * nunca uma falhada.
 */
export function nextUp(library: RecordingItem[], currentId: string, limit = 10): RecordingItem[] {
  const ok = library.filter((r) => !isFailed(r))
  const i = ok.findIndex((r) => r.id === currentId)
  const ordered = i < 0 ? ok : [...ok.slice(i + 1), ...ok.slice(0, i)]
  return ordered.slice(0, limit)
}

/**
 * «Da mesma série»: reuniões com a mesma reunião-mãe de recorrência
 * (`recurrence_parent_id`), casadas com as gravações pelo código da sala.
 * Uma sala sem reunião, ou uma reunião sem recorrência, não tem série.
 */
export function sameSeries(library: RecordingItem[], meetings: Meeting[], rec: RecordingItem): RecordingItem[] {
  const m = meetings.find((x) => x.room_code === rec.room_code)
  if (!m) return []
  const root = m.recurrence_parent_id ?? m.id
  const recurring = !!m.recurrence_parent_id || !!m.recurrence_freq || meetings.some((x) => x.recurrence_parent_id === m.id)
  if (!recurring) return []
  const rooms = new Set(
    meetings.filter((x) => (x.id === root || x.recurrence_parent_id === root) && x.room_code).map((x) => x.room_code as string),
  )
  return library.filter((r) => r.id !== rec.id && !isFailed(r) && rooms.has(r.room_code))
}

/** Instantes (s) das miniaturas: uma por cada ~30 s, entre 4 e 12, no meio do troço. */
export function frameTimes(duration: number): number[] {
  if (!Number.isFinite(duration) || duration <= 0) return []
  const n = Math.max(4, Math.min(12, Math.round(duration / 30)))
  const step = duration / n
  return Array.from({ length: n }, (_, i) => (i + 0.5) * step)
}
