import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { UsageChart } from '../components/UsageChart'
import {
  ApiKeyInfo,
  AuditEntry,
  listAudit,
  createApiKey,
  createWebhook,
  deleteSsoConfig,
  deleteWebhook,
  Employee,
  getSsoConfig,
  getOdooConfig,
  saveOdooConfig,
  rotateOdooToken,
  OdooConfig,
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
  getPlatformStorage,
  savePlatformStorage,
  testPlatformStorage,
  StorageConfig,
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

// ---------- Integração Odoo ----------

function OrgOdooIntegration({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  const [cfg, setCfg] = useState<OdooConfig | null>(null)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [newToken, setNewToken] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)
  const [err, setErr] = useState('')
  const [saved, setSaved] = useState(false)

  useEffect(() => {
    setLoading(true)
    getOdooConfig(orgId)
      .then(setCfg)
      .catch(() => setCfg(null))
      .finally(() => setLoading(false))
  }, [orgId])

  async function save() {
    if (!cfg) return
    setSaving(true)
    setErr('')
    try {
      await saveOdooConfig(orgId, {
        odoo_enabled: cfg.odoo_enabled,
        odoo_url: cfg.odoo_url,
        odoo_db: cfg.odoo_db,
        hide_org_creation: cfg.hide_org_creation,
        hide_sso_button: cfg.hide_sso_button,
      })
      setSaved(true)
      setTimeout(() => setSaved(false), 2000)
    } catch (e) {
      setErr((e as Error).message)
    } finally {
      setSaving(false)
    }
  }

  async function genToken() {
    try {
      const res = await rotateOdooToken(orgId)
      setNewToken(res.token)
      setCfg((c) => c ? { ...c, odoo_token_prefix: res.prefix } : c)
    } catch (e) {
      setErr((e as Error).message)
    }
  }

  function copyToken() {
    if (!newToken) return
    void navigator.clipboard.writeText(newToken)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  if (loading) return <p className="muted">{t('common.loading')}</p>

  return (
    <div className="odoo-panel">
      <p className="odoo-desc muted small">
        {t('admin.odooDesc', 'Integra o Delonix Meet com o Odoo via módulo nk_delonix_meet. O token de integração autentica o módulo Odoo para provisionar utilizadores e sincronizar reuniões.')}
      </p>

      <div className="field-row">
        <label className="field-label">{t('admin.odooEnabled', 'Integração Odoo')}</label>
        <label className="switch-wrap">
          <input
            type="checkbox"
            checked={cfg?.odoo_enabled ?? false}
            onChange={(e) => setCfg((c) => c ? { ...c, odoo_enabled: e.target.checked } : c)}
          />
          <span className="switch-track" />
        </label>
      </div>

      <div className="field-row">
        <label className="field-label">{t('admin.odooUrl', 'URL do Odoo')}</label>
        <input
          className="field-input"
          placeholder="http://localhost:8090"
          value={cfg?.odoo_url ?? ''}
          onChange={(e) => setCfg((c) => c ? { ...c, odoo_url: e.target.value || null } : c)}
        />
      </div>

      <div className="field-row">
        <label className="field-label">{t('admin.odooDB', 'Base de dados Odoo')}</label>
        <input
          className="field-input"
          placeholder="mycompany"
          value={cfg?.odoo_db ?? ''}
          onChange={(e) => setCfg((c) => c ? { ...c, odoo_db: e.target.value || null } : c)}
        />
      </div>

      {/* Token de integração */}
      <div className="odoo-token-section">
        <label className="field-label">{t('admin.odooToken', 'Token de integração')}</label>
        {cfg?.odoo_token_prefix ? (
          <div className="token-row">
            <code className="token-prefix">{cfg.odoo_token_prefix}…</code>
            <button className="btn-sm" onClick={() => void genToken()}>
              {t('admin.odooRotate', 'Rotar token')}
            </button>
          </div>
        ) : (
          <button className="btn-sm accent" onClick={() => void genToken()}>
            {t('admin.odooGenToken', 'Gerar token')}
          </button>
        )}
        {newToken && (
          <div className="token-reveal">
            <p className="muted small">{t('admin.odooTokenOnce', 'Guarda este token agora — não será mostrado novamente.')}</p>
            <div className="token-copy-row">
              <code className="token-full">{newToken}</code>
              <button className="btn-sm" onClick={copyToken}>
                {copied ? t('common.copied', '✓ Copiado') : t('common.copy', 'Copiar')}
              </button>
            </div>
          </div>
        )}
        {cfg?.odoo_synced_at && (
          <p className="muted small odoo-sync-at">
            {t('admin.odooLastSync', 'Último sync')}: {new Date(cfg.odoo_synced_at).toLocaleString('pt-PT')}
          </p>
        )}
      </div>

      <hr className="odoo-sep" />

      {/* Visibilidade da UI pública */}
      <p className="field-label bold">{t('admin.odooVisibility', 'Visibilidade da plataforma')}</p>

      <div className="field-row">
        <label className="field-label">{t('admin.hideOrgCreation', 'Ocultar "Criar organização"')}</label>
        <label className="switch-wrap">
          <input
            type="checkbox"
            checked={cfg?.hide_org_creation ?? false}
            onChange={(e) => setCfg((c) => c ? { ...c, hide_org_creation: e.target.checked } : c)}
          />
          <span className="switch-track" />
        </label>
      </div>
      <p className="muted small odoo-hint">{t('admin.hideOrgCreationHint', 'Remove o tab «Criar conta» da página de login. Útil quando todos os utilizadores são provisionados via Odoo.')}</p>

      <div className="field-row">
        <label className="field-label">{t('admin.hideSsoButton', 'Ocultar botão SSO')}</label>
        <label className="switch-wrap">
          <input
            type="checkbox"
            checked={cfg?.hide_sso_button ?? false}
            onChange={(e) => setCfg((c) => c ? { ...c, hide_sso_button: e.target.checked } : c)}
          />
          <span className="switch-track" />
        </label>
      </div>
      <p className="muted small odoo-hint">{t('admin.hideSsoButtonHint', 'Remove o botão «Entrar com SSO». Com integração Odoo activa, a autenticação é feita por email/senha (online/offline).')}</p>

      {err && <div className="error">{err}</div>}
      <button className="primary odoo-save" disabled={saving || !cfg} onClick={() => void save()}>
        {saved ? t('common.saved', '✓ Guardado') : saving ? '…' : t('common.save', 'Guardar')}
      </button>
    </div>
  )
}

// ---------- Armazenamento remoto (TrueNAS NFS / Nextcloud WebDAV) ----------

function PlatformStoragePanel() {
  const { t } = useTranslation()
  const [cfg, setCfg] = useState<StorageConfig | null>(null)
  const [type, setType] = useState<'local' | 'nfs' | 'webdav'>('local')
  const [nfsServer, setNfsServer] = useState('')
  const [nfsPath, setNfsPath] = useState('')
  const [wdUrl, setWdUrl] = useState('')
  const [wdUser, setWdUser] = useState('')
  const [wdPwd, setWdPwd] = useState('')
  const [wdPath, setWdPath] = useState('/remote.php/dav/files/{user}/Delonix')
  const [busy, setBusy] = useState(false)
  const [msg, setMsg] = useState('')
  const [testMsg, setTestMsg] = useState('')

  useEffect(() => {
    getPlatformStorage()
      .then((s) => {
        setCfg(s)
        setType(s.storage_type)
        setNfsServer(s.nfs_server ?? '')
        setNfsPath(s.nfs_path ?? '')
        setWdUrl(s.webdav_url ?? '')
        setWdUser(s.webdav_user ?? '')
        setWdPath(s.webdav_path)
      })
      .catch(() => {})
  }, [])

  async function save() {
    setBusy(true); setMsg('')
    try {
      await savePlatformStorage({
        storage_type: type,
        nfs_server: nfsServer || undefined,
        nfs_path: nfsPath || undefined,
        webdav_url: wdUrl || undefined,
        webdav_user: wdUser || undefined,
        webdav_password: wdPwd || undefined,
        webdav_path: wdPath || undefined,
      })
      setMsg(t('room.sala.configuracaoGuardada')); setWdPwd('')
      setCfg((c) => c ? { ...c, storage_type: type, webdav_password_set: !!(c.webdav_password_set || wdPwd) } : c)
    } catch (e) { setMsg(`Erro: ${(e as Error).message}`) } finally { setBusy(false) }
  }

  async function test() {
    setBusy(true); setTestMsg('')
    try {
      const r = await testPlatformStorage()
      setTestMsg(r.message)
    } catch (e) { setTestMsg(`Erro: ${(e as Error).message}`) } finally { setBusy(false) }
  }

  function downloadPvc() {
    const a = document.createElement('a')
    a.href = '/api/v1/platform/storage/pvc-manifest'
    a.download = 'delonix-recordings-pv.yaml'
    a.click()
  }

  return (
    <div className="odoo-panel">
      <p className="odoo-desc">
        Armazenamento para gravações e anexos. Por omissão as gravações ficam no volume local do pod.
        Configura aqui TrueNAS (NFS) ou Nextcloud/SharePoint (WebDAV) para persistência partilhada em multi-réplica.
      </p>

      <div className="field-row">
        <label className="field-label">{t('admin.tipoDeArmazenamento')}</label>
        <select value={type} onChange={(e) => setType(e.target.value as typeof type)} className="select-ctl">
          <option value="local">💽 Local (padrão)</option>
          <option value="nfs">🗄 TrueNAS / NFS</option>
          <option value="webdav">☁ Nextcloud / WebDAV</option>
        </select>
      </div>

      {type === 'nfs' && (
        <>
          <hr className="odoo-sep" />
          <p className="odoo-hint">O K8s cria um PersistentVolume com este servidor NFS. Descarrega o manifesto abaixo e aplica com <code>kubectl apply</code>.</p>
          <div className="field-row">
            <label className="field-label">Servidor NFS</label>
            <input value={nfsServer} onChange={(e) => setNfsServer(e.target.value)} placeholder="192.168.1.10" />
          </div>
          <div className="field-row">
            <label className="field-label">{t('admin.pathDeExportacao')}</label>
            <input value={nfsPath} onChange={(e) => setNfsPath(e.target.value)} placeholder="/mnt/pool/delonix" />
          </div>
          <button className="secondary" onClick={downloadPvc} type="button">⬇ Descarregar manifesto K8s PVC</button>
        </>
      )}

      {type === 'webdav' && (
        <>
          <hr className="odoo-sep" />
          <p className="odoo-hint">Gravações enviadas por WebDAV após processamento. Compatível com Nextcloud, ownCloud e SharePoint.</p>
          <div className="field-row">
            <label className="field-label">URL base WebDAV</label>
            <input value={wdUrl} onChange={(e) => setWdUrl(e.target.value)} placeholder="https://cloud.empresa.com" />
          </div>
          <div className="field-row">
            <label className="field-label">Utilizador</label>
            <input value={wdUser} onChange={(e) => setWdUser(e.target.value)} placeholder="delonix-service" />
          </div>
          <div className="field-row">
            <label className="field-label">Password {cfg?.webdav_password_set && <span className="odoo-hint">(definida — deixa em branco para manter)</span>}</label>
            <input type="password" value={wdPwd} onChange={(e) => setWdPwd(e.target.value)} placeholder={cfg?.webdav_password_set ? '••••••••' : 'nova password'} />
          </div>
          <div className="field-row">
            <label className="field-label">{t('admin.pathRemoto')}</label>
            <input value={wdPath} onChange={(e) => setWdPath(e.target.value)} placeholder="/remote.php/dav/files/{user}/Delonix" />
          </div>
        </>
      )}

      <div style={{ display: 'flex', gap: 8, marginTop: 12 }}>
        <button className="primary odoo-save" onClick={save} disabled={busy}>
          {busy ? '…' : '💾 Guardar'}
        </button>
        <button className="secondary" onClick={test} disabled={busy} type="button">
          🔌 Testar ligação
        </button>
      </div>
      {msg && <p className={msg.startsWith('✓') ? 'odoo-sync-at' : 'error'} style={{ marginTop: 8 }}>{msg}</p>}
      {testMsg && <p className="odoo-hint" style={{ marginTop: 6 }}>{testMsg}</p>}
    </div>
  )
}

type Period = 'week' | 'month' | 'quarter' | 'year'
const PERIODS: Period[] = ['week', 'month', 'quarter', 'year']

type IntegTab = 'settings' | 'sso' | 'webhooks' | 'apikeys' | 'odoo' | 'storage'

/** Consola de administração: KPIs reais da org + membros + postura + quarentena. */
export default function Analytics() {
  const { t, i18n } = useTranslation()
  const [period, setPeriod] = useState<Period>('month')
  const [orgs, setOrgs] = useState<OrgSummary[]>([])
  const [orgId, setOrgId] = useState<string>('')
  const [stats, setStats] = useState<OrgStats | null>(null)
  const [audit, setAudit] = useState<AuditEntry[]>([])
  const [members, setMembers] = useState<Employee[]>([])
  const [rows, setRows] = useState<QuarantineRow[]>([])
  const [loading, setLoading] = useState(true)
  const [ssoActive, setSsoActive] = useState(false)
  const [integTab, setIntegTab] = useState<IntegTab>('settings')

  const locale = i18n.language.startsWith('en') ? 'en-GB' : i18n.language.startsWith('fr') ? 'fr-FR' : 'pt-PT'

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
    void listAudit(orgId, 50).then(setAudit).catch(() => setAudit([]))
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
  // Última atividade relativa (fonte: audit_logs); acima de 30 dias mostra a data.
  const fmtAgo = (iso: string | null | undefined) => {
    if (!iso) return '—'
    const mins = Math.round((Date.now() - new Date(iso).getTime()) / 60000)
    const rtf = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' })
    if (mins < 60) return rtf.format(-Math.max(mins, 0), 'minute')
    if (mins < 60 * 24) return rtf.format(-Math.round(mins / 60), 'hour')
    if (mins < 60 * 24 * 30) return rtf.format(-Math.round(mins / 1440), 'day')
    return new Date(iso).toLocaleDateString(locale, { day: 'numeric', month: 'short', year: 'numeric' })
  }

  // Delta vs. período homólogo (30–60 dias atrás). Sem base de comparação → sem chip.
  const delta = (cur: number, prev: number): number | null =>
    prev > 0 ? Math.round(((cur - prev) / prev) * 100) : null

  // Qualidade média 0–5 ponderada pela distribuição real das amostras QoS
  // (boa=5, média=3.5, fraca=1.5). '—' sem amostras.
  const qMidPct = stats ? Math.max(0, 100 - stats.pct_good - stats.pct_poor) : 0
  // Preferir SEMPRE a pontuação MEDIDA (Delonix Call Quality Score, 0–100). O
  // 0–5 abaixo é um proxy DERIVADO da distribuição de perdas — foi o que havia
  // enquanto não se media mais nada, e continua a servir de recuo para
  // organizações cujos clientes ainda não reportam. Os dois são mostrados com
  // escalas diferentes de propósito: confundir um número medido com um número
  // inferido é como se perde a confiança num painel.
  const qMeasured = stats?.avg_score ?? null
  const qProxy = stats && stats.quality_samples_30d > 0
    ? ((stats.pct_good * 5 + qMidPct * 3.5 + stats.pct_poor * 1.5) / 100).toLocaleString(locale, { maximumFractionDigits: 1 })
    : null

  // 4 KPIs do template; os restantes indicadores vivem no cartão de uso.
  const kpis: { v: string; l: string; d?: number | null; tag?: string }[] = stats
    ? [
        { v: stats.meetings_30d.toLocaleString(locale), l: t('admin.kMeetings'), d: delta(stats.meetings_30d, stats.meetings_prev_30d) },
        { v: stats.meeting_minutes_30d.toLocaleString(locale), l: t('admin.kMinutes'), d: delta(stats.meeting_minutes_30d, stats.meeting_minutes_prev_30d) },
        { v: `${stats.active_users_30d} / ${stats.members_total}`, l: t('admin.kActive'), d: delta(stats.active_users_30d, stats.active_users_prev_30d) },
        {
          v: qMeasured != null ? `${qMeasured} / 100` : qProxy != null ? `${qProxy} / 5` : '—',
          l: t('admin.kQuality'),
          tag:
            qMeasured != null || qProxy != null
              ? stats.avg_loss_pct < 5
                ? t('admin.qStable')
                : t('admin.qUnstable')
              : undefined,
        },
      ]
    : []

  // Postura de segurança. O comentário que aqui estava — «SSO/SCIM/auditoria
  // são o stub do protótipo» — deixou de ser verdade e ficou a mentir ao
  // contrário: o SSO lê o estado REAL da org (`ssoActive`) e a auditoria é a
  // cadeia de hash verificável do `audit.rs`. Só o SCIM continua sem uma linha
  // de código, e é o único que aparece como «em breve».
  //
  // A regra desta lista, e a razão de não ser cosmética: uma capacidade só pode
  // dizer «Ativo» se houver caminho de código por trás. Um cliente lê isto como
  // uma garantia de conformidade.
  const posture: { l: string; v: string; on: boolean }[] = [
    { l: t('admin.secItems.tls'), v: t('admin.active'), on: true },
    { l: t('admin.secItems.e2ee'), v: t('admin.available'), on: true },
    { l: t('admin.secItems.waiting'), v: t('admin.available'), on: true },
    { l: t('admin.secItems.sso'), v: ssoActive ? t('admin.active') : t('admin.secSoon'), on: ssoActive },
    { l: t('admin.secItems.scim'), v: t('admin.secSoon'), on: false },
    { l: t('admin.secItems.audit'), v: t('admin.active'), on: true },
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
                {k.d != null && (
                  <span className={`kpi-delta ${k.d > 0 ? 'up' : k.d < 0 ? 'down' : ''}`}>
                    {k.d > 0 ? '+' : ''}{k.d}%
                  </span>
                )}
                {k.tag && <span className="kpi-delta up">{k.tag}</span>}
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
              <p className="muted small usage-substats">
                {t('admin.kAvgDur')}: {stats.avg_duration_min} min · {t('admin.kKinds')}: {stats.video_30d} / {stats.voice_30d}
                {' · '}{t('admin.kRecs')}: {stats.recordings_total} · {fmtGb(stats.recordings_bytes)}
              </p>
            </section>

            <section className="dash-card">
              <header className="dash-card-head">
                <h2>{t('admin.qualityTitle')}</h2>
              </header>
              {stats.quality_samples_30d === 0 ? (
                <p className="dash-empty">{t('admin.qualityEmpty')}</p>
              ) : (
                <>
                  {/* Distribuição Boa/Média/Fraca (amostras QoS reais; média = resto). */}
                  <div className="quality-bars">
                    {[
                      { l: t('admin.qGood'), v: stats.pct_good, c: 'good' },
                      { l: t('admin.qMid'), v: Math.max(0, 100 - stats.pct_good - stats.pct_poor), c: 'mid' },
                      { l: t('admin.qPoor'), v: stats.pct_poor, c: 'poor' },
                    ].map((b) => (
                      <div key={b.c} className="qbar-row">
                        <span className="qbar-label">{b.l}</span>
                        <span className="qbar-track">
                          <span className={`qbar-fill ${b.c}`} style={{ width: `${b.v}%` }} />
                        </span>
                        <span className="qbar-pct mono">{b.v}%</span>
                      </div>
                    ))}
                  </div>
                  <div className="quality-grid">
                    <div className="quality-stat">
                      <strong>{stats.avg_rtt_ms != null ? `${stats.avg_rtt_ms} ms` : '—'}</strong>
                      <small>{t('admin.qualityAvg')}</small>
                    </div>
                    <div className="quality-stat">
                      <strong>{stats.avg_loss_pct}%</strong>
                      <small>{t('admin.qualityLoss')}</small>
                    </div>
                    <div className="quality-stat good">
                      <strong>{stats.pct_good}%</strong>
                      <small>{t('admin.qualityGood')}</small>
                    </div>
                    <div className="quality-stat poor">
                      <strong>{stats.pct_poor}%</strong>
                      <small>{t('admin.qualityPoor')}</small>
                    </div>
                    {/* Só aparecem quando há mesmo amostras que os trazem: um
                        "0%" sem medição por trás é pior do que a ausência do
                        indicador — parece uma boa notícia e não é notícia
                        nenhuma. */}
                    {stats.avg_score != null && (
                      <div className="quality-stat">
                        <strong>{stats.avg_score} / 100</strong>
                        <small>Delonix Call Quality Score</small>
                      </div>
                    )}
                    {stats.pct_turn_relay != null && (
                      <div className="quality-stat">
                        <strong>{stats.pct_turn_relay}%</strong>
                        <small>media via TURN relay</small>
                      </div>
                    )}
                    {stats.pct_cpu_limited != null && (
                      <div className="quality-stat">
                        <strong>{stats.pct_cpu_limited}%</strong>
                        <small>limitado por CPU do cliente</small>
                      </div>
                    )}
                  </div>
                  <p className="muted small quality-note">
                    {t('admin.qualitySamples', { count: stats.quality_samples_30d })}
                  </p>
                </>
              )}
            </section>

            {/* Linha 2 do template: Membros (largo) | Postura de segurança. */}
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
                      <th>SSO</th>
                      <th>{t('admin.colActive')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {members.map((m) => (
                      <tr key={m.user_id}>
                        <td>
                          <span className="avatar-circle small">{m.username.slice(0, 2).toUpperCase()}</span>
                          <span className="member-name">
                            {m.username}
                            {(m.title || m.branch_name) && (
                              <small className="muted">{[m.title, m.branch_name].filter(Boolean).join(' · ')}</small>
                            )}
                          </span>
                        </td>
                        <td>
                          <span className={m.role === 'admin' ? 'posture-tag on' : 'posture-tag'}>
                            {m.role === 'admin' ? t('admin.roleAdmin') : t('admin.roleMember')}
                          </span>
                        </td>
                        {/* Estado por org (OIDC configurado); por-membro chega com o SCIM. */}
                        <td>{ssoActive ? <span className="posture-tag on">{t('admin.active')}</span> : <span className="muted">—</span>}</td>
                        <td className="muted">{fmtAgo(m.last_active)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </section>
            )}

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
                <h2>{t('admin.auditTitle')}</h2>
              </header>
              {audit.length === 0 && <p className="dash-empty">{t('admin.auditEmpty')}</p>}
              {audit.length > 0 && (
                <div className="audit-list">
                  {audit.map((a) => (
                    <div key={a.id} className="audit-row">
                      <span className="audit-action mono">{a.action}</span>
                      <span className="audit-detail">
                        <strong>{a.actor}</strong>
                        {a.target && <small> · {a.target}</small>}
                      </span>
                      <span className="audit-time">{new Date(a.created_at).toLocaleString('pt-PT')}</span>
                    </div>
                  ))}
                </div>
              )}
            </section>
          </div>

          {isAdmin && currentOrg && (
            <section className="dash-card integrations-card">
              <header className="dash-card-head">
                <h2>⚙️ {t('admin.integTitle', 'Integrações & Definições')}</h2>
              </header>
              <nav className="integ-tabs">
                {([
                  { key: 'settings', label: t('admin.settingsTitle', 'Definições') },
                  { key: 'odoo', label: '🔗 Odoo' },
                  { key: 'storage', label: '💾 Armazenamento' },
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
                {integTab === 'odoo' && <OrgOdooIntegration orgId={currentOrg.id} />}
                {integTab === 'storage' && <PlatformStoragePanel />}
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
