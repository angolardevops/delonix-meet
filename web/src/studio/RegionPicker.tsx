/**
 * Escolha da região a gravar: arrasta-se um rectângulo POR CIMA DO PALCO.
 *
 * As coordenadas saem em FRACÇÕES, não em pixéis — o canvas de gravação
 * (1920×1080) e o palco no ecrã têm tamanhos diferentes, e guardar pixéis de
 * um deles partia o recorte assim que a janela mudasse de tamanho.
 */
import { useEffect, useRef, useState } from 'react'
import type { PointerEvent as ReactPointerEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '../ui/kit'
import { Recorte, RECORTE_INTEIRO } from './compositor'

export default function RegionPicker({
  onAplicar,
  onFechar,
}: {
  onAplicar: (r: Recorte) => void
  onFechar: () => void
}) {
  const { t } = useTranslation()
  const ref = useRef<HTMLDivElement>(null)
  const [arrasto, setArrasto] = useState<{ x0: number; y0: number; x1: number; y1: number } | null>(null)

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onFechar()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onFechar])

  function fraccao(e: ReactPointerEvent): { x: number; y: number } {
    const r = ref.current!.getBoundingClientRect()
    return {
      x: Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)),
      y: Math.min(1, Math.max(0, (e.clientY - r.top) / r.height)),
    }
  }

  const sel = arrasto && {
    left: `${Math.min(arrasto.x0, arrasto.x1) * 100}%`,
    top: `${Math.min(arrasto.y0, arrasto.y1) * 100}%`,
    width: `${Math.abs(arrasto.x1 - arrasto.x0) * 100}%`,
    height: `${Math.abs(arrasto.y1 - arrasto.y0) * 100}%`,
  }

  return (
    <div className="st-region" role="dialog" aria-label={t('studio.recorte.rotulo')}>
      <div
        className="st-region__area"
        ref={ref}
        data-studio="recorte-area"
        onPointerDown={(e) => {
          e.stopPropagation()
          e.currentTarget.setPointerCapture(e.pointerId)
          const p = fraccao(e)
          setArrasto({ x0: p.x, y0: p.y, x1: p.x, y1: p.y })
        }}
        onPointerMove={(e) => {
          if (!arrasto) return
          const p = fraccao(e)
          setArrasto((a) => (a ? { ...a, x1: p.x, y1: p.y } : a))
        }}
        onPointerUp={() => {
          if (!arrasto) return
          const w = Math.abs(arrasto.x1 - arrasto.x0)
          const h = Math.abs(arrasto.y1 - arrasto.y0)
          setArrasto(null)
          // Um arrasto minúsculo é um clique falhado, não uma região de 2 px.
          if (w < 0.03 || h < 0.03) return
          onAplicar({ x: Math.min(arrasto.x0, arrasto.x1), y: Math.min(arrasto.y0, arrasto.y1), w, h })
        }}
      >
        {sel && <div className="st-region__sel" style={sel} />}
      </div>
      <div className="st-region__bar">
        <span>{t('studio.recorte.instrucao')}</span>
        <span className="dx-spacer" />
        <Button size="sm" variant="secondary" onClick={() => onAplicar({ ...RECORTE_INTEIRO })}>
          {t('studio.fonte.tudo')}
        </Button>
        <Button size="sm" variant="ghost" onClick={onFechar}>
          {t('studio.recorte.cancelar')}
        </Button>
      </div>
    </div>
  )
}
