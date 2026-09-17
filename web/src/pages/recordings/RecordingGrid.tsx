/**
 * Vista GRELHA: cartões com miniatura. A miniatura de uma gravação pronta é o
 * botão que a abre no leitor; a de uma FALHADA é uma imagem inerte com a causa
 * por baixo — sem ▶, sem botão, sem acções (R59). Oferecer «reproduzir» sobre
 * um ficheiro que não existe é prometer duas vezes à mesma pessoa.
 *
 * Duração e resolução aparecem sobre a miniatura quando o servidor as mediu;
 * sem medida não se desenha um selo vazio. A miniatura é a do servidor, ou o
 * tom por nome quando não há.
 */
import { useTranslation } from 'react-i18next'
import { Icon } from '../../ui/icons'
import { cx, Tag } from '../../ui/kit'
import { formatBytes } from './format'
import { formatClock, resolutionLabel, visibleState } from './libraryData'
import RecordingState from './RecordingState'
import { thumbStyle, useThumbnail } from './RecordingThumb'
import { useMetaLine } from './RecordingTable'
import type { RecordingView } from './recordingView'

export default function RecordingGrid({
  items,
  selectedId,
  retentionDays,
  onOpen,
}: {
  items: RecordingView[]
  selectedId: string | null
  retentionDays: number
  onOpen: (r: RecordingView) => void
}) {
  const { t, i18n } = useTranslation()
  const metaLine = useMetaLine()
  return (
    <ul className="rec-grid">
      {items.map((r) => {
        const failed = r.failed
        const res = resolutionLabel(r)
        return (
          <li key={r.id} className={cx('rec-card', selectedId === r.id && 'is-selected')} data-status={failed ? 'failed' : 'ready'}>
            {failed ? (
              <div className="rec-card__thumb is-failed" role="img" aria-label={t('recordings.estado.falhada')}>
                <Icon name="alert" size={20} />
              </div>
            ) : (
              <CardThumb r={r} res={res} onOpen={onOpen} />
            )}
            <div className="rec-card__body">
              <strong className="rec-card__title" title={r.name}>
                {r.name}
              </strong>
              <span className="rec-card__meta">{metaLine(r)}</span>
              <span className="rec-card__tags">
                {failed ? <RecordingState state={{ kind: 'failed' }} /> : <RecordingState state={visibleState(r, retentionDays)} />}
                {r.sizeBytes !== null && <span className="rec-card__size dx-num">{formatBytes(r.sizeBytes, i18n.language)}</span>}
                {!r.owned && <Tag plain>{t('recordings.partilhadaComigo')}</Tag>}
              </span>
              {failed && (
                <p className="rec-failure__reason" role="note">
                  {r.failureReason || t('recordings.estado.semCausa')}
                </p>
              )}
            </div>
          </li>
        )
      })}
    </ul>
  )
}

function CardThumb({ r, res, onOpen }: { r: RecordingView; res: string | null; onOpen: (r: RecordingView) => void }) {
  const { t } = useTranslation()
  const thumb = useThumbnail(r)
  return (
    <button type="button" className="rec-card__thumb" style={thumbStyle(thumb, r.name)} aria-label={t('recordings.abrir', { name: r.name })} onClick={() => onOpen(r)}>
      <span className="rec-card__play" aria-hidden="true">
        <Icon name="play" size={16} />
      </span>
      {res && <span className="rec-badge rec-card__res">{res}</span>}
      {r.durationMs !== null && <span className="rec-badge rec-card__dur dx-num">{formatClock(r.durationMs)}</span>}
    </button>
  )
}
