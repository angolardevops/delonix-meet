/**
 * Cliente da telefonia (ADR-0009).
 *
 * O que se guarda aqui é o contrato com o servidor que um ecrã não consegue
 * conferir sozinho: o caminho e o método de cada operação, o `DELETE` que
 * responde `204` sem corpo, os filtros que só vão quando existem, e o código
 * `telephony.*` por que a interface escolhe a mensagem.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'

const memoria = new Map<string, string>()
vi.stubGlobal('localStorage', {
  getItem: (k: string) => memoria.get(k) ?? null,
  setItem: (k: string, v: string) => void memoria.set(k, String(v)),
  removeItem: (k: string) => void memoria.delete(k),
  clear: () => memoria.clear(),
})
vi.stubGlobal('window', { dispatchEvent: () => true, addEventListener: () => {} })

const api = await import('./api')

type Pedido = { url: string; init: RequestInit }
let pedidos: Pedido[] = []
function responder(...respostas: Response[]) {
  pedidos = []
  vi.stubGlobal('fetch', async (url: string, init: RequestInit) => {
    pedidos.push({ url, init })
    return respostas.shift() ?? new Response(null, { status: 500 })
  })
}
const json = (corpo: unknown, status = 200) =>
  new Response(JSON.stringify(corpo), { status, headers: { 'content-type': 'application/json' } })
const corpoDe = (p: Pedido) => JSON.parse(String(p.init.body))

beforeEach(() => memoria.clear())

describe('telefonia — caminho e método de cada operação', () => {
  const O = 'org-1'
  const base = '/api/orgs/org-1/telephony'
  // [nome, chamada, método, caminho]
  const casos: [string, () => Promise<unknown>, string, string][] = [
    ['listTrunks', () => api.listTrunks(O), 'GET', '/trunks'],
    ['getTrunk', () => api.getTrunk(O, 't-1'), 'GET', '/trunks/t-1'],
    ['createTrunk', () => api.createTrunk(O, { name: 'A', short_code: 'A', host: 'sip.a.test', max_channels: 4 }), 'POST', '/trunks'],
    ['updateTrunk', () => api.updateTrunk(O, 't-1', { enabled: false }), 'PATCH', '/trunks/t-1'],
    ['setTrunkOrder', () => api.setTrunkOrder(O, ['t-2', 't-1']), 'PUT', '/trunk-order'],
    ['listTrunkPrices', () => api.listTrunkPrices(O, 't-1'), 'GET', '/trunks/t-1/prices'],
    ['createTrunkPrice', () => api.createTrunkPrice(O, 't-1', { price_per_min: { amount: '12.5', currency: 'AOA' } }), 'POST', '/trunks/t-1/prices'],
    ['listExchangeRates', () => api.listExchangeRates(O), 'GET', '/exchange-rates'],
    ['createExchangeRate', () => api.createExchangeRate(O, { currency: 'USD', aoa_per_unit: '912.5' }), 'POST', '/exchange-rates'],
    ['getDialPlan', () => api.getDialPlan(O), 'GET', '/dial-plan'],
    ['putDialPlan', () => api.putDialPlan(O, []), 'PUT', '/dial-plan'],
    ['testDialNumber', () => api.testDialNumber(O, '923447108'), 'POST', '/dial-plan/test'],
    ['getSipSettings', () => api.getSipSettings(O), 'GET', '/sip-settings'],
    ['putSipSettings', () => api.putSipSettings(O, { domain: 'a.sip.test', transport: 'tls', srtp: 'mandatory' }), 'PUT', '/sip-settings'],
    ['revealSipCredentials', () => api.revealSipCredentials(O, { password: 'x' }), 'POST', '/sip-settings/reveal-credentials'],
    ['getSipRegistration', () => api.getSipRegistration(O), 'GET', '/sip-registration'],
    ['restartSipRegistration', () => api.restartSipRegistration(O), 'POST', '/sip-registration/restart'],
    ['startTestCall', () => api.startTestCall(O, '+244923447108'), 'POST', '/test-calls'],
    ['getTestCall', () => api.getTestCall(O, 'c-1'), 'GET', '/test-calls/c-1'],
    ['listTestCalls', () => api.listTestCalls(O), 'GET', '/test-calls'],
    ['listCallRecords', () => api.listCallRecords(O), 'GET', '/call-records'],
    ['getTelephonyUsage', () => api.getTelephonyUsage(O), 'GET', '/usage'],
  ]

  it.each(casos)('%s', async (_nome, chamar, metodo, caminho) => {
    responder(json({}))
    await chamar()
    expect(pedidos).toHaveLength(1)
    expect(pedidos[0].url).toBe(base + caminho)
    expect(pedidos[0].init.method ?? 'GET').toBe(metodo)
  })

  it('são as 23 operações do contrato (22 acima + o DELETE)', () => {
    expect(casos).toHaveLength(22)
  })
})

describe('telefonia — o que o request genérico não cobre', () => {
  it('apagar um tronco com 204 sem corpo resolve (não é erro de parse)', async () => {
    responder(new Response(null, { status: 204 }))
    await expect(api.deleteTrunk('org-1', 't-1')).resolves.toBeUndefined()
    expect(pedidos[0].url).toBe('/api/orgs/org-1/telephony/trunks/t-1')
    expect(pedidos[0].init.method).toBe('DELETE')
  })

  it('a ordem dos troncos vai inteira e na ordem pedida', async () => {
    responder(json({ items: [], next_page_token: null }))
    await api.setTrunkOrder('org-1', ['t-3', 't-1', 't-2'])
    expect(corpoDe(pedidos[0])).toEqual({ trunk_ids: ['t-3', 't-1', 't-2'] })
  })

  it('o PATCH de um tronco manda SÓ o que muda — nunca uma password vazia', async () => {
    responder(json({}))
    await api.updateTrunk('org-1', 't-1', { max_channels: 8 })
    expect(corpoDe(pedidos[0])).toEqual({ max_channels: 8 })
  })

  it('os filtros das chamadas só vão quando existem', async () => {
    responder(json({ items: [], next_page_token: null }), json({ items: [], next_page_token: null }))
    await api.listCallRecords('org-1')
    expect(pedidos[0].url).toBe('/api/orgs/org-1/telephony/call-records')
    await api.listCallRecords('org-1', { direction: 'outbound', outcome: 'no_answer', page_size: 20, page_token: null })
    const u = new URL(pedidos[1].url, 'https://x.test')
    expect(Object.fromEntries(u.searchParams)).toEqual({ direction: 'outbound', outcome: 'no_answer', page_size: '20' })
  })

  it('o consumo pede o mês só quando é dado', async () => {
    responder(json({}), json({}))
    await api.getTelephonyUsage('org-1')
    expect(pedidos[0].url).toBe('/api/orgs/org-1/telephony/usage')
    await api.getTelephonyUsage('org-1', '2026-09')
    expect(pedidos[1].url).toBe('/api/orgs/org-1/telephony/usage?month=2026-09')
  })

  it('revelar credenciais manda a reautenticação e devolve a password uma vez', async () => {
    responder(json({ domain: 'a.sip.test', username: 'u', password: 'p' }))
    const r = await api.revealSipCredentials('org-1', { mfa_code: '123456' })
    expect(corpoDe(pedidos[0])).toEqual({ mfa_code: '123456' })
    expect(r.password).toBe('p')
  })
})

describe('telefonia — o código do erro é o que escolhe a mensagem', () => {
  it('devolve o código `telephony.*` de uma recusa do servidor', async () => {
    responder(json({ code: 'telephony.trunk_in_use', error: 'tronco em uso', details: [] }, 409))
    const erro = await api.deleteTrunk('org-1', 't-1').catch((e: unknown) => e)
    expect(erro).toBeInstanceOf(api.ApiError)
    expect(api.telephonyErrorCode(erro)).toBe('telephony.trunk_in_use')
  })

  it('a emergência não se bloqueia: o servidor recusa e o código chega (R210)', async () => {
    responder(json({ code: 'telephony.emergency_cannot_be_blocked', error: 'x', details: [] }, 422))
    const erro = await api.putDialPlan('org-1', [{ pattern: '112', description: 'x', action: 'block' }]).catch((e: unknown) => e)
    expect(api.telephonyErrorCode(erro)).toBe('telephony.emergency_cannot_be_blocked')
  })

  it('não inventa um código: outro domínio, erro de rede e não-erros dão null', async () => {
    responder(json({ code: 'auth.unauthenticated', error: 'x', details: [] }, 403))
    const outro = await api.getDialPlan('org-1').catch((e: unknown) => e)
    expect(api.telephonyErrorCode(outro)).toBeNull()
    expect(api.telephonyErrorCode(new Error('rede'))).toBeNull()
    expect(api.telephonyErrorCode(undefined)).toBeNull()
  })
})
