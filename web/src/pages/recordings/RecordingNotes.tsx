/**
 * Notas da reunião de onde a gravação saiu: transcrição, acta e tarefas.
 * Vêm de `/api/rooms/{room_code}/minutes` — as mesmas para todas as gravações da
 * sala. As tarefas são as linhas `- [ ]` / `- [x]` da acta, e marcá-las grava a
 * acta de volta.
 *
 * As horas da transcrição são do RELÓGIO da reunião, não posições no vídeo:
 * por isso mostram-se, mas não saltam o leitor — saltar para 10:42 de um
 * vídeo de 30 minutos seria inventar uma correspondência que não existe.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, isAbort, roomNotes, RoomNotes, saveMinutesByRoom } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Alert, Checkbox, cx, Tabs } from '../../ui/kit'

type Tab = 'transcript' | 'minutes' | 'tasks'

const EMPTY: RoomNotes = { title: '', minutes: '', transcript: '' }

export default function RecordingNotes({ roomCode }: { roomCode: string }) {
  const { t } = useTranslation()
  const [tab, setTab] = useState<Tab>('transcript')
  const [saving, setSaving] = useState(false)
  const [err, setErr] = useState('')
  const { state, reload, mutate } = useAsync(
    // Uma sala sem notas responde 404: isso é «sem notas», não um erro.
    (signal) =>
      roomNotes(roomCode).catch((e) => {
        if (isAbort(e) || signal.aborted) throw e
        const status = (e as { status?: number }).status
        if (status === 404) return EMPTY
        throw e
      }),
    [roomCode],
  )

  const notes = state.s === 'ready' ? state.d : EMPTY
  const lines = notes.transcript ? notes.transcript.split('\n').filter((l) => l.trim()) : []
  const minuteLines = notes.minutes ? notes.minutes.split('\n') : []
  const tasks = minuteLines
    .map((l, idx) => ({ idx, m: /^- \[([ x])\] (.*)$/.exec(l) }))
    .filter((x): x is { idx: number; m: RegExpExecArray } => !!x.m)
    .map((x) => ({ idx: x.idx, done: x.m[1] === 'x', text: x.m[2] }))
  const open = tasks.filter((x) => !x.done).length

  async function toggleTask(lineIdx: number, done: boolean) {
    if (saving) return
    setSaving(true)
    setErr('')
    const updated = [...minuteLines]
    updated[lineIdx] = updated[lineIdx].replace(/^- \[[ x]\]/, done ? '- [x]' : '- [ ]')
    const minutes = updated.join('\n')
    try {
      await saveMinutesByRoom(roomCode, minutes, notes.transcript)
      mutate((n) => ({ ...n, minutes }))
    } catch (e) {
      if (!isAbort(e)) setErr(apiErrorMessage(e, t('recordings.notas.erroTarefa')))
    } finally {
      setSaving(false)
    }
  }

  return (
    <section className="rec-notes" aria-label={t('recordings.notas.rotulo')}>
      <Tabs<Tab>
        label={t('recordings.notas.rotulo')}
        value={tab}
        onChange={setTab}
        tabs={[
          { value: 'transcript', label: t('recordings.notas.transcricao') },
          { value: 'minutes', label: t('recordings.notas.acta') },
          { value: 'tasks', label: t('recordings.notas.tarefas'), count: open },
        ]}
      />
      <div className="rec-notes__body">
        <AsyncSection state={state} onRetry={reload}>
          {() => (
            <>
              {err && <Alert tone="danger">{err}</Alert>}
              {tab === 'transcript' &&
                (lines.length === 0 ? (
                  <p className="rec-notes__empty">{t('recordings.notas.semTranscricao')}</p>
                ) : (
                  <ol className="rec-transcript">
                    {lines.map((l, i) => {
                      const m = /^\[(\d{1,2}:\d{2}(?::\d{2})?)\]\s*([^:]+):\s*(.*)$/.exec(l)
                      return (
                        <li key={i} className={cx('rec-transcript__line', !m && 'is-plain')}>
                          {m ? (
                            <>
                              <span className="rec-transcript__time dx-num">{m[1]}</span>
                              <span>
                                <strong>{m[2]}</strong> {m[3]}
                              </span>
                            </>
                          ) : (
                            <span>{l}</span>
                          )}
                        </li>
                      )
                    })}
                  </ol>
                ))}
              {tab === 'minutes' &&
                (notes.minutes ? (
                  <pre className="rec-minutes">{notes.minutes}</pre>
                ) : (
                  <p className="rec-notes__empty">{t('recordings.notas.semActa')}</p>
                ))}
              {tab === 'tasks' &&
                (tasks.length === 0 ? (
                  <p className="rec-notes__empty">{t('recordings.notas.semTarefas')}</p>
                ) : (
                  <ul className="rec-tasks">
                    {tasks.map((task) => (
                      <li key={task.idx} className={cx('rec-tasks__item', task.done && 'is-done')}>
                        <Checkbox
                          label={task.text}
                          checked={task.done}
                          disabled={saving}
                          onChange={(e) => void toggleTask(task.idx, e.target.checked)}
                        />
                      </li>
                    ))}
                  </ul>
                ))}
            </>
          )}
        </AsyncSection>
      </div>
    </section>
  )
}
