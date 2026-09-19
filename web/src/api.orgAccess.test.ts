/**
 * Cliente de papéis, aprovações e convites (ADR-0008). O servidor decide;
 * o que se prova aqui é o que o cliente PEDE: os caminhos, os métodos, o
 * `delivery: manual` (sem SMTP) e que o `204` sem corpo não é erro.
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
vi.stubGlobal('location', { origin: 'https://meet.exemplo', pathname: '/', hash: '' })

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
const json = (o: unknown, status = 200) => new Response(JSON.stringify(o), { status })

beforeEach(() => memoria.clear())

describe('papéis e capacidades', () => {
  it('apagar um papel com pessoas passa o reassign_to; sem corpo (204) resolve', async () => {
    responder(new Response(null, { status: 204 }))
    await expect(api.deleteOrgRole('o1', 'r1', 'membro')).resolves.toBeUndefined()
    expect(pedidos[0].url).toBe('/api/orgs/o1/roles/r1?reassign_to=membro')
    expect(pedidos[0].init.method).toBe('DELETE')
  })

  it('gravar capacidades faz PUT com { values }', async () => {
    responder(json({ role_id: 'r1', catalog_version: 1, items: [], warnings: [] }))
    await api.putRoleCapabilities('o1', 'r1', { 'sessions.create': 'deny' })
    expect(pedidos[0].url).toBe('/api/orgs/o1/roles/r1/capabilities')
    expect(pedidos[0].init.method).toBe('PUT')
    expect(JSON.parse(String(pedidos[0].init.body))).toEqual({ values: { 'sessions.create': 'deny' } })
  })

  it('duplicar sem nome envia corpo vazio; atribuir usa PUT com role_id', async () => {
    responder(json({ id: 'novo' }, 201), json({ changed: true }))
    await api.duplicateOrgRole('o1', 'r1')
    await api.assignOrgRole('o1', 'u1', 'r2')
    expect(pedidos[0].url).toBe('/api/orgs/o1/roles/r1/duplicate')
    expect(JSON.parse(String(pedidos[0].init.body))).toEqual({})
    expect(pedidos[1].url).toBe('/api/orgs/o1/members/u1/role')
    expect(pedidos[1].init.method).toBe('PUT')
    expect(JSON.parse(String(pedidos[1].init.body))).toEqual({ role_id: 'r2' })
  })
})

describe('aprovações', () => {
  it('aprovar e recusar vão a rotas diferentes', async () => {
    responder(json({ id: 'a1' }), json({ id: 'a1' }))
    await api.decideApprovalRequest('o1', 'a1', true)
    await api.decideApprovalRequest('o1', 'a1', false, 'não')
    expect(pedidos[0].url).toBe('/api/orgs/o1/approval-requests/a1/approve')
    expect(pedidos[1].url).toBe('/api/orgs/o1/approval-requests/a1/reject')
    expect(JSON.parse(String(pedidos[1].init.body))).toEqual({ reason: 'não' })
  })
})

describe('convites por link', () => {
  it('criar pede entrega manual (sem SMTP) e o link usa o token devolvido', async () => {
    responder(json({ id: 'i1', token: 'abc123', expires_at: '2030-01-01T00:00:00Z' }, 201))
    const inv = await api.createInvitation('o1', { email: 'a@b.pt', role_id: 'r1' })
    expect(JSON.parse(String(pedidos[0].init.body))).toEqual({ email: 'a@b.pt', role_id: 'r1', delivery: 'manual' })
    expect(api.invitationLink(inv.token)).toBe('https://meet.exemplo/#/invite/abc123')
  })

  it('aceitar envia só o token; revogar com 204 resolve', async () => {
    responder(json({ org_id: 'o1', role_id: 'r1' }), new Response(null, { status: 204 }))
    await api.acceptInvitation('abc123')
    await expect(api.revokeInvitation('o1', 'i1')).resolves.toBeUndefined()
    expect(pedidos[0].url).toBe('/api/invitations/accept')
    expect(JSON.parse(String(pedidos[0].init.body))).toEqual({ token: 'abc123' })
    expect(pedidos[1].url).toBe('/api/orgs/o1/invitations/i1')
  })

  it('suspender vai por acções em massa', async () => {
    responder(json({ action: 'suspend', succeeded: 1, failed: 0, results: [] }))
    await api.bulkUserAction('o1', { action: 'suspend', user_ids: ['u1'] })
    expect(pedidos[0].url).toBe('/api/orgs/o1/users/bulk-actions')
    expect(JSON.parse(String(pedidos[0].init.body))).toEqual({ action: 'suspend', user_ids: ['u1'] })
  })
})
