/**
 * Reprodução partilhada pelo painel da biblioteca e pelo leitor em página
 * inteira: estado do `<video>` (tempo, duração e resolução MEDIDAS no
 * ficheiro, pausa, som, velocidade) e a barra de progresso do template, com
 * marcas de capítulo quando as há.
 *
 * A barra é um `<input type="range">` por cima do desenho: teclado (setas,
 * Home/End), leitores de ecrã e toque funcionam sem código extra.
 */
import { RefObject, useCallback, useEffect, useState } from 'react'
import { cx } from '../../ui/kit'
import { formatClock, percentOf } from './libraryData'

export interface Playback {
  playing: boolean
  nowMs: number
  /** Duração lida do ficheiro; `null` até o browser a saber. */
  durationMs: number | null
  size: { width: number; height: number } | null
  muted: boolean
  rate: number
  ended: boolean
  toggle: () => void
  seek: (ms: number, play?: boolean) => void
  setMuted: (m: boolean) => void
  setRate: (r: number) => void
}

/**
 * `src` muda quando o ficheiro carrega; os ouvintes ligam-se a cada elemento
 * novo. `pendingSeekMs` é aplicado assim que houver metadados (clicar num
 * capítulo antes de o vídeo ter carregado).
 */
export function usePlayback(ref: RefObject<HTMLVideoElement | null>, src: string | null): Playback & { queueSeek: (ms: number) => void } {
  const [playing, setPlaying] = useState(false)
  const [nowMs, setNow] = useState(0)
  const [durationMs, setDuration] = useState<number | null>(null)
  const [size, setSize] = useState<{ width: number; height: number } | null>(null)
  const [muted, setMutedState] = useState(false)
  const [rate, setRateState] = useState(1)
  const [ended, setEnded] = useState(false)
  const [pending, setPending] = useState<number | null>(null)

  useEffect(() => {
    const v = ref.current
    if (!v || !src) return
    let measuring = false
    const onMeta = () => {
      if (v.videoWidth && v.videoHeight) setSize({ width: v.videoWidth, height: v.videoHeight })
      if (Number.isFinite(v.duration)) setDuration(v.duration * 1000)
      else {
        // WebM do MediaRecorder chega sem duração no cabeçalho: saltar para o
        // fim obriga o browser a medi-la, e volta-se ao início logo a seguir.
        measuring = true
        v.currentTime = Number.MAX_SAFE_INTEGER
      }
    }
    const onDuration = () => {
      if (!Number.isFinite(v.duration)) return
      setDuration(v.duration * 1000)
      if (measuring) {
        measuring = false
        v.currentTime = 0
      }
    }
    const onTime = () => {
      if (!measuring) setNow(v.currentTime * 1000)
    }
    const onPlay = () => {
      setPlaying(true)
      setEnded(false)
    }
    const onPause = () => setPlaying(false)
    const onEnded = () => {
      setPlaying(false)
      setEnded(true)
    }
    const onVolume = () => setMutedState(v.muted)
    const onRate = () => setRateState(v.playbackRate)
    v.addEventListener('loadedmetadata', onMeta)
    v.addEventListener('durationchange', onDuration)
    v.addEventListener('timeupdate', onTime)
    v.addEventListener('play', onPlay)
    v.addEventListener('pause', onPause)
    v.addEventListener('ended', onEnded)
    v.addEventListener('volumechange', onVolume)
    v.addEventListener('ratechange', onRate)
    if (v.readyState >= 1) onMeta()
    return () => {
      v.removeEventListener('loadedmetadata', onMeta)
      v.removeEventListener('durationchange', onDuration)
      v.removeEventListener('timeupdate', onTime)
      v.removeEventListener('play', onPlay)
      v.removeEventListener('pause', onPause)
      v.removeEventListener('ended', onEnded)
      v.removeEventListener('volumechange', onVolume)
      v.removeEventListener('ratechange', onRate)
    }
  }, [ref, src])

  const seek = useCallback(
    (ms: number, play = false) => {
      const v = ref.current
      setNow(ms)
      if (!v || !src) {
        setPending(ms)
        return
      }
      v.currentTime = ms / 1000
      if (play) void v.play().catch(() => undefined)
    },
    [ref, src],
  )

  // Salto pedido antes de o ficheiro existir: aplica-se quando houver duração.
  useEffect(() => {
    const v = ref.current
    if (pending === null || !v || !src || durationMs === null) return
    v.currentTime = pending / 1000
    setPending(null)
  }, [pending, durationMs, ref, src])

  const toggle = useCallback(() => {
    const v = ref.current
    if (!v) return
    if (v.paused) void v.play().catch(() => undefined)
    else v.pause()
  }, [ref])

  return {
    playing,
    nowMs,
    durationMs,
    size,
    muted,
    rate,
    ended,
    toggle,
    seek,
    queueSeek: (ms: number) => {
      setNow(ms)
      setPending(ms)
    },
    setMuted: (m) => {
      if (ref.current) ref.current.muted = m
    },
    setRate: (r) => {
      if (ref.current) ref.current.playbackRate = r
    },
  }
}

/** Barra de progresso do template com marcas nos instantes `ticksMs`. */
export function ProgressBar({
  nowMs,
  durationMs,
  ticksMs,
  onSeek,
  label,
  valueText,
  size = 'mini',
}: {
  nowMs: number
  durationMs: number | null
  ticksMs: number[]
  onSeek: (ms: number) => void
  label: string
  valueText: string
  size?: 'mini' | 'full'
}) {
  const pct = percentOf(nowMs, durationMs) ?? 0
  return (
    <div className={cx('rec-bar', `rec-bar--${size}`)}>
      <div className="rec-bar__track" aria-hidden="true">
        <div className="rec-bar__fill" style={{ width: `${pct}%` }} />
        {ticksMs.map((t) => {
          const p = percentOf(t, durationMs)
          return p === null || p <= 0 ? null : <span key={t} className="rec-bar__tick" style={{ left: `${p}%` }} />
        })}
        {size === 'full' && <span className="rec-bar__knob" style={{ left: `${pct}%` }} />}
      </div>
      <input
        className="rec-bar__input"
        type="range"
        min={0}
        max={durationMs ?? 0}
        step={1000}
        value={Math.min(nowMs, durationMs ?? 0)}
        disabled={!durationMs}
        aria-label={label}
        aria-valuetext={valueText}
        onChange={(e) => onSeek(Number(e.target.value))}
      />
    </div>
  )
}

export const clockPair = (nowMs: number, durationMs: number | null) => `${formatClock(nowMs)} / ${formatClock(durationMs)}`
