/**
 * Adicionar, editar e remover pessoas da organização.
 *
 * `add_employee` NÃO envia convite: se já houver uma conta com esse email,
 * liga-a à organização (e reescreve papel, cargo e filial); se não houver,
 * cria a conta com a palavra-passe dada — e sem ela o servidor usa uma
 * palavra-passe fixa. Por isso a palavra-passe é obrigatória aqui, e o texto
 * diz o que acontece de verdade. A ligação de contas existentes por email é
 * um problema de segurança aberto no servidor (HARNESS, S-add_employee); a UI
 * não o esconde nem o torna mais fácil.
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { addEmployee, Branch, Employee, issuePasswordReset, passwordResetLink, removeEmployee, updateEmployee } from '../../api'
import { copiarTexto } from '../../ui/copy'
import { Alert, Button, Dialog, Field, Select, TextInput } from '../../ui/kit'
import { orgErrorMessage } from './orgShared'

function RoleAndPlace({
  idPrefix,
  role,
  onRole,
  title,
  onTitle,
  branchId,
  onBranch,
  branches,
  allowNoBranch,
}: {
  idPrefix: string
  role: string
  onRole: (v: 'admin' | 'member') => void
  title: string
  onTitle: (v: string) => void
  branchId: string
  onBranch: (v: string) => void
  branches: Branch[]
  allowNoBranch: boolean
}) {
  const { t } = useTranslation()
  return (
    <div className="org-form__grid">
      <Field label={t('org.campo.papel')} htmlFor={`${idPrefix}-role`}>
        <Select id={`${idPrefix}-role`} value={role} onChange={(e) => onRole(e.target.value as 'admin' | 'member')}>
          <option value="member">{t('org.papel.membro')}</option>
          <option value="admin">{t('org.papel.admin')}</option>
        </Select>
      </Field>
      <Field label={t('org.campo.cargo')} htmlFor={`${idPrefix}-title`}>
        <TextInput id={`${idPrefix}-title`} value={title} maxLength={120} onChange={(e) => onTitle(e.target.value)} />
      </Field>
      <Field label={t('org.campo.filial')} htmlFor={`${idPrefix}-branch`} hint={allowNoBranch ? undefined : t('org.membro.filialSemRetirar')}>
        <Select id={`${idPrefix}-branch`} value={branchId} onChange={(e) => onBranch(e.target.value)}>
          {allowNoBranch && <option value="">{t('org.membro.semFilial')}</option>}
          {branches.map((b) => (
            <option key={b.id} value={b.id}>
              {b.name}
            </option>
          ))}
        </Select>
      </Field>
    </div>
  )
}

export function AddMemberDialog({
  orgId,
  branches,
  onClose,
  onAdded,
}: {
  orgId: string
  branches: Branch[]
  onClose: () => void
  onAdded: (e: Employee) => void
}) {
  const { t } = useTranslation()
  const [email, setEmail] = useState('')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [title, setTitle] = useState('')
  const [role, setRole] = useState<'admin' | 'member'>('member')
  const [branchId, setBranchId] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  const pwOk = password.length >= 8 && password.length <= 128
  const valid = email.includes('@') && pwOk

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!valid) return
    setBusy(true)
    setErr('')
    try {
      const emp = await addEmployee(orgId, {
        email: email.trim(),
        username: username.trim() || undefined,
        password,
        title: title.trim(),
        role,
        branch_id: branchId || undefined,
      })
      onAdded(emp)
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'org.erro.adicionar'))
      setBusy(false)
    }
  }

  return (
    <Dialog
      wide
      title={t('org.membro.adicionarTitulo')}
      onClose={onClose}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t('ui.cancelar')}
          </Button>
          <Button variant="primary" type="submit" form="org-add-member" busy={busy} disabled={!valid}>
            {t('org.membro.adicionar')}
          </Button>
        </>
      }
    >
      <form id="org-add-member" onSubmit={submit} className="org-form">
        <Alert>{t('org.membro.comoFunciona')}</Alert>
        <div className="org-form__grid">
          <Field label={t('org.campo.email')} htmlFor="org-add-email" hint={t('org.membro.emailDica')}>
            <TextInput id="org-add-email" type="email" autoComplete="off" value={email} onChange={(e) => setEmail(e.target.value)} required />
          </Field>
          <Field label={t('org.campo.nome')} htmlFor="org-add-name" hint={t('org.membro.nomeDica')}>
            <TextInput id="org-add-name" value={username} maxLength={64} onChange={(e) => setUsername(e.target.value)} />
          </Field>
          <Field
            label={t('org.campo.palavraPasse')}
            htmlFor="org-add-pw"
            hint={t('org.membro.palavraPasseDica')}
            error={password && !pwOk ? t('org.membro.palavraPasseCurta') : undefined}
          >
            <TextInput
              id="org-add-pw"
              type="password"
              autoComplete="new-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              aria-invalid={password !== '' && !pwOk}
              required
            />
          </Field>
        </div>
        <RoleAndPlace
          idPrefix="org-add"
          role={role}
          onRole={setRole}
          title={title}
          onTitle={setTitle}
          branchId={branchId}
          onBranch={setBranchId}
          branches={branches}
          allowNoBranch
        />
        {err && <Alert tone="danger">{err}</Alert>}
      </form>
    </Dialog>
  )
}

export function EditMemberDialog({
  orgId,
  member,
  branches,
  isSelf,
  onClose,
  onSaved,
}: {
  orgId: string
  member: Employee
  branches: Branch[]
  isSelf: boolean
  onClose: () => void
  onSaved: (e: Employee) => void
}) {
  const { t } = useTranslation()
  const [role, setRole] = useState<'admin' | 'member'>(member.role)
  const [title, setTitle] = useState(member.title ?? '')
  const [branchId, setBranchId] = useState(member.branch_id ?? '')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  async function submit(e: FormEvent) {
    e.preventDefault()
    setBusy(true)
    setErr('')
    try {
      const emp = await updateEmployee(orgId, member.user_id, {
        role,
        title: title.trim(),
        // O servidor não retira a filial (`branch_id` nulo é ignorado): só se muda.
        branch_id: branchId || undefined,
      })
      onSaved(emp)
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'org.erro.guardar'))
      setBusy(false)
    }
  }

  return (
    <Dialog
      title={t('org.membro.editarTitulo', { nome: member.username })}
      onClose={onClose}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t('ui.cancelar')}
          </Button>
          <Button variant="primary" type="submit" form="org-edit-member" busy={busy}>
            {t('ui.guardar')}
          </Button>
        </>
      }
    >
      <form id="org-edit-member" onSubmit={submit} className="org-form">
        <p className="dx-muted dx-num org-break">{member.email}</p>
        <RoleAndPlace
          idPrefix="org-edit"
          role={role}
          onRole={setRole}
          title={title}
          onTitle={setTitle}
          branchId={branchId}
          onBranch={setBranchId}
          branches={branches}
          allowNoBranch={!member.branch_id}
        />
        {isSelf && role === 'member' && member.role === 'admin' && <Alert tone="warning">{t('org.membro.despromoverTe')}</Alert>}
        {err && <Alert tone="danger">{err}</Alert>}
      </form>
    </Dialog>
  )
}

export function RemoveMemberDialog({
  orgId,
  member,
  onClose,
  onRemoved,
}: {
  orgId: string
  member: Employee
  onClose: () => void
  onRemoved: () => void
}) {
  const { t } = useTranslation()
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  async function confirm() {
    setBusy(true)
    setErr('')
    try {
      await removeEmployee(orgId, member.user_id)
      onRemoved()
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'org.erro.remover'))
      setBusy(false)
    }
  }

  return (
    <Dialog
      title={t('org.membro.removerTitulo', { nome: member.username })}
      onClose={onClose}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t('ui.cancelar')}
          </Button>
          <Button variant="danger" icon="trash" busy={busy} onClick={() => void confirm()}>
            {t('org.membro.remover')}
          </Button>
        </>
      }
    >
      <div className="org-form">
        <p>{t('org.membro.removerTexto')}</p>
        {err && <Alert tone="danger">{err}</Alert>}
      </div>
    </Dialog>
  )
}

/**
 * Repor a password de um membro (#288, B5). Repor é TOMAR a conta: o servidor
 * só deixa quem poderia nomear o papel do alvo (`ensure_can_assign`), e diz
 * porquê quando recusa.
 *
 * O link aparece UMA vez — a base guarda só o hash — e o administrador entrega-o
 * pelo canal que já usa. Ao ser usado, termina todas as sessões da pessoa.
 */
export function ResetPasswordDialog({
  orgId,
  member,
  onClose,
}: {
  orgId: string
  member: Employee
  onClose: () => void
}) {
  const { t, i18n } = useTranslation()
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [emitido, setEmitido] = useState<{ link: string; ate: string } | null>(null)
  const [copiado, setCopiado] = useState<boolean | null>(null)

  async function emitir() {
    setBusy(true)
    setErr('')
    try {
      const r = await issuePasswordReset(orgId, member.user_id)
      setEmitido({
        link: passwordResetLink(r.token),
        ate: new Date(r.expires_at).toLocaleString(i18n.language, { dateStyle: 'short', timeStyle: 'short' }),
      })
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'org.membro.reporErro'))
    } finally {
      setBusy(false)
    }
  }

  async function copiar() {
    if (emitido) setCopiado(await copiarTexto(emitido.link))
  }

  return (
    <Dialog
      title={t('org.membro.reporTitulo', { nome: member.username })}
      onClose={onClose}
      footer={
        emitido ? (
          <Button variant="primary" onClick={onClose}>
            {t('org.membro.reporFeito')}
          </Button>
        ) : (
          <>
            <Button variant="ghost" onClick={onClose}>
              {t('ui.cancelar')}
            </Button>
            <Button variant="primary" icon="key" busy={busy} onClick={() => void emitir()}>
              {t('org.membro.reporEmitir')}
            </Button>
          </>
        )
      }
    >
      <div className="org-form">
        {emitido ? (
          <>
            <Alert tone="warning">{t('org.membro.reporUmaVez', { ate: emitido.ate })}</Alert>
            <Field label={t('org.membro.reporLink')} htmlFor="repor-link">
              <TextInput id="repor-link" value={emitido.link} readOnly onFocus={(e) => e.currentTarget.select()} />
            </Field>
            <div>
              <Button variant="secondary" size="sm" icon="copy" onClick={() => void copiar()}>
                {t('org.membro.reporCopiar')}
              </Button>
            </div>
            {copiado === true && <p className="dx-muted" role="status">{t('org.membro.reporCopiado')}</p>}
            {copiado === false && <Alert tone="danger">{t('org.membro.reporNaoCopiou')}</Alert>}
          </>
        ) : (
          <>
            <p>{t('org.membro.reporTexto', { email: member.email })}</p>
            {err && <Alert tone="danger">{err}</Alert>}
          </>
        )}
      </div>
    </Dialog>
  )
}
