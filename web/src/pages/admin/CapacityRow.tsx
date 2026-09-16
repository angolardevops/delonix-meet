/**
 * Capacidade da organização: pessoas, grupos e salas contra as quotas, e o
 * armazenamento de gravações. Os números vêm de `orgStats` e das listas; os
 * gráficos de uso e qualidade são da Análise, não daqui. Os «nós de media»
 * e «SIP» do template não têm endpoint — não aparecem.
 */
import { useTranslation } from 'react-i18next'
import type { OrgStats } from '../../api'
import { Async } from '../../components/AsyncSection'
import { Icon, IconName } from '../../ui/icons'
import { Meter, Skeleton } from '../../ui/kit'
import { formatBytes, useLocaleTag } from './orgShared'

function Stat({
  icon,
  label,
  value,
  row,
  sub,
  used,
  max,
}: {
  icon: IconName
  label: string
  value: string | null
  /** Rótulo da linha mono do valor (o «Sessões 41 / 80» do template). */
  row: string
  sub?: string | null
  used?: number
  max?: number | null
}) {
  const { t } = useTranslation()
  const pct = max != null && max > 0 && used != null ? Math.round((used / max) * 100) : null
  const warn = pct != null && pct >= 90
  return (
    <div className={warn ? 'org-stat org-stat--warn' : 'org-stat'}>
      <div className="org-stat__head">
        <Icon name={icon} size={12} />
        <span>{label}</span>
        {warn && <span className="org-stat__warn">{pct >= 100 ? t('org.quota.atingida') : t('org.quota.perto')}</span>}
      </div>
      <div className="org-stat__sub dx-muted dx-num">{sub ?? '\u00a0'}</div>
      {max != null && used != null && <Meter value={max === 0 ? 100 : (used / max) * 100} tone={warn ? 'live' : 'success'} />}
      <div className="org-stat__row dx-num">
        <span>{row}</span>
        <span className="org-stat__value">{value ?? <Skeleton h={10} w={40} />}</span>
      </div>
    </div>
  )
}

export default function CapacityRow({
  stats,
  groups,
  rooms,
  maxGroups,
  maxRooms,
}: {
  stats: Async<OrgStats>
  /** `null` enquanto carrega; `undefined` quando a lista falhou. */
  groups: number | null | undefined
  rooms: number | null | undefined
  maxGroups: number | null | undefined
  maxRooms: number | null | undefined
}) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const s = stats.s === 'ready' ? stats.d : null
  const failed = stats.s === 'error'
  const dash = failed ? '—' : null
  const of = (used: number | null | undefined, max: number | null | undefined) =>
    used === undefined ? '—' : used === null ? null : max == null ? String(used) : t('org.quota.usado', { usado: used, max })

  return (
    <div className="org-stats" role="group" aria-label={t('org.capacidade.titulo')}>
      <Stat
        icon="people"
        label={t('org.capacidade.pessoas')}
        row={t('consola.admin.total')}
        value={s ? String(s.members_total) : dash}
        sub={s ? t('org.capacidade.activas30', { count: s.active_users_30d }) : failed ? stats.msg : null}
      />
      <Stat
        icon="people"
        label={t('org.capacidade.grupos')}
        row={t('consola.admin.emUso')}
        value={of(groups, maxGroups)}
        used={groups ?? undefined}
        max={maxGroups}
        sub={maxGroups == null ? t('org.capacidade.semQuota') : null}
      />
      <Stat
        icon="door"
        label={t('org.capacidade.salas')}
        row={t('consola.admin.emUso')}
        value={of(rooms, maxRooms)}
        used={rooms ?? undefined}
        max={maxRooms}
        sub={maxRooms == null ? t('org.capacidade.semQuota') : null}
      />
      <Stat
        icon="film"
        label={t('org.capacidade.gravacoes')}
        row={t('consola.admin.volume')}
        value={s ? formatBytes(s.recordings_bytes, locale) : dash}
        sub={s ? t('org.capacidade.ficheiros', { count: s.recordings_total }) : null}
      />
    </div>
  )
}
