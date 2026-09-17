/**
 * Histórico de chamadas com o que o servidor REGISTA, por ordem cronológica:
 *  - chamadas perdidas entre contactos (`missed_calls`, pela presença) —
 *    devolvem-se com um clique;
 *  - chamadas telefónicas que entraram pelo dial-in da organização
 *    (GET /api/orgs/{org}/voice/cdr, só admins), com número e duração.
 *
 * «Efectuadas» e «recebidas» entre contactos não aparecem porque não se
 * guardam; saídas PSTN não existem («Sem outbound» em `voice/README.md`).
 *
 * `searchable` (separador Histórico): painel de pesquisa estilo Odoo sobre as
 * entradas que já estão aqui inteiras — o servidor ainda não descreve
 * `call_records` (fase 2 do contrato), por isso filtra-se no browser e o ecrã
 * diz isso.
 */
import { ReactNode, useMemo } from 'react'
import { localSchema } from '../../ui/search/localSchema'
import { SearchBar, SearchResults } from '../../ui/search/SearchResults'
import { LocalFallback, useResourceSearch } from '../../ui/search/useResourceSearch'
import { useTranslation } from 'react-i18next'
import { listVoiceCdr, VoiceCdr } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import type { MissedCall } from '../../presence'
import { Icon } from '../../ui/icons'
import { cx } from '../../ui/kit'
import { refusalAware, useLocaleTag } from '../admin/orgShared'
import { fmtDuration } from '../admin/VoiceCard'

const DAY = 86_400_000

type Entry =
  | { kind: 'missed'; at: string; m: MissedCall }
  | { kind: 'pstn'; at: string; c: VoiceCdr }

/** «12:04» hoje, «ontem 16:41», «12 Set» antes disso. */
export function whenLabel(iso: string, locale: string, yesterday: (time: string) => string, now = Date.now()): string {
  const d = new Date(iso)
  const today = new Date(now)
  const startToday = new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime()
  const time = d.toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' })
  if (d.getTime() >= startToday) return time
  if (d.getTime() >= startToday - DAY) return yesterday(time)
  return d.toLocaleDateString(locale, { day: 'numeric', month: 'short' })
}

export default function CallHistory({
  orgId,
  isAdmin,
  missed,
  onCallBack,
  days,
  limit,
  compact,
  searchable,
}: {
  orgId: string
  isAdmin: boolean
  missed: MissedCall[]
  onCallBack: (m: MissedCall) => void
  /** Só as dos últimos N dias. */
  days?: number
  limit?: number
  compact?: boolean
  searchable?: boolean
}) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const cdr = useAsync(
    (signal) => (isAdmin ? refusalAware(listVoiceCdr(orgId, signal), t) : Promise.resolve([] as VoiceCdr[])),
    [orgId, isAdmin],
  )

  const build = useMemo(
    () => (rows: VoiceCdr[]) => {
      const since = days ? Date.now() - days * DAY : 0
      const all: Entry[] = [
        ...missed.map((m): Entry => ({ kind: 'missed', at: m.created_at, m })),
        ...rows.map((c): Entry => ({ kind: 'pstn', at: c.started_at, c })),
      ]
        .filter((e) => new Date(e.at).getTime() >= since)
        .sort((a, b) => b.at.localeCompare(a.at))
      return limit ? all.slice(0, limit) : all
    },
    [missed, days, limit],
  )

  return (
    <AsyncSection state={cdr.state} onRetry={cdr.reload}>
      {(rows) => {
        const entries = build(rows)
        if (entries.length === 0) {
          return <p className="call-empty">{days ? t('consola.chamadas.semHistoricoDias', { count: days }) : t('consola.chamadas.semHistorico')}</p>
        }
        return searchable ? (
          <HistorySearch entries={entries} render={(list) => <HistoryRows entries={list} compact={compact} onCallBack={onCallBack} locale={locale} />} />
        ) : (
          <HistoryRows entries={entries} compact={compact} onCallBack={onCallBack} locale={locale} />
        )
      }}
    </AsyncSection>
  )
}

function HistoryRows({ entries, compact, onCallBack, locale }: { entries: Entry[]; compact?: boolean; onCallBack: (m: MissedCall) => void; locale: string }) {
  const { t } = useTranslation()
  return (
    <ul className={cx('call-hist', compact && 'call-hist--compact')} role="list" data-testid="call-history">
      {entries.map((e) =>
        e.kind === 'missed' ? (
          <li key={`m${e.m.id}`}>
            <button
              type="button"
              className="call-hist__row"
              onClick={() => onCallBack(e.m)}
              title={t('org.dir.devolverA', { nome: e.m.caller_name })}
            >
              <span className="call-hist__ico call-hist__ico--missed" aria-hidden="true">
                <Icon name={e.m.kind === 'voice' ? 'phone' : 'video'} size={12} />
              </span>
              <span className="call-hist__text">
                <strong>{e.m.caller_name}</strong>
                <span className="dx-num">
                  {[
                    t('consola.chamadas.perdida'),
                    e.m.kind === 'voice' ? t('org.dir.perdidaVoz') : t('org.dir.perdidaVideo'),
                    whenLabel(e.at, locale, (hora) => t('consola.chamadas.ontem', { hora })),
                  ].join(' · ')}
                </span>
              </span>
              <span className="call-hist__go" aria-hidden="true">
                <Icon name="refresh" size={11} />
              </span>
            </button>
          </li>
        ) : (
          <li key={`p${e.c.id}`} data-testid="pstn-history">
            <div className="call-hist__row">
              <span className="call-hist__ico call-hist__ico--in" aria-hidden="true">
                <Icon name="phone" size={12} />
              </span>
              <span className="call-hist__text">
                <strong className="dx-num">{e.c.caller_number || t('consola.voz.anonimo')}</strong>
                <span className="dx-num">
                  {[
                    t('consola.chamadas.recebidaDialIn'),
                    whenLabel(e.at, locale, (hora) => t('consola.chamadas.ontem', { hora })),
                    fmtDuration(e.c.duration_secs),
                  ].join(' · ')}
                </span>
              </span>
            </div>
          </li>
        ),
      )}
    </ul>
  )
}

const HISTORY_SCHEMA = localSchema(
  'call_history',
  [
    { name: 'kind', type: 'enum', options: ['missed', 'pstn'] },
    { name: 'who', type: 'text' },
    { name: 'media', type: 'enum', options: ['voice', 'video'] },
    { name: 'at', type: 'datetime' },
    { name: 'duration_secs', type: 'number', aggregates: ['sum'] },
  ],
  [
    { name: 'missed', group: 'kind', filter: [['kind', 'eq', 'missed']] },
    { name: 'pstn', group: 'kind', filter: [['kind', 'eq', 'pstn']] },
    { name: 'today', group: 'period', filter: [['at', 'in_period', 'today']] },
    { name: 'yesterday', group: 'period', filter: [['at', 'in_period', 'yesterday']] },
  ],
  { textFields: ['who'], defaultOrder: ['-at'] },
)

function HistorySearch({ entries, render }: { entries: Entry[]; render: (list: Entry[]) => ReactNode }) {
  const { t } = useTranslation()
  const key = entries.map((e) => (e.kind === 'missed' ? `m${e.m.id}` : `p${e.c.id}`)).join(',')
  const fallback = useMemo<LocalFallback<Entry>>(
    () => ({
      load: async () => entries,
      source: {
        schema: HISTORY_SCHEMA,
        get: (e, f) => {
          switch (f) {
            case 'kind':
              return e.kind
            case 'who':
              return e.kind === 'missed' ? e.m.caller_name : e.c.caller_number
            case 'media':
              return e.kind === 'missed' ? e.m.kind : 'voice'
            case 'at':
              return e.at
            case 'duration_secs':
              return e.kind === 'pstn' ? e.c.duration_secs : null
            default:
              return null
          }
        },
        text: (e) => (e.kind === 'missed' ? e.m.caller_name : `${e.c.caller_number} ${e.c.did_e164}`),
      },
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [key],
  )
  const rs = useResourceSearch<Entry>({ resource: null, ns: 'hist.', fallback, deps: [key] })
  return (
    <div className="call-hist-search">
      <SearchBar rs={rs} label={t('search.rotulos.call_history')} className="dx-searchbar--stack" />
      <SearchResults rs={rs} emptyTitle={t('consola.chamadas.semHistorico')} renderItems={render} />
    </div>
  )
}
