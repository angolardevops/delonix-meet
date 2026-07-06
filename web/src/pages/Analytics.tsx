import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Employee,
  listEmployees,
  myOrgs,
  OrgStats,
  orgStats,
  OrgSummary,
  quarantineAnalytics,
  QuarantineRow,
} from '../api'
import { ClockIcon } from '../icons'

type Period = 'week' | 'month' | 'quarter' | 'year'
const PERIODS: Period[] = ['week', 'month', 'quarter', 'year']

/** Consola de administração: KPIs reais da org + membros + postura + quarentena. */
export default function Analytics() {
  const { t, i18n } = useTranslation()
  const [period, setPeriod] = useState<Period>('month')
  const [orgs, setOrgs] = useState<OrgSummary[]>([])
  const [orgId, setOrgId] = useState<string>('')
  const [stats, setStats] = useState<OrgStats | null>(null)
  const [members, setMembers] = useState<Employee[]>([])
  const [rows, setRows] = useState<QuarantineRow[]>([])
  const [loading, setLoading] = useState(true)

  const locale = i18n.language.startsWith('en') ? 'en-GB' : 'pt-PT'

  useEffect(() => {
    void myOrgs()
      .then((os) => {
        setOrgs(os)
        if (os.length > 0) setOrgId(os[0].id)
      })
      .catch(() => {})
  }, [])

  useEffect(() => {
    if (!orgId) {
      setStats(null)
      setMembers([])
      return
    }
    void orgStats(orgId).then(setStats).catch(() => setStats(null))
    void listEmployees(orgId).then(setMembers).catch(() => setMembers([]))
  }, [orgId])

  useEffect(() => {
    setLoading(true)
    quarantineAnalytics(period, orgId || undefined)
      .then(setRows)
      .catch(() => setRows([]))
      .finally(() => setLoading(false))
  }, [period, orgId])

  const max = Math.max(1, ...rows.map((r) => r.count))
  const weekMax = Math.max(1, ...(stats?.meetings_per_week.map((w) => w.count) ?? []))
  const weekMinMax = Math.max(1, ...(stats?.meetings_per_week.map((w) => w.minutes) ?? []))
  const fmtWeek = (iso: string) =>
    new Date(iso).toLocaleDateString(locale, { day: 'numeric', month: 'short' })
  const fmtGb = (b: number) =>
    b >= 1024 ** 3 ? `${(b / 1024 ** 3).toFixed(1)} GB` : `${(b / 1024 ** 2).toFixed(0)} MB`

  const kpis = stats
    ? [
        { v: stats.meetings_30d.toLocaleString(locale), l: t('admin.kMeetings') },
        { v: stats.meeting_minutes_30d.toLocaleString(locale), l: t('admin.kMinutes') },
        { v: `${stats.avg_duration_min} min`, l: t('admin.kAvgDur') },
        { v: `${stats.active_users_30d} / ${stats.members_total}`, l: t('admin.kActive') },
        { v: `${stats.video_30d} / ${stats.voice_30d}`, l: t('admin.kKinds') },
        { v: `${stats.recordings_total} · ${fmtGb(stats.recordings_bytes)}`, l: t('admin.kRecs') },
      ]
    : []

  // Postura de segurança: o que é real está "Ativo"; SSO/SCIM/auditoria são o stub do protótipo.
  const posture: { l: string; v: string; on: boolean }[] = [
    { l: t('admin.secItems.tls'), v: t('admin.active'), on: true },
    { l: t('admin.secItems.e2ee'), v: t('admin.available'), on: true },
    { l: t('admin.secItems.waiting'), v: t('admin.available'), on: true },
    { l: t('admin.secItems.sso'), v: t('admin.secSoon'), on: false },
    { l: t('admin.secItems.scim'), v: t('admin.secSoon'), on: false },
    { l: t('admin.secItems.audit'), v: t('admin.secSoon'), on: false },
  ]

  return (
    <div className="page">
      <header className="page-head">
        <p className="eyebrow">{t('admin.badge')}</p>
        <h1>{t('admin.title')}</h1>
        <p className="muted">{t('admin.sub')}</p>
        {orgs.length > 0 && (
          <div className="analytics-controls">
            <select className="org-select" value={orgId} onChange={(e) => setOrgId(e.target.value)}>
              {orgs.map((o) => (
                <option key={o.id} value={o.id}>{o.name}</option>
              ))}
              <option value="">{t('admin.allOrgs')}</option>
            </select>
          </div>
        )}
      </header>

      {orgs.length === 0 && <p className="muted">{t('admin.noOrg')}</p>}

      {stats && (
        <>
          <div className="kpi-grid">
            {kpis.map((k) => (
              <div key={k.l} className="kpi-card">
                <span className="kpi-v">{k.v}</span>
                <span className="kpi-l">{k.l}</span>
              </div>
            ))}
          </div>

          <div className="admin-grid">
            <section className="dash-card">
              <header className="dash-card-head">
                <h2>{t('admin.usageTitle')}</h2>
              </header>
              <div className="week-chart">
                {stats.meetings_per_week.length === 0 && (
                  <p className="dash-empty">—</p>
                )}
                {stats.meetings_per_week.map((w) => (
                  <div
                    key={w.week_start}
                    className="week-col"
                    title={`${fmtWeek(w.week_start)}: ${w.count} · ${w.minutes} min`}
                  >
                    <span className="week-count">{w.count}</span>
                    <span className="week-inner">
                      <span className="week-bar" style={{ height: `${(w.count / weekMax) * 100}%` }} />
                      <span
                        className="week-bar minutes"
                        style={{ height: `${(w.minutes / weekMinMax) * 100}%` }}
                      />
                    </span>
                    <span className="week-label">{fmtWeek(w.week_start)}</span>
                  </div>
                ))}
              </div>
              <div className="chart-legend">
                <span><i className="dot count" /> {t('admin.legMeetings')}</span>
                <span><i className="dot minutes" /> {t('admin.legMinutes')}</span>
              </div>
            </section>

            <section className="dash-card">
              <header className="dash-card-head">
                <h2>{t('admin.topOrganizers')}</h2>
              </header>
              {stats.top_organizers.length === 0 && <p className="dash-empty">—</p>}
              {stats.top_organizers.map((o, i) => (
                <div key={o.username} className="rank-row">
                  <span className="rank-pos">{i + 1}</span>
                  <span className="avatar-circle small">{o.username.slice(0, 2).toUpperCase()}</span>
                  <span className="rank-name">{o.username}</span>
                  <span className="rank-bar-wrap">
                    <span
                      className="rank-bar"
                      style={{ width: `${(o.count / Math.max(1, stats.top_organizers[0]?.count ?? 1)) * 100}%` }}
                    />
                  </span>
                  <span className="rank-count">{o.count}</span>
                </div>
              ))}
            </section>

            <section className="dash-card">
              <header className="dash-card-head">
                <h2>{t('admin.secTitle')}</h2>
              </header>
              {posture.map((p) => (
                <div key={p.l} className="posture-row">
                  <span>{p.l}</span>
                  <span className={p.on ? 'posture-tag on' : 'posture-tag'}>{p.v}</span>
                </div>
              ))}
            </section>
          </div>

          {members.length > 0 && (
            <section className="dash-card members-card">
              <header className="dash-card-head">
                <h2>{t('admin.membersTitle')}</h2>
                <span className="muted small">{t('admin.membersSub')}</span>
              </header>
              <table className="members-table">
                <thead>
                  <tr>
                    <th>{t('admin.colName')}</th>
                    <th>{t('admin.colRole')}</th>
                    <th>{t('admin.colTitle')}</th>
                    <th>{t('admin.colBranch')}</th>
                  </tr>
                </thead>
                <tbody>
                  {members.map((m) => (
                    <tr key={m.user_id}>
                      <td>
                        <span className="avatar-circle small">{m.username.slice(0, 2).toUpperCase()}</span>
                        {m.username}
                      </td>
                      <td>
                        <span className={m.role === 'admin' ? 'posture-tag on' : 'posture-tag'}>
                          {m.role === 'admin' ? t('admin.roleAdmin') : t('admin.roleMember')}
                        </span>
                      </td>
                      <td>{m.title || '—'}</td>
                      <td>{m.branch_name || '—'}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </section>
          )}
        </>
      )}

      <section className="dash-card quar-card">
        <header className="dash-card-head">
          <h2><ClockIcon /> {t('admin.quarTitle')}</h2>
          <div className="seg">
            {PERIODS.map((p) => (
              <button key={p} className={period === p ? 'seg-btn active' : 'seg-btn'} onClick={() => setPeriod(p)}>
                {t(`admin.periods.${p}`)}
              </button>
            ))}
          </div>
        </header>
        <p className="muted small">{t('admin.quarSub')}</p>

        {loading && <p className="muted">{t('common.loading')}</p>}
        {!loading && rows.length === 0 && <p className="dash-empty">{t('admin.quarEmpty')}</p>}
        {!loading && rows.length > 0 && (
          <div className="rank-list">
            {rows.map((r, i) => (
              <div key={r.user_id} className="rank-row">
                <span className="rank-pos">{i + 1}</span>
                <span className="avatar-circle small">{r.username.slice(0, 2).toUpperCase()}</span>
                <span className="rank-name">{r.username}</span>
                <span className="rank-bar-wrap">
                  <span className="rank-bar" style={{ width: `${(r.count / max) * 100}%` }} />
                </span>
                <span className="rank-count">{r.count}</span>
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  )
}
