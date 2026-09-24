/**
 * «Ocorrência 2 de 6» — contado no cliente. As instâncias de uma reunião
 * recorrente são linhas próprias com recurrence_parent_id a apontar para a
 * primeira (meetings.rs generate_instances); a primeira não tem pai. A
 * posição é a ordem por início dentro da série que a lista devolveu.
 */
import type { Meeting } from '../../api'

export interface Occurrence {
  index: number
  total: number
}

export function occurrenceOf(meetings: Meeting[], m: Meeting): Occurrence | null {
  const root = m.recurrence_parent_id ?? (m.recurrence_freq ? m.id : null)
  if (!root) return null
  const series = meetings
    .filter((x) => x.id === root || x.recurrence_parent_id === root)
    .sort((a, b) => a.starts_at.localeCompare(b.starts_at))
  const index = series.findIndex((x) => x.id === m.id)
  if (index < 0 || series.length < 2) return null
  return { index: index + 1, total: series.length }
}
