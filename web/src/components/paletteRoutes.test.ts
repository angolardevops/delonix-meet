import { describe, expect, it } from 'vitest'
import { hitHash, moreHash, withRecent } from './paletteRoutes'

describe('pesquisa global → rotas da app', () => {
  it('cada tipo abre o seu ecrã pelos ids do target (nunca pelo href da API)', () => {
    expect(hitHash({ type: 'meetings', id: 'm1', target: { meeting_id: 'm1', room_code: null } })).toBe('/calendar/m/m1')
    expect(hitHash({ type: 'recordings', id: 'r1', target: { recording_id: 'r1', at_secs: null } })).toBe('/recordings/r1')
    expect(hitHash({ type: 'recordings', id: 'r1', target: { recording_id: 'r1', at_secs: 754.6 } })).toBe('/recordings/r1?t=754')
    expect(hitHash({ type: 'people', id: 'u1', target: { user_id: 'u1' } })).toBe('/directory?u=u1')
    expect(hitHash({ type: 'whiteboards', id: 'w1', target: { whiteboard_id: 'w1' } })).toBe('/whiteboards?id=w1')
    expect(hitHash({ type: 'messages', id: 'x', target: { room_code: 'abc-def-ghi', message_id: 'x', created_at: '2026-09-01' } })).toBe('/r/abc-def-ghi')
    expect(hitHash({ type: 'rooms', id: 'x', target: {} })).toBeNull()
  })

  it('«ver todos» leva a pesquisa para a lista do recurso', () => {
    expect(moreHash('recordings', 'orçamento 2027')).toBe('/recordings?q=or%C3%A7amento%202027')
    expect(moreHash('meetings', 'x')).toBe('/calendar?vista=lista&q=x')
    expect(moreHash('audit_events', 'login')).toBe('/admin?audit.q=login')
    expect(moreHash('webhooks', 'x')).toBeNull()
  })

  it('pesquisas recentes: topo, sem repetidos, no máximo 8', () => {
    let l: string[] = []
    for (const q of ['a', 'b', 'A', ' c ', '', 'd', 'e', 'f', 'g', 'h', 'i']) l = withRecent(l, q)
    expect(l).toEqual(['i', 'h', 'g', 'f', 'e', 'd', 'c', 'A'])
  })
})
