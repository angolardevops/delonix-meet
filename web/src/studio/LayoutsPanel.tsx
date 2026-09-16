/**
 * Layouts, conteúdo do palco e qualidade de gravação — o topo da coluna
 * esquerda do DelonixStudio. Só desenha e devolve intenções; o estado é do
 * `usePalco`.
 */
import { useTranslation } from 'react-i18next'
import { cx, Select } from '../ui/kit'
import type { ConteudoDoPalco, LayoutDoPalco, Qualidade } from './palco'
import { LAYOUTS, QUALIDADES, rotuloDaQualidade } from './palco'

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

const CONTEUDOS: ConteudoDoPalco[] = ['fontes', 'quadro', 'marca']

/** Chaves de tradução sem hífen (o portão de paridade das línguas lê só identificadores). */
const CHAVE: Record<LayoutDoPalco, string> = { solo: 'solo', 'lado-a-lado': 'ladoALado', destaque: 'destaque', grelha: 'grelha' }

export default function LayoutsPanel({
  layout,
  conteudo,
  qualidade,
  qualidadeBloqueada,
  onLayout,
  onConteudo,
  onQualidade,
}: {
  layout: LayoutDoPalco
  conteudo: ConteudoDoPalco
  qualidade: Qualidade
  qualidadeBloqueada: boolean
  onLayout: (l: LayoutDoPalco) => void
  onConteudo: (c: ConteudoDoPalco) => void
  onQualidade: (q: Qualidade) => void
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

      <span className="st-label">{t('studio.layouts.conteudo')}</span>
      <div className="dx-seg st-seg" role="group" aria-label={t('studio.layouts.conteudo')}>
        {CONTEUDOS.map((c) => (
          <button key={c} type="button" aria-pressed={conteudo === c} data-studio-conteudo={c} onClick={() => onConteudo(c)}>
            {t(`studio.layouts.conteudos.${c}`)}
          </button>
        ))}
      </div>

      <label className="st-label" htmlFor="st-qualidade">
        {t('studio.qualidade.titulo')}
      </label>
      <Select
        id="st-qualidade"
        value={qualidade}
        disabled={qualidadeBloqueada}
        data-studio="qualidade"
        onChange={(e) => onQualidade(e.target.value as Qualidade)}
      >
        {(Object.keys(QUALIDADES) as Qualidade[]).map((q) => (
          <option key={q} value={q}>
            {rotuloDaQualidade(q)}
          </option>
        ))}
      </Select>
      <p className="st-note">{qualidadeBloqueada ? t('studio.qualidade.bloqueada') : t('studio.qualidade.nota')}</p>
    </section>
  )
}
