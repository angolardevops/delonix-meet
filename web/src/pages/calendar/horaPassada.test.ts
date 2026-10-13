/**
 * Marcar uma sessão cuja hora já passou.
 *
 * O caso que deu origem a isto (2026-10-13): às 10:24 marcou-se uma reunião
 * para as 08:00 do mesmo dia. Foi aceite em silêncio e nunca apareceu — a
 * Início só mostra o que ainda não acabou —, e de fora isso é indistinguível
 * de a reunião se ter perdido.
 */
import { describe, expect, it } from 'vitest'
import { jaAcabou } from './dates'

const AS_10_24 = new Date(2026, 9, 13, 10, 24).getTime()
const as = (h: number, m = 0) => new Date(2026, 9, 13, h, m)

describe('jaAcabou', () => {
  it('o caso real: às 10:24 marcar uma de 60 min para as 08:00 do mesmo dia', () => {
    expect(jaAcabou(as(8), 60, AS_10_24)).toBe(true)
  })

  it('uma que ainda está a decorrer NÃO avisa — entra-se nela', () => {
    expect(jaAcabou(as(10), 60, AS_10_24)).toBe(false)
  })

  it('o limite é o FIM, não o início: acabada agora mesmo ainda não avisa', () => {
    expect(jaAcabou(as(9, 24), 60, AS_10_24)).toBe(false)
    expect(jaAcabou(as(9, 23), 60, AS_10_24)).toBe(true)
  })

  it('no futuro, nunca', () => {
    expect(jaAcabou(as(15), 30, AS_10_24)).toBe(false)
  })

  it('sem data escolhida não há nada que avisar', () => {
    expect(jaAcabou(null, 30, AS_10_24)).toBe(false)
  })
})
