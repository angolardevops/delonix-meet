/**
 * Vista LISTA: a tabela do template — SESSÃO · DURAÇÃO · RESOLUÇÃO · TAMANHO ·
 * ESTADO · ARMAZENAMENTO. Duração e resolução são as medidas pelo servidor; o
 * que ele não mediu, e o armazenamento por gravação (que não existe), mostram
 * «—», nunca um valor inventado.
 *
 * R59: uma gravação FALHADA não é clicável nem oferece acção nenhuma. A linha
 * mostra a causa registada no sítio do estado; não há botão, não há leitor.
 */
import { useTranslation } from 'react-i18next'
import { Icon } from '../../ui/icons'
import { cx, Tag } from '../../ui/kit'
import { formatBytes, formatDate } from './format'
import { formatClock, resolutionLabel, visibleState } from './libraryData'
import RecordingState from './RecordingState'
import { thumbStyle, useThumbnail } from './RecordingThumb'
import type { RecordingView } from './recordingView'

export function NoValue() {
  const { t } = useTranslation()
  return (
    <span className="rec-none" title={t('recordings.semDado')}>
      <span aria-hidden="true">—</span>
      <span className="dx-sr-only">{t('recordings.semDado')}</span>
    </span>
  )
}

function RowThumb({ r }: { r: RecordingView }) {
  const thumb = useThumbnail(r)
  return (
    <span className={cx('rec-row__thumb', r.failed && 'is-failed')} style={r.failed ? undefined : thumbStyle(thumb, r.name)} aria-hidden="true">
      {r.failed && <Icon name="alert" size={13} />}
    </span>
  )
}

export function useMetaLine() {
  const { t, i18n } = useTranslation()
  return (r: RecordingView) =>
    [
      r.uploaderName,
      r.category ? t(`recordings.categoria.${r.category}`) : null,
      t('recordings.salaCodigo', { code: r.roomCode }),
      formatDate(r.createdAt, i18n.language),
    ]
      .filter(Boolean)
      .join(' · ')
}

export default function RecordingTable({
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
    <div className="dx-table-wrap rec-table-wrap">
      <table className="rec-table">
        <colgroup>
          <col />
          <col className="rec-col--dur" />
          <col className="rec-col--res" />
          <col className="rec-col--size" />
          <col className="rec-col--state" />
          <col className="rec-col--store" />
        </colgroup>
        <thead>
          <tr>
            <th scope="col">{t('recordings.colunas.sessao')}</th>
            <th scope="col">{t('recordings.colunas.duracao')}</th>
            <th scope="col">{t('recordings.colunas.resolucao')}</th>
            <th scope="col">{t('recordings.colunas.tamanho')}</th>
            <th scope="col">{t('recordings.colunas.estado')}</th>
            <th scope="col">{t('recordings.colunas.armazenamento')}</th>
          </tr>
        </thead>
        <tbody>
          {items.map((r) => {
            const failed = r.failed
            const res = resolutionLabel(r)
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
                    <RowThumb r={r} />
                    <span className="rec-row__id">
                      {failed ? (
                        <span className="rec-row__name">{r.name}</span>
                      ) : (
                        <button
                          type="button"
                          className="rec-row__name rec-row__open"
                          aria-label={t('recordings.abrir', { name: r.name })}
                          onClick={(e) => {
                            e.stopPropagation()
                            onOpen(r)
                          }}
                        >
                          {r.name}
                        </button>
                      )}
                      <span className="rec-row__meta">
                        <span className="rec-row__metatext">{metaLine(r)}</span>
                        {!r.owned && <Tag plain>{t('recordings.partilhadaComigo')}</Tag>}
                      </span>
                    </span>
                  </div>
                </td>
                <td className="dx-num rec-row__dur" data-label={t('recordings.colunas.duracao')}>
                  {failed ? '—' : r.durationMs !== null ? formatClock(r.durationMs) : <NoValue />}
                </td>
                <td className="rec-row__res" data-label={t('recordings.colunas.resolucao')}>
                  {res ? <span className="rec-res">{res}</span> : <NoValue />}
                </td>
                <td className="dx-num rec-row__size" data-label={t('recordings.colunas.tamanho')}>
                  {/* Uma falhada não tem tamanho: «0 MB» leria-se como ficheiro vazio. */}
                  {r.sizeBytes === null ? '—' : formatBytes(r.sizeBytes, i18n.language)}
                </td>
                <td className="rec-row__state" data-label={t('recordings.colunas.estado')}>
                  {failed ? (
                    <span className="rec-failure">
                      <RecordingState state={{ kind: 'failed' }} />
                      <span className="rec-failure__reason" role="note">
                        {r.failureReason || t('recordings.estado.semCausa')}
                      </span>
                    </span>
                  ) : (
                    <RecordingState state={visibleState(r, retentionDays)} />
                  )}
                </td>
                <td className="dx-num rec-row__store" data-label={t('recordings.colunas.armazenamento')}>
                  <NoValue />
                </td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}
