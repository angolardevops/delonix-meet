import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'

/**
 * Medidor do nível REAL do microfone (RMS em dBFS). Escreve no DOM por ref, a
 * ~12 Hz, sem estado: medir o som não pode fazer a página inteira renderizar.
 */
export function MicLevel({ stream, version }: { stream: MediaStream | null; version: number }) {
  const { t } = useTranslation()
  const barRef = useRef<HTMLSpanElement>(null)
  const dbRef = useRef<HTMLSpanElement>(null)

  useEffect(() => {
    const track = stream?.getAudioTracks()[0]
    if (!stream || !track) return
    let ctx: AudioContext
    try {
      ctx = new AudioContext()
    } catch {
      return
    }
    const source = ctx.createMediaStreamSource(new MediaStream([track]))
    const analyser = ctx.createAnalyser()
    analyser.fftSize = 512
    source.connect(analyser)
    const buf = new Float32Array(analyser.fftSize)
    let last = 0
    let raf = 0
    const tick = (now: number) => {
      raf = requestAnimationFrame(tick)
      if (now - last < 80) return
      last = now
      if (ctx.state === 'suspended') void ctx.resume().catch(() => {})
      analyser.getFloatTimeDomainData(buf)
      let sum = 0
      for (let i = 0; i < buf.length; i++) sum += buf[i] * buf[i]
      const rms = Math.sqrt(sum / buf.length)
      const db = track.enabled && rms > 0 ? Math.max(-60, 20 * Math.log10(rms)) : -60
      // −60 dBFS é silêncio; 0 é o máximo. A barra lê-se de forma linear em dB.
      const pct = Math.round(((db + 60) / 60) * 100)
      if (barRef.current) barRef.current.style.width = `${pct}%`
      if (dbRef.current) dbRef.current.textContent = db <= -60 ? '−∞ dB' : `${Math.round(db)} dB`
    }
    raf = requestAnimationFrame(tick)
    return () => {
      cancelAnimationFrame(raf)
      source.disconnect()
      void ctx.close()
    }
  }, [stream, version])

  const semMic = !stream?.getAudioTracks().length
  return (
    <div className="rm-level" role="presentation" title={t('room.preEntrada.nivelMicrofone')}>
      <span className="dx-meter dx-meter--success rm-level__meter">
        <span ref={barRef} style={{ width: 0 }} />
      </span>
      <span ref={dbRef} className="dx-num dx-muted rm-level__db">
        {semMic ? '—' : '−∞ dB'}
      </span>
    </div>
  )
}
