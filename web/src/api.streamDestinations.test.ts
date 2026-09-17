/**
 * Cliente dos destinos de directo guardados (frontend/b1-emissao).
 *
 * O que se guarda aqui é o que o `request` genérico não cobria: o `DELETE`
 * responde `204` SEM corpo, e o `request` lê sempre JSON — um «apagado com
 * sucesso» virava um erro de parse na interface.
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

const { ApiError, createStreamDestination, deleteStreamDestination, listStreamDestinations, updateStreamDestination } =
  await import('./api')

type Pedido = { url: string; init: RequestInit }
let pedidos: Pedido[] = []
function responder(...respostas: Response[]) {
  pedidos = []
  vi.stubGlobal('fetch', async (url: string, init: RequestInit) => {
    pedidos.push({ url, init })
    return respostas.shift() ?? new Response(null, { status: 500 })
  })
}

beforeEach(() => memoria.clear())

describe('destinos de directo guardados', () => {
  it('apagar com 204 sem corpo resolve (não é erro de parse)', async () => {
    responder(new Response(null, { status: 204 }))
    await expect(deleteStreamDestination('org-1', 'd-1')).resolves.toBeUndefined()
    expect(pedidos[0].url).toBe('/api/orgs/org-1/stream-destinations/d-1')
    expect(pedidos[0].init.method).toBe('DELETE')
  })

  it('apagar o que não existe dá o 404 do servidor', async () => {
    responder(new Response(JSON.stringify({ error: 'not found' }), { status: 404 }))
    const e = await deleteStreamDestination('org-1', 'x').catch((err) => err)
    expect(e).toBeInstanceOf(ApiError)
    expect((e as InstanceType<typeof ApiError>).status).toBe(404)
  })

  it('criar manda a chave UMA vez, no corpo; alterar sem chave não a manda', async () => {
    const recurso = { id: 'd-1', key_set: true }
    responder(
      new Response(JSON.stringify(recurso), { status: 201 }),
      new Response(JSON.stringify(recurso), { status: 200 }),
    )
    await createStreamDestination('org-1', { label: 'Canal', rtmp_url: 'rtmp://x/live', stream_key: 'k-1' })
    expect(pedidos[0].init.method).toBe('POST')
    expect(JSON.parse(String(pedidos[0].init.body))).toEqual({ label: 'Canal', rtmp_url: 'rtmp://x/live', stream_key: 'k-1' })
    await updateStreamDestination('org-1', 'd-1', { label: 'Outro' })
    expect(pedidos[1].init.method).toBe('PATCH')
    expect(String(pedidos[1].init.body)).not.toContain('stream_key')
  })

  it('listar vai ao caminho da organização', async () => {
    responder(new Response('[]', { status: 200 }))
    await expect(listStreamDestinations('org-9')).resolves.toEqual([])
    expect(pedidos[0].url).toBe('/api/orgs/org-9/stream-destinations')
  })
})
