/**
 * Vista GRELHA: cartões com miniatura. A miniatura de uma gravação pronta é o
 * botão que a abre no leitor; a de uma FALHADA é uma imagem inerte com a causa
 * por baixo — sem ▶, sem botão, sem acções (R59). Oferecer «reproduzir» sobre
 * um ficheiro que não existe é prometer duas vezes à mesma pessoa.
 */
import { useTranslation } from 'react-i18next'
import type { RecordingItem } from '../../api'
import { Icon } from '../../ui/icons'
import { cx, StatusBadge, Tag } from '../../ui/kit'
import { formatBytes, formatDate, isFailed, recordingName, thumbBackground } from './format'

export default function RecordingGrid({
  items,
  selectedId,
  onOpen,
}: {
  items: RecordingItem[]
  selectedId: string | null
  onOpen: (r: RecordingItem) => void
}) {
  const { t, i18n } = useTranslation()
  return (
    <ul className="rec-grid">
      {items.map((r) => {
        const failed = isFailed(r)
        const name = recordingName(r)
        return (
          <li
            key={r.id}
            className={cx('rec-card', selectedId === r.id && 'is-selected')}
            data-status={failed ? 'failed' : 'ready'}
          >
            {failed ? (
              <div className="rec-card__thumb is-failed" role="img" aria-label={t('recordings.estado.falhada')}>
                <Icon name="alert" size={20} />
              </div>
            ) : (
              <button
                type="button"
                className="rec-card__thumb"
                style={{ background: thumbBackground(r.filename) }}
                aria-label={t('recordings.abrir', { name })}
                onClick={() => onOpen(r)}
              >
                <span className="rec-card__play" aria-hidden="true">
                  <Icon name="play" size={16} />
                </span>
              </button>
            )}
            <div className="rec-card__body">
              <strong className="rec-card__title" title={name}>
                {name}
              </strong>
              <span className="rec-card__meta">
                <span className="dx-num">{r.room_code}</span>
                <span aria-hidden="true">·</span>
                <span>{r.uploader_name}</span>
              </span>
              <span className="rec-card__meta dx-num">
                {formatDate(r.created_at, i18n.language)}
                {!failed && (
                  <>
                    <span aria-hidden="true"> · </span>
                    {formatBytes(r.size_bytes, i18n.language)}
                  </>
                )}
              </span>
              <span className="rec-card__tags">
                {failed ? (
                  <StatusBadge tone="record" icon="alert">
                    {t('recordings.estado.falhada')}
                  </StatusBadge>
                ) : (
                  <StatusBadge tone="success" icon="check">
                    {t('recordings.estado.pronta')}
                  </StatusBadge>
                )}
                {!r.owned && <Tag plain>{t('recordings.partilhadaComigo')}</Tag>}
              </span>
              {failed && (
                <p className="rec-failure__reason" role="note">
                  {r.failure_reason || t('recordings.estado.semCausa')}
                </p>
              )}
            </div>
          </li>
        )
      })}
    </ul>
  )
}
