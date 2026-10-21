/**
 * Testa o FICHEIRO REAL do worklet, não uma reimplementação. O ficheiro
 * `.js` (plain JS, sem imports — é carregado cru pelo AudioWorklet via
 * `?url`, ver voiceChain.ts) só usa três coisas do scope global do worklet:
 * `AudioWorkletProcessor`, `registerProcessor` e `sampleRate`. Esboçá-las
 * aqui e importar o módulo real chama exactamente o código que vai correr
 * no browser — sem risco de a versão testada divergir da versão enviada.
 *
 * Não há jsdom neste repo (ver studio/analise.test.ts) — não é preciso: isto
 * é matemática pura sobre `Float32Array`, sem DOM nenhum envolvido.
 *
 * O que estes testes NÃO medem: a cadeia inteira com o RNNoise. Essa mede-se
 * num browser, com o WASM a sério — `e2e/voz-natural.mjs`.
 */
import { beforeAll, describe, expect, it } from 'vitest'
import { VOICE_GATE, type VoiceGateParams } from './voiceChain'

const SR = 48_000
const BLOCK = 128

/**
 * O processador que o `registerProcessor` do worklet nos entrega. É um
 * construtor vindo de JS sem tipos: tipá-lo como construtor (e não `any`) dá
 * o que o teste precisa e tira o `eslint-disable` de `no-explicit-any`, que a
 * catraca do lint contava DUAS vezes — como problema e como disable.
 */
interface GateProcessor {
  process(entradas: Float32Array[][], saidas: Float32Array[][], params: Record<string, number[]>): boolean
}
interface GateCtor {
  new (options?: { processorOptions?: { lookaheadMs?: number } }): GateProcessor
  parameterDescriptors: { name: string; defaultValue: number }[]
}
const registados = new Map<string, GateCtor>()
/** O gate do produto e a cópia congelada do que ele era antes (ver o cabeçalho dela). */
let NoiseGateProcessor: GateCtor
let GateAntigo: GateCtor

beforeAll(async () => {
  ;(globalThis as unknown as { sampleRate: number }).sampleRate = SR
  ;(globalThis as unknown as { AudioWorkletProcessor: new () => object }).AudioWorkletProcessor = class {}
  ;(
    globalThis as unknown as {
      registerProcessor: (name: string, cls: GateCtor) => void
    }
  ).registerProcessor = (name, cls) => {
    registados.set(name, cls)
  }
  // Plain JS sem tipos — carrega-se pelo efeito lateral do registerProcessor
  // esboçado acima, não pelo que o TS conseguiria tipar.
  // @ts-expect-error TS7016: ./noiseGateWorklet.js não tem declaração de tipos
  await import('./noiseGateWorklet.js')
  // @ts-expect-error TS7016: a cópia congelada também não
  await import('../e2e/voz-natural-gate-antigo.js')
  NoiseGateProcessor = registados.get('noise-gate-processor')!
  GateAntigo = registados.get('noise-gate-processor-antigo')!
})

/** Gerador determinístico: o mesmo sinal em todas as corridas. */
function semente(a: number) {
  return () => {
    a |= 0
    a = (a + 0x6d2b79f5) | 0
    let t = Math.imul(a ^ (a >>> 15), 1 | a)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

/** Chão de ruído realista (não um zero perfeito) + uma "fala" a meio. */
function fabricar(segundos: number, falaEm: [number, number]): Float32Array {
  const d = new Float32Array(Math.round(segundos * SR))
  for (let i = 0; i < d.length; i++) {
    const t = i / SR
    const aFalar = t >= falaEm[0] && t < falaEm[1]
    d[i] = aFalar ? (Math.random() - 0.5) * 0.6 : (Math.random() - 0.5) * 0.004
  }
  return d
}

/**
 * Corre um gate sobre o sinal inteiro, em blocos de 128 (tamanho real do
 * AudioWorklet). Os parâmetros que não se dão ficam com o valor por omissão
 * do próprio processador — como faz o browser.
 */
function correrCom(Ctor: GateCtor, sinal: Float32Array, params: Record<string, number> = {}, lookaheadMs = 0) {
  const gate = new Ctor({ processorOptions: { lookaheadMs } })
  const saida = new Float32Array(sinal.length)
  const parametros: Record<string, number[]> = {}
  for (const d of Ctor.parameterDescriptors) parametros[d.name] = [params[d.name] ?? d.defaultValue]
  for (let i = 0; i < sinal.length; i += BLOCK) {
    const fim = Math.min(i + BLOCK, sinal.length)
    const entrada = sinal.subarray(i, fim)
    const bloco = new Float32Array(BLOCK)
    bloco.set(entrada)
    const blocoSaida = new Float32Array(BLOCK)
    gate.process([[bloco]], [[blocoSaida]], parametros)
    saida.set(blocoSaida.subarray(0, fim - i), i)
  }
  return saida
}

/** A porta clássica: o gate SEM parâmetros (é como a mesa de som do Estúdio o usa). */
const correr = (sinal: Float32Array, params: Record<string, number> = {}) => correrCom(NoiseGateProcessor, sinal, params)

/** O gate da chamada: os parâmetros do produto, com o que o teste quiser mudar. */
function correrVoz(sinal: Float32Array, mudar: Partial<VoiceGateParams> = {}) {
  const { lookaheadMs, ...params } = { ...VOICE_GATE, ...mudar }
  return correrCom(NoiseGateProcessor, sinal, params, lookaheadMs)
}

function rms(sinal: Float32Array, inicio: number, fim: number): number {
  let soma = 0
  const a = Math.round(inicio * SR)
  const b = Math.round(fim * SR)
  for (let i = a; i < b; i++) soma += sinal[i] * sinal[i]
  return Math.sqrt(soma / (b - a))
}

function pico(sinal: Float32Array, inicio: number, fim: number): number {
  let m = 0
  for (let i = Math.round(inicio * SR); i < Math.round(fim * SR); i++) m = Math.max(m, Math.abs(sinal[i]))
  return m
}

const lin = (dB: number) => Math.pow(10, dB / 20)
const ATRASO = VOICE_GATE.lookaheadMs / 1000
const CHAO = lin(VOICE_GATE.floorDb)

/** Ruído de fundo baixo (≈ −71 dBFS), bem abaixo do limiar do gate de voz (−58). */
function chao(segundos: number, rand = semente(7)): Float32Array {
  const d = new Float32Array(Math.round(segundos * SR))
  for (let i = 0; i < d.length; i++) d[i] = (rand() - 0.5) * 0.001
  return d
}

/**
 * Uma tecla: sinusóides amortecidas, a cair cerca de 1 dB por milissegundo —
 * a forma que as teclas com corpo têm DEPOIS do RNNoise (medido no arnês
 * `e2e/voz-natural.mjs`; é a mesma receita de lá).
 */
function tecla(picoDb: number): Float32Array {
  const N = Math.round(0.045 * SR)
  const x = new Float32Array(N)
  const modos: [number, number, number][] = [
    [2300, 0.004, 1],
    [3900, 0.003, 0.7],
    [6100, 0.002, 0.5],
    [520, 0.009, 0.8],
  ]
  let m = 0
  for (let i = 0; i < N; i++) {
    const t = i / SR
    let v = 0
    for (const [f, tau, a] of modos) v += a * Math.sin(2 * Math.PI * f * t) * Math.exp(-t / tau)
    x[i] = v
    m = Math.max(m, Math.abs(v))
  }
  const g = lin(picoDb) / m
  for (let i = 0; i < N; i++) x[i] *= g
  return x
}

/** Um clique aos 0,3 s de 0,8 s de chão. */
function chaoComTecla(picoDb: number): Float32Array {
  const d = chao(0.8)
  const c = tecla(picoDb)
  const em = Math.round(0.3 * SR)
  for (let i = 0; i < c.length; i++) d[em + i] += c[i]
  return d
}

describe('noiseGateWorklet — o gate corta o chão e deixa passar a fala', () => {
  it('fecha durante o silêncio, bem longe de qualquer fala', () => {
    // Fala só nos primeiros 0,5s; ao fim de 1s (bem passado o release+hold de
    // 230ms por omissão) o gate tem de estar fechado.
    const sinal = fabricar(1.5, [0, 0.5])
    const saida = correr(sinal)
    const entradaChao = rms(sinal, 1.0, 1.5)
    const saidaChao = rms(saida, 1.0, 1.5)
    // Medido nos dois estados antes de escolher o limiar (R69): o chão de
    // entrada ronda 0,0012 RMS; fechado, a saída tem de ficar ORDENS de
    // grandeza abaixo disso, não só "um pouco menor".
    expect(saidaChao).toBeLessThan(entradaChao / 20)
  })

  it('deixa passar a fala quase sem perdas', () => {
    const sinal = fabricar(1.0, [0.2, 0.8])
    const saida = correr(sinal)
    // A meio da fala (bem depois do attack de 3ms) o gate já está aberto —
    // a energia à saída tem de estar próxima da de entrada.
    const entradaFala = rms(sinal, 0.4, 0.6)
    const saidaFala = rms(saida, 0.4, 0.6)
    expect(saidaFala).toBeGreaterThan(entradaFala * 0.9)
  })

  it('abre depressa (attack curto) — não perde a primeira sílaba', () => {
    const sinal = fabricar(0.3, [0.1, 0.3])
    const saida = correr(sinal)
    // 15ms depois do início da fala (5× o attack de 3ms) já deve estar
    // praticamente aberto.
    const amostraAberto = Math.round(0.115 * SR)
    expect(Math.abs(saida[amostraAberto])).toBeGreaterThan(Math.abs(sinal[amostraAberto]) * 0.5)
  })

  it('o hold evita engasgar numa pausa curta a meio da frase', () => {
    // Fala, uma pausa de 40ms (menor que os 80ms de hold), depois fala outra vez.
    const d = new Float32Array(Math.round(0.6 * SR))
    for (let i = 0; i < d.length; i++) {
      const t = i / SR
      const emPausaCurta = t >= 0.2 && t < 0.24
      d[i] = emPausaCurta ? (Math.random() - 0.5) * 0.004 : (Math.random() - 0.5) * 0.6
    }
    const saida = correr(d)
    // A pausa em si é baixa (input ~0,002 RMS) — não se compara a um limiar
    // absoluto, compara-se saída/entrada NESSA janela: se o gate segurou
    // aberto (ganho ~1), o rácio fica perto de 1; se fechou, perto de 0.
    const entradaPausa = rms(d, 0.205, 0.235)
    const saidaPausa = rms(saida, 0.205, 0.235)
    expect(saidaPausa / entradaPausa).toBeGreaterThan(0.5)
  })

  it('SEM PARÂMETROS dá, amostra a amostra, a saída da versão anterior — a porta do Estúdio não mudou', () => {
    // O gate ganhou chão, histerese, antecipação e rejeição de transitórios
    // para a cadeia de voz. A mesa de som do Estúdio (`studio/tv/mesaDeSom.ts`)
    // usa-o sem nenhum desses parâmetros e só mexe no limiar: tem de continuar
    // a ser, exactamente, a porta que era. Compara-se com a cópia congelada.
    const rand = semente(42)
    const d = new Float32Array(3 * SR)
    for (let i = 0; i < d.length; i++) {
      const t = i / SR
      // Fala, pausas curtas e longas, um estalo, e troços à volta do limiar.
      const nivel =
        t < 0.4 ? 0.004 : t < 0.9 ? 0.6 : t < 0.93 ? 0.004 : t < 1.4 ? 0.5 : t < 2.0 ? 0.004 : t < 2.003 ? 0.8 : t < 2.4 ? 0.006 : t < 2.7 ? 0.012 : 0.004
      d[i] = (rand() - 0.5) * nivel
    }
    for (const thresholdDb of [-50, -40, -80]) {
      const nova = correrCom(NoiseGateProcessor, d, { thresholdDb })
      const antiga = correrCom(GateAntigo, d, { thresholdDb })
      let diferenca = 0
      for (let i = 0; i < d.length; i++) diferenca = Math.max(diferenca, Math.abs(nova[i] - antiga[i]))
      expect(diferenca).toBe(0)
    }
  })
})

describe('noiseGateWorklet — o gate de voz da chamada (parâmetros de voiceChain.ts)', () => {
  it('fechado, baixa para um CHÃO — não para silêncio digital', () => {
    const sinal = chao(1.0)
    const saida = correrVoz(sinal)
    // A saída vem atrasada; compara-se com a entrada alinhada.
    const razao = rms(saida, 0.5 + ATRASO, 0.9 + ATRASO) / rms(sinal, 0.5, 0.9)
    expect(razao).toBeGreaterThan(CHAO * 0.9)
    expect(razao).toBeLessThan(CHAO * 1.1)
    // E é o chão desde a primeira amostra: nada de fade-in a partir de zero.
    const arranque = rms(saida, ATRASO + 0.001, ATRASO + 0.02) / rms(sinal, 0.001, 0.02)
    expect(arranque).toBeGreaterThan(CHAO * 0.9)
  })

  it.each([-30, -20, -10])('um clique de teclado a %i dBFS NÃO abre o gate', (picoDb) => {
    const sinal = chaoComTecla(picoDb)
    const saida = correrVoz(sinal)
    const entrada = pico(sinal, 0.3, 0.36)
    const depois = pico(saida, 0.3 + ATRASO, 0.36 + ATRASO)
    // Fica no chão (−24 dB); a margem é para o ruído por baixo do clique.
    expect(depois).toBeLessThan(entrada * lin(-20))
  })

  it('CONTROLO: sem a rejeição de transitórios, o mesmo clique passa', () => {
    const sinal = chaoComTecla(-20)
    const saida = correrVoz(sinal, { sustainMs: 0, transientDropDb: 0 })
    expect(pico(saida, 0.3 + ATRASO, 0.36 + ATRASO)).toBeGreaterThan(pico(sinal, 0.3, 0.36) * 0.7)
  })

  it('CONTROLO: só com o tempo de sustentação, um clique FORTE passa — é para isso a regra da queda', () => {
    // Quanto mais forte o clique, mais tempo demora a cair abaixo de um limiar
    // fixo: passado o tempo de sustentação ainda lá está, e o gate abria. Foi o que o arnês mediu
    // antes de haver `transientDropDb` (tecla a −15 dBFS: passava inteira).
    const sinal = chaoComTecla(-10)
    const soTempo = correrVoz(sinal, { transientDropDb: 0 })
    const comQueda = correrVoz(sinal)
    const janela: [number, number] = [0.3 + ATRASO + VOICE_GATE.sustainMs / 1000, 0.36 + ATRASO]
    // Abre no fim da sustentação: a cauda do clique sai inteira com a regra do tempo só.
    expect(rms(soTempo, ...janela)).toBeGreaterThan(rms(comQueda, ...janela) * 4)
  })

  it('não come o ataque de uma sílaba: os primeiros 30 ms saem inteiros', () => {
    const rand = semente(3)
    const sinal = chao(1.0)
    // «Fala»: energia sustentada que começa de repente aos 0,4 s.
    for (let i = Math.round(0.4 * SR); i < Math.round(0.8 * SR); i++) sinal[i] = (rand() - 0.5) * 0.6
    const saida = correrVoz(sinal)
    const razao = rms(saida, 0.4 + ATRASO, 0.43 + ATRASO) / rms(sinal, 0.4, 0.43)
    // Menos de 1 dB perdido — o gate decide ao fim da sustentação e o áudio só chega 5 ms depois.
    expect(razao).toBeGreaterThan(lin(-1))
    expect(razao).toBeLessThan(1.001)
  })

  it('CONTROLO: sem a antecipação, o mesmo ataque É comido', () => {
    const rand = semente(3)
    const sinal = chao(1.0)
    for (let i = Math.round(0.4 * SR); i < Math.round(0.8 * SR); i++) sinal[i] = (rand() - 0.5) * 0.6
    const saida = correrVoz(sinal, { lookaheadMs: 0 })
    expect(rms(saida, 0.4, 0.43) / rms(sinal, 0.4, 0.43)).toBeLessThan(lin(-3))
  })

  it('aberto, a saída é a entrada atrasada exactamente `lookaheadMs`', () => {
    const rand = semente(5)
    const sinal = chao(1.0)
    for (let i = Math.round(0.2 * SR); i < Math.round(0.9 * SR); i++) sinal[i] = (rand() - 0.5) * 0.6
    const saida = correrVoz(sinal)
    const n = Math.round(ATRASO * SR)
    let diferenca = 0
    for (let i = Math.round(0.5 * SR); i < Math.round(0.8 * SR); i++) diferenca = Math.max(diferenca, Math.abs(saida[i + n] - sinal[i]))
    expect(diferenca).toBeLessThan(1e-4)
  })

  it('histerese: aberto aguenta um sinal entre os dois limiares; fechado, não abre com ele', () => {
    // Um nível constante cuja envolvente fica 3 dB ABAIXO do limiar de abrir
    // e 3 dB ACIMA do de fechar.
    const meio = lin(VOICE_GATE.thresholdDb - VOICE_GATE.hysteresisDb / 2)
    const aberto = new Float32Array(Math.round(1.2 * SR)).fill(meio)
    for (let i = 0; i < Math.round(0.2 * SR); i++) aberto[i] = 0.3
    const fechado = new Float32Array(Math.round(1.2 * SR)).fill(meio)
    for (let i = 0; i < Math.round(0.2 * SR); i++) fechado[i] = 0.0005
    // Muito depois do hold (80 ms) e do release (150 ms).
    expect(rms(correrVoz(aberto), 0.9, 1.1) / meio).toBeGreaterThan(0.95)
    expect(rms(correrVoz(fechado), 0.9, 1.1) / meio).toBeLessThan(CHAO * 1.1)
  })

  it('o ganho nunca salta: abre e fecha em rampa, entre o chão e 1', () => {
    // Entrada sempre positiva: o ganho lê-se dividindo a saída pela entrada atrasada.
    const d = new Float32Array(Math.round(1.6 * SR))
    for (let i = 0; i < d.length; i++) {
      const t = i / SR
      d[i] = (t >= 0.3 && t < 0.6) || (t >= 1.1 && t < 1.3) ? 0.3 : 0.0005
    }
    const saida = correrVoz(d)
    const n = Math.round(ATRASO * SR)
    let anterior = saida[n] / d[0]
    let maiorSalto = 0
    let minimo = Infinity
    let maximo = 0
    for (let i = 1; i + n < d.length; i++) {
      const ganho = saida[i + n] / d[i]
      maiorSalto = Math.max(maiorSalto, Math.abs(ganho - anterior))
      minimo = Math.min(minimo, ganho)
      maximo = Math.max(maximo, ganho)
      anterior = ganho
    }
    // Com 3 ms de ataque o maior passo por amostra é (1 − chão)/144 ≈ 0,0065.
    expect(maiorSalto).toBeLessThan(0.01)
    expect(minimo).toBeGreaterThan(CHAO * 0.99)
    expect(maximo).toBeLessThan(1.0001)
    // E chegou mesmo aos dois extremos — senão a rampa não foi exercitada.
    expect(maximo).toBeGreaterThan(0.99)
    expect(minimo).toBeLessThan(CHAO * 1.05)
  })
})
