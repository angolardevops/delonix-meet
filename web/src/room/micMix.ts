/**
 * Dois microfones activos AO MESMO TEMPO (ex.: lapela do orador + microfone de
 * mesa da sala), misturados no cliente numa só track. O servidor e a chamada
 * vêem um microfone; a mistura é só WebAudio local.
 *
 * As tracks de entrada ficam vivas enquanto a mistura existir: parar a saída
 * não as pára. Quem cria a mistura é dono dela e tem de chamar `stop()`.
 */
export class MicMix {
  readonly inputs: MediaStreamTrack[]
  readonly output: MediaStreamTrack
  private ctx: AudioContext
  private sources: MediaStreamAudioSourceNode[] = []

  constructor(inputs: MediaStreamTrack[]) {
    this.inputs = inputs
    this.ctx = new AudioContext()
    const dest = this.ctx.createMediaStreamDestination()
    // Dois sinais somados saturam com facilidade: cada entrada entra a meio ganho.
    const gain = this.ctx.createGain()
    gain.gain.value = inputs.length > 1 ? 0.7 : 1
    gain.connect(dest)
    for (const tr of inputs) {
      const src = this.ctx.createMediaStreamSource(new MediaStream([tr]))
      src.connect(gain)
      this.sources.push(src)
    }
    this.output = dest.stream.getAudioTracks()[0]
    if (this.ctx.state === 'suspended') void this.ctx.resume().catch(() => {})
  }

  /** Uma saída NOVA da mesma mistura (a anterior pode ter sido parada por quem a consumia). */
  freshOutput(): MediaStreamTrack {
    return this.output.clone()
  }

  stop() {
    this.sources.forEach((s) => s.disconnect())
    this.sources = []
    this.output.stop()
    this.inputs.forEach((tr) => tr.stop())
    void this.ctx.close().catch(() => {})
  }
}
