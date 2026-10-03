/**
 * «Operadoras ligadas»: os troncos pela ordem de encaminhamento. Só leitura —
 * arrastar, criar e editar não existem neste ecrã.
 */
import { useTranslation } from 'react-i18next'
import type { Trunk } from '../../api'
import { Async, AsyncSection } from '../../components/AsyncSection'
import { Card, StatusBadge, Tag } from '../../ui/kit'
import { useLocaleTag } from '../admin/orgShared'
import { formatMoney, formatNumber, formatRatio, measured, sortTrunks, trunkTone } from './format'
import { LoadMore, NA, useTelecomText } from './shared'
import type { PagedList } from './usePaged'

function TrunkItem({ trunk: k, ordinal }: { trunk: Trunk; ordinal: number }) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const { reason, label } = useTelecomText()
  const inUse = measured(k.status.channels_in_use)
  const asr = measured(k.status.asr)
  const semMedicao = <span className="dx-muted">{t('telecom.semMedicao')}</span>
  return (
    <li className="tel-trunk">
      <div className="tel-trunk__head">
        <span className="dx-num dx-muted tel-trunk__ord" aria-label={t('telecom.operadoras.ordem', { n: ordinal })}>
          {ordinal}
        </span>
        <Tag>{k.short_code}</Tag>
        <span className="tel-trunk__id">
          <strong>{k.name}</strong>
          <span className="dx-num dx-muted tel-break">
            {k.host}:{k.port} · {k.transport.toUpperCase()}
          </span>
        </span>
        <span className="tel-trunk__state">
          {!k.enabled && (
            <StatusBadge tone="neutral" icon="ban">
              {t('telecom.operadoras.desactivada')}
            </StatusBadge>
          )}
          <StatusBadge tone={trunkTone(k.status.state)}>{label('tronco', k.status.state)}</StatusBadge>
        </span>
      </div>
      {k.status.reasons.length > 0 && (
        <ul className="tel-reasons">
          {k.status.reasons.map((c) => (
            <li key={c}>{reason(c)}</li>
          ))}
        </ul>
      )}
      <dl className="tel-trunk__facts">
        <div>
          <dt className="dx-eyebrow">{t('telecom.operadoras.canais')}</dt>
          <dd className="dx-num">
            {inUse === null ? semMedicao : formatNumber(inUse, locale)}
            <span className="dx-muted"> {t('telecom.sbc.deMax', { max: formatNumber(k.status.channels_max, locale) })}</span>
          </dd>
        </div>
        <div>
          <dt className="dx-eyebrow">{t('telecom.operadoras.prefixos')}</dt>
          <dd className="dx-num tel-break">{k.prefixes.length > 0 ? k.prefixes.join(' · ') : NA}</dd>
        </div>
        <div>
          <dt className="dx-eyebrow">{t('telecom.operadoras.custoMin')}</dt>
          <dd className="dx-num">
            {k.current_price_per_min ? (
              formatMoney(k.current_price_per_min, locale)
            ) : (
              <span className="dx-muted">{t('telecom.razao.no_price_in_force')}</span>
            )}
          </dd>
        </div>
        <div>
          <dt className="dx-eyebrow">{t('telecom.operadoras.asr')}</dt>
          <dd className="dx-num">
            {asr === null ? (
              <>
                {semMedicao}
                {k.status.asr_reason && <span className="dx-muted"> · {reason(k.status.asr_reason)}</span>}
              </>
            ) : (
              <>
                {formatRatio(asr, locale)}
                <span className="dx-muted">
                  {' '}
                  {t('telecom.operadoras.asrBase', {
                    atendidas: k.status.asr_answered,
                    tentativas: k.status.asr_attempts,
                    horas: k.status.asr_window_hours,
                  })}
                </span>
              </>
            )}
          </dd>
        </div>
      </dl>
    </li>
  )
}

export default function TrunksCard({
  state,
  reload,
  loadMore,
  busy,
  err,
}: {
  state: Async<PagedList<Trunk>>
  reload: () => void
  loadMore: () => void
  busy: boolean
  err: string
}) {
  const { t } = useTranslation()
  return (
    <Card title={t('telecom.operadoras.titulo')} eyebrow={t('telecom.operadoras.eyebrow')} flush className="tel-card">
      <AsyncSection state={state} onRetry={reload}>
        {(d) =>
          d.items.length === 0 ? (
            <p className="dx-muted tel-note">{t('telecom.operadoras.vazio')}</p>
          ) : (
            <>
              <ol className="tel-trunks" data-testid="tel-trunks">
                {sortTrunks(d.items).map((k, i) => (
                  <TrunkItem key={k.id} trunk={k} ordinal={i + 1} />
                ))}
              </ol>
              <LoadMore next={d.next} busy={busy} err={err} onMore={loadMore} />
            </>
          )
        }
      </AsyncSection>
    </Card>
  )
}
