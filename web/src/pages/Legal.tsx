/**
 * Termos de utilização e política de privacidade — públicos, sem sessão.
 * Texto honesto para uma instância auto-alojada: quem opera a instância é o
 * responsável pelo tratamento, e os dados ficam nos servidores dele.
 *
 * O índice usa botões e não âncoras `#…`: a app encaminha pelo hash, e uma
 * âncora mudava de página em vez de descer até à secção.
 */
import { useTranslation } from 'react-i18next'
import { getAppName } from '../branding'
import { Card } from '../ui/kit'
import Moldura from './publico/Moldura'

const SECCOES = [
  { id: 'termos', titulo: 'publico.legal.termosTitulo', paragrafos: ['termos1', 'termos2', 'termos3', 'termos4', 'termos5'] },
  {
    id: 'privacidade',
    titulo: 'publico.legal.privacidadeTitulo',
    paragrafos: ['privacidade1', 'privacidade2', 'privacidade3', 'privacidade4', 'privacidade5', 'privacidade6'],
  },
]

export default function Legal() {
  const { t } = useTranslation()
  const app = getAppName()

  return (
    <Moldura pagina="legal" estreita>
      <header className="pub-cabecalho">
        <h1>{t('publico.legal.titulo')}</h1>
        <p className="dx-muted">{t('publico.legal.actualizado')}</p>
      </header>

      <nav className="pub-indice" aria-label={t('publico.legal.indice')}>
        {SECCOES.map((s) => (
          <button
            key={s.id}
            type="button"
            className="dx-btn dx-btn--secondary dx-btn--sm"
            onClick={() => document.getElementById(`legal-${s.id}`)?.scrollIntoView({ behavior: 'smooth', block: 'start' })}
          >
            {t(s.titulo)}
          </button>
        ))}
      </nav>

      {SECCOES.map((s) => (
        <section key={s.id} id={`legal-${s.id}`} className="pub-legal" tabIndex={-1}>
          <Card title={t(s.titulo)} className="pub-cartao">
            <div className="pub-prosa">
              {s.paragrafos.map((p) => (
                <p key={p}>{t(`publico.legal.${p}`, { app })}</p>
              ))}
            </div>
          </Card>
        </section>
      ))}
    </Moldura>
  )
}
