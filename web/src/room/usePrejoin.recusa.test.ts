/**
 * A sondagem da fila de espera da pré-entrada pergunta a cada cinco segundos
 * quem está à porta, e pára quando a resposta diz que não vale a pena voltar a
 * perguntar.
 *
 * O 401 faltava nessa lista, e foi uma MEDIÇÃO com browser que o mostrou: um
 * convidado na pré-entrada (que não tem conta, e a rota pede uma) levava uma
 * recusa que não parava o intervalo — um pedido a cada cinco segundos enquanto
 * ele lá estivesse. O convidado já nem chega a perguntar, mas o travão fica:
 * vale para qualquer sessão que caduque com a pré-entrada aberta.
 */
import { describe, expect, it, vi } from 'vitest'

// O `api.ts` lê o armazenamento ao ser carregado — os mesmos duplos que o
// `convidado.test.ts` usa, pela mesma razão.
const armazem = () => {
  const m = new Map<string, string>()
  return {
    getItem: (k: string) => m.get(k) ?? null,
    setItem: (k: string, v: string) => void m.set(k, String(v)),
    removeItem: (k: string) => void m.delete(k),
    clear: () => m.clear(),
  }
}
vi.stubGlobal('localStorage', armazem())
vi.stubGlobal('sessionStorage', armazem())
vi.stubGlobal('window', { dispatchEvent: () => true, addEventListener: () => {} })

const { ApiError } = await import('../api')
const { recusaDefinitiva } = await import('./usePrejoin')

describe('recusaDefinitiva', () => {
  it.each([
    [401, 'identidade: não há sessão que sirva'],
    [403, 'permissão: esta pessoa não admite'],
    [404, 'a sala não tem fila para esta pessoa'],
  ])('%i pára a sondagem — %s', (status) => {
    expect(recusaDefinitiva(new ApiError(status, null, 'recusado'))).toBe(true)
  })

  it.each([[429], [500], [502], [503]])('%i NÃO pára — é transitório e a fila volta', (status) => {
    // Parar aqui deixava o anfitrião sem ver quem bate à porta durante o resto
    // da pré-entrada, por causa de um 500 de um segundo.
    expect(recusaDefinitiva(new ApiError(status, null, 'temporário'))).toBe(false)
  })

  it('uma falha de rede não pára — não é uma resposta do servidor', () => {
    expect(recusaDefinitiva(new TypeError('Failed to fetch'))).toBe(false)
    expect(recusaDefinitiva(null)).toBe(false)
  })
})
