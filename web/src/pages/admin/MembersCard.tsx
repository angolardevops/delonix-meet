/** Pessoas da organização, em tabela, com papel, cargo, filial e acções. */
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { Branch, Employee } from '../../api'
import { Async, AsyncSection } from '../../components/AsyncSection'
import { Icon } from '../../ui/icons'
import { Alert, Avatar, Button, Card, IconButton, Tag } from '../../ui/kit'
import { AddMemberDialog, EditMemberDialog, RemoveMemberDialog } from './MemberDialogs'
import { formatAgo, useLocaleTag } from './orgShared'

type RoleFilter = 'all' | 'admin' | 'member'

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
  const [q, setQ] = useState('')
  const [filter, setFilter] = useState<RoleFilter>('all')
  const [adding, setAdding] = useState(false)
  const [editing, setEditing] = useState<Employee | null>(null)
  const [removing, setRemoving] = useState<Employee | null>(null)
  const [notice, setNotice] = useState('')

  const all = state.s === 'ready' ? state.d : []
  const counts = useMemo(
    () => ({ all: all.length, admin: all.filter((m) => m.role === 'admin').length, member: all.filter((m) => m.role === 'member').length }),
    [all],
  )
  const shown = useMemo(() => {
    const term = q.trim().toLowerCase()
    return all
      .filter((m) => filter === 'all' || m.role === filter)
      .filter(
        (m) =>
          !term ||
          m.username.toLowerCase().includes(term) ||
          m.email.toLowerCase().includes(term) ||
          (m.title ?? '').toLowerCase().includes(term) ||
          (m.branch_name ?? '').toLowerCase().includes(term),
      )
  }, [all, q, filter])

  const filters: { v: RoleFilter; label: string }[] = [
    { v: 'all', label: t('org.membro.filtroTodos', { count: counts.all }) },
    { v: 'admin', label: t('org.membro.filtroAdmins', { count: counts.admin }) },
    { v: 'member', label: t('org.membro.filtroMembros', { count: counts.member }) },
  ]

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
      <div className="org-toolbar">
        <div className="org-search">
          <Icon name="search" />
          <input
            type="search"
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder={t('org.dir.pesquisar')}
            aria-label={t('org.dir.pesquisar')}
          />
        </div>
        <div className="dx-chips" role="group" aria-label={t('org.membro.filtrarPapel')}>
          {filters.map((f) => (
            <button key={f.v} type="button" className="dx-chip" aria-pressed={filter === f.v} onClick={() => setFilter(f.v)}>
              {f.label}
            </button>
          ))}
        </div>
      </div>
      {notice && (
        <div className="org-card-pad">
          <Alert tone="success">{notice}</Alert>
        </div>
      )}
      <AsyncSection state={state} onRetry={reload}>
        {() =>
          shown.length === 0 ? (
            <p className="dx-muted org-card-note">{t('ui.semResultados')}</p>
          ) : (
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
                  {shown.map((m) => {
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
          )
        }
      </AsyncSection>

      {adding && (
        <AddMemberDialog
          orgId={orgId}
          branches={branches}
          onClose={() => setAdding(false)}
          onAdded={(e) => {
            setAdding(false)
            setNotice(t('org.membro.adicionado', { nome: e.username, email: e.email }))
            reload()
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
            reload()
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
            reload()
          }}
        />
      )}
    </Card>
  )
}
