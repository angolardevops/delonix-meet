/**
 * Cliente do convidado sem conta (fim de `api.ts`). O servidor está provado em
 * `web/e2e/convidado.mjs`; aqui prova-se o que o ecrã vai ler: que o pedido sai
 * SEM a sessão de quem estiver no browser, e que cada recusa chega com a razão
 * certa e o `Retry-After`.
 */
import { describe, expect, it, vi } from 'vitest'

const memoria = new Map<string, string>([['dx_access', 'token-de-membro'], ['dx_user', '{"id":"u"}']])
vi.stubGlobal('localStorage', {
  getItem: (k: string) => memoria.get(k) ?? null,
  setItem: (k: string, v: string) => void memoria.set(k, String(v)),
  removeItem: (k: string) => void memoria.delete(k),
  clear: () => memoria.clear(),
})
vi.stubGlobal('window', { dispatchEvent: () => true, addEventListener: () => {} })

const { guestJoin, GuestJoinError, guestJoinFailure } = await import('./api')

function resposta(status: number, body: unknown, headers: Record<string, string> = {}) {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json', ...headers } })
}


describe('guestJoin', () => {
  it('vai sem Authorization nem cookies, mesmo com uma sessão no browser', async () => {
    const f = vi.fn().mockResolvedValue(resposta(200, { room_token: 't', room: { code: 'abc-defg-hij' } }))
    vi.stubGlobal('fetch', f)
    const r = await guestJoin('abc-defg-hij', 'Ana')
    expect(r.room_token).toBe('t')
    const [url, init] = f.mock.calls[0]
    expect(url).toBe('/api/rooms/abc-defg-hij/guest-join')
    expect(init.method).toBe('POST')
    expect(init.credentials).toBe('omit')
    expect(init.headers).not.toHaveProperty('Authorization')
    expect(JSON.parse(init.body)).toEqual({ display_name: 'Ana' })
  })

  it.each([
    [403, 'closed'],
    [404, 'not_found'],
    [400, 'invalid_name'],
    [422, 'invalid_name'],
    [500, 'unavailable'],
  ] as const)('%i → %s', async (status, razao) => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(resposta(status, { error: 'x' })))
    const e = await guestJoin('abc-defg-hij', 'Ana').catch((x) => x)
    expect(e).toBeInstanceOf(GuestJoinError)
    expect(e.reason).toBe(razao)
    expect(e.status).toBe(status)
  })

  it('429 traz o Retry-After em segundos', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(resposta(429, { error: 'too many requests' }, { 'Retry-After': '60' })))
    const e = await guestJoin('abc-defg-hij', 'Ana').catch((x) => x)
    expect(e.reason).toBe('rate_limited')
    expect(e.retryAfterSecs).toBe(60)
  })

  it('uma falha de rede não é uma recusa da sala', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new TypeError('Failed to fetch')))
    const e = await guestJoin('abc-defg-hij', 'Ana').catch((x) => x)
    expect(e.reason).toBe('unavailable')
    expect(guestJoinFailure(0)).toBe('unavailable')
  })
})
