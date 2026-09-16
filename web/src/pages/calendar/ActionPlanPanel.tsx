/**
 * Plano de acção 5W2H de uma reunião: a meta, e uma linha por acção (o quê,
 * quando, onde, quem, porquê, como, recursos) com o estado a rodar entre
 * por fazer → em curso → feito.
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  ActionItem,
  addActionItem,
  apiErrorMessage,
  deleteActionItem,
  getActionPlan,
  patchActionItem,
  upsertActionPlan,
} from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Alert, Button, IconButton, StatusBadge, TextInput } from '../../ui/kit'

type Status = ActionItem['status']
const NEXT: Record<Status, Status> = { todo: 'doing', doing: 'done', done: 'todo' }
const EMPTY = { what: '', when_date: '', where_text: '', who_name: '', why: '', how: '', resources: '' }
type Draft = typeof EMPTY
const COLS: (keyof Draft)[] = ['what', 'when_date', 'where_text', 'who_name', 'why', 'how', 'resources']

export default function ActionPlanPanel({ meetingId, isOwner }: { meetingId: string; isOwner: boolean }) {
  const { t } = useTranslation()
  const { state, reload } = useAsync(() => getActionPlan(meetingId), [meetingId])
  const [goal, setGoal] = useState<string | null>(null)
  const [draft, setDraft] = useState<Draft>(EMPTY)
  const [busy, setBusy] = useState<'goal' | 'item' | null>(null)
  const [err, setErr] = useState('')

  const colLabel: Record<keyof Draft, string> = {
    what: t('schedule.plano.oque'),
    when_date: t('schedule.plano.quando'),
    where_text: t('schedule.plano.onde'),
    who_name: t('schedule.plano.quem'),
    why: t('schedule.plano.porque'),
    how: t('schedule.plano.como'),
    resources: t('schedule.plano.recursos'),
  }
  const statusLabel: Record<Status, string> = {
    todo: t('schedule.plano.porFazer'),
    doing: t('schedule.plano.emCurso'),
    done: t('schedule.plano.feito'),
  }

  async function saveGoal(e: FormEvent) {
    e.preventDefault()
    if (goal === null) return
    setErr('')
    setBusy('goal')
    try {
      await upsertActionPlan(meetingId, goal.trim())
      setGoal(null)
      reload()
    } catch (e2) {
      setErr(apiErrorMessage(e2, t('schedule.plano.erroMeta')))
    } finally {
      setBusy(null)
    }
  }

  async function addItem(e: FormEvent) {
    e.preventDefault()
    if (!draft.what.trim()) return
    setErr('')
    setBusy('item')
    try {
      await addActionItem(meetingId, { ...draft, what: draft.what.trim(), when_date: draft.when_date || null })
      setDraft(EMPTY)
      reload()
    } catch (e2) {
      setErr(apiErrorMessage(e2, t('schedule.plano.erroAdicionar')))
    } finally {
      setBusy(null)
    }
  }

  async function cycle(item: ActionItem) {
    setErr('')
    try {
      await patchActionItem(item.id, { status: NEXT[item.status] })
      reload()
    } catch (e) {
      setErr(apiErrorMessage(e, t('schedule.plano.erroActualizar')))
    }
  }

  async function remove(item: ActionItem) {
    setErr('')
    try {
      await deleteActionItem(item.id)
      reload()
    } catch (e) {
      setErr(apiErrorMessage(e, t('schedule.plano.erroRemover')))
    }
  }

  return (
    <div className="cal-tabpanel">
      {err && <Alert tone="danger">{err}</Alert>}
      <AsyncSection state={state} onRetry={reload}>
        {(plan) => (
          <>
            <div className="cal-goal">
              <span className="dx-eyebrow">{t('schedule.plano.meta')}</span>
              {goal !== null ? (
                <form className="cal-inline-form" onSubmit={saveGoal}>
                  <TextInput
                    autoFocus
                    value={goal}
                    onChange={(e) => setGoal(e.target.value)}
                    placeholder={t('schedule.plano.metaPh')}
                    aria-label={t('schedule.plano.meta')}
                  />
                  <Button type="submit" variant="primary" busy={busy === 'goal'}>
                    {t('ui.guardar')}
                  </Button>
                  <Button variant="ghost" onClick={() => setGoal(null)}>
                    {t('ui.cancelar')}
                  </Button>
                </form>
              ) : (
                <div className="cal-goal__value">
                  <span className={plan?.goal ? undefined : 'dx-muted'}>{plan?.goal || t('schedule.plano.semMeta')}</span>
                  {isOwner && (
                    <IconButton icon="edit" bare label={t('schedule.plano.editarMeta')} onClick={() => setGoal(plan?.goal ?? '')} />
                  )}
                </div>
              )}
            </div>

            <div className="dx-table-wrap">
              <table className="dx-table cal-plan">
                <thead>
                  <tr>
                    {COLS.map((c) => (
                      <th key={c} scope="col">
                        {colLabel[c]}
                      </th>
                    ))}
                    <th scope="col">{t('schedule.plano.estado')}</th>
                    {isOwner && (
                      <th scope="col">
                        <span className="dx-sr-only">{t('schedule.plano.accoes')}</span>
                      </th>
                    )}
                  </tr>
                </thead>
                <tbody>
                  {(!plan || plan.items.length === 0) && (
                    <tr>
                      <td colSpan={COLS.length + (isOwner ? 2 : 1)} className="dx-muted">
                        {t('schedule.plano.vazio')}
                      </td>
                    </tr>
                  )}
                  {plan?.items.map((item) => (
                    <tr key={item.id}>
                      {COLS.map((c) => (
                        <td key={c} className={c === 'when_date' ? 'dx-num' : undefined}>
                          {item[c] || <span className="dx-muted">—</span>}
                        </td>
                      ))}
                      <td>
                        <button
                          type="button"
                          className="cal-status"
                          onClick={() => void cycle(item)}
                          title={t('schedule.plano.mudarEstado', { estado: statusLabel[NEXT[item.status]] })}
                        >
                          <StatusBadge tone={item.status === 'done' ? 'success' : item.status === 'doing' ? 'warning' : 'neutral'}>
                            {statusLabel[item.status]}
                          </StatusBadge>
                        </button>
                      </td>
                      {isOwner && (
                        <td>
                          <IconButton icon="trash" bare label={t('schedule.plano.remover', { accao: item.what })} onClick={() => void remove(item)} />
                        </td>
                      )}
                    </tr>
                  ))}
                  {isOwner && (
                    <tr className="cal-plan__new">
                      {COLS.map((c) => (
                        <td key={c}>
                          <TextInput
                            form="cal-plan-new"
                            type={c === 'when_date' ? 'date' : 'text'}
                            value={draft[c]}
                            onChange={(e) => setDraft((d) => ({ ...d, [c]: e.target.value }))}
                            aria-label={colLabel[c]}
                            placeholder={c === 'what' ? t('schedule.plano.oquePh') : undefined}
                          />
                        </td>
                      ))}
                      <td colSpan={2}>
                        <Button form="cal-plan-new" type="submit" size="sm" icon="plus" busy={busy === 'item'} disabled={!draft.what.trim()}>
                          {t('schedule.plano.adicionar')}
                        </Button>
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
            {isOwner && <form id="cal-plan-new" onSubmit={addItem} hidden />}
          </>
        )}
      </AsyncSection>
    </div>
  )
}
