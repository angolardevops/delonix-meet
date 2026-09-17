/**
 * Pontos da agenda de uma reunião: marcar como tratado, acrescentar e retirar
 * (o anfitrião). O servidor decide quem pode o quê; um 403 mostra-se.
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { addAgendaItem, AgendaItem, apiErrorMessage, deleteAgendaItem, listAgenda, patchAgendaItem } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Alert, Button, cx, Empty, IconButton, TextInput } from '../../ui/kit'

export default function AgendaPanel({ meetingId, isOwner }: { meetingId: string; isOwner: boolean }) {
  const { t } = useTranslation()
  const { state, reload, mutate } = useAsync(() => listAgenda(meetingId), [meetingId])
  const [topic, setTopic] = useState('')
  const [minutes, setMinutes] = useState(5)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  async function add(e: FormEvent) {
    e.preventDefault()
    if (!topic.trim()) return
    setErr('')
    setBusy(true)
    try {
      await addAgendaItem(meetingId, { topic: topic.trim(), duration_min: minutes })
      setTopic('')
      setMinutes(5)
      reload()
    } catch (e2) {
      setErr(apiErrorMessage(e2, t('schedule.pontos.erroAdicionar')))
    } finally {
      setBusy(false)
    }
  }

  async function toggle(item: AgendaItem) {
    setErr('')
    mutate((list) => list.map((i) => (i.id === item.id ? { ...i, done: !item.done } : i)))
    try {
      await patchAgendaItem(meetingId, item.id, { done: !item.done })
    } catch (e) {
      setErr(apiErrorMessage(e, t('schedule.pontos.erroActualizar')))
    }
    reload()
  }

  async function remove(item: AgendaItem) {
    setErr('')
    try {
      await deleteAgendaItem(meetingId, item.id)
      reload()
    } catch (e) {
      setErr(apiErrorMessage(e, t('schedule.pontos.erroRemover')))
    }
  }

  return (
    <div className="cal-tabpanel">
      {err && <Alert tone="danger">{err}</Alert>}
      <AsyncSection state={state} onRetry={reload}>
        {(items) => {
          const total = items.reduce((s, i) => s + i.duration_min, 0)
          return items.length === 0 ? (
            <Empty icon="list" title={t('schedule.pontos.vazio')}>
              {isOwner ? t('schedule.pontos.vazioDono') : null}
            </Empty>
          ) : (
            <>
              <ol className="cal-agenda">
                {[...items]
                  .sort((a, b) => a.position - b.position)
                  .map((item) => (
                    <li key={item.id} className={cx('cal-agenda__item', item.done && 'is-done')}>
                      <label className="dx-check">
                        <input type="checkbox" checked={item.done} onChange={() => void toggle(item)} />
                        <span className="cal-agenda__text">
                          <strong>{item.topic}</strong>
                          {item.description && <small>{item.description}</small>}
                        </span>
                      </label>
                      <span className="dx-num dx-muted">{t('schedule.evento.duracao', { n: item.duration_min })}</span>
                      {isOwner && (
                        <IconButton icon="trash" bare label={t('schedule.pontos.remover', { topico: item.topic })} onClick={() => void remove(item)} />
                      )}
                    </li>
                  ))}
              </ol>
              <div className="dx-muted dx-num">{t('schedule.pontos.total', { n: total })}</div>
            </>
          )
        }}
      </AsyncSection>
      {isOwner && (
        <form className="cal-inline-form" onSubmit={add}>
          <TextInput
            value={topic}
            onChange={(e) => setTopic(e.target.value)}
            placeholder={t('schedule.pontos.novoPh')}
            aria-label={t('schedule.pontos.novo')}
          />
          <TextInput
            type="number"
            min={1}
            max={180}
            value={minutes}
            onChange={(e) => setMinutes(Math.max(1, Number(e.target.value) || 1))}
            aria-label={t('schedule.pontos.minutos')}
            className="cal-inline-form__num"
          />
          <Button type="submit" icon="plus" busy={busy} disabled={!topic.trim()}>
            {t('schedule.pontos.adicionar')}
          </Button>
        </form>
      )}
    </div>
  )
}
