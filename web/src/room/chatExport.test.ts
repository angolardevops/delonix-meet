import { describe, expect, it } from 'vitest'
import { chatEmTexto, nomeFicheiroChat } from './chatExport'

describe('«Guardar chat» exporta o que o dispositivo recebeu', () => {
  it('mensagens e sondagens pela ordem em que chegaram, com hora', () => {
    const t0 = Date.UTC(2026, 8, 16, 9, 14)
    const txt = chatEmTexto(
      'Chat · sala abc',
      [
        { at: t0, username: 'Joaquim', text: 'O failover está testado?' },
        { at: t0 + 5 * 60_000, username: 'Ana', text: 'Sim.' },
      ],
      [{ at: t0 + 60_000, question: 'Onde guardar?', options: ['MinIO', 'Nextcloud'], counts: [3, 1] }],
      'pt-PT',
      'Sondagem',
    )
    const linhas = txt.split('\n')
    expect(linhas[0]).toBe('Chat · sala abc')
    expect(linhas[2]).toMatch(/^\[\d\d:\d\d\] Joaquim: O failover está testado\?$/)
    expect(linhas[3]).toMatch(/Sondagem: Onde guardar\?$/)
    expect(linhas[4]).toBe('    - MinIO: 3 (75%)')
    expect(linhas[5]).toBe('    - Nextcloud: 1 (25%)')
    expect(linhas[6]).toMatch(/Ana: Sim\.$/)
  })

  it('sondagem sem votos não divide por zero', () => {
    const txt = chatEmTexto('x', [], [{ at: 0, question: 'q', options: ['a'], counts: [0] }], 'pt-PT', 'Sondagem')
    expect(txt).toContain('    - a: 0 (0%)')
  })

  it('o nome do ficheiro não leva caracteres do código que não devia', () => {
    expect(nomeFicheiroChat('abc-def/../x', new Date('2026-09-16T10:00:00Z'))).toBe('chat-abc-defx-2026-09-16.txt')
  })
})
