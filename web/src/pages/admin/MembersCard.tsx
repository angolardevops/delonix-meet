/**
 * Pessoas da organização, em tabela, com papel, cargo, filial e acções.
 * Pesquisa, filtros, agrupar e página: o painel estilo Odoo (recurso
 * `members`; sem ele no servidor, a lista inteira filtrada no browser).
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { listInvites, revokeInvite, updateEmployee } from '../../api'
import type { Branch, Employee, Invite } from '../../api'
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
  const invites = useAsync(() => refusalAware(listInvites(orgId), t), [orgId])
  const [adding, setAdding] = useState(false)
  const [inviting, setInviting] = useState(false)
  const [importingCsv, setImportingCsv] = useState(false)
  const [editing, setEditing] = useState<Employee | null>(null)
  const [removing, setRemoving] = useState<Employee | null>(null)
  const [notice, setNotice] = useState('')
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [bulkBusy, setBulkBusy] = useState(false)
  const [bulkErr, setBulkErr] = useState('')
  const [suspending, setSuspending] = useState<string | null>(null)
  const [revoking, setRevoking] = useState<string | null>(null)

  const all = state.s === 'ready' ? state.d : []
  const pendingInvites = invites.state.s === 'ready' ? invites.state.d : []
  const activos = all.filter((m) => !m.suspended_at).length
  const suspensos = all.filter((m) => m.suspended_at).length
  const reloadAll = () => {
    reload()
    rs.reload()
    invites.reload()
  }

  async function toggleSuspend(m: Employee) {
    setSuspending(m.user_id)
    setBulkErr('')
    try {
      await updateEmployee(orgId, m.user_id, { suspended: !m.suspended_at })
      setNotice(m.suspended_at ? t('org.membro.reactivado', { nome: m.username }) : t('org.membro.suspenso', { nome: m.username }))
      reloadAll()
    } catch (e) {
      setBulkErr(orgErrorMessage(e, t, 'org.erro.guardar'))
    } finally {
      setSuspending(null)
    }
  }

  async function revoke(inv: Invite) {
    setRevoking(inv.id)
    setBulkErr('')
    try {
      await revokeInvite(orgId, inv.id)
      invites.reload()
    } catch (e) {
      setBulkErr(orgErrorMessage(e, t, 'org.erro.guardar'))
    } finally {
      setRevoking(null)
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
                            {m.suspended_at && <Tag tone="live">{t('org.membro.suspensoBadge')}</Tag>}
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
                                icon={m.suspended_at ? 'play' : 'lock'}
                                bare
                                disabled={suspending === m.user_id}
                                label={m.suspended_at ? t('org.membro.reactivarA', { nome: m.username }) : t('org.membro.suspenderA', { nome: m.username })}
                                onClick={() => void toggleSuspend(m)}
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
                    {inv.role === 'admin' ? t('org.papel.admin') : t('org.papel.membro')}
                    {' · '}
                    {t('org.convidar.expiraEm', { data: new Date(inv.expires_at).toLocaleDateString() })}
                  </span>
                </span>
                <IconButton
                  icon="x"
                  bare
                  disabled={revoking === inv.id}
                  label={t('org.convidar.revogarA', { email: inv.email })}
                  onClick={() => void revoke(inv)}
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
      {inviting && (
        <InviteDialog orgId={orgId} branches={branches} onClose={() => setInviting(false)} onInvited={() => invites.reload()} />
      )}
      {importingCsv && (
        <CsvImportDialog orgId={orgId} onClose={() => setImportingCsv(false)} onImported={() => invites.reload()} />
      )}
    </Card>
  )
}
