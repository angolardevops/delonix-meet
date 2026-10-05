import { describe, expect, it } from 'vitest'
import type { ServerMsg } from '../signaling'

// O contrato com o servidor: os nomes das mensagens e dos campos (snake_case) não mudam em silêncio.
describe('dispositivos duplicados · contrato com o servidor', () => {
  it('as mensagens que o servidor envia têm os campos que o cliente lê', () => {
    const aviso: ServerMsg = { type: 'duplicate-device', phone_id: 'p', can_hangup: true }
    const res: ServerMsg = { type: 'duplicate-resolved', phone_id: 'p', outcome: 'muted' }
    expect(aviso.type).toBe('duplicate-device')
    expect(res.type).toBe('duplicate-resolved')
    expect(['hung_up', 'muted', 'both']).toContain(res.outcome)
  })
})
