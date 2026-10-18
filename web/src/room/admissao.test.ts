/**
 * A recusa da admissão tem de chegar a quem a sofre.
 *
 * O defeito: um `/ws` recusado no HANDSHAKE não entrega ao browser nem o
 * estado HTTP nem o corpo — chega um `onclose` com `code: 1006`, igual ao de
 * uma falha de rede. A pessoa recusada por capacidade via «Ligação instável.
 * Verifica a rede» (`room.estado.ligacaoInstavel`), com a rede dela perfeita,
 * e a página recarregava em ciclo.
 */
import { describe, expect, it, vi, afterEach } from 'vitest'
import { motivoDoCorpo, perguntarPorqueRecusou } from './admissao'

afterEach(() => vi.unstubAllGlobals())

describe('motivoDoCorpo', () => {
  it.each([
    ['admission.org_at_capacity', 'org_at_capacity'],
    ['admission.node_at_capacity', 'node_at_capacity'],
    ['admission.over_fair_share', 'over_fair_share'],
  ])('%s é reconhecido', (code, esperado) => {
    expect(motivoDoCorpo({ code })).toBe(esperado)
  })

  it('um código que não é de admissão não vira frase de admissão', () => {
    // `rate_limited` era o que a quota da organização devolvia ANTES de ter
    // código próprio: indistinguível de «estás a pedir depressa de mais».
    expect(motivoDoCorpo({ code: 'rate_limited' })).toBeNull()
    expect(motivoDoCorpo({ code: 'unavailable' })).toBeNull()
  })

  it('um corpo que não serve não rebenta', () => {
    for (const x of [null, undefined, 'texto', 42, {}, { code: 7 }, []]) {
      expect(motivoDoCorpo(x)).toBeNull()
    }
  })
})

describe('perguntarPorqueRecusou', () => {
  it('vai ao prefixo /ws e leva o `room` — é o que fixa o pod', async () => {
    // Sem `room=` a sondagem cai num pod ao acaso: o ingress só encaminha o
    // /ws por `upstream-hash-by: $arg_room` (ADR-0001), e a contagem da
    // admissão é da memória de CADA pod. A resposta viria do nó errado.
    const fetchFalso = vi.fn().mockResolvedValue({
      ok: false,
      json: () => Promise.resolve({ code: 'admission.org_at_capacity' }),
    })
    vi.stubGlobal('fetch', fetchFalso)

    expect(await perguntarPorqueRecusou('tok en/zh', 'ABC-123')).toBe('org_at_capacity')
    const url = fetchFalso.mock.calls[0][0] as string
    expect(url.startsWith('/ws/admission?')).toBe(true)
    expect(url).toContain('room=ABC-123')
    expect(url).toContain(`token=${encodeURIComponent('tok en/zh')}`)
  })

  it('204 = entrarias agora, logo a recusa foi mesmo de rede', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve(null) }))
    expect(await perguntarPorqueRecusou('t', 'A')).toBeNull()
  })

  it('falhar a explicar uma falha não piora o que a pessoa vê', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new TypeError('Failed to fetch')))
    await expect(perguntarPorqueRecusou('t', 'A')).resolves.toBeNull()

    // Resposta de erro sem corpo JSON (um 502 de um proxy, p.ex.).
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false, json: () => Promise.reject(new Error('x')) }))
    await expect(perguntarPorqueRecusou('t', 'A')).resolves.toBeNull()
  })
})
