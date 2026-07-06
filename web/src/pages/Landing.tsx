import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { LanguageToggle } from '../components/Shell'


type Feature = { t: string; d: string }
type FaqItem = { q: string; a: string }

/** Landing pública — só funcionalidades e afirmações verdadeiras do produto. */
export default function Landing({ onSignIn }: { onSignIn: () => void }) {
  const { t } = useTranslation()
  const ta = <T,>(key: string): T => t(key, { returnObjects: true }) as T
  const [faqOpen, setFaqOpen] = useState(0)

  const features = ta<Feature[]>('land.features')
  const secPoints = ta<string[]>('land.secPoints')
  const faq = ta<FaqItem[]>('land.faq')


  const goto = (id: string) => document.getElementById(id)?.scrollIntoView({ behavior: 'smooth' })

  // Ícones das feature cards (mesma ordem do protótipo)
  const featIcons = ['🔐', '🎭', '📝', '🎬', '🚪', '📅']

  return (
    <div className="landing">
      <header className="land-top">
        <div className="land-top-inner">
          <span className="brand-text land-brand">
            <span className="brand-mark">◆</span> Delonix <span>Meet</span>
          </span>
          <nav className="land-nav">
            <button className="land-link" onClick={() => goto('features')}>{t('land.nav.features')}</button>
            <button className="land-link" onClick={() => goto('security')}>{t('land.nav.security')}</button>
            <button className="land-link" onClick={() => goto('faq')}>{t('land.nav.faq')}</button>
          </nav>
          <div className="land-top-actions">
            <LanguageToggle />
            <button className="land-link strong" onClick={onSignIn}>{t('land.nav.signin')}</button>
            <button className="btn-new small land-demo" onClick={onSignIn}>{t('land.nav.demo')}</button>
          </div>
        </div>
      </header>

      <section className="land-hero">
        <div className="land-hero-copy">
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
        </div>

        <div className="land-preview" aria-hidden>
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
            <span className="chip float b">🎭 {t('land.hero.chipB')}</span>
            <span className="chip float c">🔒 {t('land.hero.chipC')}</span>
          </div>
        </div>
      </section>

      <section className="land-section" id="features">
        <h2>{t('land.featTitle')}</h2>
        <p className="land-section-sub">{t('land.featSub')}</p>
        <div className="feat-grid">
          {features.map((f, i) => (
            <div key={f.t} className="feat-card">
              <span className="feat-icon">{featIcons[i]}</span>
              <h3>{f.t}</h3>
              <p>{f.d}</p>
            </div>
          ))}
        </div>
      </section>

      <section className="land-section land-sec" id="security">
        <div className="sec-copy">
          <p className="eyebrow">{t('land.secEyebrow')}</p>
          <h2>{t('land.secTitle')}</h2>
          <p className="land-section-sub left">{t('land.secSub')}</p>
          <ul className="sec-points">
            {secPoints.map((p) => (
              <li key={p}>✓ {p}</li>
            ))}
          </ul>
        </div>
      </section>

      <section className="land-section narrow" id="faq">
        <h2>{t('land.faqTitle')}</h2>
        <div className="faq-list">
          {faq.map((f, i) => (
            <div key={f.q} className={faqOpen === i ? 'faq-item open' : 'faq-item'}>
              <button className="faq-q" onClick={() => setFaqOpen(faqOpen === i ? -1 : i)}>
                {f.q}
                <span className="faq-arrow">{faqOpen === i ? '−' : '+'}</span>
              </button>
              {faqOpen === i && <p className="faq-a">{f.a}</p>}
            </div>
          ))}
        </div>
      </section>

      <section className="land-final">
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
              <span className="brand-mark">◆</span> Delonix <span>Meet</span>
            </span>
            <p>{t('land.footer.tag')}</p>
          </div>
        </div>
        <p className="foot-rights">{t('land.footer.rights')}</p>
      </footer>
    </div>
  )
}
