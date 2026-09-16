/**
 * Toque de chamada sintético (Web Audio): dois sinos com harmónico suave,
 * repetidos a cada 1,8 s até `stop()`. Sem ficheiro de som — nada a
 * descarregar e nada proprietário.
 */
export function startRingtone(): () => void {
  let ctx: AudioContext
  try {
    ctx = new AudioContext()
  } catch {
    return () => {}
  }
  let stopped = false
  const note = (f: number, at: number, dur = 0.34, vol = 0.16) => {
    for (const [mult, g] of [
      [1, vol],
      [2, vol * 0.35],
    ] as const) {
      const osc = ctx.createOscillator()
      const gain = ctx.createGain()
      osc.type = 'sine'
      osc.frequency.value = f * mult
      osc.connect(gain)
      gain.connect(ctx.destination)
      const s = ctx.currentTime + at
      gain.gain.setValueAtTime(0.0001, s)
      gain.gain.exponentialRampToValueAtTime(g, s + 0.02)
      gain.gain.exponentialRampToValueAtTime(0.0001, s + dur)
      osc.start(s)
      osc.stop(s + dur + 0.02)
    }
  }
  const ring = () => {
    if (stopped) return
    note(659.25, 0)
    note(1108.73, 0.16)
    note(880.0, 0.34, 0.5)
  }
  ring()
  const iv = setInterval(ring, 1800)
  return () => {
    stopped = true
    clearInterval(iv)
    void ctx.close().catch(() => {})
  }
}
