import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from '../ui/icons'
import { cx } from '../ui/kit'
import { useOnboardingTour } from './useOnboardingTour'

const MARGIN = 16
const GAP = 12
const CARD_W = 352

function place(anchor: string): { top: number; left: number } | null {
  const el = document.querySelector<HTMLElement>(`[data-tour="${anchor}"]`)
  if (!el) return null
  const r = el.getBoundingClientRect()
  const left = Math.min(Math.max(r.left, MARGIN), window.innerWidth - CARD_W - MARGIN)
  return { top: r.bottom + GAP, left }
}

/**
 * Guia de primeira utilização do Início (template DelonixTour): destaca um
 * mosaico de cada vez com um anel (`box-shadow` gigante — dispensa uma
 * camada de véu à parte) e mostra um cartão flutuante junto dele.
 */
export default function TourOverlay() {
  const { t } = useTranslation()
  const tour = useOnboardingTour()
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null)

  useEffect(() => {
    if (!tour.open) return
    const el = document.querySelector<HTMLElement>(`[data-tour="${tour.current.anchor}"]`)
    el?.classList.add('dx-tour-target')
    const update = () => setPos(place(tour.current.anchor))
    update()
    window.addEventListener('resize', update)
    window.addEventListener('scroll', update, true)
    return () => {
      el?.classList.remove('dx-tour-target')
      window.removeEventListener('resize', update)
      window.removeEventListener('scroll', update, true)
    }
  }, [tour.open, tour.current.anchor])

  if (!tour.open) {
    if (!tour.resumable) return null
    return (
      <button type="button" className="dx-tour-resume" onClick={tour.reopen}>
        <span className="dx-tour-resume__mark" aria-hidden="true">
          ?
        </span>
        <span className="dx-tour-resume__text">
          <strong>{t('tour.ajuda.titulo')}</strong>
          <span>{t('tour.ajuda.progresso', { feito: tour.completedCount, total: tour.total })}</span>
        </span>
      </button>
    )
  }

  const isLast = tour.step === tour.total - 1

  return (
    <div className="dx-tour-card" style={pos ? { top: pos.top, left: pos.left } : { top: '50%', left: '50%' }} role="dialog" aria-label={t('tour.rotulo')}>
      <div className="dx-tour-card__progress">
        <span style={{ width: `${Math.round(((tour.step + 1) / tour.total) * 100)}%` }} />
      </div>
      <div className="dx-tour-card__body">
        <div className="dx-tour-card__head">
          <span className="dx-tour-card__badge">{t('tour.passo', { n: tour.step + 1, total: tour.total })}</span>
          <button type="button" className="dx-tour-card__skip" onClick={tour.skip}>
            {t('tour.saltar')}
          </button>
        </div>
        <div>
          <h2 className="dx-tour-card__title">{t(tour.current.titleKey)}</h2>
          <p className="dx-tour-card__text">{t(tour.current.bodyKey)}</p>
        </div>
        {tour.current.tipKey && (
          <div className="dx-tour-card__tip">
            <Icon name="keyboard" size={12} />
            <span>{t(tour.current.tipKey)}</span>
          </div>
        )}
        <div className="dx-tour-card__foot">
          <div className="dx-tour-card__dots">
            {Array.from({ length: tour.total }, (_, i) => (
              <span key={i} className={cx(i === tour.step && 'is-on', i < tour.step && 'is-done')} />
            ))}
          </div>
          <span style={{ flex: 1 }} />
          <button type="button" className="dx-tour-card__prev" disabled={tour.step === 0} onClick={tour.prev}>
            {t('tour.anterior')}
          </button>
          <button type="button" className="dx-tour-card__next" onClick={tour.next}>
            {isLast ? t('tour.concluir') : t('tour.seguinte')}
          </button>
        </div>
      </div>
      <div className="dx-tour-card__toggle">
        <button
          type="button"
          className={cx('dx-tour-switch', tour.enabled && 'is-on')}
          role="switch"
          aria-checked={tour.enabled}
          aria-label={t('tour.guiaActivo')}
          onClick={() => tour.setEnabled(!tour.enabled)}
        />
        <span className="dx-tour-card__toggle-text">
          <strong>{t('tour.guiaActivo')}</strong>
          <span>{t('tour.guiaActivoDica')}</span>
        </span>
      </div>
    </div>
  )
}
