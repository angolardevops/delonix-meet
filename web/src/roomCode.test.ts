import { describe, expect, it } from 'vitest'
import { parseRoomCode } from './roomCode'

describe('parseRoomCode', () => {
  it('aceita o código solto', () => expect(parseRoomCode('abc-defg-hij')).toBe('abc-defg-hij'))
  it('tira o código de um link completo', () =>
    expect(parseRoomCode('https://meet.exemplo.ao/#/r/abc-defg-hij')).toBe('abc-defg-hij'))
  it('tira o código de um link de lobby', () => expect(parseRoomCode('https://x/#/lobby/abc-defg-hij')).toBe('abc-defg-hij'))
  it('ignora espaços e maiúsculas', () => expect(parseRoomCode('  ABC-DEFG-HIJ ')).toBe('abc-defg-hij'))
  it('recusa o que não é código', () => {
    expect(parseRoomCode('')).toBeNull()
    expect(parseRoomCode('reunião')).toBeNull()
    expect(parseRoomCode('abc-def')).toBeNull()
  })
})
