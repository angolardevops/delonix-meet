/** Filiais, grupos e salas físicas: listas curtas com criação no próprio cartão. */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Branch, createBranch, createMeetingRoom, Employee, Group, MeetingRoom } from '../../api'
import { Async, AsyncSection } from '../../components/AsyncSection'
import { Alert, Button, Card, TextInput } from '../../ui/kit'
import CreateGroupDialog from './CreateGroupDialog'
import { orgErrorMessage } from './orgShared'

function QuotaEyebrow({ used, max }: { used: number; max: number | null | undefined }) {
  const { t } = useTranslation()
  return <>{max == null ? String(used) : t('org.quota.usado', { usado: used, max })}</>
}

export function BranchesCard({
  orgId,
  state,
  reload,
  people,
}: {
  orgId: string
  state: Async<Branch[]>
  reload: () => void
  people: Employee[]
}) {
  const { t } = useTranslation()
  const [name, setName] = useState('')
  const [location, setLocation] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!name.trim()) return
    setBusy(true)
    setErr('')
    try {
      await createBranch(orgId, name.trim(), location.trim())
      setName('')
      setLocation('')
      reload()
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'org.erro.criarFilial'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card title={t('org.filial.titulo')} eyebrow={state.s === 'ready' ? String(state.d.length) : undefined} flush>
      <AsyncSection state={state} onRetry={reload}>
        {(branches) =>
          branches.length === 0 ? (
            <p className="dx-muted org-card-note">{t('org.filial.nenhuma')}</p>
          ) : (
            <ul className="org-simple">
              {branches.map((b) => (
                <li key={b.id}>
                  <span className="org-simple__main">
                    <strong>{b.name}</strong>
                    <span className="dx-muted">{b.location || t('org.filial.semLocal')}</span>
                  </span>
                  <span className="dx-num dx-muted">
                    {t('org.pessoasContagem', { count: people.filter((p) => p.branch_id === b.id).length })}
                  </span>
                </li>
              ))}
            </ul>
          )
        }
      </AsyncSection>
      <form className="org-inline" onSubmit={submit} aria-label={t('org.filial.nova')}>
        <TextInput value={name} maxLength={120} onChange={(e) => setName(e.target.value)} placeholder={t('org.filial.nome')} aria-label={t('org.filial.nome')} />
        <TextInput value={location} onChange={(e) => setLocation(e.target.value)} placeholder={t('org.filial.local')} aria-label={t('org.filial.local')} />
        <Button type="submit" size="sm" icon="plus" busy={busy} disabled={!name.trim()}>
          {t('org.filial.criar')}
        </Button>
      </form>
      {err && (
        <div className="org-card-pad">
          <Alert tone="danger">{err}</Alert>
        </div>
      )}
    </Card>
  )
}

export function GroupsCard({
  orgId,
  meId,
  state,
  reload,
  people,
  maxGroups,
}: {
  orgId: string
  meId: string
  state: Async<Group[]>
  reload: () => void
  people: Employee[]
  maxGroups: number | null | undefined
}) {
  const { t } = useTranslation()
  const [creating, setCreating] = useState(false)
  return (
    <Card
      title={t('org.grupo.titulo')}
      eyebrow={state.s === 'ready' ? <QuotaEyebrow used={state.d.length} max={maxGroups} /> : undefined}
      flush
      actions={
        <Button size="sm" variant="ghost" icon="plus" onClick={() => setCreating(true)}>
          {t('org.grupo.novo')}
        </Button>
      }
    >
      <AsyncSection state={state} onRetry={reload}>
        {(groups) =>
          groups.length === 0 ? (
            <p className="dx-muted org-card-note">{t('org.dir.semGrupos')}</p>
          ) : (
            <ul className="org-simple">
              {groups.map((g) => (
                <li key={g.id}>
                  <span className="org-simple__main">
                    <strong>{g.name}</strong>
                  </span>
                  <span className="dx-num dx-muted">{t('org.membrosContagem', { count: g.member_count })}</span>
                </li>
              ))}
            </ul>
          )
        }
      </AsyncSection>
      {creating && (
        <CreateGroupDialog
          orgId={orgId}
          people={people}
          meId={meId}
          onClose={() => setCreating(false)}
          onCreated={() => {
            setCreating(false)
            reload()
          }}
        />
      )}
    </Card>
  )
}

export function RoomsCard({
  orgId,
  state,
  reload,
  maxRooms,
}: {
  orgId: string
  state: Async<MeetingRoom[]>
  reload: () => void
  maxRooms: number | null | undefined
}) {
  const { t } = useTranslation()
  const [name, setName] = useState('')
  const [location, setLocation] = useState('')
  const [capacity, setCapacity] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!name.trim()) return
    setBusy(true)
    setErr('')
    try {
      await createMeetingRoom(orgId, name.trim(), location.trim(), Math.max(0, Math.floor(Number(capacity) || 0)))
      setName('')
      setLocation('')
      setCapacity('')
      reload()
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'org.erro.criarSala'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card
      title={t('org.sala.titulo')}
      eyebrow={state.s === 'ready' ? <QuotaEyebrow used={state.d.length} max={maxRooms} /> : undefined}
      flush
    >
      <p className="dx-muted org-card-note">{t('org.sala.dica')}</p>
      <AsyncSection state={state} onRetry={reload}>
        {(rooms) =>
          rooms.length === 0 ? (
            <p className="dx-muted org-card-note">{t('org.sala.nenhuma')}</p>
          ) : (
            <ul className="org-simple">
              {rooms.map((r) => (
                <li key={r.id}>
                  <span className="org-simple__main">
                    <strong>{r.name}</strong>
                    <span className="dx-muted">{r.location || t('org.filial.semLocal')}</span>
                  </span>
                  <span className="dx-num dx-muted">{r.capacity ? t('org.sala.lugares', { count: r.capacity }) : '—'}</span>
                </li>
              ))}
            </ul>
          )
        }
      </AsyncSection>
      <form className="org-inline org-inline--rooms" onSubmit={submit} aria-label={t('org.sala.nova')}>
        <TextInput value={name} maxLength={120} onChange={(e) => setName(e.target.value)} placeholder={t('org.sala.nome')} aria-label={t('org.sala.nome')} />
        <TextInput value={location} onChange={(e) => setLocation(e.target.value)} placeholder={t('org.filial.local')} aria-label={t('org.filial.local')} />
        <TextInput
          type="number"
          inputMode="numeric"
          min={0}
          value={capacity}
          onChange={(e) => setCapacity(e.target.value)}
          placeholder={t('org.sala.capacidade')}
          aria-label={t('org.sala.capacidade')}
        />
        <Button type="submit" size="sm" icon="plus" busy={busy} disabled={!name.trim()}>
          {t('org.sala.criar')}
        </Button>
      </form>
      {err && (
        <div className="org-card-pad">
          <Alert tone="danger">{err}</Alert>
        </div>
      )}
    </Card>
  )
}
