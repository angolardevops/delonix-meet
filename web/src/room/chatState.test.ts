import { describe, expect, it } from 'vitest'
import { comConfirmada, comEnviada, comHistorico, comReaccoes, comRecebida, respostasPorMae, type ChatMsg } from './chatState'

describe('chat da sala como dados', () => {
  it('a minha mensagem fica pendente e ganha id e hora do SERVIDOR no chat-sent', () => {
    let s: ChatMsg[] = []
    s = comEnviada(s, { clientId: 'c1', username: 'Ana', text: 'olá', replyTo: null, at: 1000 })
    expect(s[0]).toMatchObject({ id: null, pending: true, at: 1000, own: true })
    const key = s[0].key
    s = comConfirmada(s, { client_id: 'c1', id: 'srv-1', at: 5000 })
    expect(s[0]).toMatchObject({ id: 'srv-1', pending: false, at: 5000, key })
  })

  it('o histórico entra antes do que já chegou ao vivo e não duplica ids', () => {
    let s: ChatMsg[] = comRecebida([], { from: 'p2', username: 'Rui', text: 'ao vivo', id: 'b', at: 3000 })
    s = comHistorico(
      s,
      [
        { id: 'b', user_id: 'u2', username: 'Rui', message: 'ao vivo', created_at: new Date(3000).toISOString() },
        { id: 'a', user_id: 'u1', username: 'Ana', message: 'antiga', created_at: new Date(1000).toISOString(), parent_id: null, reactions: { '👍': 2 } },
      ],
      'u1',
    )
    expect(s.map((m) => m.id)).toEqual(['a', 'b'])
    expect(s[0]).toMatchObject({ own: true, historical: true, reactions: { '👍': 2 } })
  })

  it('uma mensagem ao vivo repetida (mesmo id) não aparece duas vezes', () => {
    let s = comRecebida([], { from: 'p', username: 'X', text: 't', id: 'z' })
    s = comRecebida(s, { from: 'p', username: 'X', text: 't', id: 'z' })
    expect(s).toHaveLength(1)
  })

  it('chat-reactions substitui as contagens e larga os zeros', () => {
    let s = comRecebida([], { from: 'p', username: 'X', text: 't', id: 'z' })
    s = comReaccoes(s, { id: 'z', counts: { '👍': 3, '🎯': 0 } })
    expect(s[0].reactions).toEqual({ '👍': 3 })
  })

  it('respostas agrupam-se pela mãe', () => {
    let s = comRecebida([], { from: 'p', username: 'X', text: 'mãe', id: 'm' })
    s = comRecebida(s, { from: 'q', username: 'Y', text: 'r1', id: 'r1', reply_to: 'm' })
    s = comRecebida(s, { from: 'q', username: 'Y', text: 'r2', id: 'r2', reply_to: 'm' })
    expect(respostasPorMae(s).get('m')?.map((m) => m.id)).toEqual(['r1', 'r2'])
  })

  it('uma privada fica marcada ao vivo, na minha e no histórico', () => {
    let s = comRecebida([], { from: 'p', username: 'Rui', text: 'só tu', id: 'x', to: 'eu', to_username: 'Ana' })
    expect(s[0]).toMatchObject({ private: true, to: 'eu', toUsername: 'Ana' })
    s = comEnviada(s, { clientId: 'c', username: 'Ana', text: 'ok', replyTo: 'x', at: 1, to: 'p', toUsername: 'Rui' })
    expect(s[1]).toMatchObject({ private: true, toUsername: 'Rui', replyTo: 'x' })
    const h = comHistorico([], [{ id: 'h', user_id: 'u2', username: 'Rui', message: 'antiga', created_at: new Date(0).toISOString(), to_user_id: 'u1', to_username: 'Ana' }], 'u1')
    expect(h[0]).toMatchObject({ private: true, toUsername: 'Ana', own: false })
  })
})
