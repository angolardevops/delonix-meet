/**
 * Quando é que o botão de remarcar aparece.
 *
 * O servidor recusa uma recorrente, uma já começada e uma hora no passado. O
 * ecrã sabe tudo isso pelo `Meeting` que já tem — e estes testes fixam que
 * sabe, para o botão não ser um botão que só falha depois de se clicar.
 */
import { describe, expect, it } from 'vitest'
import type { Meeting } from '../../api'
import { horaJaPassou, podeRemarcar, razaoNaoRemarca } from './podeRemarcar'

const base: Meeting = {
  id: 'm1',
  owner_id: 'u1',
  owner_name: 'Ana',
  title: 'Conselho',
  description: '',
  kind: 'video',
  starts_at: '2026-10-20T09:00:00Z',
  duration_min: 30,
  room_code: null,
  is_owner: true,
}

describe('razaoNaoRemarca', () => {
  it('o anfitrião de uma reunião simples que não começou pode', () => {
    expect(razaoNaoRemarca(base)).toBeNull()
    expect(podeRemarcar(base)).toBe(true)
  })

  it('quem não é anfitrião, não', () => {
    expect(razaoNaoRemarca({ ...base, is_owner: false })).toBe('nao-e-anfitriao')
  })

  it('uma que já começou, não — a hora de início deixou de ser uma marcação', () => {
    expect(razaoNaoRemarca({ ...base, started_at: '2026-10-20T09:01:00Z' })).toBe('ja-comecou')
  })

  it('o pai de uma série, não', () => {
    expect(razaoNaoRemarca({ ...base, recurrence_freq: 'weekly' })).toBe('recorrente')
  })

  it('uma INSTÂNCIA de uma série também não — é o caso que se esquece', () => {
    expect(razaoNaoRemarca({ ...base, recurrence_parent_id: 'pai' })).toBe('recorrente')
  })

  it('a ordem das razões: já começou ganha à recorrência', () => {
    // Uma instância de série que já começou: a razão mais concreta primeiro.
    const m = { ...base, started_at: '2026-10-20T09:01:00Z', recurrence_parent_id: 'pai' }
    expect(razaoNaoRemarca(m)).toBe('ja-comecou')
  })

  it('não ser anfitrião ganha a tudo: nem se chega a ver o resto', () => {
    const m = { ...base, is_owner: false, started_at: '2026-10-20T09:01:00Z' }
    expect(razaoNaoRemarca(m)).toBe('nao-e-anfitriao')
  })
})

describe('horaJaPassou', () => {
  const AGORA = new Date(2026, 9, 20, 10, 0).getTime()

  it('uma hora no passado avisa ANTES de se mandar', () => {
    expect(horaJaPassou(new Date(2026, 9, 20, 9, 59), AGORA)).toBe(true)
  })

  it('agora mesmo também: o servidor recusa `<=`', () => {
    expect(horaJaPassou(new Date(AGORA), AGORA)).toBe(true)
  })

  it('no futuro, não', () => {
    expect(horaJaPassou(new Date(2026, 9, 20, 10, 1), AGORA)).toBe(false)
  })
})
