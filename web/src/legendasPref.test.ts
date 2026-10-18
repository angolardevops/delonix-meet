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
  guardaWebSpeech,
  legendasLigadasPorOmissao,
  motorDaTranscricao,
  webSpeechAceita,
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

  it('quem o desliga dispensa o servidor — e não a soberania', () => {
    // O nome deste teste dizia «fica com o motor do browser» até 2026-10-18,
    // e era verdade sobre o código de então. Desligar o ASR do servidor
    // passa a deixar o pedido no outro motor soberano (ver
    // `motorDaTranscricao` abaixo); a Google é uma escolha à parte.
    guardaAsrDoServidor(false)
    expect(asrDoServidorPreferido()).toBe(false)
  })
})

describe('aceitação explícita da Web Speech', () => {
  it('um browser que nunca viu o ecrã NÃO aceitou o motor da Google', () => {
    // `=== '1'` e não `!== '0'`: ao contrário das outras duas preferências
    // deste módulo, aqui o que se guarda é o CONSENTIMENTO — e um
    // consentimento que vale por omissão não é consentimento.
    expect(webSpeechAceita()).toBe(false)
  })

  it('com o armazenamento bloqueado também não aceitou nada', () => {
    atira = true
    expect(webSpeechAceita()).toBe(false)
  })

  it('aceita-se e retira-se', () => {
    guardaWebSpeech(true)
    expect(webSpeechAceita()).toBe(true)
    guardaWebSpeech(false)
    expect(webSpeechAceita()).toBe(false)
  })
})

describe('motorDaTranscricao', () => {
  it('o servidor ganha a tudo — é soberano e o mais preciso', () => {
    expect(motorDaTranscricao({ asrDoServidor: true, temWebSpeech: true })).toBe('servidor')
  })

  // ─────────────────────────────────────────────────────────────────────────
  // INVERTIDO A 2026-10-18, de propósito. Este teste trancava `'webspeech'`
  // e era a razão por que a divergência durava: o módulo documentava a ordem
  // «1. servidor · 2. wasm · 3. webspeech» e o código, no recuo que mais
  // acontece, saltava o wasm. O teste não estava errado sobre o CÓDIGO —
  // descrevia-o fielmente. Estava errado sobre o PRODUTO, e por isso ficou.
  //
  // O que uma falha do `/asr` a meio de uma reunião fazia em Chrome: começava
  // a enviar o áudio aos servidores da Google, no meio de uma sessão que
  // começara soberana, enquanto a interface dizia «a usar o motor local».
  // Agora o recuo de uma falha é SEMPRE outro motor soberano.
  // ─────────────────────────────────────────────────────────────────────────
  it('ASR do servidor em baixo: o recuo é o WASM local, NUNCA a Google', () => {
    // A marca «em baixo» continua a impedir o ciclo infinito que o `start()`
    // causava (voltava a ler a preferência, ainda «servidor», e abria outro
    // WebSocket). O que mudou é para onde ela dá lugar.
    expect(
      motorDaTranscricao({ asrDoServidor: true, asrDoServidorEmBaixo: true, temWebSpeech: true }),
    ).toBe('wasm')
  })

  it('ASR do servidor em baixo e sem Web Speech (Firefox): Whisper local', () => {
    expect(
      motorDaTranscricao({ asrDoServidor: true, asrDoServidorEmBaixo: true, temWebSpeech: false }),
    ).toBe('wasm')
  })

  // A segunda afirmação do antigo «a Web Speech é a ÚLTIMA escolha», também
  // invertida: dispensar o servidor não é pedir a Google. Quem desliga o
  // `dx_asr_server` fica com o outro motor soberano.
  it('dispensar o ASR do servidor NÃO é consentir a Google: fica o WASM', () => {
    expect(motorDaTranscricao({ asrDoServidor: false, temWebSpeech: true })).toBe('wasm')
    expect(motorDaTranscricao({ asrDoServidor: false, temWebSpeech: false })).toBe('wasm')
  })

  it('a Web Speech exige TRÊS sins — e com os três é alcançada', () => {
    // Sem este caso a «escolha explícita» não escolheria nada, e a opção
    // seria uma mentira de interface.
    const comTudo = {
      asrDoServidor: false,
      temWebSpeech: true,
      aceitaWebSpeech: true,
      webSpeechPermitida: true,
    }
    expect(motorDaTranscricao(comTudo)).toBe('webspeech')
    // Falta o sim do BROWSER.
    expect(motorDaTranscricao({ ...comTudo, temWebSpeech: false })).toBe('wasm')
    // Falta o sim da PESSOA.
    expect(motorDaTranscricao({ ...comTudo, aceitaWebSpeech: false })).toBe('wasm')
    // Falta o sim da ORGANIZAÇÃO — e este vem do servidor, não do browser.
    expect(motorDaTranscricao({ ...comTudo, webSpeechPermitida: false })).toBe('wasm')
  })

  it('a política da organização manda sobre a aceitação do browser', () => {
    // É o ponto da política do lado do servidor: a preferência do cliente
    // deixa de ser a única guarda. Mesmo com o `localStorage` alterado à mão
    // (ou por uma extensão), uma organização que proíbe não é servida.
    expect(
      motorDaTranscricao({
        asrDoServidor: true,
        asrDoServidorEmBaixo: true,
        temWebSpeech: true,
        aceitaWebSpeech: true,
        webSpeechPermitida: false,
      }),
    ).toBe('wasm')
  })

  it('a política ainda por chegar do servidor não abre nada por si', () => {
    // `webSpeechPermitida` omisso = «o servidor ainda não falou» (a política
    // chega por WebSocket depois de a página abrir). Pode valer o sim porque
    // sem a aceitação da pessoa o motor não é alcançável de qualquer modo.
    expect(
      motorDaTranscricao({ asrDoServidor: true, asrDoServidorEmBaixo: true, temWebSpeech: true }),
    ).toBe('wasm')
  })

  it('quem pede o modo 100% local não cai na Web Speech, nem tendo-a aceitado', () => {
    expect(
      motorDaTranscricao({ asrDoServidor: false, temWebSpeech: true, preferirLocal: true }),
    ).toBe('wasm')
    expect(
      motorDaTranscricao({
        asrDoServidor: false,
        temWebSpeech: true,
        preferirLocal: true,
        aceitaWebSpeech: true,
        webSpeechPermitida: true,
      }),
    ).toBe('wasm')
  })

  it('o modo local NÃO passa à frente do servidor: os dois são soberanos, e o servidor é melhor', () => {
    expect(
      motorDaTranscricao({ asrDoServidor: true, temWebSpeech: true, preferirLocal: true }),
    ).toBe('servidor')
  })

  it('sem WASM servido e sem os três sins: NENHUM motor, e diz-se', () => {
    // O caminho que esta sessão existe para não deixar em silêncio: uma
    // instalação que não correu o `deploy/fetch-whisper.sh`, o `/asr` em
    // baixo, e um Chrome à mão. Antes disto, isto era «webspeech».
    expect(
      motorDaTranscricao({
        asrDoServidor: true,
        asrDoServidorEmBaixo: true,
        temWebSpeech: true,
        temWasm: false,
      }),
    ).toBe('nenhum')
    // Com a aceitação explícita, o mesmo caso tem motor.
    expect(
      motorDaTranscricao({
        asrDoServidor: true,
        asrDoServidorEmBaixo: true,
        temWebSpeech: true,
        temWasm: false,
        aceitaWebSpeech: true,
      }),
    ).toBe('webspeech')
    // E o modo 100% local sem modelo não inventa um motor.
    expect(
      motorDaTranscricao({ asrDoServidor: false, temWebSpeech: true, temWasm: false, preferirLocal: true }),
    ).toBe('nenhum')
  })
})
