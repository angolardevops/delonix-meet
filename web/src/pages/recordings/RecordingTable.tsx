/**
 * Vista LISTA: a tabela do template (sessão · sala · data · tamanho · estado).
 * Duração, resolução e armazenamento não são colunas — a API não os devolve
 * por gravação; a duração e a resolução aparecem no leitor, lidas do ficheiro.
 *
 * R59: uma gravação FALHADA não é clicável nem oferece acção nenhuma. A linha
 * mostra a causa registada no sítio do estado; não há botão, não há leitor.
 */
import { useTranslation } from 'react-i18next'
import type { RecordingItem } from '../../api'
import { Icon } from '../../ui/icons'
import { cx, StatusBadge, Tag } from '../../ui/kit'
import { formatBytes, formatDateTimeShort, isFailed, recordingName, thumbBackground } from './format'

export default function RecordingTable({
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
    <div className="dx-table-wrap rec-table-wrap">
      <table className="dx-table rec-table">
        <thead>
          <tr>
            <th scope="col">{t('recordings.colunas.sessao')}</th>
            <th scope="col">{t('recordings.colunas.sala')}</th>
            <th scope="col">{t('recordings.colunas.data')}</th>
            <th scope="col">{t('recordings.colunas.tamanho')}</th>
            <th scope="col">{t('recordings.colunas.estado')}</th>
          </tr>
        </thead>
        <tbody>
          {items.map((r) => {
            const failed = isFailed(r)
            const name = recordingName(r)
            return (
              <tr
                key={r.id}
                className={cx('rec-row', !failed && 'dx-row-link')}
                data-status={failed ? 'failed' : 'ready'}
                data-selected={selectedId === r.id || undefined}
                onClick={failed ? undefined : () => onOpen(r)}
              >
                <td>
                  <div className="rec-row__session">
                    <span
                      className={cx('rec-row__thumb', failed && 'is-failed')}
                      style={failed ? undefined : { background: thumbBackground(r.filename) }}
                      aria-hidden="true"
                    >
                      <Icon name={failed ? 'alert' : 'film'} size={13} />
                    </span>
                    <span className="rec-row__id">
                      {failed ? (
                        <span className="rec-row__name">{name}</span>
                      ) : (
                        <button
                          type="button"
                          className="rec-row__name rec-row__open"
                          aria-label={t('recordings.abrir', { name })}
                          onClick={(e) => {
                            e.stopPropagation()
                            onOpen(r)
                          }}
                        >
                          {name}
                        </button>
                      )}
                      <span className="rec-row__meta">
                        {r.uploader_name}
                        {!r.owned && <Tag plain>{t('recordings.partilhadaComigo')}</Tag>}
                      </span>
                    </span>
                  </div>
                </td>
                <td className="dx-num rec-row__room" data-label={t('recordings.colunas.sala')}>
                  {r.room_code}
                </td>
                <td className="dx-num rec-row__date" data-label={t('recordings.colunas.data')}>
                  {formatDateTimeShort(r.created_at, i18n.language)}
                </td>
                <td className="dx-num rec-row__size" data-label={t('recordings.colunas.tamanho')}>
                  {/* Uma falhada não tem tamanho: «0 MB» leria-se como ficheiro vazio. */}
                  {failed ? '—' : formatBytes(r.size_bytes, i18n.language)}
                </td>
                <td className="rec-row__state" data-label={t('recordings.colunas.estado')}>
                  {failed ? (
                    <span className="rec-failure">
                      <StatusBadge tone="record" icon="alert">
                        {t('recordings.estado.falhada')}
                      </StatusBadge>
                      <span className="rec-failure__reason" role="note">
                        {r.failure_reason || t('recordings.estado.semCausa')}
                      </span>
                    </span>
                  ) : (
                    <StatusBadge tone="success" icon="check">
                      {t('recordings.estado.pronta')}
                    </StatusBadge>
                  )}
                </td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}
