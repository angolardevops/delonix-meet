import { FormEvent, useEffect, useMemo, useState } from 'react'
import {
  checkConflicts,
  Conflicts,
  createMeeting,
  deleteMeeting,
  InviteeResponse,
  listMeetingRooms,
  listMeetings,
  Meeting,
  MeetingRoom,
  meetingInvitees,
  myOrgs,
  respondMeeting,
  searchUsers,
  startMeeting,
  User,
} from '../api'
import {
  ChevronLeftIcon, ChevronRightIcon, ClockIcon, CloseIcon, PlusIcon, TrashIcon, VideoIcon, VoiceCallIcon,
} from '../icons'

/** Carrega as salas presenciais de todas as organizações do utilizador. */
async function loadAllRooms(): Promise<MeetingRoom[]> {
  const orgs = await myOrgs().catch(() => [])
  const lists = await Promise.all(orgs.map((o) => listMeetingRooms(o.id).catch(() => [])))
  return lists.flat()
}

type View = 'month' | 'week' | 'agenda'

const WEEKDAYS = ['Seg', 'Ter', 'Qua', 'Qui', 'Sex', 'Sáb', 'Dom']
const MONTHS = [
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

export default function Calendar({ onEnterRoom }: { onEnterRoom: (code: string, voice?: boolean) => void }) {
  const [meetings, setMeetings] = useState<Meeting[]>([])
  const [loading, setLoading] = useState(true)
  const [view, setView] = useState<View>('month')
  const [cursor, setCursor] = useState(() => startOfDay(new Date()))
  const [schedule, setSchedule] = useState<{ date: string } | null>(null)
  const [selected, setSelected] = useState<Meeting | null>(null)
  const [error, setError] = useState('')

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
    else if (view === 'month') setCursor((c) => new Date(c.getFullYear(), c.getMonth() + delta, 1))
    else setCursor((c) => addDays(c, delta * 30))
  }

  const title =
    view === 'week'
      ? (() => {
          const s = mondayOf(cursor)
          const e = addDays(s, 6)
          return `${s.getDate()} ${MONTHS[s.getMonth()].slice(0, 3)} – ${e.getDate()} ${MONTHS[e.getMonth()].slice(0, 3)} ${e.getFullYear()}`
        })()
      : `${MONTHS[cursor.getMonth()]} ${cursor.getFullYear()}`

  return (
    <div className="page cal-page">
      <header className="cal-toolbar">
        <div className="cal-nav">
          <button className="btn-today" onClick={() => setCursor(startOfDay(new Date()))}>Hoje</button>
          <button className="icon-btn cal-arrow" onClick={() => move(-1)} title="Anterior"><ChevronLeftIcon /></button>
          <button className="icon-btn cal-arrow" onClick={() => move(1)} title="Seguinte"><ChevronRightIcon /></button>
          <h1 className="cal-title">{title}</h1>
        </div>
        <div className="cal-right">
          <div className="seg">
            <button className={view === 'month' ? 'seg-btn active' : 'seg-btn'} onClick={() => setView('month')}>Mês</button>
            <button className={view === 'week' ? 'seg-btn active' : 'seg-btn'} onClick={() => setView('week')}>Semana</button>
            <button className={view === 'agenda' ? 'seg-btn active' : 'seg-btn'} onClick={() => setView('agenda')}>Agenda</button>
          </div>
          <button className="btn-new small" onClick={() => setSchedule({ date: ymd(cursor) })}>
            <PlusIcon /> Agendar
          </button>
        </div>
      </header>

      {error && <div className="error">{error}</div>}
      {loading && <p className="muted">A carregar…</p>}

      {!loading && view === 'month' && (
        <MonthGrid cursor={cursor} today={today} byDay={byDay} onDayClick={(d) => setSchedule({ date: ymd(d) })} onEvent={setSelected} />
      )}
      {!loading && view === 'week' && (
        <WeekGrid cursor={cursor} today={today} byDay={byDay} onSlot={(d) => setSchedule({ date: ymd(d) })} onEvent={setSelected} />
      )}
      {!loading && view === 'agenda' && (
        <AgendaView meetings={meetings} onStart={start} onRemove={remove} />
      )}

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
  cursor, today, byDay, onDayClick, onEvent,
}: {
  cursor: Date
  today: Date
  byDay: Map<string, Meeting[]>
  onDayClick: (d: Date) => void
  onEvent: (m: Meeting) => void
}) {
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
                    title={m.title}
                  >
                    <span className="ev-dot" />
                    <span className="ev-time">{hm(m.starts_at)}</span>
                    <span className="ev-title">{m.title}</span>
                  </button>
                ))}
                {events.length > 3 && <div className="ev-more">+{events.length - 3} mais</div>}
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}

function WeekGrid({
  cursor, today, byDay, onSlot, onEvent,
}: {
  cursor: Date
  today: Date
  byDay: Map<string, Meeting[]>
  onSlot: (d: Date) => void
  onEvent: (m: Meeting) => void
}) {
  const start = mondayOf(cursor)
  const days = Array.from({ length: 7 }, (_, i) => addDays(start, i))
  return (
    <div className="week">
      {days.map((d) => {
        const events = byDay.get(ymd(d)) ?? []
        const isToday = sameDay(d, today)
        return (
          <div key={d.toISOString()} className={`week-col${isToday ? ' today' : ''}`}>
            <div className="week-colhead" onClick={() => onSlot(d)}>
              <span className="week-wd">{WEEKDAYS[(d.getDay() + 6) % 7]}</span>
              <span className={isToday ? 'week-num today' : 'week-num'}>{d.getDate()}</span>
            </div>
            <div className="week-events" onClick={() => onSlot(d)}>
              {events.map((m) => (
                <button key={m.id} className={`week-ev ${m.kind}`} onClick={(e) => { e.stopPropagation(); onEvent(m) }}>
                  <strong>{hm(m.starts_at)}</strong>
                  <span>{m.title}</span>
                </button>
              ))}
              {events.length === 0 && <div className="week-empty">—</div>}
            </div>
          </div>
        )
      })}
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
  const groups = useMemo(() => {
    const now = Date.now()
    const upcoming = meetings
      .filter((m) => +new Date(m.starts_at) + m.duration_min * 60000 > now)
      .sort((a, b) => +new Date(a.starts_at) - +new Date(b.starts_at))
    const map = new Map<string, Meeting[]>()
    for (const m of upcoming) {
      const k = new Date(m.starts_at).toLocaleDateString('pt-PT', { weekday: 'long', day: 'numeric', month: 'long' })
      map.set(k, [...(map.get(k) ?? []), m])
    }
    return [...map.entries()]
  }, [meetings])

  if (groups.length === 0) {
    return <div className="empty-state"><ClockIcon /><p>Sem reuniões futuras.</p></div>
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
                  <strong>{m.title}</strong>
                  <small>{m.duration_min} min · {m.is_owner ? 'organizada por ti' : `por ${m.owner_name}`}{m.description ? ` · ${m.description}` : ''}</small>
                </span>
                <button className="btn-sm" onClick={() => onStart(m)}>
                  {m.kind === 'voice' ? <VoiceCallIcon /> : <VideoIcon />}{m.is_owner ? 'Iniciar' : 'Entrar'}
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

function EventModal({
  meeting, onClose, onStart, onRemove, onChanged,
}: {
  meeting: Meeting
  onClose: () => void
  onStart: (m: Meeting) => void
  onRemove: (m: Meeting) => void
  onChanged: () => void
}) {
  const t = new Date(meeting.starts_at)
  const end = new Date(+t + meeting.duration_min * 60000)
  const isInvitee = !meeting.is_owner && meeting.my_status && meeting.my_status !== 'owner'
  const [responses, setResponses] = useState<InviteeResponse[]>([])
  const [declining, setDeclining] = useState(false)
  const [reason, setReason] = useState('')
  const [busy, setBusy] = useState(false)

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
              {t.toLocaleDateString('pt-PT', { weekday: 'long', day: 'numeric', month: 'long' })}
            </div>
            <div className="muted">
              {hm(meeting.starts_at)} – {end.toLocaleTimeString('pt-PT', { hour: '2-digit', minute: '2-digit' })} · {meeting.kind === 'voice' ? 'Chamada de voz' : 'Videochamada'}
            </div>
          </div>
        </div>
        {meeting.room_name && <p className="event-room">🚪 Sala presencial: <strong>{meeting.room_name}</strong></p>}
        {meeting.description && <p className="event-desc">{meeting.description}</p>}
        <p className="muted small">{meeting.is_owner ? 'Organizada por ti' : `Organizada por ${meeting.owner_name}`}</p>

        {/* Convidado: aceitar / recusar (com motivo) */}
        {isInvitee && (
          <div className="respond-box">
            {meeting.my_status === 'accepted' && <p className="resp-tag accepted">✓ Aceitaste esta reunião</p>}
            {meeting.my_status === 'declined' && <p className="resp-tag declined">✕ Recusaste esta reunião</p>}
            {meeting.my_status === 'pending' && !declining && (
              <>
                <p className="muted small">Confirmas a tua presença?</p>
                <div className="event-actions">
                  <button className="btn-sm" disabled={busy} onClick={() => void respond('accepted')}>Aceitar</button>
                  <button className="btn-sm ghost" disabled={busy} onClick={() => setDeclining(true)}>Recusar</button>
                </div>
              </>
            )}
            {declining && meeting.my_status !== 'declined' && (
              <div className="decline-form">
                <input placeholder="Motivo da recusa (obrigatório)" value={reason} onChange={(e) => setReason(e.target.value)} autoFocus />
                <div className="event-actions">
                  <button className="btn-sm danger" disabled={busy || !reason.trim()} onClick={() => void respond('declined')}>Confirmar recusa</button>
                  <button className="btn-sm ghost" onClick={() => setDeclining(false)}>Voltar</button>
                </div>
              </div>
            )}
          </div>
        )}

        {/* Anfitrião: respostas dos convidados */}
        {meeting.is_owner && responses.length > 0 && (
          <div className="resp-list">
            <h4>Respostas dos convidados</h4>
            {responses.map((r) => (
              <div key={r.user_id} className="resp-row">
                <span className="avatar-circle small">{r.username.slice(0, 2).toUpperCase()}</span>
                <span className="resp-info">
                  <strong>{r.username}</strong>
                  {r.status === 'declined' && r.decline_reason && <small className="muted">Motivo: {r.decline_reason}</small>}
                </span>
                <span className={`resp-badge ${r.status}`}>
                  {r.status === 'accepted' ? 'Aceitou' : r.status === 'declined' ? 'Recusou' : 'Sem resposta'}
                </span>
              </div>
            ))}
          </div>
        )}

        {meeting.minutes && (
          <details className="event-mom">
            <summary>Ata (MoM)</summary>
            <pre>{meeting.minutes}</pre>
          </details>
        )}
        <div className="event-actions">
          <button className="btn-new small" onClick={() => onStart(meeting)}>
            {meeting.kind === 'voice' ? <VoiceCallIcon /> : <VideoIcon />}
            {meeting.is_owner ? 'Iniciar reunião' : 'Entrar'}
          </button>
          {meeting.is_owner && (
            <button className="btn-sm ghost" onClick={() => onRemove(meeting)}><TrashIcon /> Cancelar</button>
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
      setError('Dá um título à reunião')
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
          <h3>Agendar reunião</h3>
          <button type="button" className="panel-close" onClick={onClose}><CloseIcon /></button>
        </div>

        <input placeholder="Título" value={title} onChange={(e) => setTitle(e.target.value)} autoFocus />
        <input placeholder="Descrição (opcional)" value={description} onChange={(e) => setDescription(e.target.value)} />

        <div className="kind-toggle">
          <button type="button" className={kind === 'video' ? 'kt active' : 'kt'} onClick={() => setKind('video')}>
            <VideoIcon /> Vídeo
          </button>
          <button type="button" className={kind === 'voice' ? 'kt active' : 'kt'} onClick={() => setKind('voice')}>
            <VoiceCallIcon /> Só voz
          </button>
        </div>

        <div className="field-row">
          <label>Data<input type="date" value={date} onChange={(e) => setDate(e.target.value)} /></label>
          <label>Hora<input type="time" value={time} onChange={(e) => setTime(e.target.value)} /></label>
          <label>Duração
            <select value={duration} onChange={(e) => setDuration(Number(e.target.value))}>
              {[15, 30, 45, 60, 90, 120].map((d) => <option key={d} value={d}>{d} min</option>)}
            </select>
          </label>
        </div>

        {rooms.length > 0 && (
          <label className="set-label">
            Sala presencial (opcional)
            <select value={roomRef} onChange={(e) => setRoomRef(e.target.value)}>
              <option value="">— Sem sala física —</option>
              {rooms.map((r) => (
                <option key={r.id} value={r.id}>{r.name}{r.location ? ` · ${r.location}` : ''}</option>
              ))}
            </select>
          </label>
        )}

        {roomConflict && (
          <div className="conflict-box blocking">
            <strong>⛔ Sala presencial indisponível</strong>
            {conflicts!.room.map((c) => (
              <div key={c.meeting_id} className="conflict-line">Já reservada por «{c.meeting_title}» ({hm(c.starts_at)})</div>
            ))}
            <small className="muted">Escolhe outra sala ou outro horário para agendar.</small>
          </div>
        )}
        {partConflict && (
          <div className="conflict-box">
            <strong>⚠ Sobreposição de agenda</strong>
            {conflicts!.participants.map((c, i) => (
              <div key={`${c.user_id}-${c.meeting_id}-${i}`} className="conflict-line">{c.username} já tem «{c.meeting_title}» ({hm(c.starts_at)})</div>
            ))}
            <small className="muted">Podes agendar na mesma — os participantes serão avisados para aceitar ou recusar.</small>
          </div>
        )}

        <label className="set-label">
          Convidados
          <input placeholder="Procurar por nome ou email…" value={query} onChange={(e) => setQuery(e.target.value)} />
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
          {busy ? 'A agendar…' : roomConflict ? 'Sala indisponível' : 'Agendar reunião'}
        </button>
      </form>
    </div>
  )
}
