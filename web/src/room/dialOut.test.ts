import { describe, expect, it, vi } from 'vitest'
import type { DialOut } from '../api'

// `api.ts` lê o localStorage ao ser importado: sem DOM, simula-se antes.
vi.stubGlobal('localStorage', { getItem: () => null, setItem: () => {}, removeItem: () => {} })
const { ApiError } = await import('../api')
const { chaveDaFalha, chaveDoErro, estaVivo, maisRecentes, tomDoEstado } = await import('./dialOut')

const d = (over: Partial<DialOut>): DialOut => ({
  id: 'x', room_code: 'r', extension_id: 'e', extension: '201', display_name: 'Recepção', status: 'queued',
  failure_code: null, created_at: '2026-10-05T10:00:00Z', answered_at: null, ended_at: null, billsec: null, ...over,
})

describe('dial-out · estados', () => {
  it('só tocar e em chamada estão vivos', () => {
    for (const s of ['queued', 'dialing', 'ringing', 'in_call'] as const) expect(estaVivo(s)).toBe(true)
    for (const s of ['ended', 'declined', 'no_answer', 'failed', 'cancelled'] as const) expect(estaVivo(s)).toBe(false)
  })
  it('o tom separa em chamada, a tocar e falhado', () => {
    expect(tomDoEstado('in_call')).toBe('success')
    expect(tomDoEstado('ringing')).toBe('live')
    expect(tomDoEstado('failed')).toBe('warning')
    expect(tomDoEstado('ended')).toBe('neutral')
  })
})

describe('dial-out · erros e causas', () => {
  it('um código conhecido do servidor dá uma chave i18n; um desconhecido ou um erro de rede, nada', () => {
    expect(chaveDoErro(new ApiError(422, { code: 'dial_out.room_recording' }, 'x'))).toBe('room.ligar.erro.dial_out_room_recording')
    expect(chaveDoErro(new ApiError(403, { code: 'authz.missing_capability' }, 'x'))).toBe('room.ligar.erro.authz_missing_capability')
    expect(chaveDoErro(new ApiError(500, { code: 'qualquer.coisa' }, 'x'))).toBeNull()
    expect(chaveDoErro(new ApiError(500, null, 'x'))).toBeNull()
    expect(chaveDoErro(new Error('rede'))).toBeNull()
  })
  it('só um pedido falhado tem causa a mostrar', () => {
    expect(chaveDaFalha(d({ status: 'failed', failure_code: 'USER_NOT_REGISTERED' }))).toBe('room.ligar.falha.naoRegistado')
    expect(chaveDaFalha(d({ status: 'failed', failure_code: 'stale' }))).toBe('room.ligar.falha.semResposta')
    expect(chaveDaFalha(d({ status: 'failed', failure_code: 'NORMAL_TEMPORARY_FAILURE' }))).toBe('room.ligar.falha.outra')
    expect(chaveDaFalha(d({ status: 'declined', failure_code: 'USER_BUSY' }))).toBeNull()
  })
  it('ordena do mais recente e limita', () => {
    const l = [d({ id: 'a', created_at: '2026-10-05T10:00:00Z' }), d({ id: 'b', created_at: '2026-10-05T11:00:00Z' }), d({ id: 'c', created_at: '2026-10-05T09:00:00Z' })]
    expect(maisRecentes(l, 2).map((x) => x.id)).toEqual(['b', 'a'])
  })
})
