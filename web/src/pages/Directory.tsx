import { FormEvent, useEffect, useMemo, useState } from 'react'
import {
  addEmployee,
  Branch,
  createBranch,
  createGroup,
  createMeetingRoom,
  createOrg,
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
} from '../api'
import { usePresence } from '../components/PresenceProvider'
import { CamIcon, CloseIcon, PeopleIcon, PlusIcon, TrashIcon, VoiceCallIcon } from '../icons'

type Tab = 'directory' | 'branches' | 'groups' | 'rooms'

export default function Directory() {
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

  if (loading) return <div className="page"><p className="muted">A carregar…</p></div>

  if (orgs.length === 0) {
    return (
      <div className="page">
        <div className="empty-state">
          <PeopleIcon />
          <p>Ainda não pertences a nenhuma organização.</p>
          <button className="btn-new" onClick={() => setShowCreateOrg(true)}>
            <PlusIcon /> Criar organização
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
          <PeopleIcon /> Organização
        </h1>
        <div className="org-bar">
          <select value={orgId} onChange={(e) => setOrgId(e.target.value)} className="org-select">
            {orgs.map((o) => (
              <option key={o.id} value={o.id}>
                {o.name} · {o.member_count} membros
              </option>
            ))}
          </select>
          <button className="btn-sm ghost" onClick={() => setShowCreateOrg(true)}>
            <PlusIcon /> Nova organização
          </button>
        </div>
        <div className="tabs">
          <button className={tab === 'directory' ? 'tab active' : 'tab'} onClick={() => setTab('directory')}>Diretório</button>
          <button className={tab === 'branches' ? 'tab active' : 'tab'} onClick={() => setTab('branches')}>Filiais</button>
          <button className={tab === 'groups' ? 'tab active' : 'tab'} onClick={() => setTab('groups')}>Grupos</button>
          <button className={tab === 'rooms' ? 'tab active' : 'tab'} onClick={() => setTab('rooms')}>Salas</button>
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
  const { isOnline, startCall } = usePresence()
  const [emps, setEmps] = useState<Employee[]>([])
  const [branches, setBranches] = useState<Branch[]>([])
  const [showAdd, setShowAdd] = useState(false)

  async function refresh() {
    setEmps(await listEmployees(org.id))
    setBranches(await listBranches(org.id).catch(() => []))
  }
  useEffect(() => {
    void refresh()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [org.id])

  return (
    <>
      {org.role === 'admin' && (
        <button className="btn-new small" onClick={() => setShowAdd(true)}>
          <PlusIcon /> Adicionar employee
        </button>
      )}
      <div className="emp-list">
        {emps.map((e) => (
          <div key={e.user_id} className="emp-row">
            <span className="avatar-wrap">
              <span className="avatar-circle small">{e.username.slice(0, 2).toUpperCase()}</span>
              <span className={isOnline(e.user_id) ? 'dot online' : 'dot'} />
            </span>
            <span className="emp-info">
              <strong>{e.username} {e.role === 'admin' && <span className="tag-admin">admin</span>}</strong>
              <small>{[e.title, e.branch_name, e.email].filter(Boolean).join(' · ')}</small>
            </span>
            <span className="emp-actions">
              <button className="call-btn voice" title="Chamada de voz" onClick={() => startCall({ targets: [e.user_id], kind: 'voice', title: `Chamada com ${e.username}` })}>
                <VoiceCallIcon />
              </button>
              <button className="call-btn video" title="Chamada de vídeo" onClick={() => startCall({ targets: [e.user_id], kind: 'video', title: `Chamada com ${e.username}` })}>
                <CamIcon />
              </button>
              {org.role === 'admin' && (
                <button className="icon-btn" title="Remover" onClick={() => void removeEmployee(org.id, e.user_id).then(refresh)}>
                  <TrashIcon />
                </button>
              )}
            </span>
          </div>
        ))}
      </div>
      {showAdd && (
        <AddEmployeeModal orgId={org.id} branches={branches} onClose={() => setShowAdd(false)} onAdded={() => { setShowAdd(false); void refresh() }} />
      )}
    </>
  )
}

function BranchesTab({ org }: { org: OrgSummary }) {
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
          <input placeholder="Nome da filial" value={name} onChange={(e) => setName(e.target.value)} />
          <input placeholder="Localização" value={location} onChange={(e) => setLocation(e.target.value)} />
          <button className="btn-sm" disabled={!name.trim()}><PlusIcon /> Adicionar</button>
        </form>
      )}
      <div className="branch-grid">
        {branches.map((b) => (
          <div key={b.id} className="branch-card">
            <strong>{b.name}</strong>
            <small>{b.location || 'sem localização'}</small>
          </div>
        ))}
        {branches.length === 0 && <p className="muted">Sem filiais.</p>}
      </div>
    </>
  )
}

function GroupsTab({ org }: { org: OrgSummary }) {
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
        <PlusIcon /> Criar grupo
      </button>
      <div className="group-grid">
        {groups.map((g) => (
          <div key={g.id} className="group-card">
            <div className="group-head">
              <strong>{g.name}</strong>
              <small>{g.member_count} membros</small>
            </div>
            <div className="group-actions">
              <button className="call-btn voice" title="Chamada de voz ao grupo" onClick={() => startCall({ groupId: g.id, kind: 'voice', title: g.name })}>
                <VoiceCallIcon />
              </button>
              <button className="call-btn video" title="Chamada de vídeo ao grupo" onClick={() => startCall({ groupId: g.id, kind: 'video', title: g.name })}>
                <CamIcon />
              </button>
            </div>
          </div>
        ))}
        {groups.length === 0 && <p className="muted">Sem grupos.</p>}
      </div>
      {showCreate && (
        <CreateGroupModal orgId={org.id} onClose={() => setShowCreate(false)} onCreated={() => { setShowCreate(false); void refresh() }} />
      )}
    </>
  )
}

function RoomsTab({ org }: { org: OrgSummary }) {
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
        Salas de reunião presenciais — usadas ao agendar para detetar dupla-marcação da mesma sala.
      </p>
      {org.role === 'admin' && (
        <form className="inline-form" onSubmit={add}>
          <input placeholder="Nome da sala" value={name} onChange={(e) => setName(e.target.value)} />
          <input placeholder="Localização" value={location} onChange={(e) => setLocation(e.target.value)} />
          <input placeholder="Lotação" type="number" min={0} value={capacity} onChange={(e) => setCapacity(e.target.value)} style={{ maxWidth: 110 }} />
          <button className="btn-sm" disabled={!name.trim()}><PlusIcon /> Adicionar</button>
        </form>
      )}
      <div className="branch-grid">
        {rooms.map((r) => (
          <div key={r.id} className="branch-card">
            <strong>🚪 {r.name}</strong>
            <small>{[r.location, r.capacity ? `${r.capacity} lugares` : ''].filter(Boolean).join(' · ') || 'sem detalhes'}</small>
          </div>
        ))}
        {rooms.length === 0 && <p className="muted">Sem salas presenciais.</p>}
      </div>
    </>
  )
}

// ---------- modais ----------

function CreateOrgModal({ onClose, onCreated }: { onClose: () => void; onCreated: (id: string) => void }) {
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
        <div className="modal-head"><h3>Criar organização</h3><button type="button" className="panel-close" onClick={onClose}><CloseIcon /></button></div>
        <input autoFocus placeholder="Nome da empresa" value={name} onChange={(e) => setName(e.target.value)} />
        {error && <div className="error">{error}</div>}
        <button className="primary" disabled={busy || !name.trim()}>{busy ? '…' : 'Criar'}</button>
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
        <div className="modal-head"><h3>Adicionar employee</h3><button type="button" className="panel-close" onClick={onClose}><CloseIcon /></button></div>
        <p className="muted small">Se o email já tiver conta, é ligado à organização. Senão, é criada uma conta com a password indicada.</p>
        <input autoFocus type="email" placeholder="Email" value={email} onChange={(e) => setEmail(e.target.value)} />
        <div className="field-row">
          <label>Nome<input placeholder="username" value={username} onChange={(e) => setUsername(e.target.value)} /></label>
          <label>Password inicial<input type="text" placeholder="mín. 8 (nova conta)" value={password} onChange={(e) => setPassword(e.target.value)} /></label>
        </div>
        <div className="field-row">
          <label>Cargo<input placeholder="ex.: Engenheiro" value={title} onChange={(e) => setTitle(e.target.value)} /></label>
          <label>Papel
            <select value={role} onChange={(e) => setRole(e.target.value)}>
              <option value="member">Membro</option>
              <option value="admin">Admin</option>
            </select>
          </label>
          <label>Filial
            <select value={branchId} onChange={(e) => setBranchId(e.target.value)}>
              <option value="">—</option>
              {branches.map((b) => <option key={b.id} value={b.id}>{b.name}</option>)}
            </select>
          </label>
        </div>
        {error && <div className="error">{error}</div>}
        <button className="primary" disabled={busy || !email.trim()}>{busy ? '…' : 'Adicionar'}</button>
      </form>
    </div>
  )
}

function CreateGroupModal({ orgId, onClose, onCreated }: { orgId: string; onClose: () => void; onCreated: () => void }) {
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
        <div className="modal-head"><h3>Criar grupo</h3><button type="button" className="panel-close" onClick={onClose}><CloseIcon /></button></div>
        <input autoFocus placeholder="Nome do grupo (ex.: Engenharia)" value={name} onChange={(e) => setName(e.target.value)} />
        <div className="pick-list">
          {list.map((e) => (
            <label key={e.user_id} className="pick-row">
              <input type="checkbox" checked={selected.has(e.user_id)} onChange={() => toggle(e.user_id)} />
              <span className="avatar-circle small">{e.username.slice(0, 2).toUpperCase()}</span>
              <span>{e.username} <small className="muted">{e.title}</small></span>
            </label>
          ))}
        </div>
        <button className="primary" disabled={busy || !name.trim()}>{busy ? '…' : `Criar grupo (${selected.size})`}</button>
      </form>
    </div>
  )
}
