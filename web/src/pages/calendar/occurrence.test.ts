import { describe, expect, it } from 'vitest'
import type { Meeting } from '../../api'
import { occurrenceOf } from './occurrence'

const base = { owner_id: 'o', owner_name: 'Ana', description: '', kind: 'video', duration_min: 15, room_code: null, is_owner: true } as const
const mk = (id: string, starts_at: string, parent: string | null, freq: 'weekly' | null = 'weekly'): Meeting => ({
  ...base,
  id,
  title: id,
  starts_at,
  recurrence_freq: freq,
  recurrence_parent_id: parent,
})

describe('occurrenceOf', () => {
  const series = [mk('c', '2026-09-28T08:00:00Z', 'a'), mk('a', '2026-09-14T08:00:00Z', null), mk('b', '2026-09-21T08:00:00Z', 'a')]
  const solo = mk('x', '2026-09-15T08:00:00Z', null, null)

  it('conta pela ordem de início dentro da série', () => {
    expect(occurrenceOf(series, series[1])).toEqual({ index: 1, total: 3 })
    expect(occurrenceOf(series, series[2])).toEqual({ index: 2, total: 3 })
    expect(occurrenceOf(series, series[0])).toEqual({ index: 3, total: 3 })
  })

  it('uma reunião sem série não tem ocorrência', () => {
    expect(occurrenceOf([...series, solo], solo)).toBeNull()
  })

  it('uma série com uma só instância visível não diz «1 de 1»', () => {
    expect(occurrenceOf([series[1]], series[1])).toBeNull()
  })
})
