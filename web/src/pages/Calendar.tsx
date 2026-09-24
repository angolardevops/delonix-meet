/**
 * Agenda — ver as reuniões (dia, semana, mês, ano, lista), agendar, e abrir o
 * detalhe de uma reunião.
 *
 * O estado vive na rota, para o botão «voltar» e os links da Início
 * funcionarem: `#/calendar` (ver), `#/calendar/new?d=AAAA-MM-DD&t=HHMM`
 * (agendar) e `#/calendar/m/<id>` (detalhe por cima da vista).
 *
 * Pesquisa estilo Odoo (`ui/search`, recurso `meetings`): na vista Lista é a
 * lista paginada e agrupável; nas vistas de calendário filtra o que a grelha
 * mostra. O estado da pesquisa viaja na query do hash, também ao abrir e
 * fechar o detalhe de uma reunião.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, createRoom, deleteMeeting, listMeetings, Meeting, startMeeting } from '../api'
import { AsyncSection, useAsync } from '../components/AsyncSection'
import PageBar from '../components/PageBar'
import { useShell } from '../components/shellContext'
import { Alert, Button, Dialog, IconButton, Segmented, Skeleton } from '../ui/kit'
import { hashParams, isEmptySearch } from '../ui/search/model'
import { SearchBar, SearchResults } from '../ui/search/SearchResults'
import { useResourceSearch } from '../ui/search/useResourceSearch'
import '../ui/schedule.css'
import ListView from './calendar/ListView'
import MeetingDialog from './calendar/MeetingDialog'
import { MonthView, YearView } from './calendar/MonthView'
import { occurrenceOf } from './calendar/occurrence'
import ScheduleForm from './calendar/ScheduleForm'
import { meetingsFallback, useMeetingsMatching } from './calendar/search'
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
  // «Ver todos em Agenda» da pesquisa global abre a Lista.
  if (hashParams().get('vista') === 'lista') return 'list'
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

  const { state, reload: reloadAll } = useAsync((signal) => listMeetings(signal), [])
  const rs = useResourceSearch<Meeting>({ resource: 'meetings', fallback: meetingsFallback })
  const reloadSearch = rs.reload
  const reload = useCallback(() => {
    reloadAll()
    reloadSearch()
  }, [reloadAll, reloadSearch])

  // O intervalo que a grelha mostra — a pesquisa pede só esse.
  const range = useMemo(() => {
    if (view === 'day') return [cursor, addDays(cursor, 1)] as const
    if (view === 'week') return [mondayOf(cursor), addDays(mondayOf(cursor), 7)] as const
    if (view === 'month') {
      const first = mondayOf(new Date(cursor.getFullYear(), cursor.getMonth(), 1))
      return [first, addDays(first, 42)] as const
    }
    return [new Date(cursor.getFullYear(), 0, 1), new Date(cursor.getFullYear() + 1, 0, 1)] as const
  }, [view, cursor])
  const matching = useMeetingsMatching(rs, range[0], range[1])
  const byDay = useMemo(
    () => groupByDay(matching ? (matching.s === 'ready' ? matching.d : []) : state.s === 'ready' ? state.d : []),
    [state, matching],
  )

  const go = (hash: string) => {
    location.hash = hash
  }
  // A query do hash (a pesquisa) acompanha o detalhe, para voltar à mesma vista.
  const withQuery = (path: string) => {
    const qs = hashParams().toString()
    return qs ? `${path}?${qs}` : path
  }
  const openMeeting = useCallback((m: Meeting) => go(withQuery(calendarHash.meeting(m.id))), [])
  const closeDialog = useCallback(() => go(withQuery(calendarHash.browse())), [])

  function changeView(v: View) {
    setView(v)
    // A Lista abre, como antes, nas próximas — agora como filtro visível e removível.
    if (v === 'list' && isEmptySearch(rs.search)) rs.setSearch({ ...rs.search, filters: ['upcoming'] }, { replace: true })
  }
  const listDefaulted = useRef(false)
  useEffect(() => {
    if (listDefaulted.current || view !== 'list' || !rs.schema) return
    listDefaulted.current = true
    if (isEmptySearch(rs.search) && rs.schema.filters.some((f) => f.name === 'upcoming')) rs.setSearch({ ...rs.search, filters: ['upcoming'] }, { replace: true })
  }, [view, rs])
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
          meta={<span className="cal-metachip">{odooCalendar ? t('consola.agenda.metaOdooCurta') : t('schedule.form.metaPagina')}</span>}
        >
          <Button variant="secondary" className="cal-hide-narrow" onClick={() => go(calendarHash.browse())}>
            {t('ui.cancelar')}
          </Button>
          <IconButton icon="x" className="cal-show-narrow" label={t('ui.cancelar')} onClick={() => go(calendarHash.browse())} />
          <Button type="submit" form={FORM_ID} variant="primary" busy={formState.busy} disabled={formState.blocked}>
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
            : rs.list.state.s === 'ready'
              ? t('search.grupos.registos', { count: rs.list.state.d.total })
              : ''

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
          <SearchBar rs={rs} label={t('search.rotulos.meetings')} pager={view === 'list'} className="cal-search" />
          <Segmented<View>
            label={t('schedule.vista.rotulo')}
            value={view}
            onChange={changeView}
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
        {matching?.s === 'error' && view !== 'list' && <Alert tone="danger">{matching.msg}</Alert>}

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
          {() => (
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
                <div className="cal-results">
                  <SearchResults
                    rs={rs}
                    emptyIcon="calendar"
                    emptyTitle={t('schedule.lista.vazio')}
                    emptyAction={
                      <Button size="sm" icon="plus" onClick={() => go(calendarHash.schedule())}>
                        {t('schedule.accoes.agendar')}
                      </Button>
                    }
                    renderItems={(items) => (
                      <ListView
                        meetings={items}
                        all
                        onOpen={openMeeting}
                        onEnter={(m) => void enter(m)}
                        onDelete={setToDelete}
                        entering={entering}
                        onSchedule={() => go(calendarHash.schedule())}
                      />
                    )}
                  />
                </div>
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
