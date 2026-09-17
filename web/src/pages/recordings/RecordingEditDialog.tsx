/**
 * Nome, descrição e etiquetas de uma gravação (updateRecording).
 * Renomeia-se pelo `filename`: é o nome que a biblioteca mostra e com que se
 * descarrega. O servidor normaliza as etiquetas e devolve o item como ficou.
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { updateRecording, type RecordingLibraryItem } from '../../api'
import { Alert, Button, Dialog, Field, TextArea, TextInput } from '../../ui/kit'
import { recordingErrorMessage } from './apiErrors'
import { parseTags } from './libraryData'
import type { RecordingView } from './recordingView'

export default function RecordingEditDialog({
  rec,
  onClose,
  onSaved,
}: {
  rec: RecordingView
  onClose: () => void
  onSaved: (item: RecordingLibraryItem) => void
}) {
  const { t } = useTranslation()
  const [filename, setFilename] = useState(rec.filename)
  const [description, setDescription] = useState(rec.description ?? '')
  const [tags, setTags] = useState((rec.tags ?? []).join(', '))
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  async function save(e: FormEvent) {
    e.preventDefault()
    setBusy(true)
    setErr('')
    try {
      const item = await updateRecording(rec.id, { filename: filename.trim(), description, tags: parseTags(tags) })
      onSaved(item)
    } catch (e2) {
      setErr(recordingErrorMessage(e2, t, 'player.editar.erro'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog
      title={t('player.editar.titulo')}
      onClose={onClose}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t('ui.cancelar')}
          </Button>
          <Button variant="primary" type="submit" form="pl-edit" busy={busy} disabled={!filename.trim()}>
            {t('ui.guardar')}
          </Button>
        </>
      }
    >
      <form id="pl-edit" className="pl-form" onSubmit={save}>
        <Field label={t('player.editar.nome')} htmlFor="pl-edit-name">
          <TextInput id="pl-edit-name" value={filename} maxLength={200} onChange={(e) => setFilename(e.target.value)} required />
        </Field>
        <Field label={t('player.editar.descricao')} htmlFor="pl-edit-desc">
          <TextArea id="pl-edit-desc" rows={4} value={description} maxLength={8000} onChange={(e) => setDescription(e.target.value)} />
        </Field>
        <Field label={t('player.editar.etiquetas')} htmlFor="pl-edit-tags" hint={t('player.editar.etiquetasDica')}>
          <TextInput id="pl-edit-tags" value={tags} onChange={(e) => setTags(e.target.value)} />
        </Field>
        {err && <Alert tone="danger">{err}</Alert>}
      </form>
    </Dialog>
  )
}
