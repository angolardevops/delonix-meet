// CÓPIA CONGELADA — não é código do produto, e não se «melhora».
//
// É o `web/src/noiseGateWorklet.js` tal como estava na `develop` em
// e55a5f87 (introduzido em 94f74208; sha256 do original 57a06c10…2175cb1),
// byte a byte, com DUAS diferenças: este cabeçalho e o nome com que se regista
// (`noise-gate-processor-antigo`, para caber no mesmo contexto que o novo).
//
// Existe para duas provas, e só para elas:
//   1. `e2e/voz-natural.mjs` monta com ele a cadeia de voz ANTIGA e mede-a
//      contra a nova sobre o mesmo sinal;
//   2. `src/noiseGateWorklet.test.ts` prova que o gate novo, SEM parâmetros,
//      dá a mesma saída que este — é o que garante que a porta da mesa de som
//      do Estúdio (`studio/tv/mesaDeSom.ts`), que o usa sem parâmetros, não
//      mudou de comportamento.
/**
 * Noise gate por AudioWorklet — silêncio a sério entre frases.
 *
 * O RNNoise (rnnoiseWorklet.js, da @sapphi-red/web-noise-suppressor) reduz o
 * ruído de fundo, mas não o zera: entre frases fica sempre um chão de ruído
 * residual, e é esse chão que o compressor a seguir amplificaria se ficasse
 * sem controlo. Este módulo corta esse chão a zero quando ninguém fala.
 *
 * Envelope por amostra (não por bloco de 128): um "seguidor de envolvente" de
 * um pólo, com `attack` rápido a abrir (não perder a primeira sílaba) e
 * `release` lento a fechar (não cortar a cauda de uma consoante). `hold`
 * evita "engasgar" o gate quando a voz cai por instantes abaixo do limiar a
 * meio de uma frase (respiração, pausa curta).
 *
 * Plain JS, não TS: corre no scope global do AudioWorklet, que não passa
 * pelo mesmo pipeline de build do resto da app — é o mesmo motivo por que o
 * RNNoise vem como `.js` do pacote e não como `.ts` daqui.
 */
class NoiseGateProcessor extends AudioWorkletProcessor {
  static get parameterDescriptors() {
    return [
      { name: 'thresholdDb', defaultValue: -50, minValue: -80, maxValue: 0 },
      { name: 'attackMs', defaultValue: 3, minValue: 0.1, maxValue: 100 },
      { name: 'releaseMs', defaultValue: 150, minValue: 1, maxValue: 1000 },
      { name: 'holdMs', defaultValue: 80, minValue: 0, maxValue: 1000 },
    ]
  }

  constructor() {
    super()
    this.envelope = 0
    this.gain = 0
    this.holdRemaining = 0
  }

  process(inputs, outputs, parameters) {
    const input = inputs[0]
    const output = outputs[0]
    if (!input || !input[0]) return true

    const thresholdLinear = Math.pow(10, parameters.thresholdDb[0] / 20)
    const attackCoeff = Math.exp(-1 / (sampleRate * (parameters.attackMs[0] / 1000)))
    const releaseCoeff = Math.exp(-1 / (sampleRate * (parameters.releaseMs[0] / 1000)))
    // 5 ms de alisamento na deteção — sem isto, uma única amostra alta (clique,
    // plosiva) abre e fecha o gate no mesmo bloco em vez de manter aberto.
    const envelopeCoeff = Math.exp(-1 / (sampleRate * 0.005))
    const holdSamples = (parameters.holdMs[0] / 1000) * sampleRate

    const numChannels = input.length
    const frames = input[0].length

    for (let i = 0; i < frames; i++) {
      let rectified = 0
      for (let ch = 0; ch < numChannels; ch++) {
        const v = Math.abs(input[ch][i])
        if (v > rectified) rectified = v
      }
      this.envelope = envelopeCoeff * this.envelope + (1 - envelopeCoeff) * rectified

      if (this.envelope > thresholdLinear) {
        this.holdRemaining = holdSamples
      } else if (this.holdRemaining > 0) {
        this.holdRemaining--
      }
      const targetGain = this.holdRemaining > 0 || this.envelope > thresholdLinear ? 1 : 0
      const coeff = targetGain > this.gain ? attackCoeff : releaseCoeff
      this.gain = coeff * this.gain + (1 - coeff) * targetGain

      for (let ch = 0; ch < numChannels; ch++) {
        output[ch][i] = input[ch][i] * this.gain
      }
    }
    return true
  }
}

registerProcessor('noise-gate-processor-antigo', NoiseGateProcessor)
