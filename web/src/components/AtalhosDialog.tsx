/**
 * A folha de atalhos, aberta com «?» em qualquer ecrã.
 *
 * Lê o catálogo (`ui/atalhos.ts`) — não tem lista própria. Um atalho novo
 * aparece aqui por existir; um que mude de tecla muda aqui também. Mostra só
 * os escopos que valem no ecrã onde se abriu: dentro de uma reunião não serve
 * de nada anunciar as teclas da mesa de corte.
 */
import { useTranslation } from 'react-i18next'
import { atalhosDoEscopo, eMac, ESCOPOS, type EscopoDeAtalho, teclasDoAtalho } from '../ui/atalhos'
import { Dialog } from '../ui/kit'

export default function AtalhosDialog({ escopos, onClose }: { escopos: readonly EscopoDeAtalho[]; onClose: () => void }) {
  const { t } = useTranslation()
  const mac = eMac()
  const visiveis = ESCOPOS.filter((e) => escopos.includes(e))
  return (
    <Dialog title={t('ui.atalhos.titulo')} onClose={onClose} wide>
      <p className="dx-muted">{t('ui.atalhos.nota')}</p>
      <div className="dx-keys">
        {visiveis.map((escopo) => (
          <section key={escopo} className="dx-keys__grupo">
            <h3 className="dx-eyebrow">{t(`ui.atalhos.escopo.${escopo}`)}</h3>
            <dl className="dx-keys__lista">
              {atalhosDoEscopo(escopo).map((a) => (
                <div key={a.id} className="dx-keys__linha">
                  <dt>{t(a.rotulo)}</dt>
                  <dd>
                    <kbd className="dx-kbd">{teclasDoAtalho(a, mac)}</kbd>
                  </dd>
                </div>
              ))}
            </dl>
          </section>
        ))}
      </div>
    </Dialog>
  )
}
