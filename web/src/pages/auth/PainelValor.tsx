/**
 * Painel escuro da entrada: a proposta de valor que substitui a landing antiga.
 *
 * Só diz o que o servidor faz. O template anunciava 4K, multistream para cinco
 * destinos, dial-in e SIP — nada disso tem código por trás, e por isso não está
 * aqui. As quatro capacidades abaixo existem: `e2ee.ts`, `recorder.rs`,
 * `broadcast.rs` e `mfa.rs`.
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { getLoginBg } from '../../branding'
import { BrandLockup } from '../../components/BrandMark'
import { Icon, IconName } from '../../ui/icons'

const CAPACIDADES: { chave: string; icon: IconName }[] = [
  { chave: 'auth.painel.e2ee', icon: 'lock' },
  { chave: 'auth.painel.gravacao', icon: 'record' },
  { chave: 'auth.painel.directo', icon: 'live' },
  { chave: 'auth.painel.mfa', icon: 'shieldCheck' },
]

/** O fundo personalizado muda nas definições de marca; ouve-se o mesmo evento. */
function useFundo() {
  const [bg, setBg] = useState(getLoginBg())
  useEffect(() => {
    const on = () => setBg(getLoginBg())
    window.addEventListener('dx-branding', on)
    return () => window.removeEventListener('dx-branding', on)
  }, [])
  return bg
}

export default function PainelValor() {
  const { t } = useTranslation()
  const bg = useFundo()
  return (
    <aside className="auth-aside dx-stage">
      {bg ? (
        <div className="auth-aside__bg auth-aside__bg--foto" style={{ backgroundImage: `url(${bg})` }} aria-hidden="true" />
      ) : (
        <div className="auth-aside__bg" aria-hidden="true" />
      )}
      <div className="auth-aside__marca">
        <BrandLockup size={32} tone="tile" />
      </div>
      <div className="auth-aside__proposta">
        <p className="auth-aside__titulo">{t('auth.painel.titulo')}</p>
        <p className="auth-aside__texto">{t('auth.painel.texto')}</p>
        <ul className="auth-chips">
          {CAPACIDADES.map((c) => (
            <li key={c.chave} className="auth-chip">
              <Icon name={c.icon} size={12} />
              {t(c.chave)}
            </li>
          ))}
        </ul>
      </div>
      <nav className="auth-aside__rodape" aria-label={t('auth.painel.ligacoes')}>
        <a href="#/status">{t('auth.painel.estado')}</a>
        <a href="#/api-docs">{t('auth.painel.api')}</a>
        <a href="#/legal">{t('auth.painel.legal')}</a>
      </nav>
    </aside>
  )
}
