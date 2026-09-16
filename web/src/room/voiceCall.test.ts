import { describe, expect, it } from 'vitest'
import { DirectCall, voicePhase } from './voiceCall'

const out = (p: Partial<DirectCall> = {}): DirectCall => ({
  room_code: 'abc-def-ghi',
  kind: 'voice',
  direction: 'out',
  peer_name: 'Teresa',
  ringing: ['b'],
  offline: [],
  accepted: [],
  declined: [],
  answered_by_server: true,
  ...p,
})
const base = { roomState: 'in', callState: 'connected', peers: 0, everHadPeer: false }

describe('voicePhase', () => {
  it('fora da sala é sempre «a ligar», mesmo com alguém na lista', () => {
    expect(voicePhase({ ...base, roomState: 'connecting', peers: 1, call: out() })).toBe('connecting')
  })

  it('«em chamada» só com alguém do outro lado', () => {
    expect(voicePhase({ ...base, call: out() })).toBe('calling')
    expect(voicePhase({ ...base, peers: 1, call: out() })).toBe('in-call')
    expect(voicePhase({ ...base, peers: 1, callState: 'reconnecting', call: out() })).toBe('reconnecting')
  })

  it('a chamar enquanto o servidor não respondeu, e enquanto houver quem ainda toque', () => {
    expect(voicePhase({ ...base, call: out({ answered_by_server: false, ringing: [] }) })).toBe('calling')
    expect(voicePhase({ ...base, call: out({ ringing: ['b', 'c'], declined: ['b'] }) })).toBe('calling')
  })

  it('recusou: todos os que tocaram recusaram', () => {
    expect(voicePhase({ ...base, call: out({ declined: ['b'] }) })).toBe('declined')
  })

  it('indisponível: ninguém online — não se finge que está a tocar', () => {
    expect(voicePhase({ ...base, call: out({ ringing: [], offline: ['b'] }) })).toBe('unavailable')
  })

  it('quem atendeu e ficou sozinho espera; quem teve alguém e o perdeu vê o fim', () => {
    expect(voicePhase({ ...base, call: out({ direction: 'in' }) })).toBe('waiting')
    expect(voicePhase({ ...base, call: null })).toBe('waiting')
    expect(voicePhase({ ...base, everHadPeer: true, call: out() })).toBe('ended')
  })
})
