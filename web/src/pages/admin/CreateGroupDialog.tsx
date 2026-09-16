/**
 * Criar grupo. O servidor deixa QUALQUER membro criar (`require_member`) e
 * junta sempre o criador ao grupo; a quota `max_groups` responde 409.
 */
import { FormEvent, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { createGroup, Employee } from '../../api'
import { Icon } from '../../ui/icons'
import { Alert, Avatar, Button, Dialog, Field, TextInput } from '../../ui/kit'
import { orgErrorMessage } from './orgShared'

export default function CreateGroupDialog({
  orgId,
  people,
  meId,
  onClose,
  onCreated,
}: {
  orgId: string
  people: Employee[]
  meId: string
  onClose: () => void
  onCreated: () => void
}) {
  const { t } = useTranslation()
  const [name, setName] = useState('')
  const [q, setQ] = useState('')
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  const others = useMemo(() => people.filter((p) => p.user_id !== meId), [people, meId])
  const shown = useMemo(() => {
    const term = q.trim().toLowerCase()
    if (!term) return others
    return others.filter((p) => p.username.toLowerCase().includes(term) || p.email.toLowerCase().includes(term))
  }, [others, q])

  function toggle(id: string) {
    setSelected((s) => {
      const n = new Set(s)
      if (n.has(id)) n.delete(id)
      else n.add(id)
      return n
    })
  }

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!name.trim()) return
    setBusy(true)
    setErr('')
    try {
      await createGroup(orgId, name.trim(), [...selected])
      onCreated()
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'org.erro.criarGrupo'))
      setBusy(false)
    }
  }

  return (
    <Dialog
      title={t('org.grupo.novo')}
      onClose={onClose}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t('ui.cancelar')}
          </Button>
          <Button variant="primary" type="submit" form="org-group-form" busy={busy} disabled={!name.trim()}>
            {t('org.grupo.criar', { count: selected.size + 1 })}
          </Button>
        </>
      }
    >
      <form id="org-group-form" onSubmit={submit} className="org-form">
        <Field label={t('org.grupo.nome')} htmlFor="org-group-name">
          <TextInput id="org-group-name" value={name} maxLength={120} onChange={(e) => setName(e.target.value)} required />
        </Field>
        <Field label={t('org.grupo.membros')} hint={t('org.grupo.dicaCriador')} htmlFor="org-group-q">
          <div className="org-search">
            <Icon name="search" />
            <input
              id="org-group-q"
              type="search"
              value={q}
              onChange={(e) => setQ(e.target.value)}
              placeholder={t('org.dir.pesquisar')}
            />
          </div>
        </Field>
        <ul className="org-pick" aria-label={t('org.grupo.membros')}>
          {shown.map((p) => (
            <li key={p.user_id}>
              <label className="org-pick__row">
                <input type="checkbox" checked={selected.has(p.user_id)} onChange={() => toggle(p.user_id)} />
                <Avatar name={p.username} size={24} />
                <span className="org-pick__who">
                  <strong>{p.username}</strong>
                  <span className="dx-muted">{p.title || p.email}</span>
                </span>
              </label>
            </li>
          ))}
          {shown.length === 0 && <li className="dx-muted org-pick__none">{t('ui.semResultados')}</li>}
        </ul>
        {err && <Alert tone="danger">{err}</Alert>}
      </form>
    </Dialog>
  )
}
