/**
 * Media das gravações no browser: o ficheiro de vídeo e fotogramas tirados
 * dele. O `<video>` não envia o Bearer, por isso o ficheiro chega por URL de
 * objecto, revogado quando o componente sai.
 *
 * A miniatura e as legendas do servidor ficam para quando o contrato de
 * metadados chegar à `main` (ver `recordingView.ts`).
 */
import { useEffect, useState } from 'react'
import { recordingObjectUrl } from '../../api'
import type { RecordingView } from './recordingView'

export type VideoLoad = { s: 'idle' } | { s: 'loading' } | { s: 'ready'; url: string } | { s: 'error' }

/** Descarrega o ficheiro só quando `want` fica verdadeiro. `retry()` volta a tentar. */
export function useRecordingVideo(rec: RecordingView, want: boolean): [VideoLoad, () => void] {
  const [video, setVideo] = useState<VideoLoad>({ s: 'idle' })
  const [attempt, setAttempt] = useState(0)
  useEffect(() => {
    if (!want || rec.failed) {
      setVideo({ s: 'idle' })
      return
    }
    let live = true
    let made = ''
    setVideo({ s: 'loading' })
    recordingObjectUrl(rec.source)
      .then((u) => {
        if (live) {
          made = u
          setVideo({ s: 'ready', url: u })
        } else URL.revokeObjectURL(u)
      })
      .catch(() => live && setVideo({ s: 'error' }))
    return () => {
      live = false
      if (made) URL.revokeObjectURL(made)
    }
    // O objecto `rec` muda a cada recarga da biblioteca; o ficheiro é o mesmo.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rec.id, rec.failed, want, attempt])
  return [video, () => setAttempt((n) => n + 1)]
}

/**
 * Fotogramas nos instantes pedidos (segundos), tirados do ficheiro já
 * carregado: um `<video>` escondido salta para cada instante e desenha-o num
 * canvas. Nada vai para o servidor. Devolve um array paralelo a `times`
 * (`null` enquanto não há) e `failed` se o browser não conseguir.
 */
export function useFrameGrabs(url: string | null, times: number[]): { frames: (string | null)[]; failed: boolean } {
  const key = times.map((t) => t.toFixed(2)).join(',')
  const [frames, setFrames] = useState<(string | null)[]>([])
  const [failed, setFailed] = useState(false)
  useEffect(() => {
    setFrames(times.map(() => null))
    setFailed(false)
    if (!url || times.length === 0) return
    let cancelled = false
    const v = document.createElement('video')
    v.muted = true
    v.preload = 'auto'
    v.playsInline = true
    v.src = url
    const canvas = document.createElement('canvas')
    canvas.width = 160
    canvas.height = 90
    const ctx = canvas.getContext('2d')
    const waitFor = (ev: string) =>
      new Promise<void>((resolve, reject) => {
        const cleanup = () => {
          clearTimeout(timer)
          v.removeEventListener(ev, ok)
          v.removeEventListener('error', bad)
        }
        const ok = () => {
          cleanup()
          resolve()
        }
        const bad = () => {
          cleanup()
          reject(new Error(ev))
        }
        const timer = setTimeout(bad, 8000)
        v.addEventListener(ev, ok, { once: true })
        v.addEventListener('error', bad, { once: true })
      })
    ;(async () => {
      try {
        if (v.readyState < 1) await waitFor('loadedmetadata')
        for (let i = 0; i < times.length; i++) {
          if (cancelled || !ctx) return
          v.currentTime = Math.max(0.05, times[i])
          await waitFor('seeked')
          if (cancelled) return
          const r = Math.max(canvas.width / (v.videoWidth || 16), canvas.height / (v.videoHeight || 9))
          const w = (v.videoWidth || 16) * r
          const h = (v.videoHeight || 9) * r
          ctx.drawImage(v, (canvas.width - w) / 2, (canvas.height - h) / 2, w, h)
          const data = canvas.toDataURL('image/jpeg', 0.72)
          setFrames((prev) => prev.map((x, j) => (j === i ? data : x)))
        }
      } catch {
        if (!cancelled) setFailed(true)
      }
    })()
    return () => {
      cancelled = true
      v.removeAttribute('src')
      v.load()
    }
    // `key` resume `times`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [url, key])
  return { frames, failed }
}
