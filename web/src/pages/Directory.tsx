import { FormEvent, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import PasswordInput from '../components/PasswordInput'
import {
  addEmployee,
  Branch,
  createBranch,
  createGroup,
  createMeetingRoom,
  createOrg,
  currentUser,
  Employee,
  Group,
  listBranches,
  listEmployees,
  listGroups,
  listMeetingRooms,
  MeetingRoom,
  myOrgs,
  OrgSummary,
  removeEmployee,
  updateEmployee,
} from '../api'
import { usePresence } from '../components/PresenceProvider'
import { CamIcon, CloseIcon, DoorIcon, EditIcon, PeopleIcon, PlusIcon, TrashIcon, VoiceCallIcon } from '../icons'

type Tab = 'directory' | 'branches' | 'groups' | 'rooms'

export default function Directory() {
  const { t } = useTranslation()
  const [orgs, setOrgs] = useState<OrgSummary[]>([])
  const [orgId, setOrgId] = useState<string>('')
  const [tab, setTab] = useState<Tab>('directory')
  const [loading, setLoading] = useState(true)
  const [showCreateOrg, setShowCreateOrg] = useState(false)

  async function refreshOrgs(select?: string) {
    const list = await myOrgs()
    setOrgs(list)
    setOrgId((cur) => select ?? cur ?? list[0]?.id ?? '')
    if (!select && !orgId && list[0]) setOrgId(list[0].id)
    setLoading(false)
  }
  useEffect(() => {
    void refreshOrgs()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const org = orgs.find((o) => o.id === orgId)

  if (loading) return <div className="page"><p className="muted">{t('directory.loading')}</p></div>

  if (orgs.length === 0) {
    return (
      <div className="page">
        <div className="empty-state">
          <PeopleIcon />
          <p>{t('directory.noOrgs')}</p>
          <button className="btn-new" onClick={() => setShowCreateOrg(true)}>
            <PlusIcon /> {t('directory.createOrg')}
          </button>
        </div>
        {showCreateOrg && (
          <CreateOrgModal onClose={() => setShowCreateOrg(false)} onCreated={(id) => { setShowCreateOrg(false); void refreshOrgs(id) }} />
        )}
      </div>
    )
  }

  return (
    <div className="page">
      <header className="page-head">
        <h1>
          <PeopleIcon /> {t('directory.title')}
        </h1>
        <div className="org-bar">
          <select value={orgId} onChange={(e) => setOrgId(e.target.value)} className="org-select">
            {orgs.map((o) => (
              <option key={o.id} value={o.id}>
                {o.name} · {t('directory.membersCount', { count: o.member_count })}
              </option>
            ))}
          </select>
          <button className="btn-sm ghost" onClick={() => setShowCreateOrg(true)}>
            <PlusIcon /> {t('directory.newOrg')}
          </button>
        </div>
        <div className="tabs">
          <button className={tab === 'directory' ? 'tab active' : 'tab'} onClick={() => setTab('directory')}>{t('directory.tabDirectory')}</button>
          <button className={tab === 'branches' ? 'tab active' : 'tab'} onClick={() => setTab('branches')}>{t('directory.tabBranches')}</button>
          <button className={tab === 'groups' ? 'tab active' : 'tab'} onClick={() => setTab('groups')}>{t('directory.tabGroups')}</button>
          <button className={tab === 'rooms' ? 'tab active' : 'tab'} onClick={() => setTab('rooms')}>{t('directory.tabRooms')}</button>
        </div>
      </header>

      {org && tab === 'directory' && <DirectoryTab org={org} />}
      {org && tab === 'branches' && <BranchesTab org={org} />}
      {org && tab === 'groups' && <GroupsTab org={org} />}
      {org && tab === 'rooms' && <RoomsTab org={org} />}

      {showCreateOrg && (
        <CreateOrgModal onClose={() => setShowCreateOrg(false)} onCreated={(id) => { setShowCreateOrg(false); void refreshOrgs(id) }} />
      )}
    </div>
  )
}

function DirectoryTab({ org }: { org: OrgSummary }) {
  const { t } = useTranslation()
  const { isOnline, startCall } = usePresence()
  const [emps, setEmps] = useState<Employee[]>([])
  const [branches, setBranches] = useState<Branch[]>([])
  const [showAdd, setShowAdd] = useState(false)
  const [editingEmp, setEditingEmp] = useState<Employee | null>(null)

  const [search, setSearch] = useState('')
  const [roleFilter, setRoleFilter] = useState('all')
  const [page, setPage] = useState(1)
  const pageSize = 10

  const me = useMemo(() => currentUser(), [])

  async function refresh() {
    setEmps(await listEmployees(org.id))
    setBranches(await listBranches(org.id).catch(() => []))
  }
  useEffect(() => {
    void refresh()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [org.id])

  useEffect(() => {
    setPage(1)
  }, [search, roleFilter])

  const filtered = useMemo(() => {
    return emps.filter((e) => {
      if (roleFilter !== 'all' && e.role !== roleFilter) return false
      if (search) {
        const term = search.toLowerCase()
        return e.username.toLowerCase().includes(term) || 
               (e.email && e.email.toLowerCase().includes(term)) ||
               (e.title && e.title.toLowerCase().includes(term))
      }
      return true
    })
  }, [emps, search, roleFilter])

  const totalPages = Math.ceil(filtered.length / pageSize)
  const paginated = filtered.slice((page - 1) * pageSize, page * pageSize)

  return (
    <>
      <div className="filters-bar" style={{ display: 'flex', gap: '12px', marginBottom: '16px', alignItems: 'center', flexWrap: 'wrap' }}>
        <input
          type="search"
          placeholder={t('directory.searchMembers')}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          style={{ flex: 1, minWidth: '200px' }}
        />
        <select value={roleFilter} onChange={(e) => setRoleFilter(e.target.value)}>
          <option value="all">{t('directory.allRoles')}</option>
          <option value="admin">{t('directory.admins')}</option>
          <option value="member">{t('directory.members')}</option>
        </select>
        {org.role === 'admin' && (
          <button className="btn-new small" onClick={() => setShowAdd(true)}>
            <PlusIcon /> {t('directory.add')}
          </button>
        )}
      </div>
      <div className="emp-list">
        {paginated.map((e) => (
          <div key={e.user_id} className="emp-row">
            <span className="avatar-wrap">
              <span className="avatar-circle small">{e.username.slice(0, 2).toUpperCase()}</span>
              <span className={isOnline(e.user_id) ? 'dot online' : 'dot'} />
            </span>
            <span className="emp-info">
              <strong>{e.username} {e.role === 'admin' && <span className="tag-admin">{t('directory.tagAdmin')}</span>} {e.user_id === me?.id && <span className="tag-admin" style={{background: 'var(--accent-2)', color: '#000'}}>{t('directory.tagYou')}</span>}</strong>
              <small>{[e.title, e.branch_name, e.email].filter(Boolean).join(' · ')}</small>
            </span>
            <span className="emp-actions">
              {e.user_id !== me?.id && (
                <>
                  <button className="call-btn voice" title={t('directory.voiceCall')} onClick={() => startCall({ targets: [e.user_id], kind: 'voice', title: t('directory.callWith', { name: e.username }) })}>
                    <VoiceCallIcon />
                  </button>
                  <button className="call-btn video" title={t('directory.videoCall')} onClick={() => startCall({ targets: [e.user_id], kind: 'video', title: t('directory.callWith', { name: e.username }) })}>
                    <CamIcon />
                  </button>
                </>
              )}
              {org.role === 'admin' && e.user_id !== me?.id && (
                <>
                  <button className="icon-btn" title={t('directory.editMember')} onClick={() => setEditingEmp(e)}>
                    <EditIcon />
                  </button>
                  <button className="icon-btn" title={t('directory.removeMember')} onClick={() => void removeEmployee(org.id, e.user_id).then(refresh)}>
                    <TrashIcon />
                  </button>
                </>
              )}
            </span>
          </div>
        ))}
        {paginated.length === 0 && <p className="muted">{t('directory.noMembers')}</p>}
      </div>
      {totalPages > 1 && (
        <div className="pagination" style={{ display: 'flex', gap: '8px', marginTop: '16px', justifyContent: 'center', alignItems: 'center' }}>
          <button className="btn-sm ghost" disabled={page === 1} onClick={() => setPage(p => p - 1)}>{t('directory.prev')}</button>
          <span className="muted small">{t('directory.pageOf', { page, total: totalPages })}</span>
          <button className="btn-sm ghost" disabled={page === totalPages} onClick={() => setPage(p => p + 1)}>{t('directory.next')}</button>
        </div>
      )}
      {showAdd && (
        <AddEmployeeModal orgId={org.id} branches={branches} onClose={() => setShowAdd(false)} onAdded={() => { setShowAdd(false); void refresh() }} />
      )}
      {editingEmp && (
        <EditEmployeeModal orgId={org.id} employee={editingEmp} branches={branches} onClose={() => setEditingEmp(null)} onSaved={() => { setEditingEmp(null); void refresh() }} />
      )}
    </>
  )
}

function BranchesTab({ org }: { org: OrgSummary }) {
  const { t } = useTranslation()
  const [branches, setBranches] = useState<Branch[]>([])
  const [name, setName] = useState('')
  const [location, setLocation] = useState('')

  async function refresh() {
    setBranches(await listBranches(org.id))
  }
  useEffect(() => {
    void refresh()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [org.id])

  async function add(e: FormEvent) {
    e.preventDefault()
    if (!name.trim()) return
    await createBranch(org.id, name.trim(), location.trim())
    setName('')
    setLocation('')
    void refresh()
  }

  return (
    <>
      {org.role === 'admin' && (
        <form className="inline-form" onSubmit={add}>
          <input placeholder={t('directory.branchName')} value={name} onChange={(e) => setName(e.target.value)} />
          <input placeholder={t('directory.location')} value={location} onChange={(e) => setLocation(e.target.value)} />
          <button className="btn-sm" disabled={!name.trim()}><PlusIcon /> {t('directory.add')}</button>
        </form>
      )}
      <div className="branch-grid">
        {branches.map((b) => (
          <div key={b.id} className="branch-card">
            <strong>{b.name}</strong>
            <small>{b.location || t('directory.noLocation')}</small>
          </div>
        ))}
        {branches.length === 0 && <p className="muted">{t('directory.noBranches')}</p>}
      </div>
    </>
  )
}

function GroupsTab({ org }: { org: OrgSummary }) {
  const { t } = useTranslation()
  const { startCall } = usePresence()
  const [groups, setGroups] = useState<Group[]>([])
  const [showCreate, setShowCreate] = useState(false)

  async function refresh() {
    setGroups(await listGroups(org.id))
  }
  useEffect(() => {
    void refresh()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [org.id])

  return (
    <>
      <button className="btn-new small" onClick={() => setShowCreate(true)}>
        <PlusIcon /> {t('directory.createGroup')}
      </button>
      <div className="group-grid">
        {groups.map((g) => (
          <div key={g.id} className="group-card">
            <div className="group-head">
              <strong>{g.name}</strong>
              <small>{t('directory.membersCount', { count: g.member_count })}</small>
            </div>
            <div className="group-actions">
              <button className="call-btn voice" title={t('directory.groupVoiceCall')} onClick={() => startCall({ groupId: g.id, kind: 'voice', title: g.name })}>
                <VoiceCallIcon />
              </button>
              <button className="call-btn video" title={t('directory.groupVideoCall')} onClick={() => startCall({ groupId: g.id, kind: 'video', title: g.name })}>
                <CamIcon />
              </button>
            </div>
          </div>
        ))}
        {groups.length === 0 && <p className="muted">{t('directory.noGroups')}</p>}
      </div>
      {showCreate && (
        <CreateGroupModal orgId={org.id} onClose={() => setShowCreate(false)} onCreated={() => { setShowCreate(false); void refresh() }} />
      )}
    </>
  )
}

function RoomsTab({ org }: { org: OrgSummary }) {
  const { t } = useTranslation()
  const [rooms, setRooms] = useState<MeetingRoom[]>([])
  const [name, setName] = useState('')
  const [location, setLocation] = useState('')
  const [capacity, setCapacity] = useState('')

  async function refresh() {
    setRooms(await listMeetingRooms(org.id))
  }
  useEffect(() => {
    void refresh()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [org.id])

  async function add(e: FormEvent) {
    e.preventDefault()
    if (!name.trim()) return
    await createMeetingRoom(org.id, name.trim(), location.trim(), Number(capacity) || 0)
    setName('')
    setLocation('')
    setCapacity('')
    void refresh()
  }

  return (
    <>
      <p className="muted small" style={{ marginTop: '1rem' }}>
        {t('directory.roomsHint')}
      </p>
      {org.role === 'admin' && (
        <form className="inline-form" onSubmit={add}>
          <input placeholder={t('directory.roomName')} value={name} onChange={(e) => setName(e.target.value)} />
          <input placeholder={t('directory.location')} value={location} onChange={(e) => setLocation(e.target.value)} />
          <input placeholder={t('directory.capacity')} type="number" min={0} value={capacity} onChange={(e) => setCapacity(e.target.value)} style={{ maxWidth: 110 }} />
          <button className="btn-sm" disabled={!name.trim()}><PlusIcon /> {t('directory.add')}</button>
        </form>
      )}
      <div className="branch-grid">
        {rooms.map((r) => (
          <div key={r.id} className="branch-card">
            <strong><DoorIcon /> {r.name}</strong>
            <small>{[r.location, r.capacity ? t('directory.roomSeats', { count: r.capacity }) : ''].filter(Boolean).join(' · ') || t('directory.noRoomDetails')}</small>
          </div>
        ))}
        {rooms.length === 0 && <p className="muted">{t('directory.noRooms')}</p>}
      </div>
    </>
  )
}

// ---------- modais ----------

function CreateOrgModal({ onClose, onCreated }: { onClose: () => void; onCreated: (id: string) => void }) {
  const { t } = useTranslation()
  const [name, setName] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  async function submit(e: FormEvent) {
    e.preventDefault()
    setBusy(true)
    try {
      const o = await createOrg(name.trim())
      onCreated(o.id)
    } catch (err) {
      setError((err as Error).message)
      setBusy(false)
    }
  }
  return (
    <div className="modal-overlay" onClick={onClose}>
      <form className="modal" onClick={(e) => e.stopPropagation()} onSubmit={submit}>
        <div className="modal-head"><h3>{t('directory.createOrg')}</h3><button type="button" className="panel-close" onClick={onClose}><CloseIcon /></button></div>
        <input autoFocus placeholder={t('directory.companyName')} value={name} onChange={(e) => setName(e.target.value)} />
        {error && <div className="error">{error}</div>}
        <button className="primary" disabled={busy || !name.trim()}>{busy ? '…' : t('directory.create')}</button>
      </form>
    </div>
  )
}

function AddEmployeeModal({
  orgId,
  branches,
  onClose,
  onAdded,
}: {
  orgId: string
  branches: Branch[]
  onClose: () => void
  onAdded: () => void
}) {
  const { t } = useTranslation()
  const [email, setEmail] = useState('')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [title, setTitle] = useState('')
  const [role, setRole] = useState('member')
  const [branchId, setBranchId] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')

  async function submit(e: FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError('')
    try {
      await addEmployee(orgId, {
        email: email.trim(),
        username: username.trim() || undefined,
        password: password || undefined,
        title: title.trim(),
        role,
        branch_id: branchId || undefined,
      })
      onAdded()
    } catch (err) {
      setError((err as Error).message)
      setBusy(false)
    }
  }
  return (
    <div className="modal-overlay" onClick={onClose}>
      <form className="modal" onClick={(e) => e.stopPropagation()} onSubmit={submit}>
        <div className="modal-head"><h3>{t('directory.addEmployee')}</h3><button type="button" className="panel-close" onClick={onClose}><CloseIcon /></button></div>
        <p className="muted small">{t('directory.addEmployeeHint')}</p>
        <input autoFocus type="email" placeholder={t('login.email')} value={email} onChange={(e) => setEmail(e.target.value)} />
        <div className="field-row">
          <label>{t('directory.name')}<input placeholder={t('directory.usernamePh')} value={username} onChange={(e) => setUsername(e.target.value)} /></label>
          <label>{t('directory.initialPassword')}<PasswordInput placeholder={t('directory.initialPasswordPh')} value={password} onChange={setPassword} autoComplete="new-password" /></label>
        </div>
        <div className="field-row">
          <label>{t('directory.role')}<input placeholder={t('directory.rolePh')} value={title} onChange={(e) => setTitle(e.target.value)} /></label>
          <label>{t('directory.roleField')}
            <select value={role} onChange={(e) => setRole(e.target.value)}>
              <option value="member">{t('directory.roleMember')}</option>
              <option value="admin">{t('directory.roleAdmin')}</option>
            </select>
          </label>
          <label>{t('directory.branch')}
            <select value={branchId} onChange={(e) => setBranchId(e.target.value)}>
              <option value="">—</option>
              {branches.map((b) => <option key={b.id} value={b.id}>{b.name}</option>)}
            </select>
          </label>
        </div>
        {error && <div className="error">{error}</div>}
        <button className="primary" disabled={busy || !email.trim()}>{busy ? '…' : t('directory.add')}</button>
      </form>
    </div>
  )
}

function EditEmployeeModal({
  orgId,
  employee,
  branches,
  onClose,
  onSaved,
}: {
  orgId: string
  employee: Employee
  branches: Branch[]
  onClose: () => void
  onSaved: () => void
}) {
  const { t } = useTranslation()
  const [role, setRole] = useState(employee.role)
  const [title, setTitle] = useState(employee.title ?? '')
  const [branchId, setBranchId] = useState(employee.branch_id ?? '')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')

  async function submit(e: FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError('')
    try {
      await updateEmployee(orgId, employee.user_id, {
        role,
        title: title.trim() || undefined,
        branch_id: branchId || null,
      })
      onSaved()
    } catch (err) {
      setError((err as Error).message)
      setBusy(false)
    }
  }

  return (
    <div className="modal-overlay" onClick={onClose}>
      <form className="modal" onClick={(e) => e.stopPropagation()} onSubmit={submit}>
        <div className="modal-head">
          <h3>{t('directory.editMemberTitle', { name: employee.username })}</h3>
          <button type="button" className="panel-close" onClick={onClose}><CloseIcon /></button>
        </div>
        <div className="field-row">
          <label>{t('directory.role')}
            <input placeholder={t('directory.rolePh')} value={title} onChange={(e) => setTitle(e.target.value)} />
          </label>
          <label>{t('directory.roleField')}
            <select value={role} onChange={(e) => setRole(e.target.value as 'admin' | 'member')}>
              <option value="member">{t('directory.roleMember')}</option>
              <option value="admin">{t('directory.roleAdmin')}</option>
            </select>
          </label>
          <label>{t('directory.branch')}
            <select value={branchId} onChange={(e) => setBranchId(e.target.value)}>
              <option value="">—</option>
              {branches.map((b) => <option key={b.id} value={b.id}>{b.name}</option>)}
            </select>
          </label>
        </div>
        {error && <div className="error">{error}</div>}
        <button className="primary" disabled={busy}>{busy ? '…' : t('common.save')}</button>
      </form>
    </div>
  )
}

function CreateGroupModal({ orgId, onClose, onCreated }: { orgId: string; onClose: () => void; onCreated: () => void }) {
  const { t } = useTranslation()
  const [name, setName] = useState('')
  const [emps, setEmps] = useState<Employee[]>([])
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    void listEmployees(orgId).then(setEmps)
  }, [orgId])

  const toggle = (id: string) =>
    setSelected((s) => {
      const n = new Set(s)
      n.has(id) ? n.delete(id) : n.add(id)
      return n
    })

  async function submit(e: FormEvent) {
    e.preventDefault()
    setBusy(true)
    try {
      await createGroup(orgId, name.trim(), [...selected])
      onCreated()
    } finally {
      setBusy(false)
    }
  }
  const list = useMemo(() => emps, [emps])
  return (
    <div className="modal-overlay" onClick={onClose}>
      <form className="modal" onClick={(e) => e.stopPropagation()} onSubmit={submit}>
        <div className="modal-head"><h3>{t('directory.createGroup')}</h3><button type="button" className="panel-close" onClick={onClose}><CloseIcon /></button></div>
        <input autoFocus placeholder={t('directory.groupNamePh')} value={name} onChange={(e) => setName(e.target.value)} />
        <div className="pick-list">
          {list.map((e) => (
            <label key={e.user_id} className="pick-row">
              <input type="checkbox" checked={selected.has(e.user_id)} onChange={() => toggle(e.user_id)} />
              <span className="avatar-circle small">{e.username.slice(0, 2).toUpperCase()}</span>
              <span>{e.username} <small className="muted">{e.title}</small></span>
            </label>
          ))}
        </div>
        <button className="primary" disabled={busy || !name.trim()}>{busy ? '…' : t('directory.createGroupCount', { count: selected.size })}</button>
      </form>
    </div>
  )
}
