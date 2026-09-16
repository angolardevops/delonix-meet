/**
 * Administração da organização: pessoas, filiais, grupos, salas físicas,
 * domínio, retenção e quotas, auditoria e capacidade.
 *
 * O botão no rail só aparece a admins, mas isso não é autorização: quem decide
 * é o servidor. Um membro que chegue aqui por URL vê o aviso, e qualquer
 * recusa (401/403) de um pedido de admin aparece como recusa, não como avaria.
 *
 * A versão na barra é a que o servidor declara em `/api/status`; a tabela de
 * organizações tem só as colunas que `/api/orgs` devolve; o selo «imutável» da
 * auditoria é a verificação da cadeia de hashes; o cartão de voz é o plano de
 * controlo do dial-in (DIDs, facturação, CDR).
 *
 * Fica de fora por falta de backend: nós de media/SIP/gravação/transcodificação,
 * licenças e planos, armazenamento por tipo, residência de dados, retenção de
 * chat e da auditoria, exportar auditoria.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { listBranches, listEmployees, listGroups, listMeetingRooms, orgStats, serverStatus } from '../api'
import { AsyncSection, useAsync } from '../components/AsyncSection'
import PageBar from '../components/PageBar'
import { useShell } from '../components/shellContext'
import { Button, Empty, Select } from '../ui/kit'
import AuditCard from './admin/AuditCard'
import CapacityRow from './admin/CapacityRow'
import CreateOrgDialog from './admin/CreateOrgDialog'
import MembersCard from './admin/MembersCard'
import OrgsCard from './admin/OrgsCard'
import { refusalAware, useOrgSelection } from './admin/orgShared'
import SettingsCard from './admin/SettingsCard'
import { BranchesCard, GroupsCard, RoomsCard } from './admin/StructureCards'
import VoiceCard from './admin/VoiceCard'
import type { OrgSummary } from '../api'
import '../ui/org.css'

export default function Admin() {
  const { t } = useTranslation()
  const { list, orgs, org, setOrgId } = useOrgSelection()
  const [creatingOrg, setCreatingOrg] = useState(false)
  // Versão do servidor: pública, e um erro aqui não é motivo para avisar.
  const version = useAsync((signal) => serverStatus(signal).then((s) => s.version), [])
  const versionText = version.state.s === 'ready' ? t('consola.admin.versao', { v: version.state.d }) : null

  return (
    <>
      <PageBar
        title={t('org.admin.titulo')}
        meta={
          org || versionText ? (
            <span data-testid="admin-meta">
              {[org && t('org.metaOrg', { nome: org.name, count: org.member_count }), versionText].filter(Boolean).join(' · ')}
            </span>
          ) : undefined
        }
      >
        {orgs.length > 1 && org && (
          <Select value={org.id} onChange={(e) => setOrgId(e.target.value)} aria-label={t('org.escolherOrg')} className="org-orgselect">
            {orgs.map((o) => (
              <option key={o.id} value={o.id}>
                {o.role === 'admin' ? o.name : t('org.admin.orgSemAdmin', { nome: o.name })}
              </option>
            ))}
          </Select>
        )}
        <Button variant="primary" size="sm" icon="plus" aria-label={t('org.novaOrg.titulo')} onClick={() => setCreatingOrg(true)}>
          <span className="org-hide-narrow">{t('org.novaOrg.titulo')}</span>
        </Button>
      </PageBar>
      <AsyncSection state={list.state} onRetry={list.reload}>
        {() =>
          !org ? (
            <div className="page">
              <Empty icon="building" title={t('org.semOrg.titulo')}>
                {t('org.semOrg.texto')}
              </Empty>
            </div>
          ) : org.role !== 'admin' ? (
            <div className="page">
              <Empty icon="lock" title={t('org.admin.soAdmins')}>
                {t('org.admin.soAdminsTexto', { nome: org.name })}
              </Empty>
            </div>
          ) : (
            <AdminBody key={org.id} org={org} orgs={orgs} onSelectOrg={setOrgId} onOrgChanged={list.reload} />
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

function AdminBody({
  org,
  orgs,
  onSelectOrg,
  onOrgChanged,
}: {
  org: OrgSummary
  orgs: OrgSummary[]
  onSelectOrg: (id: string) => void
  onOrgChanged: () => void
}) {
  const { t } = useTranslation()
  const { user } = useShell()
  const people = useAsync(() => refusalAware(listEmployees(org.id), t), [org.id])
  const branches = useAsync(() => refusalAware(listBranches(org.id), t), [org.id])
  const groups = useAsync(() => refusalAware(listGroups(org.id), t), [org.id])
  const rooms = useAsync(() => refusalAware(listMeetingRooms(org.id), t), [org.id])
  const stats = useAsync(() => refusalAware(orgStats(org.id), t), [org.id])

  const peopleList = people.state.s === 'ready' ? people.state.d : []
  const branchList = branches.state.s === 'ready' ? branches.state.d : []
  const count = (s: typeof groups.state | typeof rooms.state) => (s.s === 'ready' ? s.d.length : s.s === 'loading' ? null : undefined)

  function reloadMembers() {
    people.reload()
    stats.reload()
    onOrgChanged()
  }

  return (
    <div className="page org-admin">
      <CapacityRow
        stats={stats.state}
        groups={count(groups.state)}
        rooms={count(rooms.state)}
        maxGroups={org.max_groups}
        maxRooms={org.max_rooms}
      />
      <div className="org-admin__grid">
        <div className="org-admin__main">
          <OrgsCard orgs={orgs} activeId={org.id} onSelect={onSelectOrg} />
          <MembersCard orgId={org.id} meId={user.id} state={people.state} reload={reloadMembers} branches={branchList} />
          <AuditCard orgId={org.id} />
          <VoiceCard orgId={org.id} />
        </div>
        <div className="org-admin__side">
          <SettingsCard org={org} onSaved={onOrgChanged} />
          <BranchesCard orgId={org.id} state={branches.state} reload={branches.reload} people={peopleList} />
          <GroupsCard orgId={org.id} meId={user.id} state={groups.state} reload={groups.reload} people={peopleList} maxGroups={org.max_groups} />
          <RoomsCard orgId={org.id} state={rooms.state} reload={rooms.reload} maxRooms={org.max_rooms} />
        </div>
      </div>
    </div>
  )
}
