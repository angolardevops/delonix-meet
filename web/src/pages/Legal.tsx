import { useTranslation } from 'react-i18next'
import { BrandMark } from '../components/BrandMark'

/** Termos de Utilização + Política de Privacidade — página pública, sem autenticação.
 *  Conteúdo honesto para self-hosted: quem opera a instância é o responsável pelo
 *  tratamento; os dados ficam nos servidores da organização. */
export default function Legal() {
  const { t } = useTranslation()
  const ta = (key: string): string[] => t(key, { returnObjects: true }) as string[]

  return (
    <div className="legal-page">
      <header className="legal-head">
        <a href="#/" className="brand-text">
          <BrandMark /> Delonix <span>Meet</span>
        </a>
        <a className="link small-link" href="#/login">{t('legal.back')}</a>
      </header>

      <main className="legal-body">
        <h1>{t('legal.title')}</h1>
        <p className="muted">{t('legal.updated')}</p>

        <section id="terms">
          <h2>{t('legal.termsTitle')}</h2>
          {ta('legal.termsBody').map((p) => (
            <p key={p}>{p}</p>
          ))}
        </section>

        <section id="privacy">
          <h2>{t('legal.privacyTitle')}</h2>
          {ta('legal.privacyBody').map((p) => (
            <p key={p}>{p}</p>
          ))}
        </section>
      </main>
    </div>
  )
}
