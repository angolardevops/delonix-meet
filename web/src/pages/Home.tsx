import { ReactNode, useCallback, useState } from 'react'
import AsyncSection, { useLoad } from '../components/AsyncSection'
import { useTranslation } from 'react-i18next'
import {
  createRoom,
  downloadMeetingIcs,
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
import { CalendarIcon, FilmIcon, NoteIcon, PeopleIcon, PlayIcon } from '../icons'
import { QuickActions } from '../components/Shell'

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
  const [creating, setCreating] = useState(false)
  const [error, setError] = useState('')

  // Cada secção carrega e falha por si (achados 4.1/4.2). Antes eram três
  // `catch {}` a engolir o erro: com a API em baixo o dashboard aparecia
  // completo e vazio, indistinguível de não haver nada.
  const [upcoming, retryUpcoming] = useLoad<Meeting[]>(
    useCallback(async (signal: AbortSignal) => {
      const now = Date.now()
      return (await listMeetings(signal))
        .filter((m) => new Date(m.starts_at).getTime() + m.duration_min * 60_000 >= now)
        .filter((m) => m.my_status !== 'declined')
        .sort((a, b) => a.starts_at.localeCompare(b.starts_at))
        .slice(0, 3)
    }, []),
  )
  const [recent, retryRecent] = useLoad<RecordingItem[]>(
    useCallback(async (signal: AbortSignal) => (await recordingsLibrary(signal)).slice(0, 3), []),
  )
  const [boards, retryBoards] = useLoad<WhiteboardMeta[]>(
    useCallback(async (signal: AbortSignal) => (await listWhiteboards(signal)).slice(0, 3), []),
  )

  const locale = i18n.language.startsWith('en') ? 'en-GB' : 'pt-PT'
  const hour = new Date().getHours()
  const greetKey = hour < 12 ? 'dash.greetMorning' : hour < 19 ? 'dash.greetAfternoon' : 'dash.greetEvening'

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
      {/* A data e as ações primárias vivem na barra de topo do Shell — em ecrã
          LARGO. Abaixo dos 900px a barra passa-as para a gaveta (decisão 3.1.4),
          e a Home ficava sem a acção principal do produto visível: começar ou
          entrar numa reunião exigia um toque no menu, num ecrã com metade da
          altura vazia.
          Por isso o mesmo componente aparece AQUI, e o CSS mostra-o só onde a
          barra não o tem. Não é duplicação: é o mesmo bloco a viver no sítio
          onde é alcançável em cada largura (R103). */}
      <header className="dash-greet">
        <h1>{t(greetKey, { name: user.username })}</h1>
        <p className="home-sub">{t('dash.greetSub')}</p>
      </header>

      <QuickActions variant="home" onEnterRoom={onEnterRoom} username={user.username} />

      {/* Chips outline, etiqueta curta: a explicação vive no tooltip. Eram
          frases inteiras que ocupavam meia linha do dashboard. */}
      <div className="home-extra">
        <button
          className="chip-outline"
          disabled={creating}
          title={t('dash.waitingRoomHint')}
          onClick={() => void newMeeting(true)}
        >
          {t('dash.waitingRoom')}
        </button>
        <button
          className="chip-outline"
          disabled={creating}
          title={t('dash.e2eeHint')}
          onClick={() => void newMeeting(false, true)}
        >
          {t('dash.e2ee')}
        </button>
        <button
          className="chip-outline"
          disabled={creating}
          title={t('dash.trainingHint', 'Ativa as salas de grupo (breakouts)')}
          onClick={() => void newMeeting(false, false, 'training')}
        >
          {t('dash.training', 'Reunião de treino')}
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
          <AsyncSection
            load={upcoming}
            retry={retryUpcoming}
            empty={<p className="dash-empty">{t('dash.noUpcoming')}</p>}
          >
            {(ms) => ms.map((m) => (
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
                <CalendarIcon />
              </button>
              <button className="btn-ghost dash-enter" onClick={() => void enterMeeting(m)}>
                {t('dash.enter')}
              </button>
            </div>
            ))}
          </AsyncSection>
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
          <AsyncSection
            load={recent}
            retry={retryRecent}
            empty={<p className="dash-empty">{t('dash.noRecs')}</p>}
          >
            {(rs) => rs.map((r) => (
            <div key={r.id} className="dash-row">
              <div className="dash-row-main">
                <strong>{r.filename.replace(/\.webm$/, '')}</strong>
                <span className="dash-meta">
                  {fmtDate(r.created_at)} · {fmtSize(r.size_bytes)} · <span className="mono">{r.room_code}</span>
                </span>
              </div>
              <button className="btn-ghost dash-enter" aria-label={t('dash.play', 'Reproduzir')} onClick={() => onNavigate('recordings')}>
                <PlayIcon />
              </button>
            </div>
            ))}
          </AsyncSection>
        </section>

        {!(boards.s === 'ready' && boards.d.length === 0) && (
          <section className="dash-card">
            <header className="dash-card-head">
              <h2>
                <NoteIcon /> {t('dash.wbRecent')}
              </h2>
              <button className="link" onClick={() => onNavigate('whiteboards')}>
                {t('dash.viewAll')}
              </button>
            </header>
            <AsyncSection
              load={boards}
              retry={retryBoards}
              rows={2}
              empty={<p className="dash-empty">{t('dash.noWb', 'Sem quadros ainda.')}</p>}
            >
              {(ws) => ws.map((w) => (
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
            </AsyncSection>
          </section>
        )}
      </div>

      <p className="dash-secure">🔒 {t('dash.secure')}</p>
    </div>
  )
}
