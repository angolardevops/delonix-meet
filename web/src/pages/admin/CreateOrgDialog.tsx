import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { createOrg, OrgSummary } from '../../api'
import { Alert, Button, Dialog, Field, TextInput } from '../../ui/kit'
import { orgErrorMessage } from './orgShared'

export default function CreateOrgDialog({ onClose, onCreated }: { onClose: () => void; onCreated: (o: OrgSummary) => void }) {
  const { t } = useTranslation()
  const [name, setName] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!name.trim()) return
    setBusy(true)
    setErr('')
    try {
      onCreated(await createOrg(name.trim()))
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'org.erro.criarOrg'))
      setBusy(false)
    }
  }

  return (
    <Dialog
      title={t('org.novaOrg.titulo')}
      onClose={onClose}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t('ui.cancelar')}
          </Button>
          <Button variant="primary" type="submit" form="org-create-form" busy={busy} disabled={!name.trim()}>
            {t('org.novaOrg.criar')}
          </Button>
        </>
      }
    >
      <form id="org-create-form" onSubmit={submit} className="org-form">
        <Field label={t('org.novaOrg.nome')} htmlFor="org-create-name" hint={t('org.novaOrg.dica')}>
          <TextInput id="org-create-name" value={name} maxLength={120} onChange={(e) => setName(e.target.value)} required />
        </Field>
        {err && <Alert tone="danger">{err}</Alert>}
      </form>
    </Dialog>
  )
}
