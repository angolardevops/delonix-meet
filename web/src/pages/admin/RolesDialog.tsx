/**
 * Gestão de papéis e permissões: lista de papéis, matriz de leitura
 * (concedido / herdado / negado), o painel de edição do papel seleccionado
 * (nome, papel-pai, capacidades — cada uma com "requer aprovação"), atribuição
 * a membros, exportação CSV, e os pedidos de aprovação pendentes.
 *
 * Âmbito por departamento, sincronização com grupos do Odoo e "simular
 * utilizador" não estão aqui — ver a nota em server/src/rbac.rs.
 */
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  apiErrorMessage,
  assignRole,
  createRole,
  decidePermissionRequest,
  deleteRole,
  downloadRolesCsv,
  duplicateRole,
  Employee,
  listEmployees,
  listPermissionRequests,
  listRoles,
  RbacPermissionInput,
  RbacRole,
  updateRole,
} from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Icon } from '../../ui/icons'
import { Alert, Button, Checkbox, Dialog, Select, StatusBadge, Tabs, TextInput } from '../../ui/kit'
import { formatDateTime, orgErrorMessage, refusalAware, useLocaleTag } from './orgShared'

type Tab = 'papeis' | 'pedidos'

export default function RolesDialog({ orgId, onClose }: { orgId: string; onClose: () => void }) {
  const { t } = useTranslation()
  const [tab, setTab] = useState<Tab>('papeis')
  const roles = useAsync((signal) => refusalAware(listRoles(orgId, signal), t), [orgId])
  const requests = useAsync((signal) => refusalAware(listPermissionRequests(orgId, signal), t), [orgId])
  const employees = useAsync(() => refusalAware(listEmployees(orgId), t), [orgId])
  const pendingCount = requests.state.s === 'ready' ? requests.state.d.filter((r) => r.status === 'pending').length : 0

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
              {(data) => (
                <RolesTab
                  orgId={orgId}
                  data={data}
                  employees={employees.state.s === 'ready' ? employees.state.d : []}
                  onChanged={roles.reload}
                />
              )}
            </AsyncSection>
          )}
          {tab === 'pedidos' && (
            <AsyncSection state={requests.state} onRetry={requests.reload}>
              {(list) => <RequestsTab orgId={orgId} requests={list} onChanged={requests.reload} />}
            </AsyncSection>
          )}
        </div>
      </div>
    </Dialog>
  )
}

/**
 * O catálogo de `server/src/rbac.rs::PERMISSIONS` devolve um rótulo fixo em
 * português — não há aí nenhum mecanismo de i18n (é código, não conteúdo).
 * Para as chaves conhecidas troca-se por uma tradução local; uma capacidade
 * nova, ainda sem entrada em `rbac.capacidadeRotulos`, cai no rótulo do
 * servidor em vez de desaparecer.
 */
const CAPABILITY_I18N_KEY: Record<string, string> = {
  'voice.manage': 'voiceManage',
  'sms.manage': 'smsManage',
  'streaming.manage': 'streamingManage',
}

function capabilityLabel(t: (key: string) => string, c: { key: string; label: string }): string {
  const leaf = CAPABILITY_I18N_KEY[c.key]
  return leaf ? t(`rbac.capacidadeRotulos.${leaf}`) : c.label
}

/** Igual a `capabilityLabel`, a partir da chave crua — para quando só se tem o `permission` de uma concessão. */
function capabilityLabelFor(t: (key: string) => string, catalog: { key: string; label: string }[], permission: string): string {
  const c = catalog.find((x) => x.key === permission)
  return c ? capabilityLabel(t, c) : permission
}

function glyphFor(role: RbacRole, permission: string): { text: string; title: string } {
  const direct = role.permissions.find((p) => p.permission === permission)
  if (direct) return { text: direct.requires_approval ? '✓⚑' : '✓', title: 'rbac.matrizConcedido' }
  const inherited = role.inherited.find((p) => p.permission === permission)
  if (inherited) return { text: inherited.requires_approval ? '⇡⚑' : '⇡', title: 'rbac.matrizHerdado' }
  return { text: '—', title: 'rbac.matrizNegado' }
}

function RolesTab({
  orgId,
  data,
  employees,
  onChanged,
}: {
  orgId: string
  data: { catalog: { key: string; label: string }[]; roles: RbacRole[] }
  employees: Employee[]
  onChanged: () => void
}) {
  const { t } = useTranslation()
  const [selectedId, setSelectedId] = useState<string | null>(data.roles[0]?.id ?? null)
  const [creating, setCreating] = useState(false)
  const selected = useMemo(() => data.roles.find((r) => r.id === selectedId) ?? null, [data.roles, selectedId])

  async function onDuplicate(roleId: string) {
    try {
      const dup = await duplicateRole(orgId, roleId)
      onChanged()
      setSelectedId(dup.id)
    } catch {
      /* o erro real aparece se a pessoa tentar de novo e falhar no painel */
    }
  }

  return (
    <div className="rbac-grid">
      <div className="rbac-col rbac-col--roles">
        <div className="rbac-col__head">
          <span className="dx-eyebrow">{t('rbac.papeis')}</span>
          <Button size="sm" variant="ghost" icon="plus" onClick={() => { setCreating(true); setSelectedId(null) }}>
            {t('rbac.novoPapel')}
          </Button>
        </div>
        <ul className="rbac-role-list">
          {data.roles.map((r) => (
            <li key={r.id}>
              <button
                type="button"
                className="rbac-role-item"
                aria-pressed={selectedId === r.id}
                onClick={() => { setSelectedId(r.id); setCreating(false) }}
              >
                <span className="rbac-role-item__name">
                  {r.is_system && <Icon name="lock" />}
                  {r.name}
                </span>
                <span className="dx-muted dx-num">{t('rbac.membros', { count: r.member_count })}</span>
              </button>
              <button type="button" className="dx-iconbtn dx-iconbtn--bare" title={t('rbac.duplicar')} onClick={() => onDuplicate(r.id)}>
                <span aria-hidden="true">⧉</span>
              </button>
            </li>
          ))}
        </ul>
        <Button size="sm" variant="ghost" icon="download" onClick={() => void downloadRolesCsv(orgId)}>
          {t('rbac.exportarCsv')}
        </Button>
      </div>

      <div className="rbac-col rbac-col--matrix">
        <span className="dx-eyebrow">{t('rbac.matriz')}</span>
        <p className="dx-muted" style={{ margin: '4px 0 8px', fontSize: 11 }}>{t('rbac.matrizLegenda')}</p>
        <div className="dx-table-wrap">
          <table className="dx-table rbac-matrix">
            <thead>
              <tr>
                <th scope="col">{t('rbac.capacidade')}</th>
                {data.roles.map((r) => (
                  <th scope="col" key={r.id} title={r.name}>
                    {r.name}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {data.catalog.map((c) => (
                <tr key={c.key}>
                  <td>{capabilityLabel(t, c)}</td>
                  {data.roles.map((r) => {
                    const g = glyphFor(r, c.key)
                    return (
                      <td key={r.id} className="dx-num" style={{ textAlign: 'center' }} title={t(g.title)}>
                        {g.text}
                      </td>
                    )
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      <div className="rbac-col rbac-col--detail">
        {creating ? (
          <RoleForm
            orgId={orgId}
            catalog={data.catalog}
            roles={data.roles}
            onSaved={(r) => { setCreating(false); setSelectedId(r.id); onChanged() }}
            onCancel={() => setCreating(false)}
          />
        ) : selected ? (
          <RoleDetail
            key={selected.id}
            orgId={orgId}
            role={selected}
            catalog={data.catalog}
            roles={data.roles}
            employees={employees}
            onChanged={onChanged}
            onDeleted={() => { setSelectedId(null); onChanged() }}
          />
        ) : (
          <p className="dx-muted">{t('rbac.escolhePapel')}</p>
        )}
      </div>
    </div>
  )
}

function permissionsMap(role: RbacRole): Map<string, boolean> {
  return new Map(role.permissions.map((p) => [p.permission, p.requires_approval]))
}

function RoleForm({
  orgId,
  catalog,
  roles,
  initial,
  onSaved,
  onCancel,
}: {
  orgId: string
  catalog: { key: string; label: string }[]
  roles: RbacRole[]
  initial?: RbacRole
  onSaved: (r: RbacRole) => void
  onCancel: () => void
}) {
  const { t } = useTranslation()
  const [name, setName] = useState(initial?.name ?? '')
  const [parentId, setParentId] = useState<string>(initial?.parent_role_id ?? '')
  const [grants, setGrants] = useState<Map<string, boolean>>(initial ? permissionsMap(initial) : new Map())
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  function toggle(key: string) {
    setGrants((prev) => {
      const next = new Map(prev)
      if (next.has(key)) next.delete(key)
      else next.set(key, false)
      return next
    })
  }
  function toggleApproval(key: string) {
    setGrants((prev) => {
      const next = new Map(prev)
      if (next.has(key)) next.set(key, !next.get(key))
      return next
    })
  }

  async function save() {
    if (!name.trim()) return
    setBusy(true)
    setErr('')
    const permissions: RbacPermissionInput[] = [...grants.entries()].map(([permission, requires_approval]) => ({
      permission,
      requires_approval,
    }))
    try {
      const r = initial
        ? await updateRole(orgId, initial.id, { name: name.trim(), parent_role_id: parentId || null, permissions })
        : await createRole(orgId, { name: name.trim(), parent_role_id: parentId || null, permissions })
      onSaved(r)
    } catch (e) {
      setErr(orgErrorMessage(e, t, 'ui.erroGenerico'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="rbac-form">
      <span className="dx-eyebrow">{initial ? t('rbac.editarPapel') : t('rbac.novoPapel')}</span>
      <label className="st-label" htmlFor="rbac-name">{t('rbac.nome')}</label>
      <TextInput id="rbac-name" value={name} onChange={(e) => setName(e.target.value)} maxLength={60} />

      <label className="st-label" htmlFor="rbac-parent">{t('rbac.papelPai')}</label>
      <Select id="rbac-parent" value={parentId} onChange={(e) => setParentId(e.target.value)}>
        <option value="">{t('rbac.semPai')}</option>
        {roles
          .filter((r) => r.id !== initial?.id)
          .map((r) => (
            <option key={r.id} value={r.id}>
              {r.name}
            </option>
          ))}
      </Select>
      <p className="dx-muted" style={{ fontSize: 11, margin: '2px 0 8px' }}>{t('rbac.papelPaiDica')}</p>

      <span className="dx-eyebrow">{t('rbac.capacidades')}</span>
      <div className="rbac-permlist">
        {catalog.map((c) => (
          <div key={c.key} className="rbac-permlist__row">
            <Checkbox label={capabilityLabel(t, c)} checked={grants.has(c.key)} onChange={() => toggle(c.key)} />
            {grants.has(c.key) && (
              <Checkbox
                label={t('rbac.requerAprovacao')}
                checked={grants.get(c.key) ?? false}
                onChange={() => toggleApproval(c.key)}
              />
            )}
          </div>
        ))}
      </div>

      {err && <Alert tone="danger">{err}</Alert>}
      <div style={{ display: 'flex', gap: 8 }}>
        <Button variant="secondary" onClick={onCancel}>{t('ui.cancelar')}</Button>
        <Button variant="primary" busy={busy} disabled={!name.trim()} onClick={save}>{t('ui.guardar')}</Button>
      </div>
    </div>
  )
}

function RoleDetail({
  orgId,
  role,
  catalog,
  roles,
  employees,
  onChanged,
  onDeleted,
}: {
  orgId: string
  role: RbacRole
  catalog: { key: string; label: string }[]
  roles: RbacRole[]
  employees: Employee[]
  onChanged: () => void
  onDeleted: () => void
}) {
  const { t } = useTranslation()
  const [editing, setEditing] = useState(false)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [assignee, setAssignee] = useState('')

  const members = employees.filter((e) => e.user_id && role.member_count > 0)

  async function remove() {
    if (!window.confirm(t('rbac.confirmarApagar', { nome: role.name }))) return
    setBusy(true)
    setErr('')
    try {
      await deleteRole(orgId, role.id)
      onDeleted()
    } catch (e) {
      setErr(orgErrorMessage(e, t, 'ui.erroGenerico'))
      setBusy(false)
    }
  }

  async function assign() {
    if (!assignee) return
    setBusy(true)
    setErr('')
    try {
      await assignRole(orgId, assignee, role.id)
      setAssignee('')
      onChanged()
    } catch (e) {
      setErr(orgErrorMessage(e, t, 'ui.erroGenerico'))
    } finally {
      setBusy(false)
    }
  }

  if (editing) {
    return (
      <RoleForm
        orgId={orgId}
        catalog={catalog}
        roles={roles}
        initial={role}
        onSaved={() => { setEditing(false); onChanged() }}
        onCancel={() => setEditing(false)}
      />
    )
  }

  return (
    <div className="rbac-detail">
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <strong style={{ fontSize: 15 }}>{role.name}</strong>
        {role.is_system && <StatusBadge tone="neutral">{t('rbac.papelSistema')}</StatusBadge>}
      </div>
      {role.parent_role_name && <p className="dx-muted" style={{ margin: 0 }}>{t('rbac.herdaDe', { nome: role.parent_role_name })}</p>}
      <p className="dx-muted" style={{ margin: 0 }}>{t('rbac.membros', { count: role.member_count })}</p>

      <span className="dx-eyebrow">{t('rbac.capacidades')}</span>
      {role.permissions.length === 0 && role.inherited.length === 0 ? (
        <p className="dx-muted" style={{ margin: 0 }}>{t('rbac.semCapacidades')}</p>
      ) : (
        <ul className="rbac-cap-list">
          {role.permissions.map((p) => (
            <li key={p.permission}>
              {capabilityLabelFor(t, catalog, p.permission)}
              {p.requires_approval && <span className="dx-muted"> · {t('rbac.requerAprovacao')}</span>}
            </li>
          ))}
          {role.inherited.map((p) => (
            <li key={p.permission} className="dx-muted">
              {capabilityLabelFor(t, catalog, p.permission)} · {t('rbac.herdado')}
              {p.requires_approval && ` · ${t('rbac.requerAprovacao')}`}
            </li>
          ))}
        </ul>
      )}

      {err && <Alert tone="danger">{err}</Alert>}
      {!role.is_system && (
        <div style={{ display: 'flex', gap: 8 }}>
          <Button variant="secondary" onClick={() => setEditing(true)}>{t('ui.editar')}</Button>
          <Button variant="danger" busy={busy} onClick={remove}>{t('rbac.apagar')}</Button>
        </div>
      )}

      <span className="dx-eyebrow">{t('rbac.atribuir')}</span>
      <div style={{ display: 'flex', gap: 8 }}>
        <Select value={assignee} onChange={(e) => setAssignee(e.target.value)} aria-label={t('rbac.atribuir')}>
          <option value="">{t('rbac.escolherMembro')}</option>
          {employees.map((e) => (
            <option key={e.user_id} value={e.user_id}>
              {e.username} ({e.email})
            </option>
          ))}
        </Select>
        <Button variant="secondary" size="sm" busy={busy} disabled={!assignee} onClick={assign}>
          {t('rbac.atribuirBotao')}
        </Button>
      </div>
      {members.length === 0 && role.member_count === 0 && (
        <p className="dx-muted" style={{ fontSize: 11 }}>{t('rbac.ningemAinda')}</p>
      )}
    </div>
  )
}

function RequestsTab({
  orgId,
  requests,
  onChanged,
}: {
  orgId: string
  requests: { id: string; requester_username: string; requester_email: string; permission: string; status: string; created_at: string; expires_at: string | null }[]
  onChanged: () => void
}) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const [busyId, setBusyId] = useState<string | null>(null)
  const [err, setErr] = useState('')

  async function decide(id: string, approve: boolean) {
    setBusyId(id)
    setErr('')
    try {
      await decidePermissionRequest(orgId, id, approve)
      onChanged()
    } catch (e) {
      setErr(apiErrorMessage(e, t('ui.erroGenerico')))
    } finally {
      setBusyId(null)
    }
  }

  if (requests.length === 0) {
    return <p className="dx-muted">{t('rbac.semPedidos')}</p>
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
      {err && <Alert tone="danger">{err}</Alert>}
      <div className="dx-table-wrap">
        <table className="dx-table">
          <thead>
            <tr>
              <th scope="col">{t('rbac.quem')}</th>
              <th scope="col">{t('rbac.capacidade')}</th>
              <th scope="col">{t('rbac.quando')}</th>
              <th scope="col">{t('rbac.estado')}</th>
              <th scope="col" />
            </tr>
          </thead>
          <tbody>
            {requests.map((r) => (
              <tr key={r.id}>
                <td>
                  {r.requester_username} <span className="dx-muted">({r.requester_email})</span>
                </td>
                <td className="dx-num">{r.permission}</td>
                <td className="dx-muted dx-num">{formatDateTime(r.created_at, locale)}</td>
                <td>
                  <StatusBadge tone={r.status === 'pending' ? 'warning' : r.status === 'approved' ? 'success' : 'neutral'}>
                    {t(`rbac.estados.${r.status}`)}
                  </StatusBadge>
                </td>
                <td>
                  {r.status === 'pending' && (
                    <div style={{ display: 'flex', gap: 6 }}>
                      <Button size="sm" variant="primary" busy={busyId === r.id} onClick={() => decide(r.id, true)}>
                        {t('rbac.aprovar')}
                      </Button>
                      <Button size="sm" variant="secondary" busy={busyId === r.id} onClick={() => decide(r.id, false)}>
                        {t('rbac.negar')}
                      </Button>
                    </div>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}
