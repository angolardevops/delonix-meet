import { createContext, ReactNode, useContext, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ackMissedCalls } from '../api'
import { MissedCall, Presence, PresenceEvent } from '../presence'
import { CamIcon, CloseIcon, HangupIcon, MicIcon, VoiceCallIcon } from '../icons'

interface Ringing {
  room_code: string
  kind: 'video' | 'voice'
  caller_name: string
  title: string
}

interface PresenceCtx {
  online: Set<string>
  isOnline: (id: string) => boolean
  startCall: (opts: { targets?: string[]; groupId?: string; kind: 'video' | 'voice'; title?: string }) => void
  missed: MissedCall[]
  ackMissed: () => void
}

const Ctx = createContext<PresenceCtx | null>(null)
export const usePresence = () => {
  const c = useContext(Ctx)
  if (!c) throw new Error('usePresence fora do PresenceProvider')
  return c
}

export default function PresenceProvider({
  onEnterRoom,
  children,
}: {
  onEnterRoom: (code: string, voice: boolean) => void
  children: ReactNode
}) {
  const { t } = useTranslation()
  const presenceRef = useRef<Presence | null>(null)
  const [online, setOnline] = useState<Set<string>>(new Set())
  const [incoming, setIncoming] = useState<Ringing[]>([])
  const [toasts, setToasts] = useState<{ id: number; text: string }[]>([])
  const [missed, setMissed] = useState<MissedCall[]>([])
  const ringAudio = useRef<HTMLAudioElement | null>(null)

  function pushToast(text: string) {
    const id = Date.now() + Math.floor(performance.now())
    setToasts((t) => [...t, { id, text }])
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 8000)
  }

  // Notificação de sistema (desktop) quando entra uma chamada e a app NÃO está
  // visível (outro separador/minimizada). Clicar traz a janela para a frente.
  const callNotif = useRef<Notification | null>(null)
  function notifyIncoming(caller: string, title: string, kind: 'video' | 'voice') {
    if (!('Notification' in window) || Notification.permission !== 'granted') return
    if (document.visibilityState === 'visible') return // já se vê o popup na app
    try {
      callNotif.current?.close()
      const n = new Notification(`📞 ${caller} está a ligar`, {
        body: title || (kind === 'voice' ? 'Chamada de voz' : 'Videochamada'),
        tag: 'delonix-call',
        requireInteraction: true,
        icon: '/icon-192.png',
      })
      n.onclick = () => {
        window.focus()
        n.close()
      }
      callNotif.current = n
    } catch {
      /* Notifications indisponíveis — o toque + popup na app já avisam */
    }
  }

  // Pede permissão de notificações uma vez (silencioso se recusado).
  useEffect(() => {
    if ('Notification' in window && Notification.permission === 'default') {
      void Notification.requestPermission().catch(() => {})
    }
  }, [])

  useEffect(() => {
    const p = new Presence()
    presenceRef.current = p
    p.connect()
    const off = p.on((e: PresenceEvent) => {
      switch (e.type) {
        case 'presence':
          setOnline(new Set(e.online))
          break
        case 'incoming-call':
          setIncoming((cur) =>
            cur.some((c) => c.room_code === e.room_code)
              ? cur
              : [...cur, { room_code: e.room_code, kind: e.kind, caller_name: e.caller_name, title: e.title }],
          )
          notifyIncoming(e.caller_name, e.title, e.kind)
          break
        case 'ringing':
          // O chamador entra logo na sala e aguarda os outros.
          onEnterRoom(e.room_code, e.kind === 'voice')
          break
        case 'cancelled':
          setIncoming((cur) => cur.filter((c) => c.room_code !== e.room_code))
          break
        case 'meeting-declined':
          pushToast(`${e.by_name} recusou «${e.meeting_title}» — ${e.reason}`)
          break
        case 'missed-calls':
          setMissed(e.calls)
          break
        default:
          break
      }
    })
    return () => {
      off()
      p.close()
    }
    // onEnterRoom é estável (vem do App via location.hash) — só liga uma vez.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Toque enquanto há chamadas a receber — tom sintético quente estilo telemóvel
  // (não é possível embutir o som proprietário do Galaxy; recria-se um toque
  // agradável de dois sinos com harmónico suave e ligeiro eco).
  useEffect(() => {
    if (incoming.length === 0) {
      ringAudio.current?.pause()
      callNotif.current?.close()
      return
    }
    const ctx = new AudioContext()
    let stopped = false
    // Uma nota "marimba": fundamental + 2º harmónico, ataque rápido, cauda suave.
    const note = (f: number, at: number, dur = 0.34, vol = 0.16) => {
      for (const [mult, g] of [[1, vol], [2, vol * 0.35]] as const) {
        const osc = ctx.createOscillator()
        const gain = ctx.createGain()
        osc.type = 'sine'
        osc.frequency.value = f * mult
        osc.connect(gain)
        gain.connect(ctx.destination)
        const s = ctx.currentTime + at
        gain.gain.setValueAtTime(0.0001, s)
        gain.gain.exponentialRampToValueAtTime(g, s + 0.02)
        gain.gain.exponentialRampToValueAtTime(0.0001, s + dur)
        osc.start(s)
        osc.stop(s + dur + 0.02)
      }
    }
    const ring = () => {
      if (stopped) return
      // "din-don" ascendente-descendente (E5 → C#6 → A5).
      note(659.25, 0)
      note(1108.73, 0.16)
      note(880.0, 0.34, 0.5)
    }
    ring()
    const iv = setInterval(ring, 1800)
    return () => {
      stopped = true
      clearInterval(iv)
      void ctx.close()
    }
  }, [incoming.length])

  const value = useMemo<PresenceCtx>(
    () => ({
      online,
      isOnline: (id: string) => online.has(id),
      startCall: (opts) => presenceRef.current?.startCall(opts),
      missed,
      ackMissed: dismissMissed,
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [online, missed],
  )

  function accept(c: Ringing) {
    presenceRef.current?.accept(c.room_code)
    setIncoming((cur) => cur.filter((x) => x.room_code !== c.room_code))
    onEnterRoom(c.room_code, c.kind === 'voice')
  }
  function decline(c: Ringing) {
    presenceRef.current?.decline(c.room_code)
    setIncoming((cur) => cur.filter((x) => x.room_code !== c.room_code))
  }

  function dismissMissed() {
    setMissed([])
    void ackMissedCalls().catch(() => {})
  }
  function callBack(mc: MissedCall) {
    presenceRef.current?.startCall({ targets: [mc.caller_id], kind: mc.kind, title: `Chamada com ${mc.caller_name}` })
    dismissMissed()
  }

  return (
    <Ctx.Provider value={value}>
      {children}
      {missed.length > 0 && (
        <div className="missed-layer">
          <div className="missed-card">
            <div className="missed-head">
              <strong>{missed.length === 1 ? 'Chamada perdida' : `${missed.length} chamadas perdidas`}</strong>
              <button className="panel-close" onClick={dismissMissed}><CloseIcon /></button>
            </div>
            <div className="missed-list">
              {missed.map((mc) => (
                <div key={mc.id} className="missed-row">
                  <span className="missed-kind">{mc.kind === 'voice' ? <VoiceCallIcon /> : <CamIcon />}</span>
                  <span className="missed-info">
                    <strong>{mc.caller_name}</strong>
                    <small>{new Date(mc.created_at).toLocaleString('pt-PT')}</small>
                  </span>
                  <button className="missed-back" onClick={() => callBack(mc)}>{t('notif.ligarDeVolta')}</button>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}
      {toasts.length > 0 && (
        <div className="toast-layer">
          {toasts.map((t) => (
            <div key={t.id} className="toast" onClick={() => setToasts((cur) => cur.filter((x) => x.id !== t.id))}>
              ✕ {t.text}
            </div>
          ))}
        </div>
      )}
      {incoming.length > 0 && (
        <div className="ring-layer">
          {incoming.map((c) => (
            <div key={c.room_code} className="ring-card">
              <div className="ring-avatar pulse">{c.caller_name.slice(0, 2).toUpperCase()}</div>
              <div className="ring-info">
                <strong>{c.caller_name}</strong>
                <span className="ring-kind">
                  {c.kind === 'voice' ? <VoiceCallIcon /> : <CamIcon />}
                  Chamada de {c.kind === 'voice' ? 'voz' : 'vídeo'} a receber
                </span>
                <small>{c.title}</small>
              </div>
              <div className="ring-actions">
                <button className="ring-btn decline" title={t('room.espera.recusar')} onClick={() => decline(c)}>
                  <HangupIcon />
                </button>
                <button className="ring-btn accept" title={t('notif.atender')} onClick={() => accept(c)}>
                  {c.kind === 'voice' ? <MicIcon /> : <CamIcon />}
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </Ctx.Provider>
  )
}
