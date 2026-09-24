/**
 * Gestão dos capítulos (`…/chapters`), para quem gere a gravação: acrescentar
 * no instante do vídeo (ou num escrito à mão), mudar título e instante, apagar.
 * Dois capítulos no mesmo instante são recusados pelo servidor
 * (`409 recording.chapter_timestamp_taken`), e a razão aparece por extenso.
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { addRecordingChapter, deleteRecordingChapter, updateRecordingChapter } from '../../api'
import { Alert, Button, IconButton, TextInput } from '../../ui/kit'
import { recordingErrorMessage } from './apiErrors'
import { formatClock, parseClock } from './libraryData'
import type { ChapterView } from './recordingView'

export default function RecordingChaptersEditor({
  recordingId,
  chapters,
  nowMs,
  onChanged,
}: {
  recordingId: string
  chapters: ChapterView[]
  nowMs: number
  onChanged: () => void
}) {
  const { t } = useTranslation()
  const [title, setTitle] = useState('')
  const [time, setTime] = useState('')
  const [editing, setEditing] = useState<{ id: string; title: string; time: string } | null>(null)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  async function run(fn: () => Promise<unknown>, fallback: string) {
    setBusy(true)
    setErr('')
    try {
      await fn()
      onChanged()
      return true
    } catch (e) {
      setErr(recordingErrorMessage(e, t, fallback))
      return false
    } finally {
      setBusy(false)
    }
  }

  async function add(e: FormEvent) {
    e.preventDefault()
    const tMs = time.trim() ? parseClock(time) : Math.round(nowMs)
    if (tMs === null) return setErr(t('player.capitulos.instanteInvalido'))
    if (await run(() => addRecordingChapter(recordingId, tMs, title.trim()), 'player.capitulos.erro')) {
      setTitle('')
      setTime('')
    }
  }

  async function save(e: FormEvent) {
    e.preventDefault()
    if (!editing) return
    const tMs = parseClock(editing.time)
    if (tMs === null) return setErr(t('player.capitulos.instanteInvalido'))
    if (await run(() => updateRecordingChapter(recordingId, editing.id, { title: editing.title.trim(), t_ms: tMs }), 'player.capitulos.erro')) setEditing(null)
  }

  return (
    <div className="pl-chapter-edit">
      {err && <Alert tone="danger">{err}</Alert>}
      {chapters.length > 0 && (
        <ol className="pl-chapter-edit__list">
          {chapters.map((c) =>
            editing?.id === c.id ? (
              <li key={c.id}>
                <form className="pl-form__row" onSubmit={save}>
                  <TextInput className="pl-chapter-edit__time" value={editing.time} aria-label={t('player.capitulos.instante')} onChange={(e) => setEditing({ ...editing, time: e.target.value })} />
                  <TextInput value={editing.title} maxLength={200} aria-label={t('player.capitulos.tituloCampo')} onChange={(e) => setEditing({ ...editing, title: e.target.value })} />
                  <Button type="submit" size="sm" variant="primary" busy={busy} disabled={!editing.title.trim()}>
                    {t('ui.guardar')}
                  </Button>
                  <Button size="sm" variant="ghost" onClick={() => setEditing(null)}>
                    {t('ui.cancelar')}
                  </Button>
                </form>
              </li>
            ) : (
              <li key={c.id} className="pl-form__row">
                <span className="dx-num pl-chapter-edit__time">{formatClock(c.tMs)}</span>
                <span className="pl-chapter-edit__title">{c.title}</span>
                <span className="dx-spacer" />
                <IconButton icon="edit" bare label={t('player.capitulos.editar', { title: c.title })} onClick={() => setEditing({ id: c.id, title: c.title, time: formatClock(c.tMs) })} />
                <IconButton icon="trash" bare label={t('player.capitulos.apagar', { title: c.title })} onClick={() => void run(() => deleteRecordingChapter(recordingId, c.id), 'player.capitulos.erroApagar')} />
              </li>
            ),
          )}
        </ol>
      )}
      <form className="pl-form__row" onSubmit={add}>
        <TextInput
          className="pl-chapter-edit__time"
          value={time}
          placeholder={formatClock(nowMs)}
          aria-label={t('player.capitulos.instante')}
          onChange={(e) => setTime(e.target.value)}
        />
        <TextInput value={title} maxLength={200} placeholder={t('player.capitulos.novoPh')} aria-label={t('player.capitulos.tituloCampo')} onChange={(e) => setTitle(e.target.value)} />
        <Button type="submit" size="sm" icon="plus" busy={busy} disabled={!title.trim()}>
          {t('player.capitulos.adicionar')}
        </Button>
      </form>
    </div>
  )
}
