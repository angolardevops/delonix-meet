/**
 * Gate por AudioWorklet — a mesma peça em dois ofícios.
 *
 * SEM PARÂMETROS é a porta clássica de sempre: fecha a silêncio digital
 * abaixo do limiar, abre no instante em que a envolvente o passa. É assim que
 * a mesa de som do Estúdio a usa (`studio/tv/mesaDeSom.ts`), e essa
 * equivalência está presa por um teste contra a cópia congelada da versão
 * anterior (`noiseGateWorklet.test.ts`).
 *
 * COM OS PARÂMETROS DA CADEIA DE VOZ (`voiceChain.ts`) é o gate da chamada, a
 * seguir ao RNNoise, e faz três coisas que a porta clássica não fazia:
 *
 *  - CHÃO em vez de silêncio (`floorDb`): fechado, atenua; não corta. Um corte
 *    a zero entre frases faz ouvir, por contraste, a cauda de cada palavra a
 *    ser decepada — e o silêncio digital soa a chamada caída;
 *  - REJEIÇÃO DE TRANSITÓRIOS (`sustainMs`, `transientDropDb`, `lookaheadMs`):
 *    só abre quando o sinal se mantém acima do limiar durante `sustainMs` E,
 *    nesse instante, não caiu mais de `transientDropDb` abaixo do seu próprio
 *    pico. Um clique de teclado é uma rajada que já vai a cair ao fim de
 *    poucos milissegundos; o ataque de uma sílaba é seguido de energia que se
 *    aguenta. Para o ataque não ser comido enquanto se espera, o ÁUDIO sai
 *    atrasado `lookaheadMs` em relação à DETECÇÃO: quando o gate decide abrir,
 *    o início da sílaba ainda não chegou à saída.
 *
 *    A SEGUNDA CONDIÇÃO NÃO É ENFEITE (medido, `e2e/voz-natural.mjs`): só com
 *    o tempo acima do limiar, a regra depende do VOLUME do clique — uma tecla
 *    a −25 dBFS ficava de fora e a mesma tecla a −15 dBFS entrava, porque
 *    quanto mais forte, mais tempo demora a cair abaixo de um limiar fixo.
 *    Comparar com o próprio pico torna a regra independente do volume;
 *  - HISTERESE (`hysteresisDb`): fecha abaixo de onde abre, para não bater à
 *    porta com um sinal que anda à volta do limiar.
 *
 * O QUE ESTE GATE NÃO FAZ: tirar um ruído que dura tanto como uma sílaba (uma
 * batida na mesa, um «clique» de 50 ms) nem um clique que cai DURANTE a fala —
 * aí o gate está aberto, e o que houver para tirar tira-o o RNNoise.
 *
 * Envelope por amostra (não por bloco de 128): um "seguidor de envolvente" de
 * um pólo, com `attack` rápido a abrir (não perder a primeira sílaba) e
 * `release` lento a fechar (não cortar a cauda de uma consoante). `hold`
 * evita "engasgar" o gate quando a voz cai por instantes abaixo do limiar a
 * meio de uma frase (respiração, pausa curta). O ganho é alisado por um pólo
 * nos dois sentidos — nunca salta, e por isso não estala.
 *
 * Plain JS, não TS: corre no scope global do AudioWorklet, que não passa
 * pelo mesmo pipeline de build do resto da app — é o mesmo motivo por que o
 * RNNoise vem como `.js` do pacote e não como `.ts` daqui.
 */

/** Abaixo disto o chão é silêncio digital (ganho 0), como na porta clássica. */
const FLOOR_SILENCE_DB = -100
/**
 * Quanto cai por segundo o pico de referência da regra dos transitórios.
 * Devagar face a um clique (que cai ~1 dB por milissegundo depois do RNNoise)
 * e depressa o bastante para um pico antigo não ficar a mandar: 6 dB em 50 ms.
 */
const PEAK_HOLD_DB_PER_S = 120
/** Tecto do atraso: mais do que isto já se nota numa conversa. */
const MAX_LOOKAHEAD_MS = 60

class NoiseGateProcessor extends AudioWorkletProcessor {
  static get parameterDescriptors() {
    return [
      { name: 'thresholdDb', defaultValue: -50, minValue: -80, maxValue: 0 },
      { name: 'attackMs', defaultValue: 3, minValue: 0.1, maxValue: 100 },
      { name: 'releaseMs', defaultValue: 150, minValue: 1, maxValue: 1000 },
      { name: 'holdMs', defaultValue: 80, minValue: 0, maxValue: 1000 },
      // Os que se seguem valem «desligado» por omissão — e desligados é a porta clássica.
      { name: 'floorDb', defaultValue: FLOOR_SILENCE_DB, minValue: FLOOR_SILENCE_DB, maxValue: 0 },
      { name: 'hysteresisDb', defaultValue: 0, minValue: 0, maxValue: 24 },
      { name: 'sustainMs', defaultValue: 0, minValue: 0, maxValue: 100 },
      { name: 'transientDropDb', defaultValue: 0, minValue: 0, maxValue: 40 },
    ]
  }

  /**
   * `processorOptions.lookaheadMs` — o atraso do áudio. É opção do nó e não
   * `AudioParam` de propósito: mudar um atraso a meio rasga o sinal, e por
   * isso fica fixo durante a vida do nó.
   */
  constructor(options) {
    super()
    this.envelope = 0
    this.gain = 0
    this.holdRemaining = 0
    this.open = false
    /** Há quantas amostras seguidas a envolvente está acima do limiar (gate fechado). */
    this.aboveRun = 0
    /** O pico da envolvente desde que passou o limiar, a cair devagar (gate fechado). */
    this.peak = 0
    /** A primeira amostra parte do chão, não de zero — ver `process`. */
    this.started = false

    const pedido = Number(options && options.processorOptions && options.processorOptions.lookaheadMs) || 0
    const ms = Math.min(MAX_LOOKAHEAD_MS, Math.max(0, pedido))
    this.delaySamples = Math.round((ms / 1000) * sampleRate)
    /** Uma linha de atraso por canal, criada quando o canal aparece. */
    this.delayLines = []
    this.delayPos = 0
  }

  process(inputs, outputs, parameters) {
    const input = inputs[0]
    const output = outputs[0]
    if (!input || !input[0]) return true

    const thresholdLinear = Math.pow(10, parameters.thresholdDb[0] / 20)
    const closeLinear = thresholdLinear * Math.pow(10, -parameters.hysteresisDb[0] / 20)
    const floorDb = parameters.floorDb[0]
    const floorGain = floorDb <= FLOOR_SILENCE_DB ? 0 : Math.pow(10, floorDb / 20)
    const attackCoeff = Math.exp(-1 / (sampleRate * (parameters.attackMs[0] / 1000)))
    const releaseCoeff = Math.exp(-1 / (sampleRate * (parameters.releaseMs[0] / 1000)))
    // 5 ms de alisamento na deteção — sem isto, uma única amostra alta (clique,
    // plosiva) abre e fecha o gate no mesmo bloco em vez de manter aberto.
    const envelopeCoeff = Math.exp(-1 / (sampleRate * 0.005))
    const holdSamples = (parameters.holdMs[0] / 1000) * sampleRate
    const sustainSamples = (parameters.sustainMs[0] / 1000) * sampleRate
    const dropDb = parameters.transientDropDb[0]
    // 0 dB = regra desligada: qualquer envolvente passa (`env >= 0`).
    const dropLinear = dropDb > 0 ? Math.pow(10, -dropDb / 20) : 0
    const peakDecay = Math.pow(10, -PEAK_HOLD_DB_PER_S / 20 / sampleRate)

    const numChannels = input.length
    const frames = input[0].length
    const delay = this.delaySamples

    // Com chão, o ganho nasce NO chão: nascer em zero e subir até ao chão era
    // um fade-in de silêncio digital no primeiro instante da chamada.
    if (!this.started) {
      this.started = true
      this.gain = floorGain
    }
    if (delay > 0) {
      while (this.delayLines.length < numChannels) this.delayLines.push(new Float32Array(delay))
    }

    for (let i = 0; i < frames; i++) {
      let rectified = 0
      for (let ch = 0; ch < numChannels; ch++) {
        const v = Math.abs(input[ch][i])
        if (v > rectified) rectified = v
      }
      this.envelope = envelopeCoeff * this.envelope + (1 - envelopeCoeff) * rectified

      if (this.open) {
        if (this.envelope > closeLinear) {
          this.holdRemaining = holdSamples
        } else if (this.holdRemaining > 0) {
          this.holdRemaining--
        }
        if (this.holdRemaining <= 0 && this.envelope <= closeLinear) {
          this.open = false
          this.aboveRun = 0
        }
      } else if (this.envelope > thresholdLinear) {
        this.aboveRun++
        const held = this.peak * peakDecay
        this.peak = this.envelope > held ? this.envelope : held
        if (this.aboveRun > sustainSamples && this.envelope >= this.peak * dropLinear) {
          this.open = true
          this.holdRemaining = holdSamples
        }
      } else {
        this.aboveRun = 0
        this.peak = 0
      }

      const targetGain = this.open ? 1 : floorGain
      const coeff = targetGain > this.gain ? attackCoeff : releaseCoeff
      this.gain = coeff * this.gain + (1 - coeff) * targetGain

      if (delay > 0) {
        const pos = this.delayPos
        for (let ch = 0; ch < numChannels; ch++) {
          const line = this.delayLines[ch]
          const atrasada = line[pos]
          line[pos] = input[ch][i]
          output[ch][i] = atrasada * this.gain
        }
        this.delayPos = pos + 1 === delay ? 0 : pos + 1
      } else {
        for (let ch = 0; ch < numChannels; ch++) {
          output[ch][i] = input[ch][i] * this.gain
        }
      }
    }
    return true
  }
}

registerProcessor('noise-gate-processor', NoiseGateProcessor)
