import { useCallback, useMemo, useRef, useState } from 'react'
import { SHARPEN_BUDGET } from '../../media/sharpen'
import { UnsharpRenderer } from '../../media/sharpenGl'
import type { OverlayStats, VideoOverlay } from '../../media/videoOverlay'
import { createStore, type EnhanceTarget } from './target'
import { readPref, writePref } from './deviceEnv'
import { useOverlay } from './useOverlay'

export type SharpenOff = { why: 'budget' | 'lost' | 'unsupported'; ms: number | null } | null

/**
 * Realce de nitidez no vídeo RECEBIDO em destaque. Por quem vê, só neste ecrã.
 * Desliga-se sozinho se o dispositivo não aguentar (ver `SHARPEN_BUDGET`).
 */
export function useReceiveSharpen(target: EnhanceTarget | null, blocked: boolean) {
  const [wanted, setWanted] = useState(() => readPref('dx_realce', '0') === '1')
  const [strength, setStrengthState] = useState(() => {
    const v = Number(readPref('dx_realce_intensidade', '50'))
    return Number.isFinite(v) ? Math.min(100, Math.max(0, v)) : 50
  })
  const [off, setOff] = useState<SharpenOff>(null)
  const [compare, setCompareState] = useState(false)
  const [stats] = useState(() => createStore<OverlayStats | null>(null))
  const strengthRef = useRef(strength)
  strengthRef.current = strength
  const overlayObj = useRef<VideoOverlay | null>(null)
  const compareRef = useRef(compare)
  compareRef.current = compare

  const running = wanted && !off && !blocked && !!target

  const handlers = useMemo(
    () => ({
      make: (c: HTMLCanvasElement) => {
        const r = UnsharpRenderer.create(c)
        if (r) c.dataset.realce = 'on'
        return r
      },
      onStart: (_r: UnsharpRenderer, o: VideoOverlay) => {
        overlayObj.current = o
        o.setHidden(compareRef.current)
      },
      beforeRender: (r: UnsharpRenderer) => {
        r.strength = strengthRef.current / 100
      },
      onStats: (s: OverlayStats | null) => stats.set(s),
      onEnd: (why: 'budget' | 'lost' | 'unsupported', last: OverlayStats | null) => {
        overlayObj.current = null
        setOff({ why, ms: last?.p95Ms ?? null })
      },
    }),
    [stats],
  )
  useOverlay(running, target, SHARPEN_BUDGET, handlers)

  const toggle = useCallback(() => {
    setWanted((v) => {
      writePref('dx_realce', v ? '0' : '1')
      return !v
    })
    // Voltar a ligar à mão dá nova oportunidade ao dispositivo — mas não a um
    // browser sem WebGL2, que não passou a tê-lo.
    setOff((o) => (o?.why === 'unsupported' ? o : null))
  }, [])

  const setStrength = useCallback((v: number) => {
    const n = Math.min(100, Math.max(0, Math.round(v)))
    setStrengthState(n)
    writePref('dx_realce_intensidade', String(n))
  }, [])

  const setCompare = useCallback((v: boolean) => {
    setCompareState(v)
    overlayObj.current?.setHidden(v)
  }, [])

  return { wanted, running, toggle, strength, setStrength, off, compare, setCompare, stats, target }
}

export type ReceiveSharpen = ReturnType<typeof useReceiveSharpen>
