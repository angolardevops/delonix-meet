import { FormEvent, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  ActionPlan,
  addActionItem,
  addAgendaItem,
  AgendaItem,
  checkConflicts,
  Conflicts,
  createMeeting,
  createRoom,
  deleteActionItem,
  deleteAgendaItem,
  deleteMeeting,
  getActionPlan,
  InviteeResponse,
  listAgenda,
  listMeetingRooms,
  listMeetings,
  Meeting,
  MeetingRoom,
  meetingInvitees,
  myOrgs,
  patchActionItem,
  patchAgendaItem,
  RecurrenceFreq,
  respondMeeting,
  searchUsers,
  startMeeting,
  upsertActionPlan,
  downloadMeetingIcs,
  User,
} from '../api'
import { ChevronDownIcon, ChevronLeftIcon, ChevronRightIcon, ChevronUpIcon, ClockIcon, CloseIcon, EditIcon, PlusIcon, RepeatIcon, TrashIcon, VideoIcon, VoiceCallIcon } from '../icons'

/** Carrega as salas presenciais de todas as organizações do utilizador. */
async function loadAllRooms(): Promise<MeetingRoom[]> {
  const orgs = await myOrgs().catch(() => [])
  const lists = await Promise.all(orgs.map((o) => listMeetingRooms(o.id).catch(() => [])))
  return lists.flat()
}

type View = 'month' | 'week' | 'day' | 'year' | 'agenda'

const HOUR_H = 56 // px per hour in timeline views
const HOURS = Array.from({ length: 24 }, (_, i) => i)

const WEEKDAYS_FALLBACK = ['Seg', 'Ter', 'Qua', 'Qui', 'Sex', 'Sáb', 'Dom']
const MONTHS_FALLBACK = [
  'Janeiro', 'Fevereiro', 'Março', 'Abril', 'Maio', 'Junho',
  'Julho', 'Agosto', 'Setembro', 'Outubro', 'Novembro', 'Dezembro',
]

// ---- date helpers (semana começa à segunda) ----
const startOfDay = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate())
const addDays = (d: Date, n: number) => { const x = new Date(d); x.setDate(x.getDate() + n); return x }
const sameDay = (a: Date, b: Date) => a.toDateString() === b.toDateString()
const mondayOf = (d: Date) => { const x = startOfDay(d); const wd = (x.getDay() + 6) % 7; return addDays(x, -wd) }
const ymd = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
const hm = (iso: string) => new Date(iso).toLocaleTimeString('pt-PT', { hour: '2-digit', minute: '2-digit' })

type HoverState = { meeting: Meeting; x: number; y: number } | null

export default function Calendar({ onEnterRoom }: { onEnterRoom: (code: string, voice?: boolean) => void }) {
  const { t: tRaw, i18n } = useTranslation()
  const t = (k: string, opts?: Record<string, unknown>) => tRaw(`cal.${k}`, opts ?? {})
  const MONTHS = Array.isArray(tRaw('cal.months', { returnObjects: true })) ? tRaw('cal.months', { returnObjects: true }) as string[] : MONTHS_FALLBACK
  const [meetings, setMeetings] = useState<Meeting[]>([])
  const [loading, setLoading] = useState(true)
  const [view, setView] = useState<View>('month')
  const [cursor, setCursor] = useState(() => startOfDay(new Date()))
  const [schedule, setSchedule] = useState<{ date: string } | null>(null)
  const [selected, setSelected] = useState<Meeting | null>(null)
  const [error, setError] = useState('')
  const [meetNowBusy, setMeetNowBusy] = useState(false)
  const [hover, setHover] = useState<HoverState>(null)

  async function meetNow() {
    setMeetNowBusy(true)
    try {
      const now = new Date()
      const label = `${t('meetNow')} ${now.toLocaleTimeString(i18n.language, { hour: '2-digit', minute: '2-digit' })}`
      const room = await createRoom(label, 'sfu')
      onEnterRoom(room.code)
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setMeetNowBusy(false)
    }
  }

  async function refresh() {
    try {
      setMeetings(await listMeetings())
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setLoading(false)
    }
  }
  useEffect(() => {
    void refresh()
  }, [])

  const today = startOfDay(new Date())

  // Eventos por dia (YYYY-MM-DD -> ordenados por hora).
  const byDay = useMemo(() => {
    const map = new Map<string, Meeting[]>()
    for (const m of meetings) {
      const k = ymd(new Date(m.starts_at))
      map.set(k, [...(map.get(k) ?? []), m])
    }
    for (const list of map.values()) list.sort((a, b) => +new Date(a.starts_at) - +new Date(b.starts_at))
    return map
  }, [meetings])

  async function start(m: Meeting) {
    try {
      const { code, kind } = await startMeeting(m.id)
      onEnterRoom(code, kind === 'voice')
    } catch (e) {
      setError((e as Error).message)
    }
  }
  async function remove(m: Meeting) {
    await deleteMeeting(m.id).catch(() => {})
    setSelected(null)
    void refresh()
  }

  function move(delta: number) {
    if (view === 'week') setCursor((c) => addDays(c, delta * 7))
    else if (view === 'day') setCursor((c) => addDays(c, delta))
    else if (view === 'month') setCursor((c) => new Date(c.getFullYear(), c.getMonth() + delta, 1))
    else if (view === 'year') setCursor((c) => new Date(c.getFullYear() + delta, c.getMonth(), 1))
    else setCursor((c) => addDays(c, delta * 30))
  }

  const title =
    view === 'week'
      ? (() => {
          const s = mondayOf(cursor)
          const e = addDays(s, 6)
          return `${s.getDate()} ${MONTHS[s.getMonth()].slice(0, 3)} – ${e.getDate()} ${MONTHS[e.getMonth()].slice(0, 3)} ${e.getFullYear()}`
        })()
      : view === 'day'
        ? cursor.toLocaleDateString(i18n.language, { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' })
        : view === 'year'
          ? String(cursor.getFullYear())
          : `${MONTHS[cursor.getMonth()]} ${cursor.getFullYear()}`

  function openEvent(m: Meeting) { setSelected(m); setHover(null) }

  return (
    <div className="page cal-page" onMouseLeave={() => setHover(null)}>
      <header className="cal-toolbar">
        <div className="cal-nav">
          <button className="btn-today" onClick={() => { setCursor(startOfDay(new Date())); if (view === 'year') setView('month') }}>{t('today')}</button>
          <button className="icon-btn cal-arrow" onClick={() => move(-1)}><ChevronLeftIcon /></button>
          <button className="icon-btn cal-arrow" onClick={() => move(1)}><ChevronRightIcon /></button>
          <h1 className="cal-title">{title}</h1>
        </div>
        <div className="cal-right">
          <div className="seg">
            <button className={view === 'day' ? 'seg-btn active' : 'seg-btn'} onClick={() => setView('day')}>{t('day')}</button>
            <button className={view === 'week' ? 'seg-btn active' : 'seg-btn'} onClick={() => setView('week')}>{t('week')}</button>
            <button className={view === 'month' ? 'seg-btn active' : 'seg-btn'} onClick={() => setView('month')}>{t('month')}</button>
            <button className={view === 'year' ? 'seg-btn active' : 'seg-btn'} onClick={() => setView('year')}>{t('year')}</button>
            <button className={view === 'agenda' ? 'seg-btn active' : 'seg-btn'} onClick={() => setView('agenda')}>{t('agenda')}</button>
          </div>
          <button className="btn-meet-now small" disabled={meetNowBusy} onClick={() => void meetNow()}>
            <VideoIcon /> {meetNowBusy ? '…' : t('meetNow')}
          </button>
          <button className="btn-new small" onClick={() => setSchedule({ date: ymd(cursor) })}>
            <PlusIcon /> {t('schedule')}
          </button>
        </div>
      </header>

      {error && <div className="error">{error}</div>}
      {loading && <p className="muted">{t('loading')}</p>}

      {!loading && view === 'month' && (
        <MonthGrid cursor={cursor} today={today} byDay={byDay}
          onDayClick={(d) => setSchedule({ date: ymd(d) })}
          onEvent={openEvent}
          onHover={setHover} />
      )}
      {!loading && (view === 'week' || view === 'day') && (
        <TimelineView
          days={view === 'day' ? [cursor] : Array.from({ length: 7 }, (_, i) => addDays(mondayOf(cursor), i))}
          today={today} byDay={byDay}
          onSlot={(d) => setSchedule({ date: ymd(d) })}
          onEvent={openEvent}
          onHover={setHover} />
      )}
      {!loading && view === 'year' && (
        <YearView year={cursor.getFullYear()} today={today} byDay={byDay}
          onMonth={(d) => { setCursor(d); setView('month') }}
          onEvent={openEvent}
          onHover={setHover} />
      )}
      {!loading && view === 'agenda' && (
        <AgendaView meetings={meetings} onStart={start} onRemove={remove} />
      )}

      {hover && <MeetingPopover state={hover} onOpen={openEvent} onClose={() => setHover(null)} />}

      {schedule && (
        <ScheduleModal
          initialDate={schedule.date}
          onClose={() => setSchedule(null)}
          onCreated={() => { setSchedule(null); void refresh() }}
        />
      )}
      {selected && (
        <EventModal
          meeting={selected}
          onClose={() => setSelected(null)}
          onStart={start}
          onRemove={remove}
          onChanged={refresh}
        />
      )}
    </div>
  )
}

function MonthGrid({
  cursor, today, byDay, onDayClick, onEvent, onHover,
}: {
  cursor: Date
  today: Date
  byDay: Map<string, Meeting[]>
  onDayClick: (d: Date) => void
  onEvent: (m: Meeting) => void
  onHover: (h: HoverState) => void
}) {
  const { t: tRaw } = useTranslation()
  const t = (k: string, opts?: Record<string, unknown>) => tRaw(`cal.${k}`, opts ?? {})
  const WEEKDAYS = Array.isArray(tRaw('cal.weekdays', { returnObjects: true })) ? tRaw('cal.weekdays', { returnObjects: true }) as string[] : WEEKDAYS_FALLBACK
  const first = new Date(cursor.getFullYear(), cursor.getMonth(), 1)
  const gridStart = mondayOf(first)
  const days = Array.from({ length: 42 }, (_, i) => addDays(gridStart, i))
  return (
    <div className="month">
      <div className="month-head">
        {WEEKDAYS.map((w) => <div key={w} className="month-hcell">{w}</div>)}
      </div>
      <div className="month-grid">
        {days.map((d) => {
          const events = byDay.get(ymd(d)) ?? []
          const inMonth = d.getMonth() === cursor.getMonth()
          const isToday = sameDay(d, today)
          return (
            <div
              key={d.toISOString()}
              className={`month-cell${inMonth ? '' : ' out'}${isToday ? ' today' : ''}`}
              onClick={() => onDayClick(d)}
            >
              <div className="month-daynum">
                <span className={isToday ? 'daynum today' : 'daynum'}>{d.getDate()}</span>
              </div>
              <div className="month-events">
                {events.slice(0, 3).map((m) => (
                  <button
                    key={m.id}
                    className={`ev-chip ${m.kind}`}
                    onClick={(e) => { e.stopPropagation(); onEvent(m) }}
                    onMouseEnter={(e) => onHover({ meeting: m, x: e.clientX, y: e.clientY })}
                    onMouseLeave={() => onHover(null)}
                  >
                    <span className="ev-dot" />
                    <span className="ev-time">{hm(m.starts_at)}</span>
                    <span className="ev-title">{m.title}</span>
                    {m.recurrence_freq && <span className="ev-recur" title={t('recur')}><RepeatIcon /></span>}
                  </button>
                ))}
                {events.length > 3 && <div className="ev-more">+{events.length - 3}</div>}
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}

function TimelineView({
  days, today, byDay, onSlot, onEvent, onHover,
}: {
  days: Date[]
  today: Date
  byDay: Map<string, Meeting[]>
  onSlot: (d: Date) => void
  onEvent: (m: Meeting) => void
  onHover: (h: HoverState) => void
}) {
  const { t: tRaw } = useTranslation()
  const t = (k: string, opts?: Record<string, unknown>) => tRaw(`cal.${k}`, opts ?? {})
  const WEEKDAYS = Array.isArray(tRaw('cal.weekdays', { returnObjects: true })) ? tRaw('cal.weekdays', { returnObjects: true }) as string[] : WEEKDAYS_FALLBACK
  const scrollRef = useRef<HTMLDivElement>(null)

  // Scroll to 05:00 (ensures morning meetings são sempre visíveis sem scroll)
  useEffect(() => {
    if (scrollRef.current) scrollRef.current.scrollTop = 5 * HOUR_H
  }, [days.length])

  const now = new Date()
  const todayStr = ymd(today)
  const nowTop = (now.getHours() * 60 + now.getMinutes()) * HOUR_H / 60

  return (
    <div className="tl-wrap">
      {/* Sticky header */}
      <div className="tl-head">
        <div className="tl-gutter-head" />
        {days.map((d) => {
          const isToday = sameDay(d, today)
          return (
            <div key={ymd(d)} className={`tl-col-head${isToday ? ' today' : ''}`} onClick={() => onSlot(d)}>
              <span className="tl-wd">{WEEKDAYS[(d.getDay() + 6) % 7]}</span>
              <span className={`tl-num${isToday ? ' today' : ''}`}>{d.getDate()}</span>
            </div>
          )
        })}
      </div>

      {/* Scrollable body */}
      <div className="tl-body" ref={scrollRef}>
        <div className="tl-inner" style={{ height: HOUR_H * 24 }}>
          {/* Hour gutter */}
          <div className="tl-gutter">
            {HOURS.map((h) => (
              <div key={h} className="tl-hour-label" style={{ top: h * HOUR_H }}>
                {h > 0 ? `${String(h).padStart(2, '0')}:00` : ''}
              </div>
            ))}
          </div>

          {/* Day columns */}
          {days.map((d) => {
            const key = ymd(d)
            const events = byDay.get(key) ?? []
            const isToday = key === todayStr
            return (
              <div
                key={key}
                className={`tl-day-col${isToday ? ' today' : ''}`}
                onClick={(e) => {
                  const rect = e.currentTarget.getBoundingClientRect()
                  const y = e.clientY - rect.top + (scrollRef.current?.scrollTop ?? 0)
                  const totalMin = Math.floor((y / HOUR_H) * 60)
                  const h = Math.floor(totalMin / 60)
                  const min = Math.round((totalMin % 60) / 30) * 30
                  const slot = new Date(d); slot.setHours(h, min, 0, 0)
                  onSlot(slot)
                }}
              >
                {/* Hour grid lines */}
                {HOURS.map((h) => (
                  <div key={h} className="tl-hline" style={{ top: h * HOUR_H }} />
                ))}
                {/* Half-hour lines */}
                {HOURS.map((h) => (
                  <div key={`${h}h`} className="tl-hline half" style={{ top: h * HOUR_H + HOUR_H / 2 }} />
                ))}
                {/* Current time */}
                {isToday && <div className="tl-now" style={{ top: nowTop }} />}
                {/* Events */}
                {events.map((m) => {
                  const s = new Date(m.starts_at)
                  const topPx = (s.getHours() * 60 + s.getMinutes()) * HOUR_H / 60
                  const heightPx = Math.max(m.duration_min * HOUR_H / 60, 22)
                  return (
                    <button
                      key={m.id}
                      className={`tl-event ${m.kind}`}
                      style={{ top: topPx, height: heightPx }}
                      onClick={(e) => { e.stopPropagation(); onEvent(m) }}
                      onMouseEnter={(e) => onHover({ meeting: m, x: e.clientX, y: e.clientY })}
                      onMouseLeave={() => onHover(null)}
                    >
                      <strong className="tl-ev-title">{m.title}{m.recurrence_freq && <span className="ev-recur" title={t('recur')}><RepeatIcon /></span>}</strong>
                      {heightPx > 38 && <span className="tl-ev-time">{hm(m.starts_at)}</span>}
                    </button>
                  )
                })}
              </div>
            )
          })}
        </div>
      </div>
    </div>
  )
}

function YearView({
  year, today, byDay, onMonth, onEvent, onHover,
}: {
  year: number
  today: Date
  byDay: Map<string, Meeting[]>
  onMonth: (d: Date) => void
  onEvent: (m: Meeting) => void
  onHover: (h: HoverState) => void
}) {
  const { t: tRaw } = useTranslation()
  const WEEKDAYS = Array.isArray(tRaw('cal.weekdays', { returnObjects: true })) ? tRaw('cal.weekdays', { returnObjects: true }) as string[] : WEEKDAYS_FALLBACK
  const MONTHS = Array.isArray(tRaw('cal.months', { returnObjects: true })) ? tRaw('cal.months', { returnObjects: true }) as string[] : MONTHS_FALLBACK
  return (
    <div className="year-grid">
      {Array.from({ length: 12 }, (_, mi) => {
        const first = new Date(year, mi, 1)
        const gridStart = mondayOf(first)
        const days = Array.from({ length: 42 }, (_, i) => addDays(gridStart, i))
        return (
          <div key={mi} className="year-month" onClick={() => onMonth(first)}>
            <div className="year-month-name">{MONTHS[mi]} {year}</div>
            <div className="year-mini-head">
              {WEEKDAYS.map((w) => <span key={w}>{w.charAt(0)}</span>)}
            </div>
            <div className="year-mini-grid">
              {days.map((d) => {
                const key = ymd(d)
                const inMonth = d.getMonth() === mi
                const isToday = sameDay(d, today)
                const events = byDay.get(key) ?? []
                const hasEvents = inMonth && events.length > 0
                return (
                  <span
                    key={key}
                    className={[
                      'year-day',
                      inMonth ? '' : 'out',
                      isToday ? 'today' : '',
                      hasEvents ? 'has-events' : '',
                    ].filter(Boolean).join(' ')}
                    onMouseEnter={hasEvents ? (e) => onHover({ meeting: events[0], x: e.clientX, y: e.clientY }) : undefined}
                    onMouseLeave={hasEvents ? () => onHover(null) : undefined}
                    onClick={(e) => { if (hasEvents) { e.stopPropagation(); onEvent(events[0]) } }}
                  >
                    {d.getDate()}
                    {hasEvents && events.length > 1 && <sub>{events.length}</sub>}
                  </span>
                )
              })}
            </div>
          </div>
        )
      })}
    </div>
  )
}

function MeetingPopover({
  state, onOpen, onClose,
}: {
  state: NonNullable<HoverState>
  onOpen: (m: Meeting) => void
  onClose: () => void
}) {
  const { t: tRaw, i18n } = useTranslation()
  const t = (k: string, opts?: Record<string, unknown>) => tRaw(`cal.${k}`, opts ?? {})
  const { meeting: m, x, y } = state
  const start = new Date(m.starts_at)
  const end = new Date(+start + m.duration_min * 60000)
  const fmt = (d: Date) => d.toLocaleTimeString(i18n.language, { hour: '2-digit', minute: '2-digit' })

  // Clamp to viewport
  const left = Math.min(x + 12, window.innerWidth - 300)
  const top = Math.min(y + 8, window.innerHeight - 220)

  return (
    <div
      className="meet-popover"
      style={{ left, top }}
      onMouseEnter={onClose} // close if user moves to popover instead of the event
    >
      <div className={`meet-popover-accent ${m.kind}`} />
      <div className="meet-popover-body">
        <strong className="meet-popover-title">{m.title}</strong>
        {m.recurrence_freq && <span className="meet-popover-recur"><RepeatIcon /> {t('recur')}</span>}
        <div className="meet-popover-when">
          {start.toLocaleDateString(i18n.language, { weekday: 'short', day: 'numeric', month: 'short' })}
          {' · '}
          {fmt(start)} – {fmt(end)}
          {' · '}
          {m.duration_min} min
        </div>
        {m.description && <p className="meet-popover-desc">{m.description}</p>}
        <div className="meet-popover-who">
          {m.is_owner
            ? <span className="meet-popover-role owner">{t('organized')}</span>
            : <span className="meet-popover-role">{t('organizedBy', { name: m.owner_name })}</span>}
        </div>
        <button className="btn-sm meet-popover-open" onClick={() => onOpen(m)}>
          {m.kind === 'voice' ? <VoiceCallIcon /> : <VideoIcon />} {t('tabDetails')}
        </button>
      </div>
    </div>
  )
}

function AgendaView({
  meetings, onStart, onRemove,
}: {
  meetings: Meeting[]
  onStart: (m: Meeting) => void
  onRemove: (m: Meeting) => void
}) {
  const { t: tRaw, i18n } = useTranslation()
  const t = (k: string, opts?: Record<string, unknown>) => tRaw(`cal.${k}`, opts ?? {})
  const groups = useMemo(() => {
    // Mostra todas as reuniões de hoje em diante (incluindo as que já terminaram hoje)
    const todayKey = ymd(startOfDay(new Date()))
    const upcoming = meetings
      .filter((m) => ymd(new Date(m.starts_at)) >= todayKey)
      .sort((a, b) => +new Date(a.starts_at) - +new Date(b.starts_at))
    const map = new Map<string, Meeting[]>()
    for (const m of upcoming) {
      const k = new Date(m.starts_at).toLocaleDateString(i18n.language, { weekday: 'long', day: 'numeric', month: 'long' })
      map.set(k, [...(map.get(k) ?? []), m])
    }
    return [...map.entries()]
  }, [meetings])

  if (groups.length === 0) {
    return <div className="empty-state"><ClockIcon /><p>{t('noUpcoming')}</p></div>
  }
  return (
    <div className="agenda-list">
      {groups.map(([day, list]) => (
        <div key={day} className="agenda-day">
          <div className="agenda-date">{day}</div>
          <div className="agenda-items">
            {list.map((m) => (
              <div key={m.id} className="agenda-item">
                <span className={`agenda-kind ${m.kind}`}>{m.kind === 'voice' ? <VoiceCallIcon /> : <VideoIcon />}</span>
                <span className="agenda-time">{hm(m.starts_at)}</span>
                <span className="agenda-info">
                  <strong>{m.title}{m.recurrence_freq && <span className="ev-recur" title={t('recur')}><RepeatIcon /></span>}</strong>
                  <small>{m.duration_min} min · {m.is_owner ? t('organized') : t('organizedBy', { name: m.owner_name })}{m.description ? ` · ${m.description}` : ''}</small>
                </span>
                <button className="btn-sm" onClick={() => onStart(m)}>
                  {m.kind === 'voice' ? <VoiceCallIcon /> : <VideoIcon />}{m.is_owner ? t('start') : t('join')}
                </button>
                {m.is_owner && <button className="icon-btn" title="Cancelar" onClick={() => onRemove(m)}><TrashIcon /></button>}
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  )
}

// ─── Agenda Panel ────────────────────────────────────────────────────────────

function AgendaPanel({ meetingId, isOwner }: { meetingId: string; isOwner: boolean }) {
  const { t: tRaw } = useTranslation()
  const t = (k: string, opts?: Record<string, unknown>) => tRaw(`cal.${k}`, opts ?? {})
  const [items, setItems] = useState<AgendaItem[]>([])
  const [loading, setLoading] = useState(true)
  const [newTopic, setNewTopic] = useState('')
  const [newDuration, setNewDuration] = useState(5)
  const [adding, setAdding] = useState(false)

  function reload() {
    void listAgenda(meetingId).then(setItems).catch(() => {}).finally(() => setLoading(false))
  }
  useEffect(() => { reload() }, [meetingId]) // eslint-disable-line react-hooks/exhaustive-deps

  async function add() {
    if (!newTopic.trim()) return
    setAdding(true)
    try {
      await addAgendaItem(meetingId, { topic: newTopic.trim(), duration_min: newDuration })
      setNewTopic('')
      setNewDuration(5)
      reload()
    } finally { setAdding(false) }
  }

  async function toggleDone(item: AgendaItem) {
    await patchAgendaItem(meetingId, item.id, { done: !item.done }).catch(() => {})
    reload()
  }

  async function remove(itemId: string) {
    await deleteAgendaItem(meetingId, itemId).catch(() => {})
    reload()
  }

  if (loading) return <p className="muted small">{t('agendaLoading')}</p>

  return (
    <div className="agenda-panel">
      <ul className="agenda-list">
        {items.length === 0 && <li className="muted small">{t('agendaEmpty')}</li>}
        {items.map((item) => (
          <li key={item.id} className={`agenda-topic${item.done ? ' done' : ''}`}>
            <button
              className={`agenda-check${item.done ? ' checked' : ''}`}
              title={item.done ? t('agendaMarkPending') : t('agendaMarkDone')}
              onClick={() => void toggleDone(item)}
            >
              {item.done ? <ChevronUpIcon /> : <ChevronDownIcon />}
            </button>
            <span className="agenda-topic-text">
              <strong>{item.topic}</strong>
              {item.description && <small className="muted">{item.description}</small>}
            </span>
            <span className="agenda-duration">{item.duration_min} min</span>
            {isOwner && (
              <button className="agenda-del" title={t('agendaRemove')} onClick={() => void remove(item.id)}>
                <TrashIcon />
              </button>
            )}
          </li>
        ))}
      </ul>
      {isOwner && (
        <div className="agenda-add-row">
          <input
            className="agenda-add-input"
            placeholder={t('agendaNewPlaceholder')}
            value={newTopic}
            onChange={(e) => setNewTopic(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter') void add() }}
          />
          <input
            type="number"
            className="agenda-add-dur"
            min={1}
            max={180}
            value={newDuration}
            onChange={(e) => setNewDuration(Number(e.target.value))}
            title={t('agendaDurLabel')}
          />
          <button className="btn-sm" disabled={adding || !newTopic.trim()} onClick={() => void add()}>
            <PlusIcon /> {t('agendaAdd')}
          </button>
        </div>
      )}
    </div>
  )
}

// ─── Action Plan Panel (5W2H) ────────────────────────────────────────────────

function ActionPlanPanel({ meetingId, isOwner }: { meetingId: string; isOwner: boolean }) {
  const { t: tRaw } = useTranslation()
  const t = (k: string, opts?: Record<string, unknown>) => tRaw(`cal.${k}`, opts ?? {})
  const [plan, setPlan] = useState<ActionPlan | null>(null)
  const [loading, setLoading] = useState(true)
  const [goal, setGoal] = useState('')
  const [editingGoal, setEditingGoal] = useState(false)
  const [savingGoal, setSavingGoal] = useState(false)
  const [addingItem, setAddingItem] = useState(false)
  const [newItem, setNewItem] = useState({
    what: '', when_date: '', where_text: '', who_name: '', why: '', how: '', resources: '',
  })

  function reload() {
    void getActionPlan(meetingId).then((p) => {
      setPlan(p)
      if (p) setGoal(p.goal)
    }).catch(() => {}).finally(() => setLoading(false))
  }
  useEffect(() => { reload() }, [meetingId]) // eslint-disable-line react-hooks/exhaustive-deps

  async function saveGoal() {
    setSavingGoal(true)
    try {
      await upsertActionPlan(meetingId, goal)
      reload()
      setEditingGoal(false)
    } finally { setSavingGoal(false) }
  }

  async function addItem() {
    if (!newItem.what.trim()) return
    setAddingItem(true)
    try {
      await addActionItem(meetingId, {
        what: newItem.what.trim(),
        when_date: newItem.when_date || null,
        where_text: newItem.where_text,
        who_name: newItem.who_name,
        why: newItem.why,
        how: newItem.how,
        resources: newItem.resources,
      })
      setNewItem({ what: '', when_date: '', where_text: '', who_name: '', why: '', how: '', resources: '' })
      reload()
    } finally { setAddingItem(false) }
  }

  async function cycleStatus(itemId: string, current: string) {
    const next = current === 'todo' ? 'doing' : current === 'doing' ? 'done' : 'todo'
    await patchActionItem(itemId, { status: next }).catch(() => {})
    reload()
  }

  async function removeItem(itemId: string) {
    await deleteActionItem(itemId).catch(() => {})
    reload()
  }

  const STATUS_LABELS: Record<string, string> = {
    todo: t('statusTodo'),
    doing: t('statusDoing'),
    done: t('statusDone'),
  }
  if (loading) return <p className="muted small">{t('actionLoading')}</p>

  return (
    <div className="action-plan-panel">
      {/* META / Objetivo */}
      <div className="action-plan-meta">
        <span className="action-plan-meta-label">{t('actionMetaLabel')}</span>
        {editingGoal ? (
          <div className="action-plan-meta-edit">
            <input
              value={goal}
              onChange={(e) => setGoal(e.target.value)}
              placeholder={t('actionGoalPlaceholder')}
              autoFocus
            />
            <button className="btn-sm" disabled={savingGoal} onClick={() => void saveGoal()}>{tRaw('common.save')}</button>
            <button className="btn-sm ghost" onClick={() => setEditingGoal(false)}>{t('cancel')}</button>
          </div>
        ) : (
          <span className="action-plan-meta-value" onClick={() => isOwner && setEditingGoal(true)}>
            {plan?.goal || <em className="muted">{t('actionGoalPlaceholder')}</em>}
            {isOwner && <button className="action-plan-edit-btn" aria-label={t('actionGoalEdit')} onClick={() => setEditingGoal(true)}><EditIcon /></button>}
          </span>
        )}
      </div>

      {/* Tabela 5W2H */}
      <div className="action-plan-scroll">
        <table className="action-plan-table">
          <colgroup>
            <col className="col-what" /><col className="col-when" />
            <col className="col-where" /><col className="col-who" />
            <col className="col-why" /><col className="col-how" />
            <col className="col-res" /><col className="col-status" />
            {isOwner && <col className="col-del" />}
          </colgroup>
          <thead>
            <tr>
              <th>{t('actionWhat')}</th>
              <th>{t('actionWhen')}</th>
              <th>{t('actionWhere')}</th>
              <th>{t('actionWho')}</th>
              <th>{t('actionWhy')}</th>
              <th>{t('actionHow')}</th>
              <th>{t('actionResources')}</th>
              <th>{t('actionStatus')}</th>
              {isOwner && <th></th>}
            </tr>
          </thead>
          <tbody>
            {(!plan || plan.items.length === 0) && (
              <tr>
                <td colSpan={isOwner ? 9 : 8} className="muted small action-plan-empty">
                  {t('actionEmpty')}
                </td>
              </tr>
            )}
            {plan?.items.map((item) => (
              <tr key={item.id} className={`action-row status-${item.status}`}>
                <td>{item.what}</td>
                <td>{item.when_date ?? '—'}</td>
                <td>{item.where_text || '—'}</td>
                <td>{item.who_name || '—'}</td>
                <td>{item.why || '—'}</td>
                <td>{item.how || '—'}</td>
                <td>{item.resources || '—'}</td>
                <td>
                  <button
                    className={`status-badge status-${item.status}`}
                    onClick={() => void cycleStatus(item.id, item.status)}
                  >
                    {STATUS_LABELS[item.status]}
                  </button>
                </td>
                {isOwner && (
                  <td>
                    <button className="action-del" onClick={() => void removeItem(item.id)}>
                      <TrashIcon />
                    </button>
                  </td>
                )}
              </tr>
            ))}
            {/* Linha de adição */}
            {isOwner && (
              <tr className="action-row-new">
                <td><input placeholder={t('actionWhatPh')} value={newItem.what} onChange={(e) => setNewItem((p) => ({ ...p, what: e.target.value }))} /></td>
                <td><input type="date" value={newItem.when_date} onChange={(e) => setNewItem((p) => ({ ...p, when_date: e.target.value }))} /></td>
                <td><input placeholder={t('actionWherePh')} value={newItem.where_text} onChange={(e) => setNewItem((p) => ({ ...p, where_text: e.target.value }))} /></td>
                <td><input placeholder={t('actionWhoPh')} value={newItem.who_name} onChange={(e) => setNewItem((p) => ({ ...p, who_name: e.target.value }))} /></td>
                <td><input placeholder={t('actionWhyPh')} value={newItem.why} onChange={(e) => setNewItem((p) => ({ ...p, why: e.target.value }))} /></td>
                <td><input placeholder={t('actionHowPh')} value={newItem.how} onChange={(e) => setNewItem((p) => ({ ...p, how: e.target.value }))} /></td>
                <td><input placeholder={t('actionResPh')} value={newItem.resources} onChange={(e) => setNewItem((p) => ({ ...p, resources: e.target.value }))} /></td>
                <td colSpan={isOwner ? 2 : 1}>
                  <button className="btn-sm" disabled={addingItem || !newItem.what.trim()} onClick={() => void addItem()}>
                    <PlusIcon /> {t('agendaAdd')}
                  </button>
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  )
}

// ─── Event Modal ──────────────────────────────────────────────────────────────

type EventTab = 'details' | 'agenda' | 'actions'

function EventModal({
  meeting, onClose, onStart, onRemove, onChanged,
}: {
  meeting: Meeting
  onClose: () => void
  onStart: (m: Meeting) => void
  onRemove: (m: Meeting) => void
  onChanged: () => void
}) {
  const { t: tRaw, i18n } = useTranslation()
  const tr = (k: string, opts?: Record<string, unknown>) => tRaw(`cal.${k}`, opts ?? {})
  const t = new Date(meeting.starts_at)
  const end = new Date(+t + meeting.duration_min * 60000)
  const isInvitee = !meeting.is_owner && meeting.my_status && meeting.my_status !== 'owner'
  const [responses, setResponses] = useState<InviteeResponse[]>([])
  const [declining, setDeclining] = useState(false)
  const [reason, setReason] = useState('')
  const [busy, setBusy] = useState(false)
  const [tab, setTab] = useState<EventTab>('details')

  useEffect(() => {
    if (meeting.is_owner) void meetingInvitees(meeting.id).then(setResponses).catch(() => {})
  }, [meeting.id, meeting.is_owner])

  async function respond(status: 'accepted' | 'declined') {
    if (status === 'declined' && !reason.trim()) {
      setDeclining(true)
      return
    }
    setBusy(true)
    try {
      await respondMeeting(meeting.id, status, reason.trim())
      onChanged()
      onClose()
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal event-modal" onClick={(e) => e.stopPropagation()}>
        <div className={`event-accent ${meeting.kind}`} />
        <div className="modal-head">
          <h3>{meeting.title}</h3>
          <button className="panel-close" onClick={onClose}><CloseIcon /></button>
        </div>
        <div className="event-meta">
          <span className={`agenda-kind ${meeting.kind}`}>{meeting.kind === 'voice' ? <VoiceCallIcon /> : <VideoIcon />}</span>
          <div>
            <div className="event-when">
              {t.toLocaleDateString(i18n.language, { weekday: 'long', day: 'numeric', month: 'long' })}
            </div>
            <div className="muted">
              {hm(meeting.starts_at)} – {end.toLocaleTimeString(i18n.language, { hour: '2-digit', minute: '2-digit' })} · {meeting.kind === 'voice' ? tr('voiceCall') : tr('videoCall')}
            </div>
          </div>
        </div>
        {meeting.room_name && <p className="event-room">{tr('physRoom')}<strong>{meeting.room_name}</strong></p>}

        {/* Tabs */}
        <div className="event-tabs">
          <button className={`event-tab${tab === 'details' ? ' active' : ''}`} onClick={() => setTab('details')}>{tr('tabDetails')}</button>
          <button className={`event-tab${tab === 'agenda' ? ' active' : ''}`} onClick={() => setTab('agenda')}>{tr('tabAgenda')}</button>
          <button className={`event-tab${tab === 'actions' ? ' active' : ''}`} onClick={() => setTab('actions')}>{tr('tabActions')}</button>
        </div>

        {/* Tab: Detalhes */}
        {tab === 'details' && (
          <>
            {meeting.description && <p className="event-desc">{meeting.description}</p>}
            <p className="muted small">{meeting.is_owner ? tr('organized') : tr('organizedBy', { name: meeting.owner_name })}</p>

            {isInvitee && (
              <div className="respond-box">
                {meeting.my_status === 'accepted' && <p className="resp-tag accepted">{tr('accepted')}</p>}
                {meeting.my_status === 'declined' && <p className="resp-tag declined">{tr('declined')}</p>}
                {meeting.my_status === 'pending' && !declining && (
                  <>
                    <p className="muted small">{tr('inviteConfirm')}</p>
                    <div className="event-actions">
                      <button className="btn-sm" disabled={busy} onClick={() => void respond('accepted')}>{tr('accept')}</button>
                      <button className="btn-sm ghost" disabled={busy} onClick={() => setDeclining(true)}>{tr('decline')}</button>
                    </div>
                  </>
                )}
                {declining && meeting.my_status !== 'declined' && (
                  <div className="decline-form">
                    <input placeholder={tr('declineReason')} value={reason} onChange={(e) => setReason(e.target.value)} autoFocus />
                    <div className="event-actions">
                      <button className="btn-sm danger" disabled={busy || !reason.trim()} onClick={() => void respond('declined')}>{tr('confirmDecline')}</button>
                      <button className="btn-sm ghost" onClick={() => setDeclining(false)}>{tr('back')}</button>
                    </div>
                  </div>
                )}
              </div>
            )}

            {meeting.is_owner && responses.length > 0 && (
              <div className="resp-list">
                <h4>{tr('responses')}</h4>
                {responses.map((r) => (
                  <div key={r.user_id} className="resp-row">
                    <span className="avatar-circle small">{r.username.slice(0, 2).toUpperCase()}</span>
                    <span className="resp-info">
                      <strong>{r.username}</strong>
                      {r.status === 'declined' && r.decline_reason && <small className="muted">{tr('declineMotif', { reason: r.decline_reason })}</small>}
                    </span>
                    <span className={`resp-badge ${r.status}`}>
                      {r.status === 'accepted' ? tr('respAccepted') : r.status === 'declined' ? tr('respDeclined') : tr('respPending')}
                    </span>
                  </div>
                ))}
              </div>
            )}

            {meeting.minutes && (
              <details className="event-mom">
                <summary>{tr('mom')}</summary>
                <pre>{meeting.minutes}</pre>
              </details>
            )}
          </>
        )}

        {/* Tab: Agenda */}
        {tab === 'agenda' && (
          <AgendaPanel meetingId={meeting.id} isOwner={meeting.is_owner} />
        )}

        {/* Tab: Plano de Ação 5W2H */}
        {tab === 'actions' && (
          <ActionPlanPanel meetingId={meeting.id} isOwner={meeting.is_owner} />
        )}

        <div className="event-actions">
          <button className="btn-new small" onClick={() => onStart(meeting)}>
            {meeting.kind === 'voice' ? <VoiceCallIcon /> : <VideoIcon />}
            {meeting.is_owner ? tr('startRoom') : tr('enterRoom')}
          </button>
          <button
            className="btn-sm ghost"
            onClick={() => void downloadMeetingIcs(meeting.id, meeting.title).catch(() => {})}
          >
            {tr('ics')}
          </button>
          {meeting.is_owner && (
            <button className="btn-sm ghost" onClick={() => onRemove(meeting)}><TrashIcon /> {tr('cancel')}</button>
          )}
        </div>
      </div>
    </div>
  )
}

function ScheduleModal({
  initialDate, onClose, onCreated,
}: {
  initialDate: string
  onClose: () => void
  onCreated: () => void
}) {
  const { t: tRaw } = useTranslation()
  const t = (k: string, opts?: Record<string, unknown>) => tRaw(`cal.${k}`, opts ?? {})
  const [title, setTitle] = useState('')
  const [description, setDescription] = useState('')
  const [kind, setKind] = useState<'video' | 'voice'>('video')
  const [date, setDate] = useState(initialDate)
  const [time, setTime] = useState(() => new Date(Date.now() + 3600_000).toTimeString().slice(0, 5))
  const [duration, setDuration] = useState(30)
  const [query, setQuery] = useState('')
  const [results, setResults] = useState<User[]>([])
  const [invitees, setInvitees] = useState<User[]>([])
  const [rooms, setRooms] = useState<MeetingRoom[]>([])
  const [roomRef, setRoomRef] = useState('')
  const [conflicts, setConflicts] = useState<Conflicts | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  // Recorrência
  const [rrFreq, setRrFreq] = useState<RecurrenceFreq | ''>('')
  const [rrInterval, setRrInterval] = useState(1)
  const [rrUntil, setRrUntil] = useState('')
  const [rrCount, setRrCount] = useState<number | ''>('')
  const [rrByday, setRrByday] = useState<Set<string>>(new Set(['MON', 'TUE', 'WED', 'THU', 'FRI']))

  useEffect(() => {
    void loadAllRooms().then(setRooms)
  }, [])

  useEffect(() => {
    if (query.trim().length < 2) {
      setResults([])
      return
    }
    const t = setTimeout(async () => setResults(await searchUsers(query).catch(() => [])), 250)
    return () => clearTimeout(t)
  }, [query])

  // Verificação de colisão em tempo real (agenda + sala física).
  useEffect(() => {
    if (!date || !time) return
    const t = setTimeout(async () => {
      try {
        const starts_at = new Date(`${date}T${time}`).toISOString()
        setConflicts(await checkConflicts({ starts_at, duration_min: duration, invitee_ids: invitees.map((i) => i.id), room_ref: roomRef || null }))
      } catch {
        setConflicts(null)
      }
    }, 350)
    return () => clearTimeout(t)
  }, [date, time, duration, invitees, roomRef])

  const inviteeIds = new Set(invitees.map((i) => i.id))

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!title.trim()) {
      setError(t('schedErrorTitle'))
      return
    }
    setBusy(true)
    setError('')
    try {
      const starts_at = new Date(`${date}T${time}`).toISOString()
      await createMeeting({
        title: title.trim(),
        description: description.trim(),
        kind,
        starts_at,
        duration_min: duration,
        invitee_ids: invitees.map((i) => i.id),
        room_ref: roomRef || null,
        recurrence_freq: rrFreq || null,
        recurrence_interval: rrFreq ? rrInterval : undefined,
        recurrence_until: rrFreq && rrUntil ? rrUntil : null,
        recurrence_count: rrFreq && rrCount ? Number(rrCount) : null,
        recurrence_byday: rrFreq === 'weekly' ? Array.from(rrByday).join(',') : null,
      })
      onCreated()
    } catch (err) {
      setError((err as Error).message)
      setBusy(false)
    }
  }

  const roomConflict = (conflicts?.room.length ?? 0) > 0
  const partConflict = (conflicts?.participants.length ?? 0) > 0

  return (
    <div className="modal-overlay" onClick={onClose}>
      <form className="modal" onClick={(e) => e.stopPropagation()} onSubmit={submit}>
        <div className="modal-head">
          <h3>{t('schedTitle')}</h3>
          <button type="button" className="panel-close" onClick={onClose}><CloseIcon /></button>
        </div>

        <input placeholder={t('schedTitlePh')} value={title} onChange={(e) => setTitle(e.target.value)} autoFocus />
        <input placeholder={t('schedDescPh')} value={description} onChange={(e) => setDescription(e.target.value)} />

        <div className="kind-toggle">
          <button type="button" className={kind === 'video' ? 'kt active' : 'kt'} onClick={() => setKind('video')}>
            <VideoIcon /> {t('schedVideo')}
          </button>
          <button type="button" className={kind === 'voice' ? 'kt active' : 'kt'} onClick={() => setKind('voice')}>
            <VoiceCallIcon /> {t('schedVoice')}
          </button>
        </div>

        <div className="field-row">
          <label>{t('schedDate')}<input type="date" value={date} onChange={(e) => setDate(e.target.value)} /></label>
          <label>{t('schedTime')}<input type="time" value={time} onChange={(e) => setTime(e.target.value)} /></label>
          <label>{t('schedDuration')}
            <select value={duration} onChange={(e) => setDuration(Number(e.target.value))}>
              {[15, 30, 45, 60, 90, 120].map((d) => <option key={d} value={d}>{d} min</option>)}
            </select>
          </label>
        </div>

        {rooms.length > 0 && (
          <label className="set-label">
            {t('schedRoom')}
            <select value={roomRef} onChange={(e) => setRoomRef(e.target.value)}>
              <option value="">{t('schedNoRoom')}</option>
              {rooms.map((r) => (
                <option key={r.id} value={r.id}>{r.name}{r.location ? ` · ${r.location}` : ''}</option>
              ))}
            </select>
          </label>
        )}

        {/* Recorrência */}
        <div className="rrule-block">
          <label className="set-label">
            {t('schedRepeat')}
            <select value={rrFreq} onChange={(e) => setRrFreq(e.target.value as RecurrenceFreq | '')}>
              <option value="">{t('schedNoRepeat')}</option>
              <option value="daily">{t('schedDaily')}</option>
              <option value="weekly">{t('schedWeekly')}</option>
              <option value="monthly">{t('schedMonthly')}</option>
              <option value="yearly">{t('schedYearly')}</option>
            </select>
          </label>

          {rrFreq && (
            <div className="rrule-opts">
              <label>
                {t('schedEvery')}
                <input
                  type="number"
                  min={1}
                  max={99}
                  value={rrInterval}
                  onChange={(e) => setRrInterval(Math.max(1, Number(e.target.value)))}
                />
                {rrFreq === 'daily' ? t('schedDays') : rrFreq === 'weekly' ? t('schedWeeks') : rrFreq === 'monthly' ? t('schedMonths') : t('schedYears')}
              </label>

              {rrFreq === 'weekly' && (
                <div className="rrule-byday">
                  {(['MON','TUE','WED','THU','FRI','SAT','SUN'] as const).map((d) => {
                    const label = { MON:'Seg',TUE:'Ter',WED:'Qua',THU:'Qui',FRI:'Sex',SAT:'Sáb',SUN:'Dom' }[d]
                    const on = rrByday.has(d)
                    return (
                      <button
                        key={d}
                        type="button"
                        className={`rrule-day${on ? ' active' : ''}`}
                        onClick={() => {
                          const next = new Set(rrByday)
                          on ? next.delete(d) : next.add(d)
                          if (next.size > 0) setRrByday(next)
                        }}
                      >{label}</button>
                    )
                  })}
                </div>
              )}

              <div className="rrule-end">
                <label>
                  <input
                    type="radio"
                    name="rrend"
                    value="never"
                    checked={!rrUntil && !rrCount}
                    onChange={() => { setRrUntil(''); setRrCount('') }}
                  /> {t('schedNoEnd')}
                </label>
                <label>
                  <input
                    type="radio"
                    name="rrend"
                    value="until"
                    checked={!!rrUntil}
                    onChange={() => { setRrCount(''); setRrUntil(date) }}
                  /> {t('schedUntil')}
                  {rrUntil && (
                    <input
                      type="date"
                      value={rrUntil}
                      min={date}
                      onChange={(e) => setRrUntil(e.target.value)}
                    />
                  )}
                </label>
                <label>
                  <input
                    type="radio"
                    name="rrend"
                    value="count"
                    checked={!!rrCount}
                    onChange={() => { setRrUntil(''); setRrCount(10) }}
                  /> {t('schedAfter')}
                  {rrCount !== '' && (
                    <input
                      type="number"
                      min={2}
                      max={365}
                      value={rrCount}
                      onChange={(e) => setRrCount(Number(e.target.value))}
                    />
                  )}
                  {rrCount !== '' && ` ${t('schedOccurrences')}`}
                </label>
              </div>
            </div>
          )}
        </div>

        {roomConflict && (
          <div className="conflict-box blocking">
            <strong>{t('schedRoomUnavailable')}</strong>
            {conflicts!.room.map((c) => (
              <div key={c.meeting_id} className="conflict-line">{t('schedRoomBooked', { title: c.meeting_title, time: hm(c.starts_at) })}</div>
            ))}
            <small className="muted">{t('schedRoomHint')}</small>
          </div>
        )}
        {partConflict && (
          <div className="conflict-box">
            <strong>{t('schedConflict')}</strong>
            {conflicts!.participants.map((c, i) => (
              <div key={`${c.user_id}-${c.meeting_id}-${i}`} className="conflict-line">{t('schedConflictLine', { name: c.username, title: c.meeting_title, time: hm(c.starts_at) })}</div>
            ))}
            <small className="muted">{t('schedConflictHint')}</small>
          </div>
        )}

        <label className="set-label">
          {t('schedGuests')}
          <input placeholder={t('schedGuestsPh')} value={query} onChange={(e) => setQuery(e.target.value)} />
        </label>
        {results.length > 0 && (
          <div className="user-results">
            {results.map((u) => (
              <button
                key={u.id}
                type="button"
                className="user-result"
                disabled={inviteeIds.has(u.id)}
                onClick={() => { setInvitees([...invitees, u]); setQuery(''); setResults([]) }}
              >
                <span className="avatar-circle small">{u.username.slice(0, 2).toUpperCase()}</span>
                <span className="user-info"><strong>{u.username}</strong><small>{u.email}</small></span>
                {inviteeIds.has(u.id) ? <span className="muted small">adicionado</span> : <span className="add-plus">+</span>}
              </button>
            ))}
          </div>
        )}
        {invitees.length > 0 && (
          <div className="chips">
            {invitees.map((u) => (
              <span key={u.id} className="chip">
                {u.username}
                <button type="button" onClick={() => setInvitees(invitees.filter((i) => i.id !== u.id))}>×</button>
              </span>
            ))}
          </div>
        )}

        {error && <div className="error">{error}</div>}
        <button className="primary" disabled={busy || roomConflict}>
          {busy ? t('schedBusy') : roomConflict ? t('schedUnavailable') : t('schedSubmit')}
        </button>
      </form>
    </div>
  )
}
