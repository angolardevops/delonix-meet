/** Sinais fabricados para os testes de `sinal.ts`. Não entra no bundle (só os testes o importam). */
export const from = {
  seno(freq: number, amplitude: number, segundos: number, sr: number): Float32Array {
    const d = new Float32Array(Math.round(segundos * sr))
    for (let i = 0; i < d.length; i++) d[i] = amplitude * Math.sin((2 * Math.PI * freq * i) / sr)
    return d
  },
  ruido(rms: number, segundos: number, sr: number): Float32Array {
    // Uniforme em [−a, a] tem RMS a/√3.
    const a = rms * Math.sqrt(3)
    const d = new Float32Array(Math.round(segundos * sr))
    let x = 12345
    for (let i = 0; i < d.length; i++) {
      x = (x * 1103515245 + 12345) % 2147483648
      d[i] = (x / 2147483648) * 2 * a - a
    }
    return d
  },
  juntar(...xs: Float32Array[]): Float32Array {
    const out = new Float32Array(xs.reduce((n, x) => n + x.length, 0))
    let o = 0
    for (const x of xs) {
      out.set(x, o)
      o += x.length
    }
    return out
  },
}
