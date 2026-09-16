import { useEffect, useRef } from 'react'
import type { Budget, BudgetVerdict } from '../../media/sharpen'
import { VideoOverlay, type OverlayRenderer, type OverlayStats } from '../../media/videoOverlay'
import { findTargetVideo, type EnhanceTarget } from './target'

export type OverlayEnd = 'budget' | 'lost' | 'unsupported'

export interface OverlayHandlers<R extends OverlayRenderer> {
  make: (canvas: HTMLCanvasElement) => R | null
  beforeRender?: (renderer: R, video: HTMLVideoElement, now: number, frame: number) => void
  onStart?: (renderer: R, overlay: VideoOverlay, canvas: HTMLCanvasElement) => void
  onStats: (s: OverlayStats | null) => void
  onEnd: (why: OverlayEnd, last: OverlayStats | null, verdict: BudgetVerdict | null) => void
  /** Ver `OverlayOptions.ready`. */
  ready?: () => boolean
}

/**
 * Mantém um `VideoOverlay` ligado ao `<video>` do alvo enquanto `active`.
 *
 * O palco remonta retratos (mudança de orador, de vista, de página) sem avisar
 * ninguém; em vez de seguir essas mudanças, procura-se o vídeo duas vezes por
 * segundo — um `querySelector` — e refaz-se o overlay se o elemento mudou.
 * Os handlers vivem numa ref: mudar um callback não reinicia o efeito.
 */
export function useOverlay<R extends OverlayRenderer>(
  active: boolean,
  target: EnhanceTarget | null,
  budget: Budget,
  handlers: OverlayHandlers<R>,
  maxPixels?: number,
) {
  const h = useRef(handlers)
  h.current = handlers
  const overlayRef = useRef<{ overlay: VideoOverlay; video: HTMLVideoElement; renderer: R } | null>(null)
  const key = target ? `${target.kind}:${target.peerId}` : ''

  useEffect(() => {
    if (!active || !target) return
    let ended = false
    const stopCurrent = () => {
      overlayRef.current?.overlay.stop()
      overlayRef.current = null
    }
    const poll = () => {
      if (ended) return
      const video = findTargetVideo(target)
      const cur = overlayRef.current
      if (cur && cur.video === video) return
      stopCurrent()
      if (!video) {
        h.current.onStats(null)
        return
      }
      const canvas = document.createElement('canvas')
      const renderer = h.current.make(canvas)
      if (!renderer) {
        ended = true
        h.current.onEnd('unsupported', null, null)
        return
      }
      const overlay = new VideoOverlay({
        video,
        canvas,
        renderer,
        budget,
        maxPixels,
        beforeRender: (v, now, frame) => h.current.beforeRender?.(renderer, v, now, frame),
        onStats: (s) => h.current.onStats(s),
        ready: () => h.current.ready?.() ?? true,
        onStop: (why, last, verdict) => {
          if (overlayRef.current?.overlay === overlay) overlayRef.current = null
          // Retrato remontado: o próximo `poll` volta a ligar ao vídeo novo.
          if (why === 'detached') return
          ended = true
          h.current.onEnd(why, last, verdict)
        },
      })
      overlayRef.current = { overlay, video, renderer }
      overlay.start()
      h.current.onStart?.(renderer, overlay, canvas)
    }
    poll()
    const id = window.setInterval(poll, 500)
    return () => {
      ended = true
      clearInterval(id)
      stopCurrent()
      h.current.onStats(null)
    }
    // `target` entra pela chave: o nome da pessoa mudar não reinicia nada.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, key, budget, maxPixels])

  return overlayRef
}
