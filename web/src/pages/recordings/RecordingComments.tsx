/**
 * Comentários de uma gravação, por marca temporal (`…/comments`). Comentar no
 * instante do vídeo guarda `t_ms`; a marca salta o leitor. Apagar é do autor
 * ou de quem gere a gravação (`can_delete`, decidido no servidor).
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { addRecordingComment, deleteRecordingComment, recordingComments } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Alert, Avatar, Button, Checkbox, IconButton, TextArea } from '../../ui/kit'
import { recordingErrorMessage } from './apiErrors'
import { formatDateTimeShort } from './format'
import { formatClock } from './libraryData'

export default function RecordingComments({
  recordingId,
  nowMs,
  onSeek,
  onChanged,
}: {
  recordingId: string
  nowMs: number
  onSeek: (ms: number) => void
  onChanged: () => void
}) {
  const { t, i18n } = useTranslation()
  const { state, reload } = useAsync(() => recordingComments(recordingId, { page_size: 100 }), [recordingId])
  const [body, setBody] = useState('')
  const [atTime, setAtTime] = useState(true)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  async function send(e: FormEvent) {
    e.preventDefault()
    if (!body.trim()) return
    setBusy(true)
    setErr('')
    try {
      await addRecordingComment(recordingId, body.trim(), atTime ? Math.round(nowMs) : null)
      setBody('')
      reload()
      onChanged()
    } catch (e2) {
      setErr(recordingErrorMessage(e2, t, 'player.comentarios.erro'))
    } finally {
      setBusy(false)
    }
  }

  async function remove(id: string) {
    setErr('')
    try {
      await deleteRecordingComment(recordingId, id)
      reload()
      onChanged()
    } catch (e2) {
      setErr(recordingErrorMessage(e2, t, 'player.comentarios.erroApagar'))
    }
  }

  return (
    <div className="pl-comments">
      <form className="pl-form" onSubmit={send}>
        <TextArea
          rows={2}
          value={body}
          maxLength={2000}
          placeholder={t('player.comentarios.placeholder')}
          aria-label={t('player.comentarios.novo')}
          onChange={(e) => setBody(e.target.value)}
        />
        <div className="pl-form__row">
          <Checkbox label={t('player.comentarios.noInstante', { time: formatClock(nowMs) })} checked={atTime} onChange={(e) => setAtTime(e.target.checked)} />
          <span className="dx-spacer" />
          <Button type="submit" size="sm" variant="primary" busy={busy} disabled={!body.trim()}>
            {t('player.comentarios.enviar')}
          </Button>
        </div>
      </form>
      {err && <Alert tone="danger">{err}</Alert>}
      <AsyncSection state={state} onRetry={reload}>
        {(page) =>
          page.items.length === 0 ? (
            <p className="pl-empty">{t('player.comentarios.vazio')}</p>
          ) : (
            <ol className="pl-comment-list">
              {page.items.map((c) => (
                <li key={c.id} className="pl-comment">
                  <Avatar name={c.username} size={24} />
                  <div className="pl-comment__main">
                    <div className="pl-comment__head">
                      <strong>{c.username}</strong>
                      {c.t_ms !== null && (
                        <button type="button" className="pl-comment__time dx-num" onClick={() => onSeek(c.t_ms!)} aria-label={t('player.saltarPara', { tempo: formatClock(c.t_ms) })}>
                          {formatClock(c.t_ms)}
                        </button>
                      )}
                      <span className="dx-muted dx-num">{formatDateTimeShort(c.created_at, i18n.language)}</span>
                    </div>
                    <p className="pl-comment__body">{c.body}</p>
                  </div>
                  {c.can_delete && <IconButton icon="trash" bare label={t('player.comentarios.apagar')} onClick={() => void remove(c.id)} />}
                </li>
              ))}
            </ol>
          )
        }
      </AsyncSection>
    </div>
  )
}
