/**
 * Pessoas da organização, em tabela, com papel, cargo, filial e acções.
 * Pesquisa, filtros, agrupar e página: o painel estilo Odoo (recurso
 * `members`; sem ele no servidor, a lista inteira filtrada no browser).
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { invitationLink, bulkUserAction, listDirectory, listInvitations, listOrgRoles, resendInvitation, revokeInvitation, updateEmployee } from '../../api'
import type { Branch, Employee, OrgInvitation } from '../../api'
import type { Async } from '../../components/AsyncSection'
import { useAsync } from '../../components/AsyncSection'
import { Alert, Avatar, Button, Card, Checkbox, IconButton, Select, Tag } from '../../ui/kit'
import { SearchBar, SearchResults } from '../../ui/search/SearchResults'
import { useResourceSearch } from '../../ui/search/useResourceSearch'
import CsvImportDialog from './CsvImportDialog'
import InviteDialog from './InviteDialog'
import { AddMemberDialog, EditMemberDialog, RemoveMemberDialog } from './MemberDialogs'
import { formatAgo, orgErrorMessage, refusalAware, useLocaleTag } from './orgShared'
import { membersFallback } from './search'

export default function MembersCard({
  orgId,
  meId,
  state,
  reload,
  branches,
}: {
  orgId: string
  meId: string
  state: Async<Employee[]>
  reload: () => void
  branches: Branch[]
}) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const rs = useResourceSearch<Employee>({ resource: 'members', orgId, ns: 'members.', fallback: membersFallback(orgId) })
  const invites = useAsync((signal) => refusalAware(listInvitations(orgId, 'pending', signal), t), [orgId])
  const suspended = useAsync((signal) => refusalAware(listDirectory(orgId, 'suspended', signal), t), [orgId])
  const roles = useAsync((signal) => refusalAware(listOrgRoles(orgId, signal), t), [orgId])
  const [adding, setAdding] = useState(false)
  const [inviting, setInviting] = useState(false)
  const [importingCsv, setImportingCsv] = useState(false)
  const [busyId, setBusyId] = useState<string | null>(null)
  const [editing, setEditing] = useState<Employee | null>(null)
  const [removing, setRemoving] = useState<Employee | null>(null)
  const [notice, setNotice] = useState('')
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [bulkBusy, setBulkBusy] = useState(false)
  const [bulkErr, setBulkErr] = useState('')

  const all = state.s === 'ready' ? state.d : []
  const pendingInvites: OrgInvitation[] = invites.state.s === 'ready' ? invites.state.d.items : []
  const suspendedList = suspended.state.s === 'ready' ? suspended.state.d.items.filter((e) => e.user_id) : []
  const activos = all.length
  const suspensos = suspendedList.length
  const reloadAll = () => {
    reload()
    rs.reload()
    invites.reload()
    suspended.reload()
  }

  // Suspender = arquivar com razão (ADR-0008 §7): sai da lista e passa a «suspensos».
  async function setSuspended(userId: string, name: string, suspend: boolean) {
    setBusyId(userId)
    setBulkErr('')
    try {
      const r = await bulkUserAction(orgId, { action: suspend ? 'suspend' : 'reactivate', user_ids: [userId] })
      const item = r.results[0]
      if (item && !item.ok) throw new Error(item.message ?? item.code ?? '')
      setNotice(suspend ? t('org.membro.suspenso', { nome: name }) : t('org.membro.reactivado', { nome: name }))
      reloadAll()
    } catch (e) {
      setBulkErr(orgErrorMessage(e, t, 'org.erro.guardar'))
    } finally {
      setBusyId(null)
    }
  }

  async function revoke(inv: OrgInvitation) {
    setBusyId(inv.id)
    setBulkErr('')
    try {
      await revokeInvitation(orgId, inv.id)
      invites.reload()
    } catch (e) {
      setBulkErr(orgErrorMessage(e, t, 'org.erro.guardar'))
    } finally {
      setBusyId(null)
    }
  }

  // Reenviar roda o token: o link anterior deixa de servir e o novo mostra-se uma vez.
  async function resend(inv: OrgInvitation) {
    setBusyId(inv.id)
    setBulkErr('')
    try {
      const r = await resendInvitation(orgId, inv.id)
      setNotice(t('rbac.linkNovo', { email: inv.email, link: invitationLink(r.token) }))
      invites.reload()
    } catch (e) {
      setBulkErr(orgErrorMessage(e, t, 'org.erro.guardar'))
    } finally {
      setBusyId(null)
    }
  }
  function toggleOne(id: string) {
    setSelected((s) => {
      const next = new Set(s)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }
  // Acções em massa: N pedidos PATCH ao mesmo endpoint que a edição de UMA
  // pessoa já usa — não existe rota de lote no servidor, e inventar uma só
  // para poupar N-1 pedidos não vale a complexidade. Cada falha isolada
  // aparece com o nome; as que resultaram ficam aplicadas (não é tudo-ou-nada).
  async function applyToSelected(data: { role?: string; branch_id?: string | null }) {
    setBulkBusy(true)
    setBulkErr('')
    const alvo = all.filter((m) => selected.has(m.user_id) && m.user_id !== meId)
    const falhas: string[] = []
    for (const m of alvo) {
      try {
        await updateEmployee(orgId, m.user_id, data)
      } catch (e) {
        falhas.push(`${m.username}: ${orgErrorMessage(e, t, 'org.erro.guardar')}`)
      }
    }
    setBulkBusy(false)
    setSelected(new Set())
    reloadAll()
    if (falhas.length > 0) setBulkErr(falhas.join(' · '))
    else setNotice(t('org.membro.massaAplicada', { count: alvo.length }))
  }

  return (
    <Card
      className="org-members"
      title={t('org.membro.titulo')}
      eyebrow={
        state.s === 'ready'
          ? t('org.membro.contagemDetalhe', { activos, convidados: pendingInvites.length, suspensos })
          : undefined
      }
      flush
      actions={
        <span className="org-row-actions">
          <Button variant="secondary" size="sm" icon="upload" onClick={() => setImportingCsv(true)}>
            {t('org.csv.abrir')}
          </Button>
          <Button variant="secondary" size="sm" icon="link" onClick={() => setInviting(true)}>
            {t('org.convidar.abrir')}
          </Button>
          <Button variant="primary" size="sm" icon="userPlus" onClick={() => setAdding(true)}>
            {t('org.membro.adicionar')}
          </Button>
        </span>
      }
    >
      <div className="org-card-pad org-search-pad">
        <SearchBar rs={rs} label={t('search.rotulos.members')} placeholder={t('org.dir.pesquisar')} />
      </div>
      {notice && (
        <div className="org-card-pad">
          <Alert tone="success">{notice}</Alert>
        </div>
      )}
      {bulkErr && (
        <div className="org-card-pad">
          <Alert tone="danger">{bulkErr}</Alert>
        </div>
      )}
      {selected.size > 0 && (
        <div className="org-card-pad org-bulkbar" role="group" aria-label={t('org.membro.massaTitulo')}>
          <span className="dx-num">{t('org.membro.massaContagem', { count: selected.size })}</span>
          <label className="org-bulkbar__field">
            <span className="dx-sr-only">{t('org.membro.massaPapel')}</span>
            <Select
              disabled={bulkBusy}
              defaultValue=""
              onChange={(e) => {
                const v = e.target.value
                if (v) void applyToSelected({ role: v })
                e.target.value = ''
              }}
            >
              <option value="" disabled>
                {t('org.membro.massaPapel')}
              </option>
              <option value="admin">{t('org.papel.admin')}</option>
              <option value="member">{t('org.papel.membro')}</option>
            </Select>
          </label>
          {branches.length > 0 && (
            <label className="org-bulkbar__field">
              <span className="dx-sr-only">{t('org.membro.massaFilial')}</span>
              <Select
                disabled={bulkBusy}
                defaultValue=""
                onChange={(e) => {
                  const v = e.target.value
                  if (v) void applyToSelected({ branch_id: v === '_sem_' ? null : v })
                  e.target.value = ''
                }}
              >
                <option value="" disabled>
                  {t('org.membro.massaFilial')}
                </option>
                <option value="_sem_">{t('org.membro.semFilial')}</option>
                {branches.map((b) => (
                  <option key={b.id} value={b.id}>
                    {b.name}
                  </option>
                ))}
              </Select>
            </label>
          )}
          <Button variant="ghost" size="sm" disabled={bulkBusy} onClick={() => setSelected(new Set())}>
            {t('org.membro.massaLimpar')}
          </Button>
        </div>
      )}
      <div className="org-results">
        <SearchResults
          rs={rs}
          emptyIcon="people"
          emptyTitle={t('ui.semResultados')}
          renderItems={(rows) => (
            <div className="dx-table-wrap org-table-wrap">
              <table className="dx-table org-table">
                <thead>
                  <tr>
                    <th scope="col">
                      <Checkbox
                        label={<span className="dx-sr-only">{t('org.membro.massaSeleccionarTodos')}</span>}
                        checked={rows.length > 0 && rows.every((m) => selected.has(m.user_id))}
                        onChange={(e) =>
                          setSelected(e.target.checked ? new Set(rows.map((m) => m.user_id)) : new Set())
                        }
                      />
                    </th>
                    <th scope="col">{t('org.coluna.pessoa')}</th>
                    <th scope="col">{t('org.coluna.papel')}</th>
                    <th scope="col">{t('org.coluna.cargo')}</th>
                    <th scope="col">{t('org.coluna.filial')}</th>
                    <th scope="col">{t('org.coluna.actividade')}</th>
                    <th scope="col">
                      <span className="dx-sr-only">{t('org.coluna.accoes')}</span>
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((m) => {
                    const self = m.user_id === meId
                    return (
                      <tr key={m.user_id}>
                        <td>
                          {!self && (
                            <Checkbox
                              label={<span className="dx-sr-only">{t('org.membro.massaSeleccionarA', { nome: m.username })}</span>}
                              checked={selected.has(m.user_id)}
                              onChange={() => toggleOne(m.user_id)}
                            />
                          )}
                        </td>
                        <td>
                          <span className="org-person">
                            <Avatar name={m.username} size={28} />
                            <span className="org-person__text">
                              <strong>
                                {m.username}
                                {self && <span className="dx-muted"> {t('org.dir.tu')}</span>}
                              </strong>
                              <span className="dx-muted dx-num">{m.email}</span>
                            </span>
                          </span>
                        </td>
                        <td>{m.role === 'admin' ? <Tag tone="accent">{t('org.papel.admin')}</Tag> : <Tag plain>{t('org.papel.membro')}</Tag>}</td>
                        <td>{m.title || <span className="dx-muted">—</span>}</td>
                        <td>{m.branch_name || <span className="dx-muted">—</span>}</td>
                        <td className="dx-num dx-muted">{formatAgo(m.last_active, locale) ?? '—'}</td>
                        <td>
                          <span className="org-row-actions">
                            <IconButton icon="edit" bare label={t('org.membro.editarA', { nome: m.username })} onClick={() => setEditing(m)} />
                            {!self && (
                              <IconButton
                                icon="lock"
                                bare
                                disabled={busyId === m.user_id}
                                label={t('org.membro.suspenderA', { nome: m.username })}
                                onClick={() => void setSuspended(m.user_id, m.username, true)}
                              />
                            )}
                            {!self && (
                              <IconButton icon="trash" bare label={t('org.membro.removerA', { nome: m.username })} onClick={() => setRemoving(m)} />
                            )}
                          </span>
                        </td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            </div>
          )}
        />
      </div>

      {pendingInvites.length > 0 && (
        <div className="org-card-pad org-invites">
          <h3 className="org-invites__title">{t('org.convidar.pendentesTitulo', { count: pendingInvites.length })}</h3>
          <ul className="org-simple">
            {pendingInvites.map((inv) => (
              <li key={inv.id}>
                <span className="org-simple__main">
                  <strong className="dx-num">{inv.email}</strong>
                  <span className="dx-muted">
                    {inv.role_name}
                    {' · '}
                    {t('org.convidar.expiraEm', { data: new Date(inv.expires_at).toLocaleDateString() })}
                  </span>
                </span>
                <IconButton
                  icon="refresh"
                  bare
                  disabled={busyId === inv.id}
                  label={t('rbac.reenviarA', { email: inv.email })}
                  onClick={() => void resend(inv)}
                />
                <IconButton
                  icon="x"
                  bare
                  disabled={busyId === inv.id}
                  label={t('org.convidar.revogarA', { email: inv.email })}
                  onClick={() => void revoke(inv)}
                />
              </li>
            ))}
          </ul>
        </div>
      )}

      {suspendedList.length > 0 && (
        <div className="org-card-pad org-invites">
          <h3 className="org-invites__title">{t('rbac.suspensosTitulo', { count: suspendedList.length })}</h3>
          <ul className="org-simple">
            {suspendedList.map((e) => (
              <li key={e.id}>
                <span className="org-simple__main">
                  <strong>{e.name}</strong>
                  <span className="dx-muted dx-num">{e.email}</span>
                </span>
                <IconButton
                  icon="play"
                  bare
                  disabled={busyId === e.user_id}
                  label={t('org.membro.reactivarA', { nome: e.name })}
                  onClick={() => void setSuspended(e.user_id as string, e.name, false)}
                />
              </li>
            ))}
          </ul>
        </div>
      )}

      {adding && (
        <AddMemberDialog
          orgId={orgId}
          branches={branches}
          onClose={() => setAdding(false)}
          onAdded={(e) => {
            setAdding(false)
            setNotice(t('org.membro.adicionado', { nome: e.username, email: e.email }))
            reloadAll()
          }}
        />
      )}
      {editing && (
        <EditMemberDialog
          orgId={orgId}
          member={editing}
          branches={branches}
          isSelf={editing.user_id === meId}
          onClose={() => setEditing(null)}
          onSaved={(e) => {
            setEditing(null)
            setNotice(t('org.membro.guardado', { nome: e.username }))
            reloadAll()
          }}
        />
      )}
      {removing && (
        <RemoveMemberDialog
          orgId={orgId}
          member={removing}
          onClose={() => setRemoving(null)}
          onRemoved={() => {
            setNotice(t('org.membro.removido', { nome: removing.username }))
            setRemoving(null)
            reloadAll()
          }}
        />
      )}
      {inviting && roles.state.s === 'ready' && (
        <InviteDialog orgId={orgId} roles={roles.state.d.items} onClose={() => setInviting(false)} onInvited={() => invites.reload()} />
      )}
      {importingCsv && (
        <CsvImportDialog orgId={orgId} onClose={() => setImportingCsv(false)} onImported={() => invites.reload()} />
      )}
    </Card>
  )
}
