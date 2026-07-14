import { FormEvent, ReactNode, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  createRoom,
  downloadMeetingIcs,
  getRoom,
  listMeetings,
  listWhiteboards,
  Meeting,
  recordingsLibrary,
  RecordingItem,
  startMeeting,
  User,
  WhiteboardMeta,
} from '../api'
import { NavKey } from '../components/Shell'
import { CalendarIcon, FilmIcon, NoteIcon, PeopleIcon } from '../icons'

// Ícone de vídeo local (evita conflito de nomes com CamIcon).
function VideoBadge() {
  return (
    <svg width={26} height={26} viewBox="0 0 24 24" fill="currentColor">
      <path d="M4 6h11a1 1 0 0 1 1 1v3.5l4-3.5v10l-4-3.5V17a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1Z" />
    </svg>
  )
}

export default function Home({
  user,
  onEnterRoom,
  onNavigate,
}: {
  user: User
  onEnterRoom: (code: string, voice?: boolean) => void
  onNavigate: (k: NavKey) => void
}) {
  const { t, i18n } = useTranslation()
  const [joinCode, setJoinCode] = useState('')
  const [creating, setCreating] = useState(false)
  const [error, setError] = useState('')
  const [upcoming, setUpcoming] = useState<Meeting[]>([])
  const [recent, setRecent] = useState<RecordingItem[]>([])
  const [boards, setBoards] = useState<WhiteboardMeta[]>([])

  const locale = i18n.language.startsWith('en') ? 'en-GB' : 'pt-PT'
  const hour = new Date().getHours()
  const greetKey = hour < 12 ? 'dash.greetMorning' : hour < 19 ? 'dash.greetAfternoon' : 'dash.greetEvening'

  useEffect(() => {
    void (async () => {
      try {
        const now = Date.now()
        const meetings = (await listMeetings())
          .filter((m) => new Date(m.starts_at).getTime() + m.duration_min * 60_000 >= now)
          .filter((m) => m.my_status !== 'declined')
          .sort((a, b) => a.starts_at.localeCompare(b.starts_at))
        setUpcoming(meetings.slice(0, 3))
      } catch {
        /* agenda indisponível não bloqueia o dashboard */
      }
      try {
        setRecent((await recordingsLibrary()).slice(0, 3))
      } catch {
        /* idem para gravações */
      }
      try {
        setBoards((await listWhiteboards()).slice(0, 3))
      } catch {
        /* idem para quadros */
      }
    })()
  }, [])

  async function newMeeting(waitingRoom = false, e2ee = false, format: 'normal' | 'training' = 'normal') {
    setError('')
    setCreating(true)
    try {
      const label = format === 'training' ? 'Treino' : 'Reunião'
      const room = await createRoom(`${label} de ${user.username}`, 'sfu', waitingRoom, e2ee, format)
      onEnterRoom(room.code)
    } catch (err) {
      setError((err as Error).message)
      setCreating(false)
    }
  }

  async function join(e: FormEvent) {
    e.preventDefault()
    setError('')
    // Aceita link completo, código com sufixo (?voice) ou código puro:
    // extrai o padrão do código (xxx-xxxx-xxx) de qualquer texto colado.
    const raw = joinCode.trim().toLowerCase()
    const code = (raw.match(/[a-z]+-[a-z]+-[a-z]+/) ?? [raw.replace(/^.*\/r\//, '')])[0]
    if (!code) {
      setError(t('dash.notFound'))
      return
    }
    try {
      const room = await getRoom(code)
      onEnterRoom(room.code)
    } catch {
      setError(t('dash.notFound'))
    }
  }

  async function enterMeeting(m: Meeting) {
    setError('')
    try {
      const { code, kind } = await startMeeting(m.id)
      onEnterRoom(code, kind === 'voice')
    } catch (err) {
      setError((err as Error).message)
    }
  }

  const fmtTime = (iso: string) =>
    new Date(iso).toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' })
  const fmtDay = (iso: string) => {
    const d = new Date(iso)
    return d.toDateString() === new Date().toDateString()
      ? ''
      : d.toLocaleDateString(locale, { day: 'numeric', month: 'short' })
  }
  const fmtDate = (iso: string) =>
    new Date(iso).toLocaleDateString(locale, { day: 'numeric', month: 'short' })
  const fmtSize = (b: number) => `${(b / 1024 / 1024).toFixed(1)} MB`

  return (
    <div className="home">
      <header className="dash-greet">
        <h1>{t(greetKey, { name: user.username })}</h1>
        <p className="home-sub">
          <span className="home-date">
            {new Date().toLocaleDateString(locale, { weekday: 'long', day: 'numeric', month: 'long' })}
          </span>
          {' — '}{t('dash.greetSub')}
        </p>
      </header>

      <div className="home-actions">
        <button className="btn-new" disabled={creating} onClick={() => void newMeeting(false)}>
          <VideoBadge />
          {creating ? t('dash.creating') : t('dash.newMeeting')}
        </button>

        <form className="join-box" onSubmit={join}>
          <span className="join-icon">⌨</span>
          <input
            placeholder={t('dash.joinPh')}
            value={joinCode}
            onChange={(e) => setJoinCode(e.target.value)}
          />
          <button className="join-btn" disabled={!joinCode.trim()}>
            {t('dash.join')}
          </button>
        </form>
      </div>

      <div className="home-extra">
        <button className="link" onClick={() => void newMeeting(true)}>
          {t('dash.waitingRoom')}
        </button>
        <button className="link" onClick={() => void newMeeting(false, true)}>
          {t('dash.e2ee')}
        </button>
        <button
          className="link"
          title="Reunião de treino: ativa as salas de grupo (breakouts)"
          onClick={() => void newMeeting(false, false, 'training')}
        >
          🎓 {t('dash.training', 'Reunião de treino')}
        </button>
      </div>
      {error && <div className="error">{error}</div>}

      {/* Atalhos para as áreas principais — a Home é a porta de entrada. */}
      <div className="home-shortcuts">
        {([
          { k: 'calendar', ic: <CalendarIcon />, l: t('nav.calendar'), d: t('dash.scCalendar') },
          { k: 'recordings', ic: <FilmIcon />, l: t('nav.recordings'), d: t('dash.scRecs') },
          { k: 'whiteboards', ic: <NoteIcon />, l: t('nav.whiteboards'), d: t('dash.scWb') },
          { k: 'directory', ic: <PeopleIcon />, l: t('nav.org'), d: t('dash.scOrg') },
        ] as { k: NavKey; ic: ReactNode; l: string; d: string }[]).map((s) => (
          <button key={s.k} className="shortcut-tile" onClick={() => onNavigate(s.k)}>
            <span className="shortcut-ic">{s.ic}</span>
            <span className="shortcut-txt">
              <strong>{s.l}</strong>
              <small>{s.d}</small>
            </span>
          </button>
        ))}
      </div>

      <div className="dash-grid">
        <section className="dash-card">
          <header className="dash-card-head">
            <h2>
              <CalendarIcon /> {t('dash.upcoming')}
            </h2>
            <button className="link" onClick={() => onNavigate('calendar')}>
              {t('dash.viewAll')}
            </button>
          </header>
          {upcoming.length === 0 && <p className="dash-empty">{t('dash.noUpcoming')}</p>}
          {upcoming.map((m) => (
            <div key={m.id} className="dash-row">
              <div className="dash-row-main">
                <strong>{m.title}</strong>
                <span className="dash-meta">
                  {fmtDay(m.starts_at) && <span>{fmtDay(m.starts_at)} · </span>}
                  <span className="mono">{fmtTime(m.starts_at)}</span> · {m.duration_min} min
                  {m.kind === 'voice' ? ' · 🎙' : ''}
                </span>
              </div>
              <button
                className="icon-btn"
                title="Adicionar ao calendário (Google/Outlook — .ics)"
                onClick={() => void downloadMeetingIcs(m.id, m.title).catch(() => {})}
              >
                📅
              </button>
              <button className="btn-ghost dash-enter" onClick={() => void enterMeeting(m)}>
                {t('dash.enter')}
              </button>
            </div>
          ))}
        </section>

        <section className="dash-card">
          <header className="dash-card-head">
            <h2>
              <FilmIcon /> {t('dash.recent')}
            </h2>
            <button className="link" onClick={() => onNavigate('recordings')}>
              {t('dash.viewAll')}
            </button>
          </header>
          {recent.length === 0 && <p className="dash-empty">{t('dash.noRecs')}</p>}
          {recent.map((r) => (
            <div key={r.id} className="dash-row">
              <div className="dash-row-main">
                <strong>{r.filename.replace(/\.webm$/, '')}</strong>
                <span className="dash-meta">
                  {fmtDate(r.created_at)} · {fmtSize(r.size_bytes)} · <span className="mono">{r.room_code}</span>
                </span>
              </div>
              <button className="btn-ghost dash-enter" onClick={() => onNavigate('recordings')}>
                ▶
              </button>
            </div>
          ))}
        </section>

        {boards.length > 0 && (
          <section className="dash-card">
            <header className="dash-card-head">
              <h2>
                <NoteIcon /> {t('dash.wbRecent')}
              </h2>
              <button className="link" onClick={() => onNavigate('whiteboards')}>
                {t('dash.viewAll')}
              </button>
            </header>
            {boards.map((w) => (
              <div key={w.id} className="dash-row">
                <div className="dash-row-main">
                  <strong>{w.title}</strong>
                  <span className="dash-meta">
                    {fmtDate(w.created_at)}
                    {w.room_code && <> · <span className="mono">{w.room_code}</span></>}
                  </span>
                </div>
                <button className="btn-ghost dash-enter" onClick={() => onNavigate('whiteboards')}>
                  {t('dash.open')}
                </button>
              </div>
            ))}
          </section>
        )}
      </div>

      <p className="dash-secure">🔒 {t('dash.secure')}</p>
    </div>
  )
}
