import { useEffect, useState } from 'react'
import { myOrgs, OrgSummary, quarantineAnalytics, QuarantineRow } from '../api'
import { ClockIcon } from '../icons'

type Period = 'week' | 'month' | 'quarter' | 'year'
const PERIODS: { key: Period; label: string }[] = [
  { key: 'week', label: 'Semanal' },
  { key: 'month', label: 'Mensal' },
  { key: 'quarter', label: 'Trimestral' },
  { key: 'year', label: 'Anual' },
]

export default function Analytics() {
  const [period, setPeriod] = useState<Period>('month')
  const [orgs, setOrgs] = useState<OrgSummary[]>([])
  const [orgId, setOrgId] = useState<string>('')
  const [rows, setRows] = useState<QuarantineRow[]>([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    void myOrgs().then(setOrgs).catch(() => {})
  }, [])

  useEffect(() => {
    setLoading(true)
    quarantineAnalytics(period, orgId || undefined)
      .then(setRows)
      .catch(() => setRows([]))
      .finally(() => setLoading(false))
  }, [period, orgId])

  const max = Math.max(1, ...rows.map((r) => r.count))

  return (
    <div className="page">
      <header className="page-head">
        <h1><ClockIcon /> Análises · Quarentena de meet</h1>
        <p className="muted">
          Quem mais é posto em quarentena por não responder (aceitar ou recusar) a reuniões para que foi convidado.
        </p>
        <div className="analytics-controls">
          <div className="seg">
            {PERIODS.map((p) => (
              <button key={p.key} className={period === p.key ? 'seg-btn active' : 'seg-btn'} onClick={() => setPeriod(p.key)}>
                {p.label}
              </button>
            ))}
          </div>
          {orgs.length > 0 && (
            <select className="org-select" value={orgId} onChange={(e) => setOrgId(e.target.value)}>
              <option value="">Todas as organizações</option>
              {orgs.map((o) => (
                <option key={o.id} value={o.id}>{o.name}</option>
              ))}
            </select>
          )}
        </div>
      </header>

      {loading && <p className="muted">A carregar…</p>}
      {!loading && rows.length === 0 && (
        <div className="empty-state">
          <ClockIcon />
          <p>Ninguém em quarentena neste período. 🎉</p>
        </div>
      )}

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
    </div>
  )
}
