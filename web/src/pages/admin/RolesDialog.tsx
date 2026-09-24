/**
 * Gestão de papéis e capacidades (ADR-0008): a lista de papéis, o editor do
 * papel seleccionado (nome, herança e o valor de cada capacidade), a matriz
 * de leitura, a exportação CSV, e os pedidos de aprovação pendentes.
 *
 * O servidor é a fonte de verdade. Uma capacidade que ele não impõe
 * (`enforced: false`) ou que está bloqueada (papel de sistema, só de sistema)
 * aparece desactivada — a UI nunca oferece o que o servidor recusa.
 */
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  CapabilityCatalog,
  CapabilityValue,
  ApprovalRequest,
  apiErrorMessage,
  capabilityCatalog,
  createOrgRole,
  decideApprovalRequest,
  deleteOrgRole,
  duplicateOrgRole,
  listApprovalRequests,
  listOrgRoles,
  OrgRole,
  permissionMatrix,
  putRoleCapabilities,
  roleCapabilities,
  updateOrgRole,
} from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Icon } from '../../ui/icons'
import { Alert, Button, Dialog, Select, Tabs, TextInput } from '../../ui/kit'
import { formatDateTime, refusalAware, useLocaleTag } from './orgShared'

type Tab = 'papeis' | 'pedidos'

export default function RolesDialog({ orgId, onClose }: { orgId: string; onClose: () => void }) {
  const { t } = useTranslation()
  const [tab, setTab] = useState<Tab>('papeis')
  const roles = useAsync((signal) => refusalAware(listOrgRoles(orgId, signal), t), [orgId])
  const catalog = useAsync(() => refusalAware(capabilityCatalog(), t), [])
  const requests = useAsync((signal) => refusalAware(listApprovalRequests(orgId, 'pending', signal), t), [orgId])
  const pendingCount = requests.state.s === 'ready' ? requests.state.d.items.length : 0

  return (
    <Dialog title={t('rbac.titulo')} onClose={onClose} wide>
      <div className="settings-drawer">
        <Tabs
          label={t('rbac.titulo')}
          value={tab}
          onChange={setTab}
          tabs={[
            { value: 'papeis', label: t('rbac.tabPapeis') },
            { value: 'pedidos', label: t('rbac.tabPedidos'), count: pendingCount },
          ]}
        />
        <div className="settings-body rbac-body">
          {tab === 'papeis' && (
            <AsyncSection state={roles.state} onRetry={roles.reload}>
              {(page) => (
                <AsyncSection state={catalog.state} onRetry={catalog.reload}>
                  {(cat) => <RolesTab orgId={orgId} roles={page.items} catalog={cat} onChanged={roles.reload} />}
                </AsyncSection>
              )}
            </AsyncSection>
          )}
          {tab === 'pedidos' && (
            <AsyncSection state={requests.state} onRetry={requests.reload}>
              {(page) => <RequestsTab orgId={orgId} requests={page.items} onChanged={requests.reload} />}
            </AsyncSection>
          )}
        </div>
      </div>
    </Dialog>
  )
}

function RolesTab({
  orgId,
  roles,
  catalog,
  onChanged,
}: {
  orgId: string
  roles: OrgRole[]
  catalog: CapabilityCatalog
  onChanged: () => void
}) {
  const { t } = useTranslation()
  const [selectedId, setSelectedId] = useState<string | null>(roles[0]?.id ?? null)
  const [creating, setCreating] = useState(false)
  const [err, setErr] = useState('')
  const selected = useMemo(() => roles.find((r) => r.id === selectedId) ?? null, [roles, selectedId])

  async function onDuplicate(roleId: string) {
    setErr('')
    try {
      const dup = await duplicateOrgRole(orgId, roleId)
      onChanged()
      setSelectedId(dup.id)
    } catch (e) {
      setErr(apiErrorMessage(e, t('rbac.erroGuardar')))
    }
  }

  async function exportCsv() {
    setErr('')
    try {
      const m = await permissionMatrix(orgId)
      const esc = (s: string) => `"${s.replace(/"/g, '""')}"`
      const head = [t('rbac.capacidade'), ...m.roles.map((r) => r.name)].map(esc).join(',')
      const rows = m.rows.map((r) => [r.label, ...m.roles.map((ro) => r.effective[ro.id] ?? r.values[ro.id] ?? '')].map(esc).join(','))
      const blob = new Blob([[head, ...rows].join('\n')], { type: 'text/csv;charset=utf-8' })
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = 'papeis-e-permissoes.csv'
      document.body.appendChild(a)
      a.click()
      a.remove()
      URL.revokeObjectURL(url)
    } catch (e) {
      setErr(apiErrorMessage(e, t('rbac.erroGuardar')))
    }
  }

  return (
    <div className="rbac-grid">
      <div className="rbac-col rbac-col--roles">
        <div className="rbac-col__head">
          <span className="dx-eyebrow">{t('rbac.papeis')}</span>
          <Button
            size="sm"
            variant="ghost"
            icon="plus"
            onClick={() => {
              setCreating(true)
              setSelectedId(null)
            }}
          >
            {t('rbac.novoPapel')}
          </Button>
        </div>
        <ul className="rbac-role-list">
          {roles.map((r) => (
            <li key={r.id}>
              <button
                type="button"
                className="rbac-role-item"
                aria-pressed={selectedId === r.id}
                onClick={() => {
                  setSelectedId(r.id)
                  setCreating(false)
                }}
              >
                <span className="rbac-role-item__name">
                  {r.system && <Icon name="lock" />}
                  {r.name}
                </span>
                <span className="dx-muted dx-num">{t('rbac.membros', { count: r.member_count })}</span>
              </button>
              <button type="button" className="dx-iconbtn dx-iconbtn--bare" title={t('rbac.duplicar')} onClick={() => void onDuplicate(r.id)}>
                <span aria-hidden="true">⧉</span>
              </button>
            </li>
          ))}
        </ul>
        <Button size="sm" variant="ghost" icon="download" onClick={() => void exportCsv()}>
          {t('rbac.exportarCsv')}
        </Button>
        {err && <Alert tone="danger">{err}</Alert>}
      </div>

      <div className="rbac-col rbac-col--detail" style={{ gridColumn: 'span 2' }}>
        {creating ? (
          <NewRole
            orgId={orgId}
            roles={roles}
            onSaved={(r) => {
              setCreating(false)
              setSelectedId(r.id)
              onChanged()
            }}
            onCancel={() => setCreating(false)}
          />
        ) : selected ? (
          <RoleEditor
            key={selected.id}
            orgId={orgId}
            role={selected}
            roles={roles}
            catalog={catalog}
            onChanged={onChanged}
            onDeleted={() => {
              setSelectedId(null)
              onChanged()
            }}
          />
        ) : (
          <p className="dx-muted">{t('rbac.escolhePapel')}</p>
        )}
      </div>
    </div>
  )
}

function NewRole({
  orgId,
  roles,
  onSaved,
  onCancel,
}: {
  orgId: string
  roles: OrgRole[]
  onSaved: (r: OrgRole) => void
  onCancel: () => void
}) {
  const { t } = useTranslation()
  const [name, setName] = useState('')
  const [parent, setParent] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  async function save() {
    if (!name.trim()) return
    setBusy(true)
    setErr('')
    try {
      onSaved(await createOrgRole(orgId, { name: name.trim(), inherits_from: parent || undefined }))
    } catch (e) {
      setErr(apiErrorMessage(e, t('rbac.erroGuardar')))
    } finally {
      setBusy(false)
    }
  }
  return (
    <div className="org-form">
      <span className="dx-eyebrow">{t('rbac.novoPapel')}</span>
      <label>
        <span>{t('rbac.nome')}</span>
        <TextInput value={name} maxLength={80} onChange={(e) => setName(e.target.value)} />
      </label>
      <label>
        <span>{t('rbac.papelPai')}</span>
        <Select value={parent} onChange={(e) => setParent(e.target.value)}>
          <option value="">{t('rbac.semPai')}</option>
          {roles.map((r) => (
            <option key={r.id} value={r.id}>
              {r.name}
            </option>
          ))}
        </Select>
      </label>
      <p className="dx-muted">{t('rbac.papelPaiDica')}</p>
      {err && <Alert tone="danger">{err}</Alert>}
      <div className="org-row-actions">
        <Button variant="ghost" onClick={onCancel}>
          {t('ui.cancelar')}
        </Button>
        <Button variant="primary" busy={busy} disabled={!name.trim()} onClick={() => void save()}>
          {t('rbac.criar')}
        </Button>
      </div>
    </div>
  )
}

function RoleEditor({
  orgId,
  role,
  roles,
  catalog,
  onChanged,
  onDeleted,
}: {
  orgId: string
  role: OrgRole
  roles: OrgRole[]
  catalog: CapabilityCatalog
  onChanged: () => void
  onDeleted: () => void
}) {
  const { t } = useTranslation()
  const caps = useAsync((signal) => refusalAware(roleCapabilities(orgId, role.id, signal), t), [orgId, role.id])
  const [name, setName] = useState(role.name)
  const [parent, setParent] = useState(role.inherits_from ?? '')
  const [edits, setEdits] = useState<Record<string, CapabilityValue>>({})
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [notice, setNotice] = useState('')
  const [confirmDelete, setConfirmDelete] = useState(false)
  const items = new Map(catalog.items.map((c) => [c.code, c]))
  const memberRole = roles.find((r) => r.key === 'member')

  async function save() {
    setBusy(true)
    setErr('')
    setNotice('')
    try {
      if (!role.system && (name.trim() !== role.name || parent !== (role.inherits_from ?? ''))) {
        await updateOrgRole(orgId, role.id, { name: name.trim(), inherits_from: parent || null })
      }
      if (Object.keys(edits).length > 0) {
        const res = await putRoleCapabilities(orgId, role.id, edits)
        if (res.warnings.length > 0) setNotice(t('rbac.avisoSod', { regras: res.warnings.map((w) => w.rule_name).join(', ') }))
      }
      setEdits({})
      caps.reload()
      onChanged()
    } catch (e) {
      setErr(apiErrorMessage(e, t('rbac.erroGuardar')))
    } finally {
      setBusy(false)
    }
  }

  async function remove() {
    setBusy(true)
    setErr('')
    try {
      await deleteOrgRole(orgId, role.id, role.member_count > 0 ? memberRole?.id : undefined)
      onDeleted()
    } catch (e) {
      setErr(apiErrorMessage(e, t('rbac.erroGuardar')))
      setBusy(false)
    }
  }

  return (
    <div className="org-form">
      <span className="dx-eyebrow">{role.system ? t('rbac.papelSistema') : t('rbac.editarPapel')}</span>
      <label>
        <span>{t('rbac.nome')}</span>
        <TextInput value={name} disabled={role.system} maxLength={80} onChange={(e) => setName(e.target.value)} />
      </label>
      {!role.system && (
        <label>
          <span>{t('rbac.papelPai')}</span>
          <Select value={parent} onChange={(e) => setParent(e.target.value)}>
            <option value="">{t('rbac.semPai')}</option>
            {roles
              .filter((r) => r.id !== role.id)
              .map((r) => (
                <option key={r.id} value={r.id}>
                  {r.name}
                </option>
              ))}
          </Select>
        </label>
      )}
      <AsyncSection state={caps.state} onRetry={caps.reload}>
        {(data) => (
          <div className="dx-table-wrap">
            <table className="dx-table rbac-matrix">
              <thead>
                <tr>
                  <th scope="col">{t('rbac.capacidade')}</th>
                  <th scope="col">{t('rbac.valor')}</th>
                </tr>
              </thead>
              <tbody>
                {data.items.map((row) => {
                  const info = items.get(row.capability)
                  const enforced = info?.enforced ?? false
                  const locked = role.system || row.locked || !enforced || (info?.system_only ?? false)
                  const value = edits[row.capability] ?? row.value
                  return (
                    <tr key={row.capability}>
                      <td>
                        <strong>{info?.label ?? row.capability}</strong>
                        <div className="dx-muted" style={{ fontSize: 11 }}>
                          {!enforced ? t('rbac.naoImposta') : (info?.hint ?? '')}
                        </div>
                      </td>
                      <td>
                        <Select
                          aria-label={info?.label ?? row.capability}
                          value={value}
                          disabled={locked}
                          onChange={(e) => setEdits((prev) => ({ ...prev, [row.capability]: e.target.value as CapabilityValue }))}
                        >
                          {catalog.values
                            .filter((v) => v !== 'requires_approval' || (info?.approval_supported ?? false))
                            .map((v) => (
                              <option key={v} value={v}>
                                {t(`rbac.valores.${v}`, { defaultValue: v })}
                              </option>
                            ))}
                        </Select>
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </div>
        )}
      </AsyncSection>
      {notice && <Alert tone="warning">{notice}</Alert>}
      {err && <Alert tone="danger">{err}</Alert>}
      <div className="org-row-actions">
        {!role.system && (
          <>
            {confirmDelete ? (
              <Button variant="danger" busy={busy} onClick={() => void remove()}>
                {t('rbac.confirmarApagar', { nome: role.name })}
              </Button>
            ) : (
              <Button variant="ghost" icon="trash" onClick={() => setConfirmDelete(true)}>
                {t('rbac.apagar')}
              </Button>
            )}
            <Button variant="primary" busy={busy} disabled={!name.trim()} onClick={() => void save()}>
              {t('rbac.guardar')}
            </Button>
          </>
        )}
      </div>
    </div>
  )
}

function RequestsTab({ orgId, requests, onChanged }: { orgId: string; requests: ApprovalRequest[]; onChanged: () => void }) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const [busy, setBusy] = useState<string | null>(null)
  const [err, setErr] = useState('')

  async function decide(r: ApprovalRequest, approve: boolean) {
    setBusy(r.id)
    setErr('')
    try {
      await decideApprovalRequest(orgId, r.id, approve)
      onChanged()
    } catch (e) {
      setErr(apiErrorMessage(e, t('rbac.erroGuardar')))
    } finally {
      setBusy(null)
    }
  }

  if (requests.length === 0) return <p className="dx-muted">{t('rbac.semPedidos')}</p>
  return (
    <div className="dx-table-wrap">
      {err && <Alert tone="danger">{err}</Alert>}
      <table className="dx-table">
        <thead>
          <tr>
            <th scope="col">{t('rbac.quem')}</th>
            <th scope="col">{t('rbac.capacidade')}</th>
            <th scope="col">{t('rbac.quando')}</th>
            <th scope="col">
              <span className="dx-sr-only">{t('org.coluna.accoes')}</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {requests.map((r) => (
            <tr key={r.id}>
              <td>{r.requester_name}</td>
              <td className="dx-num">{r.capability}</td>
              <td className="dx-num dx-muted">{formatDateTime(r.created_at, locale)}</td>
              <td>
                <span className="org-row-actions">
                  <Button size="sm" variant="secondary" disabled={busy === r.id} onClick={() => void decide(r, true)}>
                    {t('rbac.aprovar')}
                  </Button>
                  <Button size="sm" variant="ghost" disabled={busy === r.id} onClick={() => void decide(r, false)}>
                    {t('rbac.negar')}
                  </Button>
                </span>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
