import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { RING_TIMEOUT_MS, roomCodeInHash } from './callRing'

describe('toque de chamada', () => {
  it('reconhece a sala aberta no hash', () => {
    expect(roomCodeInHash('#/r/abc-defg-hij')).toBe('abc-defg-hij')
    expect(roomCodeInHash('#/r/abc-defg-hij?voice')).toBe('abc-defg-hij')
    expect(roomCodeInHash('#/directory')).toBeNull()
    expect(roomCodeInHash('')).toBeNull()
  })

  it('o toque tem fim e quem liga cancela ao sair antes de atenderem', () => {
    expect(RING_TIMEOUT_MS).toBeGreaterThan(0)
    const src = readFileSync(new URL('./components/PresenceProvider.tsx', import.meta.url), 'utf8')
    // Sem isto o destinatário tocava até recarregar a página.
    expect(src).toContain('presenceRef.current?.cancel(code)')
    expect(src).toContain("case 'accepted':")
  })

  it('atender/recusar pára o toque nos outros dispositivos da mesma conta', () => {
    const rs = readFileSync(new URL('../../server/src/presence.rs', import.meta.url), 'utf8')
    const accept = rs.slice(rs.indexOf('CallClientMsg::CallAccept'), rs.indexOf('CallClientMsg::CallCancel'))
    expect(accept.match(/CallServerMsg::Cancelled/g)?.length).toBe(2)
  })
})
