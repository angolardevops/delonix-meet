/**
 * Agenda — ver as reuniões (dia, semana, mês, ano, lista), agendar, e abrir o
 * detalhe de uma reunião.
 *
 * O estado vive na rota, para o botão «voltar» e os links da Início
 * funcionarem: `#/calendar` (ver), `#/calendar/new?d=AAAA-MM-DD&t=HHMM`
 * (agendar) e `#/calendar/m/<id>` (detalhe por cima da vista).
 */
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, createRoom, deleteMeeting, listMeetings, Meeting, startMeeting } from '../api'
import { AsyncSection, useAsync } from '../components/AsyncSection'
import PageBar from '../components/PageBar'
import { useShell } from '../components/shellContext'
import { Alert, Button, Dialog, IconButton, Segmented, Skeleton } from '../ui/kit'
import '../ui/schedule.css'
import ListView from './calendar/ListView'
import MeetingDialog from './calendar/MeetingDialog'
import { MonthView, YearView } from './calendar/MonthView'
import { occurrenceOf } from './calendar/occurrence'
import ScheduleForm from './calendar/ScheduleForm'
import { useOdooCalendar } from './home/useOdooCalendar'
import WeekView from './calendar/WeekView'
import {
  addDays,
  calendarHash,
  CalendarRoute,
  fmtTime,
  groupByDay,
  hhmm,
  localeOf,
  mondayOf,
  parseCalendarHash,
  parseYmd,
  startOfDay,
  ymd,
} from './calendar/dates'

type View = 'day' | 'week' | 'month' | 'year' | 'list'
const FORM_ID = 'schedule-form'

function initialView(): View {
  try {
    if (window.matchMedia('(max-width: 720px)').matches) return 'list'
  } catch {
    /* sem matchMedia: vista de semana */
  }
  return 'week'
}

export default function Calendar() {
  const { t, i18n } = useTranslation()
  const locale = localeOf(i18n.language)
  const { user, enterRoom } = useShell()
  const [route, setRoute] = useState<CalendarRoute>(() => parseCalendarHash(location.hash))
  const [view, setView] = useState<View>(initialView)
  const [cursor, setCursor] = useState(() => startOfDay(new Date()))
  const [err, setErr] = useState('')
  const [entering, setEntering] = useState<string | null>(null)
  const [meetNowBusy, setMeetNowBusy] = useState(false)
  const [toDelete, setToDelete] = useState<Meeting | null>(null)
  const [deleting, setDeleting] = useState(false)
  const [formState, setFormState] = useState({ busy: false, blocked: false })
  const odooCalendar = useOdooCalendar()

  useEffect(() => {
    const on = () => setRoute(parseCalendarHash(location.hash))
    window.addEventListener('hashchange', on)
    return () => window.removeEventListener('hashchange', on)
  }, [])

  const { state, reload } = useAsync((signal) => listMeetings(signal), [])
  const byDay = useMemo(() => groupByDay(state.s === 'ready' ? state.d : []), [state])

  const go = (hash: string) => {
    location.hash = hash
  }
  const openMeeting = useCallback((m: Meeting) => go(calendarHash.meeting(m.id)), [])
  const closeDialog = useCallback(() => go(calendarHash.browse()), [])
  const cancelDelete = useCallback(() => setToDelete(null), [])
  const onFormBusy = useCallback((busy: boolean, blocked: boolean) => setFormState({ busy, blocked }), [])

  async function enter(m: Meeting) {
    setErr('')
    setEntering(m.id)
    try {
      const { code, kind } = await startMeeting(m.id)
      enterRoom(code, kind === 'voice')
    } catch (e) {
      setErr(apiErrorMessage(e, t('schedule.erroEntrar')))
      setEntering(null)
    }
  }

  async function meetNow() {
    setErr('')
    setMeetNowBusy(true)
    try {
      const room = await createRoom(t('schedule.reuniaoAgoraNome', { hora: fmtTime(new Date(), locale), nome: user.username }), 'sfu')
      enterRoom(room.code)
    } catch (e) {
      setErr(apiErrorMessage(e, t('schedule.erroReuniaoAgora')))
      setMeetNowBusy(false)
    }
  }

  async function confirmDelete() {
    if (!toDelete) return
    setDeleting(true)
    try {
      await deleteMeeting(toDelete.id)
      setToDelete(null)
      reload()
    } catch (e) {
      setErr(apiErrorMessage(e, t('schedule.detalhe.erroEliminar')))
      setToDelete(null)
    } finally {
      setDeleting(false)
    }
  }

  function move(delta: number) {
    setCursor((c) =>
      view === 'day'
        ? addDays(c, delta)
        : view === 'week'
          ? addDays(c, delta * 7)
          : view === 'month'
            ? new Date(c.getFullYear(), c.getMonth() + delta, 1)
            : new Date(c.getFullYear() + delta, c.getMonth(), 1),
    )
  }
  const showDay = (d: Date) => {
    setCursor(startOfDay(d))
    setView('day')
  }
  const scheduleAt = (d: Date, withTime = false) => go(calendarHash.schedule(ymd(d), withTime ? hhmm(d) : undefined))

  // ------------------------------------------------------------- agendar
  if (route.kind === 'schedule') {
    return (
      <>
        <PageBar
          title={t('schedule.form.tituloPagina')}
          meta={odooCalendar ? t('consola.agenda.metaOdoo') : t('schedule.form.metaPagina')}
        >
          <Button variant="secondary" className="cal-hide-narrow" onClick={() => go(calendarHash.browse())}>
            {t('ui.cancelar')}
          </Button>
          <IconButton icon="x" className="cal-show-narrow" label={t('ui.cancelar')} onClick={() => go(calendarHash.browse())} />
          <Button type="submit" form={FORM_ID} variant="primary" icon="send" busy={formState.busy} disabled={formState.blocked}>
            {formState.blocked ? t('schedule.form.salaIndisponivel') : t('schedule.form.guardar')}
          </Button>
        </PageBar>
        <div className="page sched-page">
          <ScheduleForm
            formId={FORM_ID}
            initialDate={route.date}
            initialTime={route.time}
            meetings={state.s === 'ready' ? state.d : []}
            onBusy={onFormBusy}
            onCreated={(date) => {
              setCursor(parseYmd(date) ?? startOfDay(new Date()))
              reload()
              go(calendarHash.browse())
            }}
          />
        </div>
      </>
    )
  }

  // ---------------------------------------------------------------- ver
  const weekStart = mondayOf(cursor)
  const rangeLabel =
    view === 'day'
      ? cursor.toLocaleDateString(locale, { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' })
      : view === 'week'
        ? t('schedule.vista.intervalo', {
            de: weekStart.toLocaleDateString(locale, { day: 'numeric', month: 'short' }),
            ate: addDays(weekStart, 6).toLocaleDateString(locale, { day: 'numeric', month: 'short', year: 'numeric' }),
          })
        : view === 'month'
          ? cursor.toLocaleDateString(locale, { month: 'long', year: 'numeric' })
          : view === 'year'
            ? String(cursor.getFullYear())
            : t('schedule.vista.deHojeEmDiante')

  const detailMeeting =
    route.kind === 'meeting' && state.s === 'ready' ? (state.d.find((m) => m.id === route.id) ?? null) : undefined

  return (
    <>
      <PageBar title={t('schedule.tituloPagina')} meta={rangeLabel}>
        <Button variant="secondary" icon="video" busy={meetNowBusy} onClick={() => void meetNow()} className="cal-hide-narrow">
          {t('schedule.accoes.reuniaoAgora')}
        </Button>
        <Button variant="primary" icon="plus" onClick={() => go(calendarHash.schedule(view === 'list' ? undefined : ymd(cursor)))}>
          {t('schedule.accoes.agendar')}
        </Button>
      </PageBar>
      <div className="page cal-page">
        <div className="cal-toolbar">
          {view !== 'list' && (
            <div className="cal-toolbar__nav">
              <Button
                variant="secondary"
                size="sm"
                onClick={() => {
                  setCursor(startOfDay(new Date()))
                  if (view === 'year') setView('month')
                }}
              >
                {t('schedule.vista.hoje')}
              </Button>
              <IconButton icon="chevronLeft" label={t('schedule.vista.anterior')} onClick={() => move(-1)} />
              <IconButton icon="chevronRight" label={t('schedule.vista.seguinte')} onClick={() => move(1)} />
              <h2 className="cal-toolbar__label">{rangeLabel}</h2>
            </div>
          )}
          <span className="dx-spacer" />
          <Segmented<View>
            label={t('schedule.vista.rotulo')}
            value={view}
            onChange={setView}
            options={[
              { value: 'day', label: t('schedule.vista.dia') },
              { value: 'week', label: t('schedule.vista.semana') },
              { value: 'month', label: t('schedule.vista.mes') },
              { value: 'year', label: t('schedule.vista.ano') },
              { value: 'list', label: t('schedule.vista.lista') },
            ]}
          />
        </div>

        {err && <Alert tone="danger">{err}</Alert>}

        <AsyncSection
          state={state}
          onRetry={reload}
          skeleton={
            <div className="cal-panel" aria-busy="true" style={{ padding: 14, display: 'grid', gap: 10 }}>
              <Skeleton h={18} w="40%" />
              <Skeleton h={220} />
            </div>
          }
        >
          {(meetings) => (
            <>
              {(view === 'week' || view === 'day') && (
                <WeekView
                  days={view === 'day' ? [cursor] : Array.from({ length: 7 }, (_, i) => addDays(weekStart, i))}
                  byDay={byDay}
                  onOpen={openMeeting}
                  onSlot={(d) => scheduleAt(d, true)}
                  onDay={view === 'week' ? showDay : undefined}
                />
              )}
              {view === 'month' && (
                <MonthView cursor={cursor} byDay={byDay} onOpen={openMeeting} onNewOnDay={(d) => scheduleAt(d)} onShowDay={showDay} />
              )}
              {view === 'year' && (
                <YearView
                  year={cursor.getFullYear()}
                  byDay={byDay}
                  onMonth={(d) => {
                    setCursor(d)
                    setView('month')
                  }}
                  onShowDay={showDay}
                />
              )}
              {view === 'list' && (
                <ListView
                  meetings={meetings}
                  onOpen={openMeeting}
                  onEnter={(m) => void enter(m)}
                  onDelete={setToDelete}
                  entering={entering}
                  onSchedule={() => go(calendarHash.schedule())}
                />
              )}
            </>
          )}
        </AsyncSection>
      </div>

      {route.kind === 'meeting' && detailMeeting !== undefined && (
        <MeetingDialog
          key={route.id}
          meeting={detailMeeting}
          onClose={closeDialog}
          onChanged={reload}
          onEnter={(m) => void enter(m)}
          entering={entering === route.id}
          occurrence={detailMeeting && state.s === 'ready' ? occurrenceOf(state.d, detailMeeting) : null}
        />
      )}

      {toDelete && (
        <Dialog
          title={t('schedule.detalhe.eliminarTitulo')}
          onClose={cancelDelete}
          footer={
            <>
              <Button variant="ghost" onClick={cancelDelete}>
                {t('ui.cancelar')}
              </Button>
              <Button variant="danger" icon="trash" busy={deleting} onClick={() => void confirmDelete()}>
                {t('schedule.evento.eliminar')}
              </Button>
            </>
          }
        >
          <p style={{ margin: 0 }}>{t('schedule.detalhe.eliminarTexto', { titulo: toDelete.title })}</p>
        </Dialog>
      )}
    </>
  )
}
