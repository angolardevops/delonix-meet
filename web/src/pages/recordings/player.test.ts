import { describe, expect, it } from 'vitest'
import type { Meeting, RecordingItem } from '../../api'
import { frameTimes, nextUp, sameSeries } from './playerData'
import { playerHash, studioEditHash, studioEditTarget } from './studioLink'

const ID = '6f1c2a34-9b8d-4e7f-a1b2-c3d4e5f6a7b8'

const rec = (id: string, room: string, status = 'ready'): RecordingItem => ({
  id,
  room_id: 'r',
  uploader_id: 'u',
  filename: `${id}.webm`,
  size_bytes: 1,
  created_at: '2026-09-16T10:00:00Z',
  room_code: room,
  uploader_name: 'Ana',
  owned: true,
  share_count: 0,
  can_download: true,
  status,
  failure_reason: null,
})

const meeting = (id: string, room: string | null, parent: string | null = null, freq: Meeting['recurrence_freq'] = null): Meeting => ({
  id,
  owner_id: 'o',
  owner_name: 'Ana',
  title: id,
  description: '',
  kind: 'video',
  starts_at: '2026-09-16T10:00:00Z',
  duration_min: 30,
  room_code: room,
  is_owner: true,
  recurrence_parent_id: parent,
  recurrence_freq: freq,
})

describe('contrato «Editar no Studio»', () => {
  it('vai e volta pelo endereço', () => {
    expect(studioEditHash(ID)).toBe(`#/studio?editar=${ID}`)
    expect(studioEditTarget(studioEditHash(ID))).toBe(ID)
    expect(studioEditTarget(`#/studio?x=1&editar=${ID.toUpperCase()}`)).toBe(ID)
  })

  it('recusa o que não é um id de gravação', () => {
    expect(studioEditTarget('#/studio')).toBeNull()
    expect(studioEditTarget('#/studio?editar=../../api')).toBeNull()
    expect(studioEditTarget(`#/recordings?editar=${ID}`)).toBeNull()
  })

  it('o leitor tem endereço próprio', () => {
    expect(playerHash(ID)).toBe(`#/recordings/${ID}`)
  })
})

describe('«A seguir» e «Da mesma série»', () => {
  const lib = [rec('a', 'sala-a-um'), rec('b', 'sala-b-um', 'failed'), rec('c', 'sala-c-um'), rec('d', 'sala-d-um'), rec('e', 'sala-e-um')]

  it('a seguir: as seguintes na ordem da biblioteca, sem falhadas nem a própria, e depois as anteriores', () => {
    expect(nextUp(lib, 'c').map((r) => r.id)).toEqual(['d', 'e', 'a'])
    expect(nextUp(lib, 'e').map((r) => r.id)).toEqual(['a', 'c', 'd'])
    expect(nextUp(lib, 'zzz').map((r) => r.id)).toEqual(['a', 'c', 'd', 'e'])
  })

  it('mesma série: pela reunião-mãe da recorrência, casada pela sala', () => {
    const meetings = [
      meeting('mae', 'sala-a-um', null, 'weekly'),
      meeting('f1', 'sala-c-um', 'mae'),
      meeting('f2', 'sala-e-um', 'mae'),
      meeting('solta', 'sala-d-um'),
    ]
    expect(sameSeries(lib, meetings, lib[2]).map((r) => r.id)).toEqual(['a', 'e'])
    expect(sameSeries(lib, meetings, lib[0]).map((r) => r.id)).toEqual(['c', 'e'])
    // Reunião sem recorrência: não há série.
    expect(sameSeries(lib, meetings, lib[3])).toEqual([])
    // Sala sem reunião: não há série.
    expect(sameSeries(lib, [], lib[0])).toEqual([])
  })
})

describe('miniaturas por tempo', () => {
  it('quantidade pela duração, no meio de cada troço', () => {
    expect(frameTimes(0)).toEqual([])
    expect(frameTimes(Number.NaN)).toEqual([])
    const short = frameTimes(40)
    expect(short).toHaveLength(4)
    expect(short[0]).toBeCloseTo(5)
    const long = frameTimes(3 * 3600)
    expect(long).toHaveLength(12)
    expect(long[11]).toBeLessThan(3 * 3600)
  })
})
