import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { postQos } from '../api'
import type { PeerInfo, Role } from '../signaling'
import type { QosReport } from '../webrtc'
import type { LocalConditions } from '../layerPolicy'
import { chaveFracos, deveActualizarQos, INTERVALO_RELATORIO_MS, intervaloQos } from './qosAmostra'
import type { RemotePeer, RoomCore } from './useRoomCore'

/** Papel de um PeerInfo: o do servidor, ou deduzido dos campos antigos. */
export function papelDe(p: Pick<PeerInfo, 'host' | 'can_admit' | 'role'>): Role {
  return p.role ?? (p.host ? 'host' : p.can_admit ? 'cohost' : 'attendee')
}

export function toPeer(p: PeerInfo): RemotePeer {
  return {
    peerId: p.peer_id,
    username: p.username,
    host: p.host,
    hand: p.hand,
    camOn: p.cam ?? true,
    micOn: p.mic ?? true,
    canAdmit: p.can_admit ?? p.host,
    stream: null,
    is_pstn: p.is_pstn,
    is_bot: p.is_bot,
    role: papelDe(p),
    origin: p.origin,
    title: p.title,
  }
}

/**
 * Quem está na sala, quem está à porta, e o que o anfitrião pode fazer a
 * cada um. Todas as acções passam pelo servidor, que revalida: o botão é a
 * conveniência, não a autorização.
 */
export function useParticipants(core: RoomCore, peoplePanelOpen: boolean) {
  const { signal, code, setPeers } = core
  const [waitingQueue, setWaitingQueue] = useState<PeerInfo[]>([])
  const [roomLocked, setRoomLocked] = useState(false)
  /** Sala de espera ligada AGORA (`room-settings.waiting_room`); `null` até o servidor dizer. */
  const [waitingRoomOn, setWaitingRoomOn] = useState<boolean | null>(null)
  const [qos, setQos] = useState<QosReport | null>(null)
  const [conditions, setConditions] = useState<LocalConditions>({})
  const [talkOver, setTalkOver] = useState(false)
  const talkOverSince = useRef(0)

  useEffect(() => {
    const offs = [
      // A grelha é orientada ao roster: retrato ao entrar, stream quando chegar.
      signal.on('joined', (m) => setPeers(m.peers.map(toPeer))),
      // Quem reclama o lugar volta com o MESMO peer_id: actualiza-se a entrada
      // existente em vez de criar outra, e limpa-se a marca de «a voltar».
      signal.on('peer-joined', (m) => setPeers((ps) => [...ps.filter((p) => p.peerId !== m.peer.peer_id), toPeer(m.peer)])),
      signal.on('peer-left', (m) => {
        core.levelsRef.current?.unwatch(m.peer_id)
        setPeers((ps) => ps.filter((p) => p.peerId !== m.peer_id))
      }),
      // O socket do outro caiu mas o lugar está reservado (R91): o retrato FICA.
      signal.on('peer-reconnecting', (m) =>
        setPeers((ps) => ps.map((p) => (p.peerId === m.peer_id ? { ...p, reconnecting: true } : p))),
      ),
      signal.on('hand', (m) => setPeers((ps) => ps.map((p) => (p.peerId === m.from ? { ...p, hand: m.raised } : p)))),
      signal.on('media', (m) =>
        setPeers((ps) => ps.map((p) => (p.peerId === m.from ? { ...p, camOn: m.cam, micOn: m.mic } : p))),
      ),
      // `peer-role` do b1-sala traz o papel; o antigo só `can_admit`. O MEU
      // papel também chega por aqui (é difundido à sala inteira).
      signal.onB1('peer-role', (m) => {
        const role = m.role ?? (m.can_admit ? 'cohost' : 'attendee')
        if (m.peer_id === core.meuPeerIdRef.current) core.setMyRole(role)
        setPeers((ps) => ps.map((p) => (p.peerId === m.peer_id ? { ...p, canAdmit: m.can_admit, role: p.host ? 'host' : role } : p)))
      }),
      signal.on('waiting-join', (m) => setWaitingQueue((q) => [...q.filter((p) => p.peer_id !== m.peer.peer_id), m.peer])),
      signal.on('waiting-left', (m) => setWaitingQueue((q) => q.filter((p) => p.peer_id !== m.peer_id))),
      signal.on('admit-role', (m) => {
        if (!m.allowed) setWaitingQueue([])
      }),
      signal.onB1('room-settings', (m) => {
        setRoomLocked(m.locked)
        if (m.waiting_room !== undefined) setWaitingRoomOn(m.waiting_room)
      }),
    ]
    return () => offs.forEach((off) => off())
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signal, setPeers, core.levelsRef])

  // Telemetria: UMA amostra de `getStats` serve o retrato («▲ FRACA»), a
  // política de camada e o relatório ao servidor — com ou sem painel aberto
  // (ver `qosAmostra.ts`). O painel só encurta o intervalo.
  const ultimoRelatorio = useRef(0)
  const chaveFracosRef = useRef<string | null>(null)
  useEffect(() => {
    if (core.roomState !== 'in') return
    let alive = true
    const tick = async () => {
      const r = await core.callRef.current?.qos?.().catch(() => null)
      if (!alive || !r) return
      const chave = chaveFracos(r.byPeer)
      if (deveActualizarQos(peoplePanelOpen, chaveFracosRef.current, chave)) {
        chaveFracosRef.current = chave
        setQos(r)
      }
      setConditions({
        backgrounded: document.visibilityState === 'hidden',
        cpuLimited: r.limitedBy === 'cpu',
        lossPct: r.lossPct,
        rttMs: r.rttMs ?? undefined,
        // Sem estimativa portável do downlink: inventar um tecto seria pior.
        downlinkKbps: null,
        dataSaver: Boolean((navigator as { connection?: { saveData?: boolean } }).connection?.saveData),
      })
      const agora = Date.now()
      if (agora - ultimoRelatorio.current < INTERVALO_RELATORIO_MS) return
      ultimoRelatorio.current = agora
      // A amostra COMPLETA: a média escondia o participante inaudível.
      void postQos(code, {
        rtt_ms: r.rttMs,
        loss_pct: r.lossPct,
        up_kbps: r.upKbps,
        down_kbps: r.downKbps,
        jitter_ms: r.jitterMs,
        score: r.score,
        freeze_ms: Math.round(r.freezeMs),
        concealment_pct: Math.round(r.concealmentRatio * 1000) / 10,
        frames_dropped: r.framesDropped,
        nack: r.nack,
        pli: r.pli,
        fir: r.fir,
        turn_relay: r.turnRelay,
        candidate_pair: r.candidatePair,
        limited_by: r.limitedBy,
      }).catch(() => {})
    }
    // Abrir o painel mostra números JÁ, não daqui a dois segundos.
    if (peoplePanelOpen) void tick()
    const id = setInterval(() => void tick(), intervaloQos(peoplePanelOpen))
    return () => {
      alive = false
      clearInterval(id)
    }
  }, [peoplePanelOpen, core.roomState, code, core.callRef])

  // Fala simultânea: duas ou mais pessoas durante mais de 1,5 s seguidos.
  useEffect(() => {
    if (core.speaking.size >= 2) {
      if (!talkOverSince.current) talkOverSince.current = Date.now()
      else if (Date.now() - talkOverSince.current > 1500) setTalkOver(true)
    } else {
      talkOverSince.current = 0
      const id = setTimeout(() => setTalkOver(false), 1200)
      return () => clearTimeout(id)
    }
  }, [core.speaking])

  const talkOverIds = useMemo(() => (talkOver ? [...core.speaking] : []), [talkOver, core.speaking])

  // Callbacks ESTÁVEIS: devolvem o `peerId` que recebem, e por isso um único
  // objecto serve todos os retratos sem anular o `memo` a jusante.
  const admit = useCallback(
    (peerId: string, ok: boolean) => {
      signal.send({ type: ok ? 'admit' : 'deny', to: peerId })
      setWaitingQueue((q) => q.filter((p) => p.peer_id !== peerId))
    },
    [signal],
  )
  const mute = useCallback((peerId: string) => signal.send({ type: 'force-mute', to: peerId }), [signal])
  const camOff = useCallback((peerId: string) => signal.send({ type: 'force-cam', to: peerId }), [signal])
  const kick = useCallback((peerId: string) => signal.send({ type: 'kick', to: peerId }), [signal])
  const transferHost = useCallback((peerId: string) => signal.send({ type: 'transfer-host', to: peerId }), [signal])
  const promoteAdmit = useCallback(
    (peerId: string, allowed: boolean) => signal.send({ type: 'promote-admit', to: peerId, allowed }),
    [signal],
  )
  const muteAll = useCallback((allowUnmute: boolean) => signal.send({ type: 'mute-all', allow_unmute: allowUnmute }), [signal])
  const setLocked = useCallback((locked: boolean) => signal.send({ type: 'room-lock', locked }), [signal])
  /** Só anfitrião. `host` passa-se pelo `transfer-host`, nunca por aqui. */
  const setRole = useCallback((peerId: string, role: Exclude<Role, 'host'>) => signal.sendB1({ type: 'set-role', to: peerId, role }), [signal])
  /** Anfitrião ou co-anfitrião: o servidor admite a fila inteira de uma vez. */
  const admitAll = useCallback(() => {
    signal.sendB1({ type: 'admit-all' })
    setWaitingQueue([])
  }, [signal])
  const setWaitingRoom = useCallback((on: boolean) => signal.sendB1({ type: 'waiting-room', on }), [signal])

  return {
    waitingQueue,
    roomLocked,
    qos,
    conditions,
    talkOver,
    talkOverIds,
    admit,
    admitAll,
    waitingRoomOn,
    setWaitingRoom,
    setRole,
    mute,
    camOff,
    kick,
    transferHost,
    promoteAdmit,
    muteAll,
    setLocked,
  }
}

export type Participants = ReturnType<typeof useParticipants>
