import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { listMeetings, Meeting } from '../api'
import { usePresence } from './PresenceProvider'
import EmptyState from './EmptyState'
import type { NavKey } from './Shell'
import { CalendarIcon, CamIcon, VoiceCallIcon } from '../icons'

// Centro de notificações — sino + painel persistente. Agrega chamadas perdidas
// (via PresenceProvider) e convites de reunião por responder (listMeetings).

function BellIcon() {
  return (
    <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9" />
      <path d="M13.73 21a2 2 0 0 1-3.46 0" />
    </svg>
  )
}

function timeAgo(iso: string, nowLabel: string): string {
  const d = new Date(iso).getTime()
  if (Number.isNaN(d)) return ''
  const secs = Math.round((Date.now() - d) / 1000)
  if (secs < 60) return nowLabel
  const mins = Math.round(secs / 60)
  if (mins < 60) return `${mins} min`
  const hrs = Math.round(mins / 60)
  if (hrs < 24) return `${hrs} h`
  return new Date(iso).toLocaleDateString()
}

export default function NotificationCenter({ onNavigate }: { onNavigate: (k: NavKey) => void }) {
  const { t } = useTranslation()
  const { missed, ackMissed, startCall } = usePresence()
  const [open, setOpen] = useState(false)
  const [meetings, setMeetings] = useState<Meeting[]>([])
  const ref = useRef<HTMLDivElement>(null)

  // Convites por responder (my_status pendente) — carregados ao montar e ao abrir.
  const loadMeetings = () => { listMeetings().then(setMeetings).catch(() => {}) }
  useEffect(() => { loadMeetings() }, [])
  useEffect(() => { if (open) loadMeetings() }, [open])

  const pending = useMemo(
    () => meetings.filter((m) => m.my_status === 'pending' && new Date(m.starts_at).getTime() > Date.now() - 3600_000),
    [meetings],
  )
  const count = missed.length + pending.length

  // Fecha ao clicar fora / Esc.
  useEffect(() => {
    if (!open) return
    const onDoc = (e: MouseEvent) => { if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false) }
    const onEsc = (e: KeyboardEvent) => { if (e.key === 'Escape') setOpen(false) }
    document.addEventListener('mousedown', onDoc)
    document.addEventListener('keydown', onEsc)
    return () => { document.removeEventListener('mousedown', onDoc); document.removeEventListener('keydown', onEsc) }
  }, [open])

  return (
    <div className="notif" ref={ref}>
      <button
        className="nav-item notif-bell"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={t('notif.title', 'Notificações')}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="notif-bell-icon"><BellIcon />{count > 0 && <span className="notif-badge">{count > 9 ? '9+' : count}</span>}</span>
        <span>{t('notif.title', 'Notificações')}</span>
      </button>

      {open && (
        <div className="notif-panel" role="menu">
          <header className="notif-head">
            <strong>{t('notif.title', 'Notificações')}</strong>
            {missed.length > 0 && (
              <button className="notif-clear" onClick={ackMissed}>{t('notif.markRead', 'Marcar como lidas')}</button>
            )}
          </header>
          <div className="notif-list">
            {count === 0 && (
              <EmptyState icon={<BellIcon />} title={t('notif.emptyTitle', 'Sem novidades')} hint={t('notif.emptyHint', 'As chamadas perdidas e os convites por responder aparecem aqui.')} />
            )}

            {missed.map((mc) => (
              <div key={mc.id} className="notif-row">
                <span className="notif-icon danger">{mc.kind === 'voice' ? <VoiceCallIcon /> : <CamIcon />}</span>
                <div className="notif-body">
                  <span className="notif-line"><strong>{mc.caller_name}</strong> — {t('notif.missedCall', 'chamada perdida')}</span>
                  <span className="notif-time">{timeAgo(mc.created_at, t('notif.now', 'agora'))}</span>
                </div>
                <button className="notif-cta" onClick={() => { startCall({ targets: [mc.caller_id], kind: mc.kind, title: `${t('notif.callWith', 'Chamada com')} ${mc.caller_name}` }); ackMissed() }}>
                  {t('notif.callBack', 'Ligar')}
                </button>
              </div>
            ))}

            {pending.map((m) => (
              <div key={m.id} className="notif-row">
                <span className="notif-icon"><CalendarIcon /></span>
                <div className="notif-body">
                  <span className="notif-line"><strong>{m.title}</strong> — {t('notif.inviteFrom', 'convite de')} {m.owner_name}</span>
                  <span className="notif-time">{new Date(m.starts_at).toLocaleString()}</span>
                </div>
                <button className="notif-cta" onClick={() => { setOpen(false); onNavigate('calendar') }}>
                  {t('notif.respond', 'Responder')}
                </button>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}
