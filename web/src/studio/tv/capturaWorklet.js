/**
 * Captura de amostras para o medidor de sonoridade da mesa de som.
 *
 * Junta blocos CONSECUTIVOS de 100 ms (por canal) e manda-os à thread
 * principal. Existe porque o `AnalyserNode` dá uma janela quando se lhe pede,
 * não um fluxo contínuo: a sonoridade integrada da norma precisa de todas as
 * amostras, pela ordem, com o estado dos filtros a atravessar os blocos.
 *
 * Plain JS pelo mesmo motivo do `noiseGateWorklet.js`: corre no scope global
 * do AudioWorklet, fora do pipeline de build.
 */
class CapturaProcessor extends AudioWorkletProcessor {
  constructor() {
    super()
    this.tamanho = Math.round(sampleRate * 0.1)
    this.buffers = []
    this.pos = 0
  }

  process(inputs, outputs) {
    const input = inputs[0]
    const output = outputs[0]
    if (input && input.length) {
      if (this.buffers.length !== input.length) {
        this.buffers = input.map(() => new Float32Array(this.tamanho))
        this.pos = 0
      }
      const n = input[0].length
      let i = 0
      while (i < n) {
        const k = Math.min(n - i, this.tamanho - this.pos)
        for (let c = 0; c < input.length; c++) this.buffers[c].set(input[c].subarray(i, i + k), this.pos)
        this.pos += k
        i += k
        if (this.pos === this.tamanho) {
          const envio = this.buffers
          this.port.postMessage(envio, envio.map((b) => b.buffer))
          this.buffers = input.map(() => new Float32Array(this.tamanho))
          this.pos = 0
        }
      }
      // Passa o som tal como veio: o nó fica no caminho sem o alterar.
      for (let c = 0; c < output.length && c < input.length; c++) output[c].set(input[c])
    }
    return true
  }
}

registerProcessor('captura-processor', CapturaProcessor)
