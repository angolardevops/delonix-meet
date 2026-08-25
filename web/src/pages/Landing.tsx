import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import LanguageToggle from '../components/LanguageToggle'

type Feature = { t: string; d: string }
type FaqItem = { q: string; a: string }
type Testimonial = { q: string; n: string; r: string }
type Plan = { name: string; monthly: number | null; popular?: boolean; desc: string; cta: string; features: string[] }
type AiPoint = { t: string; d: string }

/** Revela os elementos `.reveal` ao entrarem no viewport (entrada elegante,
 *  respeitando prefers-reduced-motion). Uma passagem: adiciona `.is-visible`. */
function useReveal() {
  useEffect(() => {
    const els = Array.from(document.querySelectorAll<HTMLElement>('.reveal'))
    const reveal = (el: Element) => el.classList.add('is-visible')
    const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    const vh = window.innerHeight || 0
    // Fail-safe: sem IntersectionObserver, com reduced-motion, ou num viewport
    // sem altura útil (o conteúdo NUNCA pode ficar preso invisível).
    if (reduce || typeof IntersectionObserver === 'undefined' || vh === 0) {
      els.forEach(reveal)
      return
    }
    // Acima/dentro da dobra → revela já (evita o flash do hero no 1º paint).
    const pending: HTMLElement[] = []
    els.forEach((el) => {
      if (el.getBoundingClientRect().top < vh * 0.92) reveal(el)
      else pending.push(el)
    })
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (e.isIntersecting) {
            reveal(e.target)
            io.unobserve(e.target)
          }
        }
      },
      { threshold: 0.12, rootMargin: '0px 0px -8% 0px' },
    )
    pending.forEach((el) => io.observe(el))
    return () => io.disconnect()
  }, [])
}

/** Landing pública — só funcionalidades e afirmações verdadeiras do produto. */
export default function Landing({ onSignIn }: { onSignIn: () => void }) {
  const { t, i18n } = useTranslation()
  const ta = <T,>(key: string): T => t(key, { returnObjects: true }) as T
  const [faqOpen, setFaqOpen] = useState(0)
  const [billing, setBilling] = useState<'monthly' | 'annual'>('monthly')
  const heroRef = useRef<HTMLElement>(null)

  const features = ta<Feature[]>('land.features')
  const secPoints = ta<string[]>('land.secPoints')
  const faq = ta<FaqItem[]>('land.faq')
  const testimonials = ta<Testimonial[]>('land.testimonials')
  const plans = ta<Plan[]>('land.plans')
  const aiPoints = ta<AiPoint[]>('land.ai.points')
  const integrations = ta<string[]>('land.integItems')

  useReveal()

  // Parallax subtil do preview do hero ao scroll (desliga em reduced-motion).
  useEffect(() => {
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return
    const onScroll = () => {
      const el = heroRef.current
      if (!el) return
      const y = Math.min(window.scrollY, 400)
      el.style.setProperty('--hero-shift', `${y * 0.06}px`)
    }
    window.addEventListener('scroll', onScroll, { passive: true })
    return () => window.removeEventListener('scroll', onScroll)
  }, [])

  const fmtPrice = (monthly: number) => {
    const v = billing === 'annual' ? Math.round(monthly * 0.8) : monthly
    return `${v.toLocaleString(i18n.language.startsWith('pt') ? 'pt-PT' : 'en-US')} Kz`
  }

  const goto = (id: string) => document.getElementById(id)?.scrollIntoView({ behavior: 'smooth' })

  // Ícones das feature cards (9 — mesma ordem do i18n)
  const featIcons = ['🔐', '🎭', '📝', '🌍', '🏆', '🖊️', '🎬', '🚪', '🔗']
  const aiIcons = ['📝', '🌍', '🎭']

  return (
    <div className="landing">
      <header className="land-top">
        <div className="land-top-inner">
          <span className="brand-text land-brand">
            <img src="/logo.svg" alt="" className="brand-logo" /> Delonix <span>Meet</span>
          </span>
          <nav className="land-nav">
            <button className="land-link" onClick={() => goto('ai')}>{t('land.nav.ai')}</button>
            <button className="land-link" onClick={() => goto('features')}>{t('land.nav.features')}</button>
            <button className="land-link" onClick={() => goto('security')}>{t('land.nav.security')}</button>
            <button className="land-link" onClick={() => goto('pricing')}>{t('land.nav.pricing')}</button>
            <button className="land-link" onClick={() => goto('faq')}>{t('land.nav.faq')}</button>
          </nav>
          <div className="land-top-actions">
            <LanguageToggle />
            <button className="land-link strong" onClick={onSignIn}>{t('land.nav.signin')}</button>
            <button className="btn-new small land-demo" onClick={onSignIn}>{t('land.nav.demo')}</button>
          </div>
        </div>
      </header>

      <section className="land-hero" ref={heroRef}>
        <div className="land-hero-glow" aria-hidden />
        <div className="land-hero-copy reveal">
          <p className="eyebrow">{t('land.hero.eyebrow')}</p>
          <h1>
            {t('land.hero.t1')}
            <span className="acc">{t('land.hero.acc')}</span>
            {t('land.hero.t2')}
          </h1>
          <p className="land-sub">{t('land.hero.sub')}</p>
          <div className="land-ctas">
            <button className="btn-new" onClick={onSignIn}>{t('land.hero.cta1')}</button>
            <button className="btn-ghost" onClick={onSignIn}>{t('land.hero.cta2')}</button>
          </div>
          <p className="land-trust">{t('land.hero.trust')}</p>
        </div>

        <div className="land-preview reveal delay-2" aria-hidden>
          <div className="preview-window">
            <span className="pill live">● {t('land.hero.live')}</span>
            <div className="preview-grid">
              {['WA', 'HM', 'RT', 'SA'].map((ini, i) => (
                <div key={ini} className={`preview-tile g${i}`}>
                  <span className="avatar-circle">{ini}</span>
                </div>
              ))}
            </div>
            <span className="chip float a">✓ {t('land.hero.chipA')}</span>
            <span className="chip float b">🌍 {t('land.hero.chipB')}</span>
            <span className="chip float c">🔒 {t('land.hero.chipC')}</span>
            <span className="chip float d">🏆 {t('land.hero.chipD')}</span>
          </div>
        </div>
      </section>

      {/* Banda de IA local — o maior diferenciador (soberania). */}
      <section className="land-ai reveal" id="ai">
        <div className="land-ai-inner">
          <div className="land-ai-head">
            <p className="eyebrow">{t('land.ai.eyebrow')}</p>
            <h2>{t('land.ai.title')}</h2>
            <p className="land-section-sub left">{t('land.ai.sub')}</p>
            <span className="ai-sov-badge">🛡 {t('land.ai.badge')}</span>
          </div>
          <div className="ai-points">
            {aiPoints.map((p, i) => (
              <div key={p.t} className="ai-point reveal" style={{ transitionDelay: `${i * 90}ms` }}>
                <span className="ai-point-ic">{aiIcons[i]}</span>
                <h3>{p.t}</h3>
                <p>{p.d}</p>
              </div>
            ))}
          </div>
        </div>
      </section>

      <section className="land-section" id="features">
        <h2 className="reveal">{t('land.featTitle')}</h2>
        <p className="land-section-sub reveal">{t('land.featSub')}</p>
        <div className="feat-grid">
          {features.map((f, i) => (
            <div key={f.t} className="feat-card reveal" style={{ transitionDelay: `${(i % 3) * 80}ms` }}>
              <span className="feat-icon">{featIcons[i]}</span>
              <h3>{f.t}</h3>
              <p>{f.d}</p>
            </div>
          ))}
        </div>
      </section>

      <section className="land-section land-sec reveal" id="security">
        <div className="sec-copy">
          <p className="eyebrow">{t('land.secEyebrow')}</p>
          <h2>{t('land.secTitle')}</h2>
          <p className="land-section-sub left">{t('land.secSub')}</p>
          <ul className="sec-points">
            {secPoints.map((p) => (
              <li key={p}>✓ {p}</li>
            ))}
          </ul>
          <div className="trust-badges">
            {ta<string[]>('land.secBadges').map((b) => (
              <span key={b} className="trust-badge">{b}</span>
            ))}
          </div>
        </div>
      </section>

      {/* Faixa de integrações — liga ao ERP/stack corporativo. */}
      <section className="land-integ reveal" id="integrations">
        <p className="eyebrow center">{t('land.integEyebrow')}</p>
        <h2 className="center">{t('land.integTitle')}</h2>
        <p className="land-section-sub">{t('land.integSub')}</p>
        <div className="integ-row">
          {integrations.map((name) => (
            <span key={name} className="integ-chip">{name}</span>
          ))}
        </div>
      </section>

      <section className="land-section" id="testimonials">
        <h2 className="center reveal">{t('land.testTitle')}</h2>
        <div className="test-grid">
          {testimonials.map((q, i) => (
            <figure key={q.n} className="test-card reveal" style={{ transitionDelay: `${i * 90}ms` }}>
              <blockquote>“{q.q}”</blockquote>
              <figcaption>
                <span className="avatar-circle small">{q.n.split(' ').map((w) => w[0]).slice(0, 2).join('')}</span>
                <span className="test-who">
                  <strong>{q.n}</strong>
                  <small>{q.r}</small>
                </span>
              </figcaption>
            </figure>
          ))}
        </div>
      </section>

      <section className="land-section" id="pricing">
        <p className="eyebrow center reveal">{t('land.priceEyebrow')}</p>
        <h2 className="center reveal">{t('land.priceTitle')}</h2>
        <p className="land-section-sub reveal">{t('land.priceSub')}</p>
        <div className="billing-toggle reveal" role="group" aria-label={t('land.priceEyebrow')}>
          <button
            className={billing === 'monthly' ? 'billing-btn active' : 'billing-btn'}
            onClick={() => setBilling('monthly')}
          >
            {t('land.monthly')}
          </button>
          <button
            className={billing === 'annual' ? 'billing-btn active' : 'billing-btn'}
            onClick={() => setBilling('annual')}
          >
            {t('land.annual')} <em>{t('land.save')}</em>
          </button>
        </div>
        <div className="plans-grid">
          {plans.map((p, i) => (
            <div
              key={p.name}
              className={`${p.popular ? 'plan-card popular' : 'plan-card'} reveal`}
              style={{ transitionDelay: `${i * 90}ms` }}
            >
              {p.popular && <span className="plan-badge">{t('land.popular')}</span>}
              <h3>{p.name}</h3>
              <p className="plan-desc">{p.desc}</p>
              <p className="plan-price">
                {p.monthly == null ? (
                  <strong>{t('land.custom')}</strong>
                ) : (
                  <>
                    <strong>{fmtPrice(p.monthly)}</strong>
                    <small>{t('land.perUser')}</small>
                  </>
                )}
              </p>
              <ul className="plan-features">
                {p.features.map((f) => (
                  <li key={f}>✓ {f}</li>
                ))}
              </ul>
              <button className={p.popular ? 'btn-new plan-cta' : 'btn-ghost plan-cta'} onClick={onSignIn}>
                {p.cta}
              </button>
            </div>
          ))}
        </div>
      </section>

      <section className="land-section narrow" id="faq">
        <h2 className="reveal">{t('land.faqTitle')}</h2>
        <div className="faq-list">
          {faq.map((f, i) => (
            <div key={f.q} className={`${faqOpen === i ? 'faq-item open' : 'faq-item'} reveal`}>
              <button className="faq-q" onClick={() => setFaqOpen(faqOpen === i ? -1 : i)}>
                {f.q}
                <span className="faq-arrow">{faqOpen === i ? '−' : '+'}</span>
              </button>
              {faqOpen === i && <p className="faq-a">{f.a}</p>}
            </div>
          ))}
        </div>
      </section>

      <section className="land-final reveal">
        <div className="land-final-glow" aria-hidden />
        <h2>{t('land.finalTitle')}</h2>
        <p className="land-section-sub">{t('land.finalSub')}</p>
        <div className="land-ctas center">
          <button className="btn-new" onClick={onSignIn}>{t('land.hero.cta1')}</button>
          <button className="btn-ghost" onClick={onSignIn}>{t('land.hero.cta2')}</button>
        </div>
      </section>

      <footer className="land-footer">
        <div className="foot-grid">
          <div className="foot-brand">
            <span className="brand-text">
              <img src="/logo.svg" alt="" className="brand-logo" /> Delonix <span>Meet</span>
            </span>
            <p>{t('land.footer.tag')}</p>
            <a className="link small-link" href="#/status">{t('land.statusLink')}</a>
            <a className="link small-link" href="#/api-docs">API REST</a>
            <a className="link small-link" href="#/legal">{t('land.legalLink')}</a>
          </div>
        </div>
        <p className="foot-rights">{t('land.footer.rights')}</p>
      </footer>
    </div>
  )
}
