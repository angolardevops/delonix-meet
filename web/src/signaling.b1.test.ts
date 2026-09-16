// Contrato de fio das mensagens novas da sala (frontend/b1-sala): o que o
// cliente escreve tem de ser exactamente o que o servidor desserializa (os
// nomes kebab-case do `ClientMsg`), e o que o servidor manda tem de chegar ao
// handler certo. Uma mensagem com o nome errado é descartada EM SILÊNCIO pelo
// servidor — é por isso que se testa o texto, não o tipo.
import { describe, expect, it, vi, beforeEach } from 'vitest'
import { Signaling, sendB1, onB1 } from './signaling'

class FakeWs {
  static OPEN = 1
  static last: FakeWs | null = null
  readyState = 1
  sent: string[] = []
  onmessage: ((e: { data: string }) => void) | null = null
  onclose: (() => void) | null = null
  constructor(public url: string) {
    FakeWs.last = this
  }
  send(d: string) {
    this.sent.push(d)
  }
  close() {}
}

beforeEach(() => {
  vi.stubGlobal('WebSocket', FakeWs)
  vi.stubGlobal('location', { protocol: 'http:', host: 'x' })
})

describe('sinalização b1-sala', () => {
  it('envia as mensagens novas com os nomes que o servidor aceita', () => {
    const sig = new Signaling('tok')
    sendB1(sig, { type: 'set-role', to: 'p', role: 'cohost' })
    sendB1(sig, { type: 'qa-hide', id: 'q' })
    sendB1(sig, { type: 'wb-cursor', x: 0.1, y: 0.2, laser: true, input: 'pen' })
    sendB1(sig, { type: 'breakouts-create', count: 2, minutes: null, assign: 'manual' })
    const tipos = FakeWs.last!.sent.map((s) => JSON.parse(s).type)
    expect(tipos).toEqual(['set-role', 'qa-hide', 'wb-cursor', 'breakouts-create'])
  })

  it('entrega as mensagens novas do servidor ao handler certo', () => {
    const sig = new Signaling('tok')
    const vistos: string[] = []
    onB1(sig, 'peer-role', (m) => vistos.push(`${m.peer_id}:${m.role}:${m.can_admit}`))
    onB1(sig, 'chat-reactions', (m) => vistos.push(`${m.id}:${m.counts['👍']}`))
    FakeWs.last!.onmessage!({ data: JSON.stringify({ type: 'peer-role', peer_id: 'b', role: 'cohost', can_admit: true }) })
    FakeWs.last!.onmessage!({ data: JSON.stringify({ type: 'chat-reactions', id: 'm', counts: { '👍': 2 } }) })
    expect(vistos).toEqual(['b:cohost:true', 'm:2'])
  })
})
