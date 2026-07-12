import { describe, it, expect, vi } from 'vitest'
import { makeCallHolderStart } from './sfuLifecycle'

// Codifica as regressões R1/R2 como testes executáveis (avaliação de arquitetura
// #2/#4). Antes viviam "na cabeça das pessoas" em docs/reference/regressions.md.

describe('makeCallHolderStart (R1/R2)', () => {
  it('R2: cria a chamada UMA só vez, mesmo com start() repetido (anti-duplo-arranque)', () => {
    const ref: { current: unknown } = { current: null }
    const create = vi.fn(() => ({ id: 'call' }))
    const start = makeCallHolderStart({ ref, isCancelled: () => false, create })

    start()
    start()
    start()

    expect(create).toHaveBeenCalledTimes(1)
    expect(ref.current).toEqual({ id: 'call' })
  })

  it('R2: NÃO monta a chamada enquanto o convidado aguarda admissão (cancelled)', () => {
    const ref: { current: unknown } = { current: null }
    const create = vi.fn(() => ({ id: 'call' }))
    // isCancelled=true simula "em espera / efeito limpo": montar aqui geraria
    // oferta stale → glare → flood → reload após admitir (a regressão R2).
    const start = makeCallHolderStart({ ref, isCancelled: () => true, create })

    start()

    expect(create).not.toHaveBeenCalled()
    expect(ref.current).toBeNull()
  })

  it('cria assim que deixa de estar cancelado (arranque no momento certo, ex.: após joined)', () => {
    const ref: { current: unknown } = { current: null }
    const create = vi.fn(() => ({ id: 'call' }))
    let waiting = true
    const start = makeCallHolderStart({ ref, isCancelled: () => waiting, create })

    start() // ainda em espera → no-op
    expect(create).not.toHaveBeenCalled()

    waiting = false // admitido (handler 'joined')
    start()
    expect(create).toHaveBeenCalledTimes(1)
  })
})
