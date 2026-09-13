/**
 * Testa o FICHEIRO REAL do worklet, não uma reimplementação. O ficheiro
 * `.js` (plain JS, sem imports — é carregado cru pelo AudioWorklet via
 * `?url`, ver media.ts) só usa três coisas do scope global do worklet:
 * `AudioWorkletProcessor`, `registerProcessor` e `sampleRate`. Esboçá-las
 * aqui e importar o módulo real chama exactamente o código que vai correr
 * no browser — sem risco de a versão testada divergir da versão enviada.
 *
 * Não há jsdom neste repo (ver studio/analise.test.ts) — não é preciso: isto
 * é matemática pura sobre `Float32Array`, sem DOM nenhum envolvido.
 */
import { beforeAll, describe, expect, it } from 'vitest'

const SR = 48_000
const BLOCK = 128

// eslint-disable-next-line @typescript-eslint/no-explicit-any
let NoiseGateProcessor: any

beforeAll(async () => {
  ;(globalThis as unknown as { sampleRate: number }).sampleRate = SR
  ;(globalThis as unknown as { AudioWorkletProcessor: new () => object }).AudioWorkletProcessor = class {}
  ;(globalThis as unknown as { registerProcessor: (name: string, cls: unknown) => void }).registerProcessor = (
    _name,
    cls,
  ) => {
    NoiseGateProcessor = cls
  }
  // Plain JS sem tipos — carrega-se pelo efeito lateral do registerProcessor
  // esboçado acima, não pelo que o TS conseguiria tipar.
  // @ts-expect-error TS7016: ./noiseGateWorklet.js não tem declaração de tipos
  await import('./noiseGateWorklet.js')
})

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

/** Corre o gate sobre o sinal inteiro, em blocos de 128 (tamanho real do AudioWorklet). */
function correr(sinal: Float32Array, params: Record<string, number> = {}) {
  const gate = new NoiseGateProcessor()
  const saida = new Float32Array(sinal.length)
  const parametros = {
    thresholdDb: [params.thresholdDb ?? -50],
    attackMs: [params.attackMs ?? 3],
    releaseMs: [params.releaseMs ?? 150],
    holdMs: [params.holdMs ?? 80],
  }
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

function rms(sinal: Float32Array, inicio: number, fim: number): number {
  let soma = 0
  const a = Math.round(inicio * SR)
  const b = Math.round(fim * SR)
  for (let i = a; i < b; i++) soma += sinal[i] * sinal[i]
  return Math.sqrt(soma / (b - a))
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
})
