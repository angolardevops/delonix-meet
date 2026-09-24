/**
 * Convidar uma pessoa por LINK — este produto não tem SMTP. O servidor cria o
 * convite (ADR-0008 §11) e devolve o token UMA vez, com `delivery: "manual"`;
 * esta caixa monta o link (`#/invite/:token`) e o admin copia-o e envia-o ele
 * mesmo. A UI diz isso, sem fingir que há um email a caminho.
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { createInvitation, invitationLink, OrgInvitationWithToken, OrgRole } from '../../api'
import { Alert, Button, Dialog, Field, Select, TextInput } from '../../ui/kit'
import { orgErrorMessage } from './orgShared'

export default function InviteDialog({
  orgId,
  roles,
  onClose,
  onInvited,
}: {
  orgId: string
  roles: OrgRole[]
  onClose: () => void
  onInvited: () => void
}) {
  const { t } = useTranslation()
  // O Proprietário nunca se convida (`invitation.owner_forbidden`).
  const offered = roles.filter((r) => r.key !== 'owner')
  const [email, setEmail] = useState('')
  const [roleId, setRoleId] = useState(offered.find((r) => r.key === 'member')?.id ?? offered[0]?.id ?? '')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [created, setCreated] = useState<OrgInvitationWithToken | null>(null)
  const [copied, setCopied] = useState(false)

  const valid = email.includes('@') && roleId !== ''
  const link = created ? invitationLink(created.token) : ''

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!valid) return
    setBusy(true)
    setErr('')
    try {
      setCreated(await createInvitation(orgId, { email: email.trim(), role_id: roleId }))
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
          <Field label={t('org.campo.papel')} htmlFor="org-invite-role">
            <Select id="org-invite-role" value={roleId} onChange={(e) => setRoleId(e.target.value)}>
              {offered.map((r) => (
                <option key={r.id} value={r.id}>
                  {r.name}
                </option>
              ))}
            </Select>
          </Field>
          {err && <Alert tone="danger">{err}</Alert>}
        </form>
      )}
    </Dialog>
  )
}
