import { useEffect, useLayoutEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Tour de onboarding animado — spotlight sobre elementos reais da app (data-tour),
// com Skip a qualquer momento. Mostra-se na 1ª sessão (localStorage dx_tour_v1) e
// pode ser relançado via evento `dx-start-tour` (botão nas Definições).

const STORAGE_KEY = 'dx_tour_v1'

type Step = { target?: string; titleKey: string; bodyKey: string }

// Passos = tour da plataforma completa. Sem `target` = cartão centrado.
const STEPS: Step[] = [
  { titleKey: 'tour.welcome.t', bodyKey: 'tour.welcome.b' },
  { target: '[data-tour="nav-home"]', titleKey: 'tour.home.t', bodyKey: 'tour.home.b' },
  { target: '[data-tour="nav-calendar"]', titleKey: 'tour.calendar.t', bodyKey: 'tour.calendar.b' },
  { target: '[data-tour="nav-directory"]', titleKey: 'tour.directory.t', bodyKey: 'tour.directory.b' },
  { target: '[data-tour="nav-recordings"]', titleKey: 'tour.recordings.t', bodyKey: 'tour.recordings.b' },
  { target: '[data-tour="nav-whiteboards"]', titleKey: 'tour.whiteboards.t', bodyKey: 'tour.whiteboards.b' },
  { target: '[data-tour="nav-analytics"]', titleKey: 'tour.analytics.t', bodyKey: 'tour.analytics.b' },
  { target: '[data-tour="settings"]', titleKey: 'tour.settings.t', bodyKey: 'tour.settings.b' },
  { titleKey: 'tour.done.t', bodyKey: 'tour.done.b' },
]

const CARD_W = 340

export default function OnboardingTour() {
  const { t } = useTranslation()
  const [active, setActive] = useState(false)
  const [i, setI] = useState(0)
  const [rect, setRect] = useState<DOMRect | null>(null)

  // Arranque: 1ª sessão (auto, com delay p/ o shell pintar) OU sob pedido.
  useEffect(() => {
    const start = () => { setI(0); setActive(true) }
    let timer: number | undefined
    if (!localStorage.getItem(STORAGE_KEY)) timer = window.setTimeout(start, 700)
    window.addEventListener('dx-start-tour', start)
    return () => { if (timer) clearTimeout(timer); window.removeEventListener('dx-start-tour', start) }
  }, [])

  const step = STEPS[i]
  const isLast = i >= STEPS.length - 1

  function finish() { localStorage.setItem(STORAGE_KEY, 'done'); setActive(false) }
  function next() { if (isLast) finish(); else setI((n) => n + 1) }
  function prev() { setI((n) => Math.max(0, n - 1)) }

  // Medir o alvo (e re-medir em resize/scroll).
  useLayoutEffect(() => {
    if (!active) return
    const measure = () => {
      if (!step.target) { setRect(null); return }
      const el = document.querySelector(step.target) as HTMLElement | null
      if (!el) { setRect(null); return }
      el.scrollIntoView({ block: 'nearest', behavior: 'smooth' })
      setRect(el.getBoundingClientRect())
    }
    measure()
    window.addEventListener('resize', measure)
    window.addEventListener('scroll', measure, true)
    return () => {
      window.removeEventListener('resize', measure)
      window.removeEventListener('scroll', measure, true)
    }
  }, [active, i, step])

  // Teclado: Esc = saltar, →/Enter = seguinte, ← = anterior.
  useEffect(() => {
    if (!active) return
    const on = (e: KeyboardEvent) => {
      if (e.key === 'Escape') finish()
      else if (e.key === 'ArrowRight' || e.key === 'Enter') next()
      else if (e.key === 'ArrowLeft') prev()
    }
    window.addEventListener('keydown', on)
    return () => window.removeEventListener('keydown', on)
  })

  if (!active) return null

  const pad = 8
  const holeStyle = rect
    ? { top: rect.top - pad, left: rect.left - pad, width: rect.width + pad * 2, height: rect.height + pad * 2 }
    : undefined

  // Posição do cartão: à direita do alvo (a sidebar é à esquerda); em ecrã
  // estreito ou passo central, ao centro.
  const narrow = typeof window !== 'undefined' && window.innerWidth < 720
  let cardStyle: React.CSSProperties
  let anchored = false
  if (rect && !narrow) {
    anchored = true
    const top = Math.min(Math.max(rect.top - 8, 16), window.innerHeight - 260)
    let left = rect.right + 18
    if (left + CARD_W > window.innerWidth - 16) left = Math.max(16, rect.left - CARD_W - 18)
    cardStyle = { top, left, width: CARD_W }
  } else {
    cardStyle = { left: '50%', bottom: 40, transform: 'translateX(-50%)', width: Math.min(CARD_W, window.innerWidth - 32) }
  }

  return (
    <div className="tour-root" role="dialog" aria-modal="true" aria-label={t('tour.aria', 'Introdução à plataforma')}>
      {/* Bloqueador de interação (transparente) — captura cliques atrás do tour. */}
      <div className="tour-blocker" />
      {/* Escurecimento: com alvo, o buraco tem box-shadow gigante; sem alvo, dim total. */}
      {holeStyle ? (
        <div key={i} className="tour-hole" style={holeStyle} />
      ) : (
        <div className="tour-dim" />
      )}

      <div className={anchored ? 'tour-card anchored' : 'tour-card center'} style={cardStyle}>
        <button className="tour-skip" onClick={finish} aria-label={t('tour.skip', 'Saltar')}>
          {t('tour.skip', 'Saltar')} ✕
        </button>
        <div className="tour-step-num">{i + 1} / {STEPS.length}</div>
        <h3>{t(step.titleKey)}</h3>
        <p>{t(step.bodyKey)}</p>
        <div className="tour-dots" aria-hidden="true">
          {STEPS.map((_, k) => (
            <span key={k} className={k === i ? 'on' : k < i ? 'past' : ''} />
          ))}
        </div>
        <div className="tour-actions">
          {i > 0 ? (
            <button className="btn-ghost small" onClick={prev}>{t('tour.back', 'Anterior')}</button>
          ) : (
            <button className="btn-ghost small" onClick={finish}>{t('tour.skipAll', 'Saltar tour')}</button>
          )}
          <button className="btn-sm primary" onClick={next}>
            {isLast ? t('tour.startCta', 'Começar a usar') : t('tour.next', 'Seguinte')}
          </button>
        </div>
      </div>
    </div>
  )
}
