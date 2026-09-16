/**
 * Os quatro layouts — o topo da coluna esquerda do DelonixStudio. Só desenha e devolve intenções; o estado é do
 * `usePalco`.
 */
import { useTranslation } from 'react-i18next'
import { cx } from '../ui/kit'
import { LAYOUTS, type LayoutDoPalco } from './palco'

/** O desenho de cada layout em miniatura: quantas caixas e como se arrumam. */
function Miniatura({ layout }: { layout: LayoutDoPalco }) {
  return (
    <span className={cx('st-lay__mini', `st-lay__mini--${layout}`)} aria-hidden="true">
      {layout === 'solo' && <span />}
      {layout === 'lado-a-lado' && (
        <>
          <span />
          <span />
        </>
      )}
      {layout === 'destaque' && (
        <>
          <span />
          <span className="st-lay__col">
            <span />
            <span />
          </span>
        </>
      )}
      {layout === 'grelha' && (
        <>
          <span />
          <span />
          <span />
          <span />
        </>
      )}
    </span>
  )
}

/** Chaves de tradução sem hífen (o portão de paridade das línguas lê só identificadores). */
const CHAVE: Record<LayoutDoPalco, string> = { solo: 'solo', 'lado-a-lado': 'ladoALado', destaque: 'destaque', grelha: 'grelha' }

export default function LayoutsPanel({
  layout,
  onLayout,
}: {
  layout: LayoutDoPalco
  onLayout: (l: LayoutDoPalco) => void
}) {
  const { t } = useTranslation()
  return (
    <section className="st-group" data-studio-grupo="layouts" aria-labelledby="st-lay-h">
      <h2 id="st-lay-h" className="st-group__title">
        {t('studio.layouts.titulo')}
      </h2>
      <div className="st-lay" role="group" aria-label={t('studio.layouts.titulo')}>
        {LAYOUTS.map((l) => (
          <button
            key={l}
            type="button"
            className="st-lay__btn"
            aria-pressed={layout === l}
            aria-label={t(`studio.layouts.${CHAVE[l]}`)}
            title={t(`studio.layouts.${CHAVE[l]}`)}
            data-studio-layout={l}
            onClick={() => onLayout(l)}
          >
            <Miniatura layout={l} />
          </button>
        ))}
      </div>

    </section>
  )
}
