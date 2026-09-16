/**
 * Chamadas (DelonixCall): o diretório da organização na gramática ESCURA do
 * template — contactos à esquerda (pesquisa, separadores, presença), o
 * contacto em foco ao centro com as formas de lhe ligar, e à direita as
 * acções, o histórico de 7 dias e o dial-in da organização.
 *
 * A chamada em si acontece na sala (o `PresenceProvider` leva quem liga para
 * lá); este ecrã escolhe a quem e como. Por isso a área central mostra o
 * CONTACTO, não uma chamada a decorrer — um cronómetro ou «A GRAVAR» aqui
 * seriam inventados.
 *
 * Telefonia, medida no servidor:
 *  - o dial-in PSTN de ENTRADA existe (`server/src/voice.rs`): quem administra
 *    vê as chamadas que entraram (CDR) no histórico e os números no cartão
 *    «Dial-in»;
 *  - NÃO existem chamadas de saída («Sem outbound» em `voice/README.md`), por
 *    isso não há teclado nem «Ligar por PSTN»; nem mensagem de transferência
 *    (`presence.rs`), por isso não há «Transferir»; nem DTMF, que depende da
 *    ponte FreeSWITCH↔SFU. Nada disto aparece como botão inerte;
 *  - a presença só sabe online/offline: «em reunião» ou «ausente» não se
 *    mostram;
 *  - entre contactos só as chamadas PERDIDAS ficam registadas: o histórico
 *    não tem «efectuadas» nem «recebidas» com duração, fora o CDR de voz.
 */
import { ReactNode, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Employee, Group, listBranches, listEmployees, listGroups, listMeetingRooms } from '../api'
import { Async, AsyncSection, useAsync } from '../components/AsyncSection'
import PageBar from '../components/PageBar'
import { usePresence } from '../components/PresenceProvider'
import { useShell } from '../components/shellContext'
import { Button, Empty, Select, Skeleton } from '../ui/kit'
import CreateGroupDialog from './admin/CreateGroupDialog'
import CreateOrgDialog from './admin/CreateOrgDialog'
import { refusalAware, useOrgSelection } from './admin/orgShared'
import CallStage, { GroupStage, OrgStage } from './directory/CallStage'
import ContactList, { DirTab, Selection } from './directory/ContactList'
import '../ui/org.css'
import '../ui/call.css'

export default function Directory() {
  const { t } = useTranslation()
  const shell = useShell()
  const { list, orgs, org, setOrgId } = useOrgSelection()
  const [creatingOrg, setCreatingOrg] = useState(false)

  return (
    <div className="dx-stage call">
      <PageBar
        title={t('consola.chamadas.titulo')}
        meta={org ? t('org.metaOrg', { nome: org.name, count: org.member_count }) : undefined}
      />
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
            <DirectoryBody
              key={org.id}
              orgId={org.id}
              orgName={org.name}
              isAdmin={org.role === 'admin'}
              meId={shell.user.id}
              orgPicker={
                orgs.length > 1 ? (
                  <Select value={org.id} onChange={(e) => setOrgId(e.target.value)} aria-label={t('org.escolherOrg')}>
                    {orgs.map((o) => (
                      <option key={o.id} value={o.id}>
                        {o.name}
                      </option>
                    ))}
                  </Select>
                ) : null
              }
            />
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
    </div>
  )
}

export function Pending<T>({ state, onRetry }: { state: Async<T>; onRetry: () => void }) {
  if (state.s === 'loading') {
    return (
      <div className="call-loading" aria-busy="true">
        <Skeleton h={30} />
        <Skeleton h={30} />
        <Skeleton h={30} />
      </div>
    )
  }
  return (
    <AsyncSection state={state} onRetry={onRetry}>
      {() => null}
    </AsyncSection>
  )
}

function DirectoryBody({
  orgId,
  orgName,
  isAdmin,
  meId,
  orgPicker,
}: {
  orgId: string
  orgName: string
  isAdmin: boolean
  meId: string
  orgPicker: ReactNode
}) {
  const { t } = useTranslation()
  const shell = useShell()
  const presence = usePresence()
  const people = useAsync(() => refusalAware(listEmployees(orgId), t), [orgId])
  const groups = useAsync(() => refusalAware(listGroups(orgId), t), [orgId])
  const places = useAsync(() => refusalAware(Promise.all([listBranches(orgId), listMeetingRooms(orgId)]), t), [orgId])
  const [tab, setTab] = useState<DirTab>('people')
  const [q, setQ] = useState('')
  const [branchFilter, setBranchFilter] = useState('')
  // `#/directory?u=<id>` (pesquisa global) abre já essa pessoa. Só a escolha
  // EXPLÍCITA abre, em ecrã estreito, o detalhe por cima da lista.
  const [selection, setSelection] = useState<Selection>(() => {
    const i = location.hash.indexOf('?')
    const u = i < 0 ? null : new URLSearchParams(location.hash.slice(i + 1)).get('u')
    return u ? { kind: 'person', id: u } : null
  })
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
      .sort(
        (a, b) =>
          Number(online.has(b.user_id)) - Number(online.has(a.user_id)) || a.username.localeCompare(b.username),
      )
  }, [allPeople, q, branchFilter, online])

  const shownGroups = useMemo(() => {
    const term = q.trim().toLowerCase()
    return allGroups.filter((g) => !term || g.name.toLowerCase().includes(term))
  }, [allGroups, q])

  // Em ecrã estreito o detalhe abre por cima da lista; Esc fecha-o.
  useEffect(() => {
    if (!selection) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && window.matchMedia('(max-width: 900px)').matches) setSelection(null)
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

  // Sem escolha explícita, o centro mostra o primeiro contacto da lista (o
  // template abre com um contacto em foco) — nunca a própria pessoa.
  const firstOther = shownPeople.find((p) => p.user_id !== meId)
  const focus: Selection = selection ?? (firstOther ? { kind: 'person', id: firstOther.user_id } : null)
  const selPerson = focus?.kind === 'person' ? allPeople.find((p) => p.user_id === focus.id) : undefined
  const selGroup = focus?.kind === 'group' ? allGroups.find((g) => g.id === focus.id) : undefined

  const side = {
    orgId,
    isAdmin,
    missed: presence.missed,
    onCallBack: presence.callBack,
    onNewGroup: () => setCreatingGroup(true),
    onBack: selection ? () => setSelection(null) : undefined,
  }

  let main
  if (selPerson) {
    main = (
      <CallStage
        {...side}
        person={selPerson}
        me={selPerson.user_id === meId}
        online={isOnline(selPerson.user_id)}
        onCall={(k) => callPerson(selPerson, k)}
      />
    )
  } else if (selGroup) {
    main = <GroupStage {...side} group={selGroup} onCall={(k) => callGroup(selGroup, k)} />
  } else {
    main = (
      <OrgStage
        {...side}
        orgName={orgName}
        places={places.state}
        onRetry={places.reload}
        people={allPeople}
        onManage={isAdmin ? () => shell.navigate('admin') : undefined}
      />
    )
  }

  return (
    <div className="call-body">
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
        focus={focus}
        onSelect={setSelection}
        onCallPerson={callPerson}
        onCallGroup={callGroup}
        onCallBack={presence.callBack}
        onAckMissed={presence.ackMissed}
        onNewGroup={() => setCreatingGroup(true)}
        orgId={orgId}
        isAdmin={isAdmin}
        orgPicker={orgPicker}
        pending={
          tab === 'groups' ? <Pending state={groups.state} onRetry={groups.reload} /> : <Pending state={people.state} onRetry={people.reload} />
        }
      />
      <section className={selection ? 'call-main call-main--open' : 'call-main'} aria-label={t('org.dir.detalhe')}>
        {main}
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
