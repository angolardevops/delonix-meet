import { beforeEach, describe, expect, it, vi } from 'vitest'

// A bateria corre em `node` (sem jsdom): o `localStorage` é um esboço nosso,
// e tem de existir ANTES do import do módulo — daí o `await import()`.
const guardado = new Map<string, string>()
let atira = false
vi.stubGlobal('localStorage', {
  getItem: (k: string) => {
    if (atira) throw new Error('SecurityError: acesso ao armazenamento bloqueado')
    return guardado.get(k) ?? null
  },
  setItem: (k: string, v: string) => {
    if (atira) throw new Error('SecurityError: acesso ao armazenamento bloqueado')
    guardado.set(k, v)
  },
  removeItem: (k: string) => void guardado.delete(k),
  clear: () => guardado.clear(),
})

const {
  asrDoServidorPreferido,
  guardaAsrDoServidor,
  guardaLegendas,
  legendasLigadasPorOmissao,
  motorDaTranscricao,
} = await import('./legendasPref')

beforeEach(() => {
  guardado.clear()
  atira = false
})

describe('legendas ligadas por omissão', () => {
  it('um browser que nunca viu a sala entra COM legendas', () => {
    // É a decisão de 2026-10-13: «activar a legenda automática». Antes disto a
    // omissão era desligada e cada participante tinha de carregar no botão —
    // e, sem ninguém a carregar, o servidor não recebia transcrição nenhuma e
    // a ata nunca tinha o que resumir.
    expect(legendasLigadasPorOmissao()).toBe(true)
  })

  it('só a RECUSA explícita de quem desligou o botão as mantém desligadas', () => {
    guardaLegendas(false)
    expect(legendasLigadasPorOmissao()).toBe(false)
    guardaLegendas(true)
    expect(legendasLigadasPorOmissao()).toBe(true)
  })

  it('um valor estranho na chave não desliga nada (só o «0» desliga)', () => {
    guardado.set('dx_cc', 'talvez')
    expect(legendasLigadasPorOmissao()).toBe(true)
  })

  it('com o armazenamento bloqueado (navegação privada) valem as omissões, sem atirar', () => {
    atira = true
    expect(legendasLigadasPorOmissao()).toBe(true)
    expect(asrDoServidorPreferido()).toBe(true)
    expect(() => guardaLegendas(false)).not.toThrow()
  })
})

describe('ASR do servidor preferido por omissão', () => {
  it('por omissão o áudio vai ao NOSSO servidor, não à Google', () => {
    expect(asrDoServidorPreferido()).toBe(true)
  })

  it('quem o desliga fica com o motor do browser', () => {
    guardaAsrDoServidor(false)
    expect(asrDoServidorPreferido()).toBe(false)
  })
})

describe('motorDaTranscricao', () => {
  it('o servidor ganha a tudo — é soberano e o mais preciso', () => {
    expect(motorDaTranscricao({ asrDoServidor: true, temWebSpeech: true })).toBe('servidor')
  })

  it('ASR do servidor em baixo: cai para o motor do browser e NÃO volta a tentar', () => {
    // Esta é a regressão que o ciclo infinito causava: o `start()` voltava a
    // ler a preferência (ainda «servidor»), abria outro WebSocket e falhava,
    // sem nunca chegar ao motor seguinte. A marca «em baixo» é o que o impede.
    expect(
      motorDaTranscricao({ asrDoServidor: true, asrDoServidorEmBaixo: true, temWebSpeech: true }),
    ).toBe('webspeech')
  })

  it('ASR do servidor em baixo e sem Web Speech (Firefox): Whisper local', () => {
    expect(
      motorDaTranscricao({ asrDoServidor: true, asrDoServidorEmBaixo: true, temWebSpeech: false }),
    ).toBe('wasm')
  })

  it('a Web Speech é a ÚLTIMA escolha: é a única que envia o áudio para fora', () => {
    expect(motorDaTranscricao({ asrDoServidor: false, temWebSpeech: true })).toBe('webspeech')
    expect(motorDaTranscricao({ asrDoServidor: false, temWebSpeech: false })).toBe('wasm')
  })

  it('quem pede o modo 100% local não cai na Web Speech, mesmo tendo-a', () => {
    expect(
      motorDaTranscricao({ asrDoServidor: false, temWebSpeech: true, preferirLocal: true }),
    ).toBe('wasm')
  })

  it('o modo local NÃO passa à frente do servidor: os dois são soberanos, e o servidor é melhor', () => {
    expect(
      motorDaTranscricao({ asrDoServidor: true, temWebSpeech: true, preferirLocal: true }),
    ).toBe('servidor')
  })
})
