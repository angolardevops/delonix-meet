import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { chaveFracos, deveActualizarQos, intervaloQos, ligacaoFraca } from './qosAmostra'

describe('«▲ FRACA» não depende do painel de participantes', () => {
  it('a amostra corre com o painel FECHADO (5 s) e encurta com ele aberto (2 s)', () => {
    expect(intervaloQos(false)).toBe(5_000)
    expect(intervaloQos(true)).toBe(2_000)
  })

  it('com o painel fechado só se renderiza quando o conjunto de fracos muda', () => {
    const a = chaveFracos({ x: { lossPct: 9 }, y: { lossPct: 1 } })
    const b = chaveFracos({ y: { lossPct: 1 }, x: { lossPct: 12 } })
    expect(a).toBe('x')
    expect(a).toBe(b)
    expect(deveActualizarQos(false, a, b)).toBe(false)
    expect(deveActualizarQos(false, a, '')).toBe(true) // recuperou: o retrato limpa a marca
    expect(deveActualizarQos(false, null, '')).toBe(true) // primeira amostra
    expect(deveActualizarQos(true, a, b)).toBe(true)
    expect(ligacaoFraca(5)).toBe(false)
    expect(ligacaoFraca(5.1)).toBe(true)
    expect(ligacaoFraca(undefined)).toBe(false)
  })

  it('o hook não volta a condicionar a amostragem ao painel', () => {
    const src = readFileSync(join(__dirname, 'useParticipants.ts'), 'utf8')
    // A regressão era exactamente esta guarda à cabeça do efeito que escrevia `qos`.
    expect(src).not.toMatch(/if \(!peoplePanelOpen\) return/)
    const i = src.indexOf('setQos(r)')
    expect(i).toBeGreaterThan(0)
    // e a escrita vive no mesmo efeito que alimenta a política de camada
    const efeito = src.slice(src.lastIndexOf('useEffect(', i), src.indexOf('}, [peoplePanelOpen', i))
    expect(efeito).toContain("if (core.roomState !== 'in') return")
    expect(efeito).toContain('setConditions(')
  })
})
