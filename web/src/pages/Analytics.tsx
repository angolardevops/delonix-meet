import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { UsageChart } from '../components/UsageChart'
import {
  ApiKeyInfo,
  createApiKey,
  createWebhook,
  deleteSsoConfig,
  deleteWebhook,
  Employee,
  getSsoConfig,
  listApiKeys,
  listEmployees,
  listWebhooks,
  myOrgs,
  OrgStats,
  orgStats,
  OrgSummary,
  quarantineAnalytics,
  QuarantineRow,
  revokeApiKey,
  saveSsoConfig,
  SsoConfig,
  updateOrgSettings,
  Webhook,
} from '../api'
import { ClockIcon } from '../icons'

/** Definições da organização (admin): domínio de produção + retenção. */
function OrgSettings({ org, onSaved }: { org: OrgSummary; onSaved: () => void }) {
  const { t } = useTranslation()
  const [domain, setDomain] = useState(org.domain ?? '')
  const [retention, setRetention] = useState(org.retention_days ?? 0)
  // Quotas: '' = ilimitado.
  const q = (v?: number | null) => (v == null || v < 0 ? '' : String(v))
  const [maxGroups, setMaxGroups] = useState(q(org.max_groups))
  const [maxRooms, setMaxRooms] = useState(q(org.max_rooms))
  const [maxMeetings, setMaxMeetings] = useState(q(org.max_meetings))
  const [saved, setSaved] = useState(false)
  const [err, setErr] = useState('')
  useEffect(() => {
    setDomain(org.domain ?? '')
    setRetention(org.retention_days ?? 0)
    setMaxGroups(q(org.max_groups))
    setMaxRooms(q(org.max_rooms))
    setMaxMeetings(q(org.max_meetings))
  }, [org.id, org.domain, org.retention_days, org.max_groups, org.max_rooms, org.max_meetings])
  async function save() {
    setErr('')
    const lim = (s: string) => (s.trim() === '' ? null : Math.max(0, Number(s)))
    try {
      await updateOrgSettings(org.id, domain, retention, {
        max_groups: lim(maxGroups),
        max_rooms: lim(maxRooms),
        max_meetings: lim(maxMeetings),
      })
      setSaved(true)
      setTimeout(() => setSaved(false), 2000)
      onSaved()
    } catch (e) {
      setErr((e as Error).message)
    }
  }
  return (
    <div className="integ-panel">
      <header className="dash-card-head">
        <h2>⚙️ {t('admin.settingsTitle')}</h2>
      </header>
      <label className="org-set-row">
        <span>
          <strong>{t('admin.domainLabel')}</strong>
          <small>{t('admin.domainSub')}</small>
        </span>
        <input placeholder="meet.acme.com" value={domain} onChange={(e) => setDomain(e.target.value)} />
      </label>
      <label className="org-set-row">
        <span>
          <strong>{t('admin.retentionLabel')}</strong>
          <small>{t('admin.retentionSub')}</small>
        </span>
        <input
          type="number"
          min={0}
          max={3650}
          value={retention}
          onChange={(e) => setRetention(Number(e.target.value))}
        />
      </label>
      <div className="org-quota-head"><strong>{t('admin.quotasLabel')}</strong><small>{t('admin.quotasSub')}</small></div>
      <div className="org-quota-grid">
        <label><span>{t('admin.quotaGroups')}</span>
          <input type="number" min={0} placeholder="∞" value={maxGroups} onChange={(e) => setMaxGroups(e.target.value)} /></label>
        <label><span>{t('admin.quotaRooms')}</span>
          <input type="number" min={0} placeholder="∞" value={maxRooms} onChange={(e) => setMaxRooms(e.target.value)} /></label>
        <label><span>{t('admin.quotaMeetings')}</span>
          <input type="number" min={0} placeholder="∞" value={maxMeetings} onChange={(e) => setMaxMeetings(e.target.value)} /></label>
      </div>
      {err && <div className="error">{err}</div>}
      <button className="btn-sm" onClick={() => void save()}>
        {saved ? '✓ ' + t('common.save') : t('common.save')}
      </button>
    </div>
  )
}

/** Chaves de API da organização (admin): integração REST /api/v1. */
function OrgApiKeys({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  const [keys, setKeys] = useState<ApiKeyInfo[]>([])
  const [name, setName] = useState('')
  const [fresh, setFresh] = useState<{ prefix: string; key: string } | null>(null)
  const [copied, setCopied] = useState(false)

  const refresh = () => void listApiKeys(orgId).then(setKeys).catch(() => setKeys([]))
  useEffect(refresh, [orgId])

  async function add() {
    const k = await createApiKey(orgId, name).catch(() => null)
    if (k) {
      setFresh({ prefix: k.prefix, key: k.key })
      setName('')
      refresh()
    }
  }

  return (
    <div className="integ-panel">
      <header className="dash-card-head">
        <h2>🔑 {t('admin.apiKeysTitle')}</h2>
        <a className="link small-link" href="#/api-docs" target="_blank" rel="noreferrer">{t('admin.apiDocsLink')}</a>
      </header>
      <p className="muted small">{t('admin.apiKeysSub')}</p>
      {fresh && (
        <div className="apikey-new">
          <code>{fresh.key}</code>
          <button
            className="icon-btn"
            title={t('common.save')}
            onClick={() => { void navigator.clipboard.writeText(fresh.key); setCopied(true); setTimeout(() => setCopied(false), 1500) }}
          >
            {copied ? '✓' : '⧉'}
          </button>
        </div>
      )}
      {fresh && <p className="muted small">{t('admin.apiKeyOnce')}</p>}
      {keys.length === 0 && !fresh && <p className="dash-empty">{t('admin.apiKeysEmpty')}</p>}
      {keys.map((k) => (
        <div key={k.id} className="apikey-row">
          <span className="apikey-prefix">{k.prefix}…</span>
          {k.name && <span className="muted small">{k.name}</span>}
          <span className="apikey-meta">
            {k.last_used_at ? t('admin.apiKeyUsed') : t('admin.apiKeyNever')}
          </span>
          <button className="icon-btn" title={t('common.delete')} onClick={() => void revokeApiKey(orgId, k.id).then(refresh)}>
            ✕
          </button>
        </div>
      ))}
      <div className="apikey-form">
        <input placeholder={t('admin.apiKeyName')} value={name} onChange={(e) => setName(e.target.value)} />
        <button className="btn-sm" onClick={() => void add()}>+ {t('admin.apiKeyCreate')}</button>
      </div>
    </div>
  )
}

/** SSO/OIDC da organização (admin). */
function OrgSso({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  const [cfg, setCfg] = useState<SsoConfig | null>(null)
  const [issuer, setIssuer] = useState('')
  const [clientId, setClientId] = useState('')
  const [clientSecret, setClientSecret] = useState('')
  const [enforce, setEnforce] = useState(false)
  const [editing, setEditing] = useState(false)
  const [err, setErr] = useState('')
  const [busy, setBusy] = useState(false)

  const load = () =>
    void getSsoConfig(orgId).then((c) => {
      setCfg(c)
      if (c) {
        setIssuer(c.issuer_url)
        setClientId(c.client_id)
        setEnforce(c.enforce_sso)
      }
    }).catch(() => {})

  useEffect(load, [orgId])

  async function save() {
    setErr('')
    setBusy(true)
    try {
      await saveSsoConfig(orgId, { issuer_url: issuer, client_id: clientId, client_secret: clientSecret, enforce_sso: enforce })
      setClientSecret('')
      setEditing(false)
      load()
    } catch (e) {
      setErr((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  async function remove() {
    if (!confirm(t('admin.ssoDeleteConfirm'))) return
    setBusy(true)
    try {
      await deleteSsoConfig(orgId)
      setCfg(null)
      setIssuer('')
      setClientId('')
      setEnforce(false)
      setEditing(false)
    } catch (e) {
      setErr((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="integ-panel">
      <header className="dash-card-head">
        <h2>🔐 {t('admin.ssoTitle')}</h2>
        <span className="muted small">{t('admin.ssoSub')}</span>
      </header>

      {cfg && !editing ? (
        <>
          <div className="posture-row">
            <span>{t('admin.ssoIssuer')}</span>
            <span className="mono small">{cfg.issuer_url}</span>
          </div>
          <div className="posture-row">
            <span>{t('admin.ssoClientId')}</span>
            <span className="mono small">{cfg.client_id}</span>
          </div>
          <div className="posture-row">
            <span>{t('admin.ssoEnforceLabel')}</span>
            <span className={cfg.enforce_sso ? 'posture-tag on' : 'posture-tag'}>
              {cfg.enforce_sso ? t('admin.ssoEnforceOn') : t('admin.ssoEnforceOff')}
            </span>
          </div>
          <div className="wh-form" style={{ marginTop: '0.75rem' }}>
            <button className="btn-sm" onClick={() => setEditing(true)}>{t('common.edit')}</button>
            <button className="btn-sm danger" disabled={busy} onClick={() => void remove()}>{t('common.delete')}</button>
          </div>
        </>
      ) : (
        <>
          {!cfg && !editing && (
            <p className="dash-empty">{t('admin.ssoEmpty')}</p>
          )}
          {(editing || !cfg) && (
            <div className="wh-form" style={{ flexDirection: 'column', gap: '0.5rem' }}>
              <input
                placeholder={t('admin.ssoIssuerPlaceholder')}
                value={issuer}
                onChange={(e) => setIssuer(e.target.value)}
              />
              <input
                placeholder={t('admin.ssoClientIdPlaceholder')}
                value={clientId}
                onChange={(e) => setClientId(e.target.value)}
              />
              <input
                type="password"
                placeholder={cfg ? t('admin.ssoSecretKeep') : t('admin.ssoSecretPlaceholder')}
                value={clientSecret}
                onChange={(e) => setClientSecret(e.target.value)}
                autoComplete="new-password"
              />
              <label className="check-row">
                <input type="checkbox" checked={enforce} onChange={(e) => setEnforce(e.target.checked)} />
                {t('admin.ssoEnforceLabel')}
              </label>
              {err && <div className="error">{err}</div>}
              <div style={{ display: 'flex', gap: '0.5rem' }}>
                <button
                  className="btn-sm"
                  disabled={busy || !issuer.trim() || !clientId.trim()}
                  onClick={() => void save()}
                >
                  {busy ? '…' : t('common.save')}
                </button>
                {editing && (
                  <button className="btn-sm" onClick={() => { setEditing(false); setErr('') }}>
                    {t('common.cancel')}
                  </button>
                )}
              </div>
            </div>
          )}
        </>
      )}
    </div>
  )
}

function OrgWebhooks({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  const WH_KINDS = [
    { k: 'slack', label: 'Slack' },
    { k: 'mattermost', label: 'Mattermost' },
    { k: 'teams', label: 'Microsoft Teams' },
    { k: 'generic', label: t('admin.webhookGeneric') },
  ]
  const [hooks, setHooks] = useState<Webhook[]>([])
  const [kind, setKind] = useState('slack')
  const [url, setUrl] = useState('')
  const [secret, setSecret] = useState('')
  const [err, setErr] = useState('')

  const refresh = () => void listWebhooks(orgId).then(setHooks).catch(() => setHooks([]))
  useEffect(refresh, [orgId])

  async function add() {
    setErr('')
    try {
      await createWebhook(orgId, { kind, url, secret: secret || undefined })
      setUrl('')
      setSecret('')
      refresh()
    } catch (e) {
      setErr((e as Error).message)
    }
  }

  return (
    <div className="integ-panel">
      <header className="dash-card-head">
        <h2>🔗 {t('admin.webhooksTitle')}</h2>
        <span className="muted small">{t('admin.webhooksSub')}</span>
      </header>
      {hooks.length === 0 && <p className="dash-empty">{t('admin.webhooksEmpty')}</p>}
      {hooks.map((h) => (
        <div key={h.id} className="wh-row">
          <span className="posture-tag on">{h.kind}</span>
          <span className="wh-url mono">{h.url}</span>
          <button className="icon-btn" title={t('common.delete')} onClick={() => void deleteWebhook(orgId, h.id).then(refresh)}>
            ✕
          </button>
        </div>
      ))}
      <div className="wh-form">
        <select className="dx-select" value={kind} onChange={(e) => setKind(e.target.value)}>
          {WH_KINDS.map((k) => (
            <option key={k.k} value={k.k}>{k.label}</option>
          ))}
        </select>
        <input placeholder="https://hooks.slack.com/…" value={url} onChange={(e) => setUrl(e.target.value)} />
        {kind === 'generic' && (
          <input placeholder={t('admin.webhookSecret')} value={secret} onChange={(e) => setSecret(e.target.value)} />
        )}
        <button className="btn-sm" disabled={!url.trim()} onClick={() => void add()}>
          + {t('admin.webhookAdd')}
        </button>
      </div>
      {err && <div className="error">{err}</div>}
    </div>
  )
}

type Period = 'week' | 'month' | 'quarter' | 'year'
const PERIODS: Period[] = ['week', 'month', 'quarter', 'year']

type IntegTab = 'settings' | 'sso' | 'webhooks' | 'apikeys'

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
  const [ssoActive, setSsoActive] = useState(false)
  const [integTab, setIntegTab] = useState<IntegTab>('settings')

  const locale = i18n.language.startsWith('en') ? 'en-GB' : 'pt-PT'

  const loadOrgs = () =>
    void myOrgs()
      .then((os) => {
        setOrgs(os)
        setOrgId((cur) => cur || (os.length > 0 ? os[0].id : ''))
      })
      .catch(() => {})
  useEffect(loadOrgs, [])

  const currentOrg = orgs.find((o) => o.id === orgId)
  const isAdmin = currentOrg?.role === 'admin'

  useEffect(() => {
    if (!orgId) {
      setStats(null)
      setMembers([])
      return
    }
    void orgStats(orgId).then(setStats).catch(() => setStats(null))
    void listEmployees(orgId).then(setMembers).catch(() => setMembers([]))
    void getSsoConfig(orgId).then((c) => setSsoActive(!!c)).catch(() => setSsoActive(false))
  }, [orgId])

  useEffect(() => {
    setLoading(true)
    quarantineAnalytics(period, orgId || undefined)
      .then(setRows)
      .catch(() => setRows([]))
      .finally(() => setLoading(false))
  }, [period, orgId])

  const max = Math.max(1, ...rows.map((r) => r.count))
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
    { l: t('admin.secItems.sso'), v: ssoActive ? t('admin.active') : t('admin.secSoon'), on: ssoActive },
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
              <UsageChart
                weeks={stats.meetings_per_week}
                fmtWeek={fmtWeek}
                labels={{ meetings: t('admin.legMeetings'), minutes: t('admin.legMinutes') }}
              />
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

          {isAdmin && currentOrg && (
            <section className="dash-card integrations-card">
              <header className="dash-card-head">
                <h2>⚙️ {t('admin.integTitle', 'Integrações & Definições')}</h2>
              </header>
              <nav className="integ-tabs">
                {([
                  { key: 'settings', label: t('admin.settingsTitle', 'Definições') },
                  { key: 'sso', label: t('admin.ssoTitle', 'SSO / OIDC') },
                  { key: 'webhooks', label: t('admin.webhooksTitle', 'Webhooks') },
                  { key: 'apikeys', label: t('admin.apiKeysTitle', 'API Keys') },
                ] as { key: IntegTab; label: string }[]).map((tb) => (
                  <button
                    key={tb.key}
                    className={integTab === tb.key ? 'integ-tab active' : 'integ-tab'}
                    onClick={() => setIntegTab(tb.key)}
                  >
                    {tb.label}
                  </button>
                ))}
              </nav>
              <div className="integ-body">
                {integTab === 'settings' && <OrgSettings org={currentOrg} onSaved={loadOrgs} />}
                {integTab === 'sso' && <OrgSso orgId={currentOrg.id} />}
                {integTab === 'webhooks' && <OrgWebhooks orgId={currentOrg.id} />}
                {integTab === 'apikeys' && <OrgApiKeys orgId={currentOrg.id} />}
              </div>
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
