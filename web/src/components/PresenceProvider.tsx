/**
 * Presença e chamadas directas. Vive ACIMA do router: uma chamada toca em
 * qualquer ecrã, incluindo dentro de uma sala. Mostra o cartão de chamada a
 * entrar (atender / recusar), o toque, a notificação do sistema quando a app
 * não está visível, e os avisos de reunião recusada.
 */
import { createContext, ReactNode, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ackMissedCalls } from '../api'
import { MissedCall, Presence, PresenceEvent } from '../presence'
import { startRingtone } from '../ringtone'
import { RING_TIMEOUT_MS, roomCodeInHash } from '../callRing'
import { Icon } from '../ui/icons'
import { Avatar, Button, IconButton } from '../ui/kit'

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
  callBack: (mc: MissedCall) => void
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
  const notif = useRef<Notification | null>(null)
  // Chamadas que ESTA sessão fez e que ainda ninguém atendeu: se quem liga
  // sair da sala antes disso, os outros deixam de tocar (`call-cancel`).
  const outgoing = useRef(new Set<string>())
  const enterRef = useRef(onEnterRoom)
  enterRef.current = onEnterRoom

  const pushToast = useCallback((text: string) => {
    const id = Date.now() + Math.random()
    setToasts((cur) => [...cur, { id, text }])
    setTimeout(() => setToasts((cur) => cur.filter((x) => x.id !== id)), 8000)
  }, [])

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
          if ('Notification' in window && Notification.permission === 'granted' && document.visibilityState !== 'visible') {
            try {
              notif.current?.close()
              const n = new Notification(t('shell.chamada.aLigar', { nome: e.caller_name }), {
                body: e.title || (e.kind === 'voice' ? t('shell.chamada.voz') : t('shell.chamada.video')),
                tag: 'delonix-call',
                requireInteraction: true,
                icon: '/icon-192.png',
              })
              n.onclick = () => {
                window.focus()
                n.close()
              }
              notif.current = n
            } catch {
              /* notificações indisponíveis — o cartão e o toque avisam */
            }
          }
          break
        case 'ringing':
          // Quem liga entra logo na sala e espera pelos outros.
          outgoing.current.add(e.room_code)
          enterRef.current(e.room_code, e.kind === 'voice')
          break
        case 'accepted':
          outgoing.current.delete(e.room_code)
          break
        case 'cancelled':
          setIncoming((cur) => cur.filter((c) => c.room_code !== e.room_code))
          break
        case 'meeting-declined':
          pushToast(t('shell.chamada.reuniaoRecusada', { nome: e.by_name, titulo: e.meeting_title, motivo: e.reason }))
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Sair da sala de uma chamada por atender = desligar antes de atenderem.
  // Entrar na sala de uma chamada a tocar (por link, noutro ecrã) = atendida.
  useEffect(() => {
    const onHash = () => {
      const here = roomCodeInHash(location.hash)
      for (const code of [...outgoing.current]) {
        if (code !== here) {
          outgoing.current.delete(code)
          presenceRef.current?.cancel(code)
        }
      }
      if (here) setIncoming((cur) => (cur.some((c) => c.room_code === here) ? cur.filter((c) => c.room_code !== here) : cur))
    }
    window.addEventListener('hashchange', onHash)
    return () => window.removeEventListener('hashchange', onHash)
  }, [])

  // Cada chamada a entrar deixa de tocar ao fim de RING_TIMEOUT_MS.
  const ringingCodes = incoming.map((c) => c.room_code).join(',')
  useEffect(() => {
    if (!ringingCodes) return
    const timers = ringingCodes.split(',').map((code) =>
      setTimeout(() => setIncoming((cur) => cur.filter((c) => c.room_code !== code)), RING_TIMEOUT_MS),
    )
    return () => timers.forEach(clearTimeout)
  }, [ringingCodes])

  useEffect(() => {
    if (incoming.length === 0) {
      notif.current?.close()
      return
    }
    return startRingtone()
  }, [incoming.length])

  const ackMissed = useCallback(() => {
    setMissed([])
    void ackMissedCalls().catch(() => {})
  }, [])

  const value = useMemo<PresenceCtx>(
    () => ({
      online,
      isOnline: (id) => online.has(id),
      startCall: (opts) => presenceRef.current?.startCall(opts),
      missed,
      ackMissed,
      callBack: (mc) => {
        presenceRef.current?.startCall({ targets: [mc.caller_id], kind: mc.kind, title: t('shell.chamada.com', { nome: mc.caller_name }) })
        ackMissed()
      },
    }),
    [online, missed, ackMissed, t],
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

  return (
    <Ctx.Provider value={value}>
      {children}
      {incoming.length > 0 && (
        <div className="call-ring-stack" role="alertdialog" aria-live="assertive" aria-label={t('shell.chamada.aEntrar')}>
          {incoming.map((c) => (
            <div key={c.room_code} className="call-ring dx-stage">
              <Avatar name={c.caller_name} size={44} />
              <div className="call-ring__who">
                <span className="dx-eyebrow">{c.kind === 'voice' ? t('shell.chamada.voz') : t('shell.chamada.video')}</span>
                <strong>{c.caller_name}</strong>
                {c.title && <span className="dx-muted">{c.title}</span>}
              </div>
              <Button variant="danger" icon="phoneOff" onClick={() => decline(c)}>
                {t('shell.chamada.recusar')}
              </Button>
              <Button variant="primary" icon={c.kind === 'voice' ? 'phone' : 'video'} onClick={() => accept(c)} autoFocus>
                {t('shell.chamada.atender')}
              </Button>
            </div>
          ))}
        </div>
      )}
      {toasts.length > 0 && (
        <div className="dx-toasts" role="status">
          {toasts.map((x) => (
            <div key={x.id} className="dx-toast">
              <Icon name="info" />
              <span style={{ flex: 1 }}>{x.text}</span>
              <IconButton icon="x" bare label={t('ui.fechar')} onClick={() => setToasts((cur) => cur.filter((y) => y.id !== x.id))} />
            </div>
          ))}
        </div>
      )}
    </Ctx.Provider>
  )
}
