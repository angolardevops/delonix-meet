/**
 * Convidar uma pessoa por LINK — este produto não tem SMTP (grep por
 * `smtp`/`lettre`/`mailer` em `server/` não devolve nada). O servidor gera o
 * token e devolve o convite; esta caixa de diálogo monta o link
 * (`#/invite/:token`) e o admin copia-o e envia-o ele mesmo — a UI diz isso
 * mesmo, sem fingir que existe um email a caminho.
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Branch, createInvite, Invite } from '../../api'
import { Alert, Button, Dialog, Field, Select, TextInput } from '../../ui/kit'
import { orgErrorMessage } from './orgShared'

export default function InviteDialog({
  orgId,
  branches,
  onClose,
  onInvited,
}: {
  orgId: string
  branches: Branch[]
  onClose: () => void
  onInvited: () => void
}) {
  const { t } = useTranslation()
  const [email, setEmail] = useState('')
  const [title, setTitle] = useState('')
  const [role, setRole] = useState<'admin' | 'member'>('member')
  const [branchId, setBranchId] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [created, setCreated] = useState<Invite | null>(null)
  const [copied, setCopied] = useState(false)

  const valid = email.includes('@')
  const link = created ? `${location.origin}${location.pathname}#/invite/${created.token}` : ''

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!valid) return
    setBusy(true)
    setErr('')
    try {
      const invite = await createInvite(orgId, {
        email: email.trim(),
        title: title.trim(),
        role,
        branch_id: branchId || undefined,
      })
      setCreated(invite)
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'org.erro.convidar'))
    } finally {
      setBusy(false)
    }
  }

  function close() {
    onInvited()
    onClose()
  }

  return (
    <Dialog
      title={t('org.convidar.titulo')}
      onClose={close}
      footer={
        created ? (
          <Button variant="primary" disabled={!copied} onClick={close}>
            {t('org.convidar.concluido')}
          </Button>
        ) : (
          <>
            <Button variant="ghost" onClick={onClose}>
              {t('ui.cancelar')}
            </Button>
            <Button variant="primary" type="submit" form="org-invite-form" busy={busy} disabled={!valid}>
              {t('org.convidar.gerarLink')}
            </Button>
          </>
        )
      }
    >
      {created ? (
        <div className="org-form">
          <Alert>{t('org.convidar.avisoEnvio')}</Alert>
          <Field label={t('org.convidar.linkGerado')} htmlFor="org-invite-link">
            <TextInput id="org-invite-link" readOnly value={link} onFocus={(e) => e.currentTarget.select()} />
          </Field>
          {err && <Alert tone="danger">{err}</Alert>}
          <Button
            variant="secondary"
            icon="copy"
            onClick={async () => {
              try {
                await navigator.clipboard.writeText(link)
                setCopied(true)
              } catch {
                setErr(t('org.convidar.erroCopiar'))
              }
            }}
          >
            {copied ? t('org.convidar.copiado') : t('org.convidar.copiar')}
          </Button>
          <p className="dx-muted">{t('org.convidar.expiraEm', { data: new Date(created.expires_at).toLocaleDateString() })}</p>
        </div>
      ) : (
        <form id="org-invite-form" onSubmit={submit} className="org-form">
          <Field label={t('org.campo.email')} htmlFor="org-invite-email" hint={t('org.membro.emailDica')}>
            <TextInput id="org-invite-email" type="email" autoComplete="off" value={email} onChange={(e) => setEmail(e.target.value)} required />
          </Field>
          <div className="org-form__grid">
            <Field label={t('org.campo.papel')} htmlFor="org-invite-role">
              <Select id="org-invite-role" value={role} onChange={(e) => setRole(e.target.value as 'admin' | 'member')}>
                <option value="member">{t('org.papel.membro')}</option>
                <option value="admin">{t('org.papel.admin')}</option>
              </Select>
            </Field>
            <Field label={t('org.campo.cargo')} htmlFor="org-invite-title">
              <TextInput id="org-invite-title" value={title} maxLength={120} onChange={(e) => setTitle(e.target.value)} />
            </Field>
            <Field label={t('org.campo.filial')} htmlFor="org-invite-branch">
              <Select id="org-invite-branch" value={branchId} onChange={(e) => setBranchId(e.target.value)}>
                <option value="">{t('org.membro.semFilial')}</option>
                {branches.map((b) => (
                  <option key={b.id} value={b.id}>
                    {b.name}
                  </option>
                ))}
              </Select>
            </Field>
          </div>
          {err && <Alert tone="danger">{err}</Alert>}
        </form>
      )}
    </Dialog>
  )
}
