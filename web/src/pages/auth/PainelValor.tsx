/**
 * Painel escuro da entrada: a proposta de valor que substitui a landing antiga.
 *
 * Só diz o que o servidor faz. As capacidades abaixo existem: `e2ee.ts`,
 * `recorder.rs`, `broadcast.rs` (vários destinos RTMP por emissão, tecto
 * `MAX_DESTINOS_POR_DIRECTO`, 4 por omissão) e `mfa.rs`; a videoaula é o
 * formato `training` de `rooms.rs`; o login com a palavra-passe do Odoo é
 * `auth.rs` + `odoo_sso.rs`.
 *
 * O que fica de fora, e porquê:
 *  - «Gravação até 4K»: o gravador não tem resolução configurável.
 *  - «Multistream 5 destinos»: o número depende da instalação e não é público;
 *    diz-se «vários destinos» sem inventar o tecto.
 *  - «Dial-in +244»: o plano de controlo do dial-in EXISTE (`voice.rs`: DIDs,
 *    PIN, CDR) e a media Kamailio/FreeSWITCH está em `voice/`, mas falta a
 *    ponte FreeSWITCH↔SFU — quem liga não ouve a reunião. Anunciá-lo na
 *    entrada era prometer uma forma de entrar que não entra.
 *  - SIP · Kamailio, Media · FreeSWITCH, residência AO-LAD: dependem da
 *    instalação, não do produto.
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { getLoginBg } from '../../branding'
import { BrandLockup } from '../../components/BrandMark'
import { Icon } from '../../ui/icons'

/** Três fichas numa linha, como no template: ponto, triângulo e visto de cor. */
const CAPACIDADES: { chave: string; marca: 'ponto' | 'play' | 'visto' }[] = [
  { chave: 'auth.painel.gravacao', marca: 'ponto' },
  { chave: 'consola.entrar.multidestino', marca: 'play' },
  { chave: 'auth.painel.e2ee', marca: 'visto' },
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
        <p className="auth-aside__titulo">{t('consola.entrar.titulo')}</p>
        <p className="auth-aside__texto">{t('consola.entrar.texto')}</p>
        <ul className="auth-chips">
          {CAPACIDADES.map((c) => (
            <li key={c.chave} className="auth-chip">
              {c.marca === 'visto' ? (
                <Icon name="check" size={11} />
              ) : (
                <span className={`auth-chip__marca auth-chip__marca--${c.marca}`} aria-hidden="true" />
              )}
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
