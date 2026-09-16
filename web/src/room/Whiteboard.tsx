import { PointerEvent as ReactPointerEvent, useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from '../ui/icons'
import { Button, cx } from '../ui/kit'
import type { WbStroke } from '../signaling'
import { WB_COLORS } from './useWhiteboard'

/**
 * Quadro branco colaborativo: traços com coordenadas normalizadas (0..1),
 * difundidos pela sinalização e redesenhados a cada redimensionamento.
 */
export function Whiteboard({
  strokes,
  onStroke,
  onClear,
  onSave,
  onClose,
}: {
  strokes: WbStroke[]
  onStroke: (s: WbStroke) => void
  onClear: () => void
  onSave: (pngBase64: string) => Promise<void>
  onClose: () => void
}) {
  const { t } = useTranslation()
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const wrapRef = useRef<HTMLDivElement>(null)
  const drawing = useRef<WbStroke | null>(null)
  const saved = useRef(false)
  const [color, setColor] = useState(WB_COLORS[0])
  const [width, setWidth] = useState(3)
  const [busy, setBusy] = useState(false)

  const drawStroke = (ctx: CanvasRenderingContext2D, s: WbStroke, W: number, H: number) => {
    if (s.pts.length < 2) return
    ctx.strokeStyle = s.c
    ctx.lineWidth = s.w
    ctx.lineCap = 'round'
    ctx.lineJoin = 'round'
    ctx.beginPath()
    ctx.moveTo(s.pts[0][0] * W, s.pts[0][1] * H)
    for (const [x, y] of s.pts.slice(1)) ctx.lineTo(x * W, y * H)
    ctx.stroke()
  }

  const redraw = useCallback(() => {
    const c = canvasRef.current
    const wrap = wrapRef.current
    if (!c || !wrap) return
    c.width = wrap.clientWidth
    c.height = wrap.clientHeight
    const ctx = c.getContext('2d')
    if (!ctx) return
    ctx.clearRect(0, 0, c.width, c.height)
    for (const s of strokes) drawStroke(ctx, s, c.width, c.height)
  }, [strokes])

  useEffect(() => {
    redraw()
    const ro = new ResizeObserver(redraw)
    if (wrapRef.current) ro.observe(wrapRef.current)
    return () => ro.disconnect()
  }, [redraw])

  const norm = (e: ReactPointerEvent): [number, number] => {
    const r = canvasRef.current!.getBoundingClientRect()
    return [(e.clientX - r.left) / r.width, (e.clientY - r.top) / r.height]
  }

  /** Fundo branco opaco + traços num PNG (o canvas em si é transparente). */
  const snapshot = (): string | null => {
    const c = canvasRef.current
    if (!c || strokes.length === 0) return null
    const off = document.createElement('canvas')
    off.width = c.width
    off.height = c.height
    const ctx = off.getContext('2d')
    if (!ctx) return null
    ctx.fillStyle = '#ffffff'
    ctx.fillRect(0, 0, off.width, off.height)
    for (const s of strokes) drawStroke(ctx, s, off.width, off.height)
    return off.toDataURL('image/png')
  }

  const save = async () => {
    const png = snapshot()
    if (!png) return
    saved.current = true
    setBusy(true)
    try {
      await onSave(png)
    } finally {
      setBusy(false)
    }
  }

  // Fechar guarda na biblioteca se houver conteúdo por guardar.
  const closeAndSave = async () => {
    if (!saved.current && strokes.length > 0) await save()
    onClose()
  }

  return (
    <section className="rm-wb" aria-label={t('room.quadro.titulo')}>
      <header className="rm-wb__bar">
        <Icon name="board" />
        <strong>{t('room.quadro.titulo')}</strong>
        <span className="dx-muted dx-num">{t('room.quadro.tracos', { count: strokes.length })}</span>
        <span className="dx-spacer" />
        <Button size="sm" variant="outline" icon="download" busy={busy} disabled={strokes.length === 0} onClick={() => void save()}>
          {t('room.quadro.guardar')}
        </Button>
        <Button size="sm" variant="primary" icon="x" onClick={() => void closeAndSave()}>
          {t('room.quadro.fechar')}
        </Button>
      </header>
      <div className="rm-wb__body">
        <div className="rm-wb__rail" role="toolbar" aria-label={t('room.quadro.ferramentas')}>
          <button type="button" className={cx('rm-wb__tool', width === 3 && 'is-on')} aria-pressed={width === 3} onClick={() => setWidth(3)}>
            <Icon name="pen" />
            <span>{t('room.quadro.fino')}</span>
          </button>
          <button type="button" className={cx('rm-wb__tool', width === 8 && 'is-on')} aria-pressed={width === 8} onClick={() => setWidth(8)}>
            <Icon name="edit" />
            <span>{t('room.quadro.grosso')}</span>
          </button>
          <button type="button" className="rm-wb__tool" onClick={onClear}>
            <Icon name="eraser" />
            <span>{t('room.quadro.limpar')}</span>
          </button>
          <span className="dx-spacer" />
          <div className="rm-wb__colors" role="radiogroup" aria-label={t('room.quadro.cor')}>
            {WB_COLORS.map((c, i) => (
              <button
                key={c}
                type="button"
                role="radio"
                aria-checked={c === color}
                aria-label={t('room.quadro.corN', { n: i + 1 })}
                className={cx('rm-wb__swatch', c === color && 'is-on')}
                style={{ background: c }}
                onClick={() => setColor(c)}
              />
            ))}
          </div>
        </div>
        <div className="rm-wb__paper" data-theme="light" ref={wrapRef}>
          <canvas
            ref={canvasRef}
            onPointerDown={(e) => {
              try {
                e.currentTarget.setPointerCapture(e.pointerId)
              } catch {
                /* ponteiro sintético */
              }
              drawing.current = { pts: [norm(e)], c: color, w: width }
            }}
            onPointerMove={(e) => {
              const s = drawing.current
              if (!s) return
              const p = norm(e)
              const last = s.pts[s.pts.length - 1]
              if (Math.abs(p[0] - last[0]) + Math.abs(p[1] - last[1]) < 0.002) return
              s.pts.push(p)
              const c = canvasRef.current
              const ctx = c?.getContext('2d')
              if (c && ctx) drawStroke(ctx, { ...s, pts: s.pts.slice(-2) }, c.width, c.height)
            }}
            onPointerUp={() => {
              const s = drawing.current
              drawing.current = null
              if (s && s.pts.length >= 2) onStroke(s)
            }}
          />
        </div>
      </div>
    </section>
  )
}
