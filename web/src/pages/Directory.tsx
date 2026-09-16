/**
 * Contactos: o diretório da organização — pessoas com presença, grupos,
 * chamadas perdidas — e ligar por vídeo ou voz. A chamada acontece na sala
 * (o `PresenceProvider` leva quem liga para lá); este ecrã só escolhe a quem.
 *
 * Telefonia: o dial-in PSTN de ENTRADA existe (`server/src/voice.rs` +
 * `voice/`), e quem administra vê aqui as chamadas que entraram (CDR). O
 * teclado de marcação, transferir e DTMF do template não existem: não há
 * chamadas de saída («Sem outbound» em `voice/README.md`), nem mensagem de
 * transferência, e o DTMF depende da ponte FreeSWITCH↔SFU que ainda falta.
 */
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Employee, Group, listBranches, listEmployees, listGroups, listMeetingRooms } from '../api'
import { Async, AsyncSection, useAsync } from '../components/AsyncSection'
import PageBar from '../components/PageBar'
import { usePresence } from '../components/PresenceProvider'
import { useShell } from '../components/shellContext'
import { Button, Empty, IconButton, Select, Skeleton } from '../ui/kit'
import CreateGroupDialog from './admin/CreateGroupDialog'
import CreateOrgDialog from './admin/CreateOrgDialog'
import { refusalAware, useOrgSelection } from './admin/orgShared'
import { GroupDetail, PersonDetail } from './directory/ContactDetail'
import PstnHistory from './directory/PstnHistory'
import ContactList, { DirTab, Selection } from './directory/ContactList'
import OrgOverview from './directory/OrgOverview'
import '../ui/org.css'

export default function Directory() {
  const { t } = useTranslation()
  const shell = useShell()
  const { list, orgs, org, setOrgId } = useOrgSelection()
  const [creatingOrg, setCreatingOrg] = useState(false)

  return (
    <>
      <PageBar
        title={t('org.dir.titulo')}
        meta={org ? t('org.metaOrg', { nome: org.name, count: org.member_count }) : undefined}
      >
        {orgs.length > 1 && org && (
          <Select value={org.id} onChange={(e) => setOrgId(e.target.value)} aria-label={t('org.escolherOrg')} className="org-orgselect">
            {orgs.map((o) => (
              <option key={o.id} value={o.id}>
                {o.name}
              </option>
            ))}
          </Select>
        )}
      </PageBar>
      <AsyncSection state={list.state} onRetry={list.reload}>
        {() =>
          !org ? (
            <div className="page">
              <Empty
                icon="people"
                title={t('org.semOrg.titulo')}
                action={
                  <Button variant="primary" icon="plus" onClick={() => setCreatingOrg(true)}>
                    {t('org.novaOrg.titulo')}
                  </Button>
                }
              >
                {t('org.semOrg.texto')}
              </Empty>
            </div>
          ) : (
            <DirectoryBody key={org.id} orgId={org.id} orgName={org.name} isAdmin={org.role === 'admin'} meId={shell.user.id} />
          )
        }
      </AsyncSection>
      {creatingOrg && (
        <CreateOrgDialog
          onClose={() => setCreatingOrg(false)}
          onCreated={(o) => {
            setCreatingOrg(false)
            setOrgId(o.id)
            list.reload()
          }}
        />
      )}
    </>
  )
}

function Pending<T>({ state, onRetry }: { state: Async<T>; onRetry: () => void }) {
  if (state.s === 'loading') {
    return (
      <div className="org-dir__loading" aria-busy="true">
        <Skeleton h={32} />
        <Skeleton h={32} />
        <Skeleton h={32} />
      </div>
    )
  }
  return (
    <AsyncSection state={state} onRetry={onRetry}>
      {() => null}
    </AsyncSection>
  )
}

function DirectoryBody({ orgId, orgName, isAdmin, meId }: { orgId: string; orgName: string; isAdmin: boolean; meId: string }) {
  const { t } = useTranslation()
  const shell = useShell()
  const presence = usePresence()
  const people = useAsync(() => refusalAware(listEmployees(orgId), t), [orgId])
  const groups = useAsync(() => refusalAware(listGroups(orgId), t), [orgId])
  const places = useAsync(() => refusalAware(Promise.all([listBranches(orgId), listMeetingRooms(orgId)]), t), [orgId])
  const [tab, setTab] = useState<DirTab>('people')
  const [q, setQ] = useState('')
  const [branchFilter, setBranchFilter] = useState('')
  const [selection, setSelection] = useState<Selection>(null)
  const [creatingGroup, setCreatingGroup] = useState(false)

  const branches = places.state.s === 'ready' ? places.state.d[0] : []
  const allPeople = useMemo(() => (people.state.s === 'ready' ? people.state.d : []), [people.state])
  const allGroups = useMemo(() => (groups.state.s === 'ready' ? groups.state.d : []), [groups.state])
  const { isOnline, online } = presence

  const shownPeople = useMemo(() => {
    const term = q.trim().toLowerCase()
    return allPeople
      .filter((p) => !branchFilter || p.branch_id === branchFilter)
      .filter(
        (p) =>
          !term ||
          p.username.toLowerCase().includes(term) ||
          p.email.toLowerCase().includes(term) ||
          (p.title ?? '').toLowerCase().includes(term) ||
          (p.branch_name ?? '').toLowerCase().includes(term),
      )
      .sort((a, b) => Number(online.has(b.user_id)) - Number(online.has(a.user_id)) || a.username.localeCompare(b.username))
  }, [allPeople, q, branchFilter, online])

  const shownGroups = useMemo(() => {
    const term = q.trim().toLowerCase()
    return allGroups.filter((g) => !term || g.name.toLowerCase().includes(term))
  }, [allGroups, q])

  const onlineCount = useMemo(
    () => allPeople.filter((p) => p.user_id !== meId && online.has(p.user_id)).length,
    [allPeople, online, meId],
  )

  // Em ecrã estreito o detalhe abre por cima da lista; Esc fecha-o.
  useEffect(() => {
    if (!selection) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && window.matchMedia('(max-width: 760px)').matches) setSelection(null)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [selection])

  function callPerson(p: Employee, kind: 'video' | 'voice') {
    presence.startCall({ targets: [p.user_id], kind, title: t('org.dir.chamadaCom', { nome: p.username }) })
  }
  function callGroup(g: Group, kind: 'video' | 'voice') {
    presence.startCall({ groupId: g.id, kind, title: g.name })
  }

  const selPerson = selection?.kind === 'person' ? allPeople.find((p) => p.user_id === selection.id) : undefined
  const selGroup = selection?.kind === 'group' ? allGroups.find((g) => g.id === selection.id) : undefined

  let detail
  if (selPerson) {
    detail = (
      <PersonDetail
        person={selPerson}
        me={selPerson.user_id === meId}
        online={isOnline(selPerson.user_id)}
        onCall={(k) => callPerson(selPerson, k)}
      />
    )
  } else if (selGroup) {
    detail = <GroupDetail group={selGroup} onCall={(k) => callGroup(selGroup, k)} />
  } else {
    detail = (
      <AsyncSection state={places.state} onRetry={places.reload}>
        {([b, r]) => (
          <OrgOverview orgName={orgName} branches={b} rooms={r} people={allPeople} isAdmin={isAdmin} onManage={() => shell.navigate('admin')} />
        )}
      </AsyncSection>
    )
  }


  return (
    <div className="org-dir">
      <ContactList
        tab={tab}
        onTab={setTab}
        q={q}
        onQ={setQ}
        branchFilter={branchFilter}
        onBranchFilter={setBranchFilter}
        branches={branches}
        people={people.state.s === 'ready' ? shownPeople : null}
        groups={groups.state.s === 'ready' ? shownGroups : null}
        missed={presence.missed}
        meId={meId}
        isOnline={isOnline}
        onlineCount={onlineCount}
        selection={selection}
        onSelect={setSelection}
        onCallPerson={callPerson}
        onCallGroup={callGroup}
        onCallBack={presence.callBack}
        onAckMissed={presence.ackMissed}
        onNewGroup={() => setCreatingGroup(true)}
        phoneHistory={isAdmin ? <PstnHistory orgId={orgId} /> : undefined}
        pending={
          tab === 'groups' ? <Pending state={groups.state} onRetry={groups.reload} /> : <Pending state={people.state} onRetry={people.reload} />
        }
      />
      <section className={selection ? 'org-dir__detail org-dir__detail--open' : 'org-dir__detail'} aria-label={t('org.dir.detalhe')}>
        {selection && (
          <div className="org-dir__back">
            <IconButton icon="chevronLeft" label={t('ui.voltar')} onClick={() => setSelection(null)} />
          </div>
        )}
        {detail}
      </section>
      {creatingGroup && (
        <CreateGroupDialog
          orgId={orgId}
          people={allPeople}
          meId={meId}
          onClose={() => setCreatingGroup(false)}
          onCreated={() => {
            setCreatingGroup(false)
            groups.reload()
          }}
        />
      )}
    </div>
  )
}
