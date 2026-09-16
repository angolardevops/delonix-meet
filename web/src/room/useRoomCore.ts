import { Dispatch, MutableRefObject, SetStateAction, useCallback, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { Call } from '../webrtc'
import type { BackgroundEffect, Denoiser, HeadTracker, LevelWatcher } from '../media'
import type { MicMix } from './micMix'
import { RoomSignal } from './signalBus'

/** Um participante remoto tal como a sala o conhece. */
export interface RemotePeer {
  peerId: string
  username: string
  host: boolean
  hand: boolean
  camOn: boolean
  micOn: boolean
  /** Foi promovido a co-admitir entradas (o anfitrião tem-no sempre). */
  canAdmit: boolean
  stream: MediaStream | null
  is_pstn?: boolean
  is_bot?: boolean
  /** O socket caiu e o lugar está reservado (R91): o retrato fica, esbatido. */
  reconnecting?: boolean
}

export type RoomState = 'prejoin' | 'connecting' | 'waiting' | 'denied' | 'kicked' | 'notfound' | 'in' | 'e2ee-pass'

export interface Presentation {
  peerId: string
  stream: MediaStream
}

/**
 * O estado PARTILHADO da sala — o que mais de um hook lê ou escreve. Cada
 * funcionalidade tem o seu hook; este é o chão comum onde se encontram, para
 * que nenhum hook tenha de ser chamado antes de outro só para lhe passar um
 * setter.
 */
export interface RoomCore {
  code: string
  signal: RoomSignal
  roomState: RoomState
  setRoomState: Dispatch<SetStateAction<RoomState>>
  status: string
  setStatus: Dispatch<SetStateAction<string>>
  topology: string
  setTopology: Dispatch<SetStateAction<string>>
  isHost: boolean
  setIsHost: Dispatch<SetStateAction<boolean>>
  peers: RemotePeer[]
  setPeers: Dispatch<SetStateAction<RemotePeer[]>>
  presentation: Presentation | null
  setPresentation: Dispatch<SetStateAction<Presentation | null>>
  sharing: boolean
  setSharing: Dispatch<SetStateAction<boolean>>
  speaking: Set<string>
  /** Só troca o estado quando o CONJUNTO muda (ver `sameSet`). */
  updateSpeaking: (s: Set<string>) => void

  callRef: MutableRefObject<Call | null>
  localStreamRef: MutableRefObject<MediaStream | null>
  cameraTrackRef: MutableRefObject<MediaStreamTrack | null>
  previewStreamRef: MutableRefObject<MediaStream | null>
  prejoinPermErrRef: MutableRefObject<string>
  localVideoRef: MutableRefObject<HTMLVideoElement | null>
  displayStreamRef: MutableRefObject<MediaStream | null>
  effectRef: MutableRefObject<BackgroundEffect | null>
  headRef: MutableRefObject<HeadTracker | null>
  denoiserRef: MutableRefObject<Denoiser | null>
  rawMicRef: MutableRefObject<MediaStreamTrack | null>
  levelsRef: MutableRefObject<LevelWatcher | null>
  roomTokenRef: MutableRefObject<string | null>
  e2eeKeyRef: MutableRefObject<string | null>
  joinedAtRef: MutableRefObject<number>
  meuPeerIdRef: MutableRefObject<string>
  peersRef: MutableRefObject<RemotePeer[]>
  isHostRef: MutableRefObject<boolean>
  bgModeRef: MutableRefObject<'none' | 'blur' | 'image'>
  /** Dois microfones misturados (pré-entrada → sala). Dono: quem a parar. */
  micMixRef: MutableRefObject<MicMix | null>
  /** Segunda fonte de vídeo escolhida na pré-entrada, à espera de ser publicada. */
  secondSourceRef: MutableRefObject<MediaStream | null>
}

/**
 * Atualizador de «quem está a falar» que só troca o estado quando o CONJUNTO
 * muda. O `LevelWatcher` dispara a cada 180 ms; um `new Set` a cada tique
 * re-renderizava a sala inteira ~5,5×/s, mesmo em silêncio absoluto.
 */
export function sameSet(a: Set<string>, b: Set<string>): boolean {
  if (a.size !== b.size) return false
  for (const v of a) if (!b.has(v)) return false
  return true
}

export function useRoomCore(code: string, initialState: RoomState): RoomCore {
  const { t } = useTranslation()
  const signalRef = useRef<RoomSignal | null>(null)
  if (!signalRef.current) signalRef.current = new RoomSignal()

  const [roomState, setRoomState] = useState<RoomState>(initialState)
  const [status, setStatus] = useState(() => t('room.estado.aLigar'))
  const [topology, setTopology] = useState('')
  const [isHost, setIsHost] = useState(false)
  const [peers, setPeers] = useState<RemotePeer[]>([])
  const [presentation, setPresentation] = useState<Presentation | null>(null)
  const [sharing, setSharing] = useState(false)
  const [speaking, setSpeaking] = useState<Set<string>>(() => new Set())
  const updateSpeaking = useCallback(
    (s: Set<string>) => setSpeaking((prev) => (sameSet(prev, s) ? prev : new Set(s))),
    [],
  )

  const peersRef = useRef<RemotePeer[]>([])
  peersRef.current = peers
  const isHostRef = useRef(false)
  isHostRef.current = isHost
  const joinedAtRef = useRef(0)

  // A duração conta desde a PRIMEIRA entrada. Marca-se no RENDER e não num
  // efeito: um efeito só corre depois deste render, e o relógio já teria
  // recebido `startedAt` a 0 — ficava preso nesse zero até outro render.
  if (roomState === 'in' && !joinedAtRef.current) joinedAtRef.current = Date.now()

  return {
    code,
    signal: signalRef.current,
    roomState,
    setRoomState,
    status,
    setStatus,
    topology,
    setTopology,
    isHost,
    setIsHost,
    peers,
    setPeers,
    presentation,
    setPresentation,
    sharing,
    setSharing,
    speaking,
    updateSpeaking,
    callRef: useRef<Call | null>(null),
    localStreamRef: useRef<MediaStream | null>(null),
    cameraTrackRef: useRef<MediaStreamTrack | null>(null),
    previewStreamRef: useRef<MediaStream | null>(null),
    prejoinPermErrRef: useRef(''),
    localVideoRef: useRef<HTMLVideoElement | null>(null),
    displayStreamRef: useRef<MediaStream | null>(null),
    effectRef: useRef<BackgroundEffect | null>(null),
    headRef: useRef<HeadTracker | null>(null),
    denoiserRef: useRef<Denoiser | null>(null),
    rawMicRef: useRef<MediaStreamTrack | null>(null),
    levelsRef: useRef<LevelWatcher | null>(null),
    roomTokenRef: useRef<string | null>(null),
    e2eeKeyRef: useRef<string | null>(null),
    joinedAtRef,
    meuPeerIdRef: useRef(''),
    peersRef,
    isHostRef,
    bgModeRef: useRef<'none' | 'blur' | 'image'>('none'),
    micMixRef: useRef<MicMix | null>(null),
    secondSourceRef: useRef<MediaStream | null>(null),
  }
}
