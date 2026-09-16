import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { postQos } from '../api'
import type { PeerInfo } from '../signaling'
import type { QosReport } from '../webrtc'
import type { LocalConditions } from '../layerPolicy'
import type { RemotePeer, RoomCore } from './useRoomCore'

function toPeer(p: PeerInfo): RemotePeer {
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
      signal.on('peer-role', (m) =>
        setPeers((ps) => ps.map((p) => (p.peerId === m.peer_id ? { ...p, canAdmit: m.can_admit } : p))),
      ),
      signal.on('waiting-join', (m) => setWaitingQueue((q) => [...q.filter((p) => p.peer_id !== m.peer.peer_id), m.peer])),
      signal.on('waiting-left', (m) => setWaitingQueue((q) => q.filter((p) => p.peer_id !== m.peer_id))),
      signal.on('admit-role', (m) => {
        if (!m.allowed) setWaitingQueue([])
      }),
      signal.on('room-settings', (m) => setRoomLocked(m.locked)),
    ]
    return () => offs.forEach((off) => off())
  }, [signal, setPeers, core.levelsRef])

  // Telemetria por participante: amostra a cada 2 s com o painel aberto.
  useEffect(() => {
    if (!peoplePanelOpen) return
    const call = core.callRef.current
    if (!call?.qos) return
    let alive = true
    const tick = async () => {
      const r = await call.qos!().catch(() => null)
      if (alive && r) setQos(r)
    }
    void tick()
    const id = setInterval(tick, 2000)
    return () => {
      alive = false
      clearInterval(id)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [peoplePanelOpen, core.roomState])

  // Amostra a cada 5 s (a política de camada precisa de reagir em segundos) e
  // REPORTA a cada sexta (~30 s). 5 s e não 1 s: duas leituras dentro do mesmo
  // intervalo de estatísticas do Chrome dão delta zero (R37).
  useEffect(() => {
    if (core.roomState !== 'in') return
    let n = 0
    const report = async () => {
      const r = await core.callRef.current?.qos?.().catch(() => null)
      if (!r) return
      setConditions({
        backgrounded: document.visibilityState === 'hidden',
        cpuLimited: r.limitedBy === 'cpu',
        lossPct: r.lossPct,
        rttMs: r.rttMs ?? undefined,
        // Sem estimativa portável do downlink: inventar um tecto seria pior.
        downlinkKbps: null,
        dataSaver: Boolean((navigator as { connection?: { saveData?: boolean } }).connection?.saveData),
      })
      if (n++ % 6 !== 0) return
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
    const id = setInterval(() => void report(), 5_000)
    return () => clearInterval(id)
  }, [core.roomState, code, core.callRef])

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

  return {
    waitingQueue,
    roomLocked,
    qos,
    conditions,
    talkOver,
    talkOverIds,
    admit,
    admitAll: () => waitingQueue.forEach((p) => admit(p.peer_id, true)),
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
