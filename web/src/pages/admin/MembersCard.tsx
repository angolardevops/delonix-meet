/**
 * Pessoas da organização, em tabela, com papel, cargo, filial e acções.
 * Pesquisa, filtros, agrupar e página: o painel estilo Odoo (recurso
 * `members`; sem ele no servidor, a lista inteira filtrada no browser).
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { Branch, Employee } from '../../api'
import type { Async } from '../../components/AsyncSection'
import { Alert, Avatar, Button, Card, IconButton, Tag } from '../../ui/kit'
import { SearchBar, SearchResults } from '../../ui/search/SearchResults'
import { useResourceSearch } from '../../ui/search/useResourceSearch'
import { AddMemberDialog, EditMemberDialog, RemoveMemberDialog } from './MemberDialogs'
import { formatAgo, useLocaleTag } from './orgShared'
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
  const [adding, setAdding] = useState(false)
  const [editing, setEditing] = useState<Employee | null>(null)
  const [removing, setRemoving] = useState<Employee | null>(null)
  const [notice, setNotice] = useState('')

  const all = state.s === 'ready' ? state.d : []
  const reloadAll = () => {
    reload()
    rs.reload()
  }

  return (
    <Card
      className="org-members"
      title={t('org.membro.titulo')}
      eyebrow={state.s === 'ready' ? t('org.pessoasContagem', { count: all.length }) : undefined}
      flush
      actions={
        <Button variant="primary" size="sm" icon="userPlus" onClick={() => setAdding(true)}>
          {t('org.membro.adicionar')}
        </Button>
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
    </Card>
  )
}
