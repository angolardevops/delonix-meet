/**
 * Media das gravações no browser: miniatura do servidor, o ficheiro de vídeo,
 * a legenda VTT e fotogramas tirados do próprio ficheiro.
 *
 * Os elementos `<img>`, `<video>` e `<track>` não enviam o Bearer, por isso
 * tudo passa por URLs de objecto. As miniaturas ficam em memória durante a
 * sessão (são JPEG de poucos KB e aparecem na tabela, no painel e no leitor);
 * o vídeo e o VTT são revogados quando o componente sai.
 */
import { useEffect, useState } from 'react'
import { RecordingItem, recordingCaptionVttUrl, recordingObjectUrl, recordingThumbnailUrl } from '../../api'

const thumbs = new Map<string, Promise<string | null>>()

/** URL da miniatura medida pelo servidor, ou `null` (sem miniatura ou erro). */
export function useThumbnail(id: string, has: boolean): string | null {
  const [url, setUrl] = useState<string | null>(null)
  useEffect(() => {
    if (!has) {
      setUrl(null)
      return
    }
    let live = true
    let p = thumbs.get(id)
    if (!p) {
      p = recordingThumbnailUrl(id).catch(() => {
        thumbs.delete(id)
        return null
      })
      thumbs.set(id, p)
    }
    void p.then((u) => live && setUrl(u))
    return () => {
      live = false
    }
  }, [id, has])
  return url
}

export type VideoLoad = { s: 'idle' } | { s: 'loading' } | { s: 'ready'; url: string } | { s: 'error' }

/** Descarrega o ficheiro só quando `want` fica verdadeiro. `retry()` volta a tentar. */
export function useRecordingVideo(rec: RecordingItem, want: boolean): [VideoLoad, () => void] {
  const [video, setVideo] = useState<VideoLoad>({ s: 'idle' })
  const [attempt, setAttempt] = useState(0)
  useEffect(() => {
    if (!want || rec.status === 'failed') {
      setVideo({ s: 'idle' })
      return
    }
    let live = true
    let made = ''
    setVideo({ s: 'loading' })
    recordingObjectUrl(rec)
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
  }, [rec.id, rec.status, want, attempt])
  return [video, () => setAttempt((n) => n + 1)]
}

/** URL do VTT publicado numa língua, ou `null`. */
export function useCaptionTrack(id: string, lang: string | null): string | null {
  const [url, setUrl] = useState<string | null>(null)
  useEffect(() => {
    setUrl(null)
    if (!lang) return
    let live = true
    let made = ''
    recordingCaptionVttUrl(id, lang)
      .then((u) => {
        if (live) {
          made = u
          setUrl(u)
        } else URL.revokeObjectURL(u)
      })
      .catch(() => undefined)
    return () => {
      live = false
      if (made) URL.revokeObjectURL(made)
    }
  }, [id, lang])
  return url
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
