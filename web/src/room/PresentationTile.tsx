import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from '../ui/icons'
import { cx } from '../ui/kit'

/**
 * Ecrã partilhado em palco, com zoom e arrasto. O `<video>` vai sempre mudo
 * (para o autoplay nunca ser bloqueado) e o áudio do separador toca num
 * `<audio>` próprio — excepto na própria apresentação, para não haver eco.
 */
export function PresentationTile({
  stream,
  label,
  own,
  onRequestControl,
  onToggleFullscreen,
}: {
  stream: MediaStream
  label: string
  own: boolean
  /** `undefined` esconde o botão (ver `useRemoteControl`). */
  onRequestControl?: () => void
  onToggleFullscreen: () => void
}) {
  const { t } = useTranslation()
  const attach = useCallback(
    (node: HTMLVideoElement | null) => {
      if (!node) return
      if (node.srcObject !== stream) node.srcObject = stream
      const tryPlay = () => node.play().catch(() => {})
      void tryPlay()
      node.onloadedmetadata = () => void tryPlay()
    },
    [stream],
  )
  const attachAudio = useCallback(
    (node: HTMLAudioElement | null) => {
      if (!node || own) return
      if (node.srcObject !== stream) node.srcObject = stream
      node.play().catch(() => {})
    },
    [stream, own],
  )

  const viewportRef = useRef<HTMLDivElement | null>(null)
  const [zoom, setZoomState] = useState(1)
  const [pan, setPanState] = useState({ x: 0, y: 0 })
  const [panning, setPanning] = useState(false)
  const zoomRef = useRef(1)
  const panRef = useRef({ x: 0, y: 0 })
  const dragRef = useRef<{ sx: number; sy: number; px: number; py: number } | null>(null)

  const applyZoom = useCallback((next: number, cx?: number, cy?: number) => {
    const el = viewportRef.current
    if (!el) return
    const rect = el.getBoundingClientRect()
    const z0 = zoomRef.current
    const z = Math.min(4, Math.max(1, next))
    let { x, y } = panRef.current
    // Zoom centrado no cursor: o ponto por baixo do rato fica no sítio.
    if (cx != null && cy != null && z !== z0) {
      const ox = cx - rect.left - rect.width / 2
      const oy = cy - rect.top - rect.height / 2
      x = (x - ox) * (z / z0) + ox
      y = (y - oy) * (z / z0) + oy
    }
    const maxX = (rect.width * (z - 1)) / 2
    const maxY = (rect.height * (z - 1)) / 2
    x = z === 1 ? 0 : Math.min(maxX, Math.max(-maxX, x))
    y = z === 1 ? 0 : Math.min(maxY, Math.max(-maxY, y))
    zoomRef.current = z
    panRef.current = { x, y }
    setZoomState(z)
    setPanState({ x, y })
  }, [])

  // Listener nativo: o `onWheel` do React é passivo e a página fazia scroll.
  useEffect(() => {
    const el = viewportRef.current
    if (!el) return
    const onWheel = (e: WheelEvent) => {
      e.preventDefault()
      applyZoom(zoomRef.current * (e.deltaY < 0 ? 1.15 : 1 / 1.15), e.clientX, e.clientY)
    }
    el.addEventListener('wheel', onWheel, { passive: false })
    return () => el.removeEventListener('wheel', onWheel)
  }, [applyZoom])

  // Apresentação nova: repõe o zoom.
  useEffect(() => {
    zoomRef.current = 1
    panRef.current = { x: 0, y: 0 }
    setZoomState(1)
    setPanState({ x: 0, y: 0 })
  }, [stream])

  const endDrag = () => {
    dragRef.current = null
    setPanning(false)
  }

  return (
    <div className="rm-pres" data-presentation={own ? 'propria' : 'remota'}>
      <div
        ref={viewportRef}
        className={cx('rm-pres__viewport', zoom > 1 && 'is-pannable', panning && 'is-panning')}
        onPointerDown={(e) => {
          if (zoomRef.current <= 1) return
          e.currentTarget.setPointerCapture?.(e.pointerId)
          dragRef.current = { sx: e.clientX, sy: e.clientY, px: panRef.current.x, py: panRef.current.y }
          setPanning(true)
        }}
        onPointerMove={(e) => {
          const d = dragRef.current
          const el = viewportRef.current
          if (!d || !el) return
          const rect = el.getBoundingClientRect()
          const z = zoomRef.current
          const maxX = (rect.width * (z - 1)) / 2
          const maxY = (rect.height * (z - 1)) / 2
          const x = Math.min(maxX, Math.max(-maxX, d.px + (e.clientX - d.sx)))
          const y = Math.min(maxY, Math.max(-maxY, d.py + (e.clientY - d.sy)))
          panRef.current = { x, y }
          setPanState({ x, y })
        }}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        onDoubleClick={(e) => applyZoom(zoomRef.current > 1 ? 1 : 2, e.clientX, e.clientY)}
      >
        <video
          ref={attach}
          autoPlay
          playsInline
          muted
          style={{ transform: `translate(${pan.x}px, ${pan.y}px) scale(${zoom})`, transition: panning ? 'none' : undefined }}
        />
      </div>
      {!own && <audio ref={attachAudio} autoPlay />}
      <div className="rm-pres__head">
        <span className="rm-pres__label">
          <Icon name="screen" size={12} />
          {label}
        </span>
      </div>
      <div className="rm-pres__tools" role="group" aria-label={t('room.apresentacao.zoom')}>
        <button type="button" onClick={() => applyZoom(zoomRef.current / 1.25)} aria-label={t('room.apresentacao.reduzir')} title={t('room.apresentacao.reduzir')}>
          <Icon name="minus" size={13} />
        </button>
        <span className="dx-num">{Math.round(zoom * 100)}%</span>
        <button type="button" onClick={() => applyZoom(zoomRef.current * 1.25)} aria-label={t('room.apresentacao.ampliar')} title={t('room.apresentacao.ampliar')}>
          <Icon name="plus" size={13} />
        </button>
        {zoom > 1 && (
          <button type="button" onClick={() => applyZoom(1)} aria-label={t('room.apresentacao.repor')} title={t('room.apresentacao.repor')}>
            <Icon name="undo" size={13} />
          </button>
        )}
        <button type="button" onClick={onToggleFullscreen} aria-label={t('room.apresentacao.ecraInteiro')} title={t('room.apresentacao.ecraInteiro')}>
          <Icon name="maximize" size={13} />
        </button>
        {!own && onRequestControl && (
          <button type="button" onClick={onRequestControl} aria-label={t('room.apresentacao.pedirControlo')} title={t('room.apresentacao.pedirControlo')}>
            <Icon name="cube" size={13} />
          </button>
        )}
      </div>
    </div>
  )
}
