/**
 * Histórico de chamadas com o que o servidor REGISTA, por ordem cronológica:
 *  - chamadas perdidas entre contactos (`missed_calls`, pela presença) —
 *    devolvem-se com um clique;
 *  - chamadas telefónicas que entraram pelo dial-in da organização
 *    (GET /api/orgs/{org}/voice/call-records, só admins), com número e duração.
 *
 * «Efectuadas» e «recebidas» entre contactos não aparecem porque não se
 * guardam; saídas PSTN não existem («Sem outbound» em `voice/README.md`).
 */
import { useMemo } from 'react'
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
}: {
  orgId: string
  isAdmin: boolean
  missed: MissedCall[]
  onCallBack: (m: MissedCall) => void
  /** Só as dos últimos N dias. */
  days?: number
  limit?: number
  compact?: boolean
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
      }}
    </AsyncSection>
  )
}
