import { createContext, ReactNode, useContext, useEffect, useMemo, useRef, useState } from 'react'
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

  // Toca enquanto há chamadas a receber (tom sintético via WebAudio).
  useEffect(() => {
    if (incoming.length === 0) {
      ringAudio.current?.pause()
      return
    }
    const ctx = new AudioContext()
    let stopped = false
    const beep = () => {
      if (stopped) return
      const osc = ctx.createOscillator()
      const gain = ctx.createGain()
      osc.frequency.value = 480
      osc.connect(gain)
      gain.connect(ctx.destination)
      gain.gain.setValueAtTime(0.0001, ctx.currentTime)
      gain.gain.exponentialRampToValueAtTime(0.15, ctx.currentTime + 0.05)
      gain.gain.exponentialRampToValueAtTime(0.0001, ctx.currentTime + 0.9)
      osc.start()
      osc.stop(ctx.currentTime + 1)
    }
    beep()
    const iv = setInterval(beep, 2000)
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
    }),
    [online],
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
                  <button className="missed-back" onClick={() => callBack(mc)}>Ligar de volta</button>
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
                <button className="ring-btn decline" title="Recusar" onClick={() => decline(c)}>
                  <HangupIcon />
                </button>
                <button className="ring-btn accept" title="Atender" onClick={() => accept(c)}>
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
