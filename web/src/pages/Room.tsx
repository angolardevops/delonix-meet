import { CSSProperties, ReactNode, useEffect, useMemo, useRef, useState } from 'react'
import {
  currentUser, downloadRecording, iceServers, joinRoom, listRecordings, Recording,
  saveMinutesByRoom, uploadRecording,
} from '../api'
import {
  audioConstraints,
  BackgroundEffect,
  DeviceSets,
  HeadTracker,
  LevelWatcher,
  listDevices,
  MeetingRecorder,
  presetBackgrounds,
  speechSupported,
  Transcriber,
  videoConstraints,
} from '../media'
import { deriveRoomKey, e2eeSupported, FrameCrypto } from '../e2ee'
import { PeerInfo, Signaling } from '../signaling'
import { Call, MeshCall, SfuCall } from '../webrtc'
import {
  BlurIcon, CamIcon, CamOffIcon, ChatIcon, ChevronUpIcon, CloseIcon, CubeIcon, DownloadIcon, EmojiIcon, HandIcon,
  HangupIcon, MicIcon, MicOffIcon, NoteIcon, PeopleIcon, RecordIcon, SettingsIcon, ShareIcon, StageIcon, StopIcon,
} from '../icons'

interface RemotePeer {
  peerId: string
  username: string
  host: boolean
  hand: boolean
  stream: MediaStream | null
}

interface ChatMsg {
  username: string
  text: string
  own: boolean
}

interface FloatingReaction {
  id: number
  emoji: string
  username: string
}

const REACTION_EMOJIS = ['👍', '❤️', '😂', '🎉', '👏', '😮']
let reactionSeq = 0

/**
 * Constrói uma ata (Minutes of Meeting) simples a partir das linhas de
 * transcrição: resumo por tópicos e extração heurística de decisões/ações.
 * (Nota bruta assistida — não substitui revisão humana.)
 */
function buildMoM(lines: string[]): string {
  if (lines.length === 0) return ''
  const clean = lines.map((l) => l.replace(/^\[\d{2}:\d{2}\]\s*/, '').trim()).filter(Boolean)
  const actionRe = /(vamos|temos de|precisamos|fica responsável|ação|action|decidimos|ficou decidido|próximo passo|até (?:amanhã|sexta|segunda|ao fim))/i
  const actions = clean.filter((l) => actionRe.test(l))
  const date = new Date().toLocaleDateString('pt-PT', { day: 'numeric', month: 'long', year: 'numeric' })
  const out: string[] = []
  out.push(`# Ata da reunião — ${date}`, '')
  out.push('## Resumo', ...clean.slice(0, 12).map((l) => `- ${l}`), '')
  if (actions.length) out.push('## Decisões e ações', ...actions.map((l) => `- [ ] ${l}`), '')
  out.push(`_${clean.length} intervenções transcritas._`)
  return out.join('\n')
}

type RoomState = 'connecting' | 'waiting' | 'denied' | 'kicked' | 'in' | 'e2ee-pass'
type Panel = 'none' | 'chat' | 'people' | 'settings'

export default function Room({
  code,
  voiceOnly = false,
  onLeave,
  onSwitch,
}: {
  code: string
  voiceOnly?: boolean
  onLeave: () => void
  onSwitch?: (code: string) => void
}) {
  const [roomState, setRoomState] = useState<RoomState>('connecting')
  const [peers, setPeers] = useState<RemotePeer[]>([])
  const [waitingQueue, setWaitingQueue] = useState<PeerInfo[]>([])
  const [reactions, setReactions] = useState<FloatingReaction[]>([])
  const [chat, setChat] = useState<ChatMsg[]>([])
  const [chatInput, setChatInput] = useState('')
  const [panel, setPanel] = useState<Panel>('none')
  const [pickerOpen, setPickerOpen] = useState(false)
  const [deviceMenu, setDeviceMenu] = useState<'none' | 'mic' | 'cam'>('none')
  const [micOn, setMicOn] = useState(true)
  const [camOn, setCamOn] = useState(true)
  const [sharing, setSharing] = useState(false)
  const [handRaised, setHandRaised] = useState(false)
  const [hasLocalVideo, setHasLocalVideo] = useState(true)
  const [isHost, setIsHost] = useState(false)
  const [status, setStatus] = useState('A ligar…')
  const [topology, setTopology] = useState('')
  const [clock, setClock] = useState(() => new Date().toLocaleTimeString('pt-PT', { hour: '2-digit', minute: '2-digit' }))

  // Dispositivos & qualidade
  const [devices, setDevices] = useState<DeviceSets>({ mics: [], cams: [], speakers: [] })
  const [micId, setMicId] = useState('')
  const [camId, setCamId] = useState('')
  const [speakerId, setSpeakerId] = useState('')
  const [noiseSuppression, setNoiseSuppression] = useState(true)
  const [bgMode, setBgMode] = useState<'none' | 'blur' | 'image'>('none')
  const [bgImageUrl, setBgImageUrl] = useState('')
  const [bgBusy, setBgBusy] = useState(false)
  const presets = useMemo(() => presetBackgrounds(), [])

  // Voz / fala simultânea
  const [speaking, setSpeaking] = useState<Set<string>>(new Set())
  const [talkOver, setTalkOver] = useState(false)

  // Gravação
  const [recording, setRecording] = useState(false)      // eu estou a gravar
  const [remoteRecorder, setRemoteRecorder] = useState('') // alguém está a gravar
  const [recordings, setRecordings] = useState<Recording[]>([])
  const [recBusy, setRecBusy] = useState(false)

  // E2EE
  const [passInput, setPassInput] = useState('')
  const [passTry, setPassTry] = useState(0)
  const [e2eeOn, setE2eeOn] = useState(false)

  // Breakout rooms
  const [breakoutRooms, setBreakoutRooms] = useState<{ code: string; label: string }[]>([])
  const returnTo = sessionStorage.getItem(`dx_return_${code}`)

  // Conferência / 3D / notas AI
  const [viewMode, setViewMode] = useState<'grid' | 'stage'>('grid')
  const [parallax, setParallax] = useState(false)
  const [tilt, setTilt] = useState({ x: 0, y: 0 })
  const [notesOpen, setNotesOpen] = useState(false)
  const [transcribing, setTranscribing] = useState(false)
  const [lines, setLines] = useState<string[]>([])
  const [interim, setInterim] = useState('')
  const [momSaved, setMomSaved] = useState(false)

  const localVideo = useRef<HTMLVideoElement>(null)
  const callRef = useRef<Call | null>(null)
  const signalRef = useRef<Signaling | null>(null)
  const localStreamRef = useRef<MediaStream | null>(null)
  const cameraTrackRef = useRef<MediaStreamTrack | null>(null)
  const effectRef = useRef<BackgroundEffect | null>(null)
  const uploadRef = useRef<HTMLInputElement>(null)
  const headRef = useRef<HeadTracker | null>(null)
  const transcriberRef = useRef<Transcriber | null>(null)
  const levelsRef = useRef<LevelWatcher | null>(null)
  const recorderRef = useRef<MeetingRecorder | null>(null)
  const talkOverSince = useRef(0)
  const peersRef = useRef<RemotePeer[]>([])
  peersRef.current = peers

  function floatReaction(emoji: string, username: string) {
    const id = ++reactionSeq
    setReactions((rs) => [...rs, { id, emoji, username }])
    setTimeout(() => setReactions((rs) => rs.filter((r) => r.id !== id)), 3500)
  }

  useEffect(() => {
    const t = setInterval(
      () => setClock(new Date().toLocaleTimeString('pt-PT', { hour: '2-digit', minute: '2-digit' })),
      30_000,
    )
    return () => clearInterval(t)
  }, [])

  useEffect(() => {
    let cancelled = false
    async function start() {
      try {
        // Camera+mic (máx. resolução), depois só mic, depois modo espectador — nunca bloquear a entrada.
        // Reunião de voz: pede só o microfone (entra sem vídeo).
        const stream = await navigator.mediaDevices
          .getUserMedia(
            voiceOnly ? { audio: audioConstraints() } : { audio: audioConstraints(), video: videoConstraints() },
          )
          .catch(() => navigator.mediaDevices.getUserMedia({ audio: audioConstraints() }))
          .catch(() => new MediaStream())
        stream.getVideoTracks().forEach((t) => (t.contentHint = 'motion'))
        if (cancelled) {
          stream.getTracks().forEach((t) => t.stop())
          return
        }
        const spectator = stream.getTracks().length === 0
        if (spectator) setStatus('Sem câmara/microfone — modo espectador')
        setHasLocalVideo(stream.getVideoTracks().length > 0)
        localStreamRef.current = stream
        cameraTrackRef.current = stream.getVideoTracks()[0] ?? null
        if (localVideo.current) localVideo.current.srcObject = stream

        // Deteção de voz local + lista de dispositivos (labels só após permissão).
        const levels = new LevelWatcher((s) => setSpeaking(new Set(s)))
        levelsRef.current = levels
        levels.watch('me', stream)
        void listDevices().then((d) => {
          if (cancelled) return
          setDevices(d)
          setMicId(stream.getAudioTracks()[0]?.getSettings().deviceId ?? '')
          setCamId(stream.getVideoTracks()[0]?.getSettings().deviceId ?? '')
        })
        navigator.mediaDevices.addEventListener?.('devicechange', () => void listDevices().then(setDevices))

        const [{ room, room_token }, rtcConfig] = await Promise.all([joinRoom(code), iceServers()])
        setTopology(room.topology)
        const amHost = room.owner_id === currentUser()?.id
        setIsHost(amHost)

        // E2EE: deriva a chave da frase-chave ANTES de ligar; a chave nunca
        // sai deste dispositivo — o servidor/SFU só vê frames cifrados.
        let crypto: FrameCrypto | undefined
        if (room.e2ee) {
          if (!e2eeSupported()) {
            setStatus('Este browser não suporta E2EE (Insertable Streams)')
            return
          }
          const pass = sessionStorage.getItem(`dx_e2ee_${code}`)
          if (!pass) {
            setRoomState('e2ee-pass')
            return
          }
          crypto = new FrameCrypto()
          await crypto.setKey(await deriveRoomKey(pass, code))
          setE2eeOn(true)
        }

        const signal = new Signaling(room_token)
        signalRef.current = signal
        signal.on('chat', (m) => setChat((c) => [...c, { username: m.username, text: m.text, own: false }]))
        signal.on('error', (m) => setStatus(m.message))
        signal.onclose = () => setStatus('Ligação terminada')

        // Estados da sala de espera / controlo do anfitrião.
        signal.on('waiting', () => setRoomState('waiting'))
        signal.on('denied', () => setRoomState('denied'))
        signal.on('kicked', () => {
          setRoomState('kicked')
          callRef.current?.hangup()
        })
        signal.on('force-muted', () => {
          const track = localStreamRef.current?.getAudioTracks()[0]
          if (track) track.enabled = false
          setMicOn(false)
          setStatus('O anfitrião silenciou o teu microfone')
        })
        signal.on('waiting-join', (m) =>
          setWaitingQueue((q) => [...q.filter((p) => p.peer_id !== m.peer.peer_id), m.peer]),
        )
        signal.on('waiting-left', (m) => setWaitingQueue((q) => q.filter((p) => p.peer_id !== m.peer_id)))

        // Breakout rooms: mover para o grupo (guardando o caminho de volta)
        // ou regressar à sala principal quando o anfitrião encerra.
        signal.on('breakout-move', (m) => {
          if (m.back) sessionStorage.removeItem(`dx_return_${code}`)
          else sessionStorage.setItem(`dx_return_${m.code}`, code)
          onSwitch?.(m.code)
        })
        signal.on('breakouts-created', (m) => setBreakoutRooms(m.rooms))

        // A grelha é orientada ao roster: tile ao entrar, stream quando chegar.
        signal.on('joined', (m) => {
          setRoomState('in')
          setStatus(spectator ? 'Sem câmara/microfone — modo espectador' : '')
          setPeers(
            m.peers.map((p) => ({
              peerId: p.peer_id,
              username: p.username,
              host: p.host,
              hand: p.hand,
              stream: null,
            })),
          )
          void listRecordings(code).then(setRecordings).catch(() => {})
        })
        signal.on('peer-joined', (m) =>
          setPeers((ps) => [
            ...ps.filter((p) => p.peerId !== m.peer.peer_id),
            { peerId: m.peer.peer_id, username: m.peer.username, host: m.peer.host, hand: m.peer.hand, stream: null },
          ]),
        )
        signal.on('peer-left', (m) => {
          levelsRef.current?.unwatch(m.peer_id)
          setPeers((ps) => ps.filter((p) => p.peerId !== m.peer_id))
        })
        signal.on('hand', (m) =>
          setPeers((ps) => ps.map((p) => (p.peerId === m.from ? { ...p, hand: m.raised } : p))),
        )
        signal.on('reaction', (m) => floatReaction(m.emoji, m.username))
        signal.on('recording', (m) => {
          setRemoteRecorder(m.active ? m.username : '')
          if (!m.active) void listRecordings(code).then(setRecordings).catch(() => {})
        })
        // Transcrição de outro participante — junta à transcrição partilhada.
        signal.on('transcript', (m) => {
          const stamp = new Date().toLocaleTimeString('pt-PT', { hour: '2-digit', minute: '2-digit' })
          setLines((l) => [...l, `[${stamp}] ${m.username}: ${m.text}`])
        })

        const callbacks = {
          // Media para um peer desconhecido é descartada — o roster chega
          // sempre primeiro; evita tiles fantasma de m-lines sem publisher.
          onStream: (peerId: string, remote: MediaStream) => {
            levelsRef.current?.watch(peerId, remote)
            setPeers((ps) => ps.map((p) => (p.peerId === peerId ? { ...p, stream: remote } : p)))
          },
          onPeerLeft: (peerId: string) => {
            levelsRef.current?.unwatch(peerId)
            setPeers((ps) => ps.map((p) => (p.peerId === peerId ? { ...p, stream: null } : p)))
          },
        }
        callRef.current =
          room.topology === 'sfu'
            ? new SfuCall(signal, stream, rtcConfig, callbacks, crypto)
            : new MeshCall(signal, stream, rtcConfig, callbacks, crypto)
      } catch (err) {
        setStatus(`Erro: ${(err as Error).message}`)
      }
    }
    start()
    return () => {
      cancelled = true
      levelsRef.current?.close()
      effectRef.current?.stop()
      headRef.current?.stop()
      transcriberRef.current?.stop()
      callRef.current?.hangup()
      localStreamRef.current?.getTracks().forEach((t) => t.stop())
    }
  }, [code, passTry])

  // Aviso de fala simultânea: ≥2 pessoas a falar durante >1.5s seguidos.
  useEffect(() => {
    if (speaking.size >= 2) {
      if (!talkOverSince.current) talkOverSince.current = Date.now()
      else if (Date.now() - talkOverSince.current > 1500) setTalkOver(true)
    } else {
      talkOverSince.current = 0
      const t = setTimeout(() => setTalkOver(false), 1200)
      return () => clearTimeout(t)
    }
  }, [speaking])

  // A gravação segue as mudanças de participantes/streams.
  useEffect(() => {
    if (!recorderRef.current) return
    recorderRef.current.setSources([
      { id: 'me', label: 'eu', stream: localStreamRef.current },
      ...peers.map((p) => ({ id: p.peerId, label: p.username, stream: p.stream })),
    ])
  }, [peers, recording])

  function toggleMic() {
    levelsRef.current?.resume()
    const track = localStreamRef.current?.getAudioTracks()[0]
    if (track) {
      track.enabled = !track.enabled
      setMicOn(track.enabled)
    }
  }

  async function toggleCam() {
    const existing = localStreamRef.current?.getVideoTracks()[0]
    if (existing) {
      existing.enabled = !existing.enabled
      setCamOn(existing.enabled)
      return
    }
    // Voz -> vídeo: adquirir câmara e publicar (renegoceia no SFU).
    try {
      const s = await navigator.mediaDevices.getUserMedia({ video: videoConstraints(camId || undefined) })
      const track = s.getVideoTracks()[0]
      track.contentHint = 'motion'
      cameraTrackRef.current = track
      localStreamRef.current?.addTrack(track)
      setHasLocalVideo(true)
      setCamOn(true)
      let sendTrack = track
      if (bgMode !== 'none' && effectRef.current) sendTrack = await effectRef.current.start(track)
      await callRef.current?.enableVideo(sendTrack, localStreamRef.current ?? s)
      if (localVideo.current) {
        localVideo.current.srcObject = bgMode !== 'none' ? new MediaStream([sendTrack]) : localStreamRef.current
      }
      setStatus('')
    } catch {
      setStatus('Não foi possível ligar a câmara')
    }
  }

  /** Efeito de sala 3D: segue a cabeça e inclina a grelha (parallax). */
  async function toggleParallax() {
    if (parallax) {
      headRef.current?.stop()
      headRef.current = null
      setParallax(false)
      setTilt({ x: 0, y: 0 })
      return
    }
    if (!cameraTrackRef.current) {
      setStatus('O efeito 3D precisa da câmara ligada')
      return
    }
    try {
      const h = new HeadTracker()
      h.onUpdate = (x, y) => setTilt({ x, y })
      await h.start(new MediaStream([cameraTrackRef.current]))
      headRef.current = h
      setParallax(true)
    } catch {
      setStatus('Efeito 3D indisponível neste dispositivo')
    }
  }

  /** Notas AI: transcrição contínua do microfone local. */
  function toggleTranscription() {
    if (transcribing) {
      transcriberRef.current?.stop()
      transcriberRef.current = null
      setTranscribing(false)
      return
    }
    if (!speechSupported()) {
      setStatus('Transcrição não suportada neste browser (usa Chrome/Edge)')
      return
    }
    const t = new Transcriber()
    t.onFinal = (text) => {
      const stamp = new Date().toLocaleTimeString('pt-PT', { hour: '2-digit', minute: '2-digit' })
      setLines((l) => [...l, `[${stamp}] eu: ${text}`])
      setInterim('')
      // Difunde a minha frase para os outros montarem a transcrição partilhada.
      signalRef.current?.send({ type: 'transcript', text })
    }
    t.onInterim = (text) => setInterim(text)
    t.start('pt-PT')
    transcriberRef.current = t
    setTranscribing(true)
    setNotesOpen(true)
  }

  /** Gera as MoM a partir da transcrição e guarda na reunião (se houver). */
  async function saveMinutes() {
    const transcript = lines.join('\n')
    const mom = buildMoM(lines)
    try {
      await saveMinutesByRoom(code, mom, transcript)
      setMomSaved(true)
      setStatus('Ata (MoM) guardada na reunião')
      setTimeout(() => setMomSaved(false), 3000)
    } catch {
      setStatus('Não foi possível guardar a ata')
    }
  }

  /** Troca o microfone e/ou reaplica a supressão de ruído. */
  async function switchMic(deviceId: string, ns = noiseSuppression) {
    try {
      const s = await navigator.mediaDevices.getUserMedia({ audio: audioConstraints(deviceId || undefined, ns) })
      const newTrack = s.getAudioTracks()[0]
      const old = localStreamRef.current?.getAudioTracks()[0]
      newTrack.enabled = micOn
      if (old) {
        localStreamRef.current?.removeTrack(old)
        old.stop()
      }
      localStreamRef.current?.addTrack(newTrack)
      await callRef.current?.replaceAudioTrack(newTrack)
      if (localStreamRef.current) levelsRef.current?.watch('me', localStreamRef.current)
      setMicId(newTrack.getSettings().deviceId ?? deviceId)
    } catch {
      setStatus('Não foi possível mudar de microfone')
    }
  }

  /** Troca a câmara (mantendo desfoque se ativo). */
  async function switchCam(deviceId: string) {
    try {
      const s = await navigator.mediaDevices.getUserMedia({ video: videoConstraints(deviceId || undefined) })
      const newTrack = s.getVideoTracks()[0]
      newTrack.contentHint = 'motion'
      newTrack.enabled = camOn
      const old = cameraTrackRef.current
      cameraTrackRef.current = newTrack
      if (old) {
        localStreamRef.current?.removeTrack(old)
        old.stop()
      }
      localStreamRef.current?.addTrack(newTrack)
      setHasLocalVideo(true)
      let sendTrack = newTrack
      if (bgMode !== 'none' && effectRef.current) {
        effectRef.current.stop()
        sendTrack = await effectRef.current.start(newTrack)
      }
      if (!sharing) {
        await callRef.current?.replaceVideoTrack(sendTrack)
        if (localVideo.current) {
          localVideo.current.srcObject = bgMode !== 'none' ? new MediaStream([sendTrack]) : localStreamRef.current
        }
      }
      setCamId(newTrack.getSettings().deviceId ?? deviceId)
    } catch {
      setStatus('Não foi possível mudar de câmara')
    }
  }

  async function toggleNoiseSuppression() {
    const next = !noiseSuppression
    setNoiseSuppression(next)
    await switchMic(micId, next)
  }

  /**
   * Efeitos de fundo (IA local, nada sai do dispositivo):
   * 'none' repõe a câmara, 'blur' aplica vidro fosco, 'image' usa a imagem
   * dada — a mudança entre blur/imagem é instantânea (mesmo pipeline).
   */
  async function applyBackground(mode: 'none' | 'blur' | 'image', imageUrl?: string) {
    if (bgBusy || !cameraTrackRef.current) return
    setBgBusy(true)
    try {
      if (mode === 'none') {
        const raw = effectRef.current?.stop() ?? cameraTrackRef.current
        if (!sharing && raw) {
          await callRef.current?.replaceVideoTrack(raw)
          if (localVideo.current) localVideo.current.srcObject = localStreamRef.current
        }
        setBgMode('none')
        setBgImageUrl('')
        return
      }
      effectRef.current = effectRef.current ?? new BackgroundEffect()
      const effect = effectRef.current
      if (mode === 'image' && imageUrl) await effect.setImage(imageUrl)
      else effect.mode = 'blur'
      if (!effect.started) {
        const processed = await effect.start(cameraTrackRef.current)
        if (!sharing) {
          await callRef.current?.replaceVideoTrack(processed)
          if (localVideo.current) localVideo.current.srcObject = new MediaStream([processed])
        }
      }
      setBgMode(mode)
      setBgImageUrl(mode === 'image' ? imageUrl ?? '' : '')
    } catch (e) {
      console.warn('[background]', e)
      setStatus('Efeito de fundo indisponível neste dispositivo')
    } finally {
      setBgBusy(false)
    }
  }

  function onUploadBackground(file: File | null) {
    if (!file) return
    void applyBackground('image', URL.createObjectURL(file))
  }

  async function toggleShare() {
    if (sharing) {
      // O efeito continua a processar durante a partilha; basta repor a track.
      const back = (bgMode !== 'none' && effectRef.current?.output) || cameraTrackRef.current
      if (back) await callRef.current?.replaceVideoTrack(back)
      if (localVideo.current && localStreamRef.current) {
        localVideo.current.srcObject =
          bgMode !== 'none' && back !== cameraTrackRef.current ? new MediaStream([back!]) : localStreamRef.current
      }
      setSharing(false)
      return
    }
    try {
      const display = await navigator.mediaDevices.getDisplayMedia({ video: true })
      const screenTrack = display.getVideoTracks()[0]
      screenTrack.contentHint = 'detail' // privilegiar nitidez de texto sobre framerate
      await callRef.current?.replaceVideoTrack(screenTrack)
      if (localVideo.current) localVideo.current.srcObject = display
      screenTrack.onended = () => toggleShare()
      setSharing(true)
    } catch {
      /* user cancelled the picker */
    }
  }

  async function toggleRecording() {
    if (recBusy) return
    if (!recording) {
      levelsRef.current?.resume()
      recorderRef.current = new MeetingRecorder()
      recorderRef.current.setSources([
        { id: 'me', label: 'eu', stream: localStreamRef.current },
        ...peersRef.current.map((p) => ({ id: p.peerId, label: p.username, stream: p.stream })),
      ])
      setRecording(true)
      signalRef.current?.send({ type: 'recording', active: true })
      return
    }
    setRecBusy(true)
    try {
      const blob = await recorderRef.current!.stop()
      recorderRef.current = null
      setRecording(false)
      signalRef.current?.send({ type: 'recording', active: false })
      setStatus('A carregar gravação…')
      const now = new Date()
      const stamp = `${now.toLocaleDateString('pt-PT')} ${now.toLocaleTimeString('pt-PT', { hour: '2-digit', minute: '2-digit' })}`
      await uploadRecording(code, blob, `Reunião ${code} — ${stamp}.webm`)
      setStatus('')
      const recs = await listRecordings(code)
      setRecordings(recs)
      setPanel('people')
    } catch {
      setStatus('Falha ao guardar a gravação')
    } finally {
      setRecBusy(false)
    }
  }

  function toggleHand() {
    const raised = !handRaised
    setHandRaised(raised)
    signalRef.current?.send({ type: 'hand', raised })
  }

  function sendReaction(emoji: string) {
    signalRef.current?.send({ type: 'reaction', emoji })
    floatReaction(emoji, 'eu')
    setPickerOpen(false)
  }

  function sendChat() {
    const text = chatInput.trim()
    if (!text) return
    signalRef.current?.send({ type: 'chat', text })
    setChat((c) => [...c, { username: 'eu', text, own: true }])
    setChatInput('')
  }

  function admit(peerId: string, ok: boolean) {
    signalRef.current?.send({ type: ok ? 'admit' : 'deny', to: peerId })
    setWaitingQueue((q) => q.filter((p) => p.peer_id !== peerId))
  }

  const total = peers.length + 1
  const cols = total <= 1 ? 1 : total <= 4 ? 2 : total <= 9 ? 3 : 4
  const meSpeaking = speaking.has('me') && micOn
  const anyoneRecording = recording || !!remoteRecorder

  // Conferência: o orador ativo (a falar) vai para o palco; se ninguém fala,
  // fica o primeiro participante. A plateia é toda a gente menos o do palco.
  const stagePeer =
    peers.find((p) => speaking.has(p.peerId)) ?? peers[0] ?? null
  // Transformação 3D (parallax) aplicada à área de vídeo. O scale(1.14) é
  // overscan: mantém a área sempre maior que a moldura, por isso a rotação/
  // deslocamento nunca revela o fundo escuro nos cantos. Deslocamento em %
  // (relativo ao próprio tamanho) para acompanhar o overscan.
  const parallaxStyle: CSSProperties = parallax
    ? {
        transform: `perspective(1600px) rotateY(${tilt.x * 4}deg) rotateX(${tilt.y * -4}deg) scale(1.14) translate(${tilt.x * 1.6}%, ${tilt.y * 1.6}%)`,
        transformOrigin: 'center center',
        transition: 'transform 0.09s ease-out',
      }
    : {}

  const talkOverNames = useMemo(() => {
    if (!talkOver) return ''
    const names = [...speaking].map((id) => (id === 'me' ? 'tu' : peers.find((p) => p.peerId === id)?.username ?? '')).filter(Boolean)
    return names.join(' e ')
  }, [talkOver, speaking, peers])

  if (roomState === 'e2ee-pass') {
    return (
      <div className="waiting-page">
        <h2>🔒 Reunião encriptada de ponta a ponta</h2>
        <p className="muted" style={{ maxWidth: 460 }}>
          Introduz a frase-chave combinada entre os participantes (fora da plataforma). A chave é
          derivada localmente e <strong>nunca é enviada ao servidor</strong>.
        </p>
        <form
          className="e2ee-form"
          onSubmit={(e) => {
            e.preventDefault()
            const pass = passInput.trim()
            if (!pass) return
            sessionStorage.setItem(`dx_e2ee_${code}`, pass)
            setRoomState('connecting')
            setPassTry((n) => n + 1)
          }}
        >
          <input
            type="password"
            placeholder="Frase-chave da reunião"
            value={passInput}
            onChange={(e) => setPassInput(e.target.value)}
            autoFocus
          />
          <button className="primary">Entrar na reunião</button>
        </form>
        <p className="muted" style={{ fontSize: '0.82rem', maxWidth: 460 }}>
          Com a frase errada não vês nem ouves os outros — os frames que não autenticam são
          descartados, nunca reproduzidos.
        </p>
        <button className="link" onClick={onLeave}>
          Cancelar
        </button>
      </div>
    )
  }

  if (roomState === 'denied' || roomState === 'kicked') {
    return (
      <div className="waiting-page">
        <h2>{roomState === 'denied' ? 'O anfitrião recusou a tua entrada' : 'Foste removido da reunião'}</h2>
        <button className="primary" style={{ width: 'auto', padding: '0.7rem 2rem' }} onClick={onLeave}>
          Voltar ao início
        </button>
      </div>
    )
  }

  return (
    <div className="room-page">
      {isHost && waitingQueue.length > 0 && (
        <div className="admit-bar">
          {waitingQueue.map((p) => (
            <div key={p.peer_id} className="admit-item">
              <span>
                <strong>{p.username}</strong> quer entrar
              </span>
              <button className="admit-yes" onClick={() => admit(p.peer_id, true)}>
                Admitir
              </button>
              <button className="admit-no" onClick={() => admit(p.peer_id, false)}>
                Recusar
              </button>
            </div>
          ))}
        </div>
      )}

      {anyoneRecording && (
        <div className="rec-banner">
          <span className="rec-dot" />
          {recording ? 'Estás a gravar esta reunião' : `${remoteRecorder} está a gravar esta reunião`} — todos os
          participantes têm acesso à gravação no painel «Participantes».
        </div>
      )}

      {talkOver && (
        <div className="talkover-banner">
          🎙️ {talkOverNames ? `${talkOverNames} estão a falar ao mesmo tempo` : 'Duas pessoas estão a falar ao mesmo tempo'} —
          para uma boa comunicação, tentem dar espaço para cada um terminar.
        </div>
      )}

      <div className="room-body">
        {roomState === 'waiting' && (
          <div className="waiting-overlay">
            <div className="spinner" />
            <h2>À espera que o anfitrião te deixe entrar…</h2>
            <p className="muted">Podes preparar a câmara e o microfone entretanto.</p>
          </div>
        )}

        <div className="video-area">
        {(() => {
          const selfTile = (
            <div className={meSpeaking ? 'tile speaking' : 'tile'}>
              <video
                ref={localVideo}
                autoPlay
                muted
                playsInline
                className={hasLocalVideo && !sharing && bgMode === 'none' ? 'mirror' : undefined}
                style={{ display: (hasLocalVideo && camOn) || sharing ? undefined : 'none' }}
              />
              {!(hasLocalVideo && camOn) && !sharing && (
                <div className="tile-avatar">
                  <span className="avatar-circle">EU</span>
                </div>
              )}
              {handRaised && <span className="hand-badge">✋</span>}
              <span className="tile-name">
                {!micOn && <span className="name-mic-off"><MicOffIcon /></span>}
                {micOn && meSpeaking && <SpeakingBars />}
                eu{isHost ? ' · anfitrião' : ''}
                {sharing ? ' (a partilhar ecrã)' : ''}
              </span>
            </div>
          )
          const remoteTile = (p: RemotePeer) => (
            <RemoteTile
              key={p.peerId}
              peer={p}
              isHost={isHost}
              sinkId={speakerId}
              speaking={speaking.has(p.peerId)}
              onMute={() => signalRef.current?.send({ type: 'force-mute', to: p.peerId })}
              onKick={() => signalRef.current?.send({ type: 'kick', to: p.peerId })}
            />
          )

          if (viewMode === 'stage' && stagePeer) {
            const audience = peers.filter((p) => p.peerId !== stagePeer.peerId)
            return (
              <div className="stage-wrap" style={parallaxStyle}>
                <div className="stage-main">{remoteTile(stagePeer)}</div>
                <div className="stage-audience">
                  {selfTile}
                  {audience.map(remoteTile)}
                </div>
              </div>
            )
          }
          return (
            <div className="video-grid" style={{ gridTemplateColumns: `repeat(${cols}, 1fr)`, ...parallaxStyle }}>
              {selfTile}
              {peers.map(remoteTile)}
            </div>
          )
        })()}
        </div>

        <div className="reactions-layer">
          {reactions.map((r) => (
            <div key={r.id} className="floating-reaction">
              <span className="emoji">{r.emoji}</span>
              <span className="who">{r.username}</span>
            </div>
          ))}
        </div>

        {panel === 'chat' && (
          <aside className="side-panel">
            <div className="panel-head">
              <h3>Mensagens na chamada</h3>
              <button className="panel-close" onClick={() => setPanel('none')}><CloseIcon /></button>
            </div>
            <div className="chat-messages">
              {chat.map((m, i) => (
                <div key={i} className={m.own ? 'chat-msg own' : 'chat-msg'}>
                  <strong>{m.username}</strong> {m.text}
                </div>
              ))}
            </div>
            <div className="chat-input">
              <input
                value={chatInput}
                onChange={(e) => setChatInput(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && sendChat()}
                placeholder="Escreve uma mensagem…"
              />
              <button onClick={sendChat}>➤</button>
            </div>
          </aside>
        )}

        {panel === 'people' && (
          <aside className="side-panel">
            <div className="panel-head">
              <h3>Participantes ({total})</h3>
              <button className="panel-close" onClick={() => setPanel('none')}><CloseIcon /></button>
            </div>
            <div className="people-list">
              <div className="person-row">
                <span className="avatar-circle small">EU</span>
                <span className="person-name">eu{isHost ? ' · anfitrião' : ''}</span>
                {micOn ? (meSpeaking ? <SpeakingBars /> : <MicIcon />) : <span className="danger-ic"><MicOffIcon /></span>}
              </div>
              {peers.map((p) => (
                <div key={p.peerId} className="person-row">
                  <span className="avatar-circle small">{p.username.slice(0, 2).toUpperCase()}</span>
                  <span className="person-name">
                    {p.username}
                    {p.host ? ' · anfitrião' : ''}
                  </span>
                  {speaking.has(p.peerId) && <SpeakingBars />}
                </div>
              ))}
            </div>
            {isHost && (
              <div className="breakout-box">
                <h4>Salas de grupo</h4>
                {breakoutRooms.length === 0 ? (
                  <div className="breakout-create">
                    <span className="muted small">Dividir participantes em</span>
                    {[2, 3, 4].map((n) => (
                      <button
                        key={n}
                        onClick={() => signalRef.current?.send({ type: 'breakouts-create', count: n })}
                      >
                        {n}
                      </button>
                    ))}
                    <span className="muted small">grupos</span>
                  </div>
                ) : (
                  <>
                    {breakoutRooms.map((b) => (
                      <button
                        key={b.code}
                        className="rec-row"
                        title="Visitar este grupo"
                        onClick={() => {
                          sessionStorage.setItem(`dx_return_${b.code}`, code)
                          onSwitch?.(b.code)
                        }}
                      >
                        <PeopleIcon />
                        <span className="rec-file">
                          {b.label}
                          <small>{b.code}</small>
                        </span>
                      </button>
                    ))}
                    <button
                      className="admit-no breakout-close"
                      onClick={() => signalRef.current?.send({ type: 'breakouts-close' })}
                    >
                      Encerrar grupos e chamar todos de volta
                    </button>
                  </>
                )}
              </div>
            )}
            <div className="rec-list">
              <h4>Gravações desta sala</h4>
              {recordings.length === 0 && <p className="muted small">Ainda não há gravações.</p>}
              {recordings.map((r) => (
                <button key={r.id} className="rec-row" onClick={() => void downloadRecording(r).catch(() => setStatus('Falha ao descarregar'))}>
                  <DownloadIcon />
                  <span className="rec-file">
                    {r.filename}
                    <small>{new Date(r.created_at).toLocaleString('pt-PT')} · {(r.size_bytes / 1_048_576).toFixed(1)} MB</small>
                  </span>
                </button>
              ))}
            </div>
          </aside>
        )}

        {panel === 'settings' && (
          <aside className="side-panel">
            <div className="panel-head">
              <h3>Definições</h3>
              <button className="panel-close" onClick={() => setPanel('none')}><CloseIcon /></button>
            </div>
            <div className="settings-body">
              <label className="set-label">
                Microfone
                <select value={micId} onChange={(e) => void switchMic(e.target.value)}>
                  {devices.mics.map((d) => (
                    <option key={d.deviceId} value={d.deviceId}>{d.label || 'Microfone'}</option>
                  ))}
                </select>
              </label>
              <label className="set-label">
                Câmara
                <select value={camId} onChange={(e) => void switchCam(e.target.value)}>
                  {devices.cams.map((d) => (
                    <option key={d.deviceId} value={d.deviceId}>{d.label || 'Câmara'}</option>
                  ))}
                </select>
              </label>
              <label className="set-label">
                Altifalantes
                <select value={speakerId} onChange={(e) => setSpeakerId(e.target.value)}>
                  <option value="">Predefinido do sistema</option>
                  {devices.speakers.map((d) => (
                    <option key={d.deviceId} value={d.deviceId}>{d.label || 'Altifalante'}</option>
                  ))}
                </select>
              </label>

              <label className="set-toggle">
                <input type="checkbox" checked={noiseSuppression} onChange={() => void toggleNoiseSuppression()} />
                <span>
                  Supressão de ruído
                  <small>Remove ruídos externos (teclado, ventoinha, trânsito) para um áudio mais limpo.</small>
                </span>
              </label>
              <div className="bg-section">
                <span className="set-label">Fundo {bgBusy ? '· a aplicar…' : ''}</span>
                <small className="muted">
                  IA local segmenta a pessoa; o fundo real nunca fica visível para os outros.
                </small>
                <div className="bg-grid">
                  <button
                    className={bgMode === 'none' ? 'bg-opt selected' : 'bg-opt'}
                    disabled={bgBusy || !hasLocalVideo}
                    onClick={() => void applyBackground('none')}
                  >
                    <span className="bg-none">Ø</span>
                    <small>Nenhum</small>
                  </button>
                  <button
                    className={bgMode === 'blur' ? 'bg-opt selected' : 'bg-opt'}
                    disabled={bgBusy || !hasLocalVideo}
                    onClick={() => void applyBackground('blur')}
                  >
                    <span className="bg-blur-preview"><BlurIcon /></span>
                    <small>Vidro</small>
                  </button>
                  {presets.map((p) => (
                    <button
                      key={p.name}
                      className={bgMode === 'image' && bgImageUrl === p.url ? 'bg-opt selected' : 'bg-opt'}
                      disabled={bgBusy || !hasLocalVideo}
                      onClick={() => void applyBackground('image', p.url)}
                    >
                      <img src={p.url} alt={p.name} />
                      <small>{p.name}</small>
                    </button>
                  ))}
                  <button className="bg-opt" disabled={bgBusy || !hasLocalVideo} onClick={() => uploadRef.current?.click()}>
                    <span className="bg-none">＋</span>
                    <small>Imagem…</small>
                  </button>
                </div>
                <input
                  ref={uploadRef}
                  type="file"
                  accept="image/*"
                  style={{ display: 'none' }}
                  onChange={(e) => onUploadBackground(e.target.files?.[0] ?? null)}
                />
              </div>
            </div>
          </aside>
        )}

        {notesOpen && (
          <aside className="side-panel notes-panel">
            <div className="panel-head">
              <h3>Notas AI {transcribing && <span className="rec-dot" />}</h3>
              <button className="panel-close" onClick={() => setNotesOpen(false)}><CloseIcon /></button>
            </div>
            <p className="muted small">
              Transcrição partilhada: cada participante que ative transcreve o seu microfone (Chrome/Edge) e as
              frases aparecem aqui legendadas por orador. Ao terminar, gera a ata (MoM) e guarda-a na reunião.
            </p>
            <div className="notes-body">
              {lines.length === 0 && !interim && <p className="muted small">Ativa a transcrição para começar…</p>}
              {lines.map((l, i) => (
                <div key={i} className="note-line">{l}</div>
              ))}
              {interim && <div className="note-line interim">{interim}…</div>}
            </div>
            <div className="notes-actions">
              <button className={transcribing ? 'btn-sm danger' : 'btn-sm'} onClick={toggleTranscription}>
                {transcribing ? 'Parar transcrição' : 'Iniciar transcrição'}
              </button>
              <button className="btn-sm ghost" disabled={lines.length === 0} onClick={() => void saveMinutes()}>
                {momSaved ? '✓ Guardada' : 'Guardar ata (MoM)'}
              </button>
            </div>
          </aside>
        )}
      </div>

      <footer className="controls-bar">
        <div className="bar-left">
          <span className="clock">{clock}</span>
          <span className="sep">|</span>
          <span className="room-code">{code}</span>
          {topology && <span className="room-topo">{topology === 'sfu' ? 'SFU' : 'P2P'}</span>}
          {e2eeOn && (
            <span className="room-topo e2ee-badge" title="Media encriptado de ponta a ponta — o servidor não consegue ver/ouvir">
              🔒 E2EE
            </span>
          )}
          {returnTo && (
            <button
              className="return-main"
              title="Regressar à sala principal"
              onClick={() => {
                sessionStorage.removeItem(`dx_return_${code}`)
                onSwitch?.(returnTo)
              }}
            >
              ← Sala principal
            </button>
          )}
          {status && <span className="room-status">{status}</span>}
        </div>

        <div className="bar-center">
          <DeviceControl
            label={micOn ? 'Desativar microfone' : 'Ativar microfone'}
            off={!micOn}
            pulse={meSpeaking}
            onToggle={toggleMic}
            open={deviceMenu === 'mic'}
            onChevron={() => setDeviceMenu(deviceMenu === 'mic' ? 'none' : 'mic')}
            devices={devices.mics}
            currentId={micId}
            onPick={(id) => { setDeviceMenu('none'); void switchMic(id) }}
            emptyLabel="Microfone"
          >
            {micOn ? <MicIcon /> : <MicOffIcon />}
          </DeviceControl>
          <DeviceControl
            label={camOn ? 'Desativar câmara' : 'Ativar câmara'}
            off={!camOn}
            onToggle={toggleCam}
            open={deviceMenu === 'cam'}
            onChevron={() => setDeviceMenu(deviceMenu === 'cam' ? 'none' : 'cam')}
            devices={devices.cams}
            currentId={camId}
            onPick={(id) => { setDeviceMenu('none'); void switchCam(id) }}
            emptyLabel="Câmara"
          >
            {camOn ? <CamIcon /> : <CamOffIcon />}
          </DeviceControl>
          <Ctrl label="Partilhar ecrã" active={sharing} onClick={() => void toggleShare()}>
            <ShareIcon />
          </Ctrl>
          <Ctrl
            label={bgMode === 'none' ? 'Ativar efeito de fundo' : 'Remover efeito de fundo'}
            active={bgMode !== 'none'}
            onClick={() => void applyBackground(bgMode === 'none' ? 'blur' : 'none')}
          >
            <BlurIcon />
          </Ctrl>
          <Ctrl label={handRaised ? 'Baixar a mão' : 'Levantar a mão'} active={handRaised} onClick={toggleHand}>
            <HandIcon />
          </Ctrl>
          <div className="picker-wrap">
            {pickerOpen && (
              <div className="reaction-picker">
                {REACTION_EMOJIS.map((e) => (
                  <button key={e} onClick={() => sendReaction(e)}>
                    {e}
                  </button>
                ))}
              </div>
            )}
            <Ctrl label="Reações" active={pickerOpen} onClick={() => setPickerOpen(!pickerOpen)}>
              <EmojiIcon />
            </Ctrl>
          </div>
          <Ctrl
            label={recording ? 'Parar gravação' : 'Gravar reunião'}
            active={recording}
            danger={recording}
            onClick={() => void toggleRecording()}
          >
            {recording ? <StopIcon /> : <RecordIcon />}
          </Ctrl>
          <button className="ctrl hangup" onClick={onLeave} title="Sair da chamada">
            <HangupIcon />
          </button>
        </div>

        <div className="bar-right">
          <Ctrl
            plain
            label={viewMode === 'grid' ? 'Modo conferência (palco)' : 'Modo grelha'}
            active={viewMode === 'stage'}
            onClick={() => setViewMode(viewMode === 'grid' ? 'stage' : 'grid')}
          >
            <StageIcon />
          </Ctrl>
          <Ctrl plain label={parallax ? 'Desligar efeito 3D' : 'Efeito de sala 3D'} active={parallax} onClick={() => void toggleParallax()}>
            <CubeIcon />
          </Ctrl>
          <Ctrl plain label="Notas AI / Ata (MoM)" active={notesOpen} onClick={() => setNotesOpen(!notesOpen)}>
            <NoteIcon />
            {transcribing && <span className="badge live">●</span>}
          </Ctrl>
          <Ctrl plain label="Participantes e gravações" active={panel === 'people'} onClick={() => setPanel(panel === 'people' ? 'none' : 'people')}>
            <PeopleIcon />
            <span className="badge">{total}</span>
          </Ctrl>
          <Ctrl plain label="Chat" active={panel === 'chat'} onClick={() => setPanel(panel === 'chat' ? 'none' : 'chat')}>
            <ChatIcon />
          </Ctrl>
          <Ctrl plain label="Definições" active={panel === 'settings'} onClick={() => setPanel(panel === 'settings' ? 'none' : 'settings')}>
            <SettingsIcon />
          </Ctrl>
        </div>
      </footer>
    </div>
  )
}

function Ctrl({
  children,
  label,
  onClick,
  off,
  active,
  danger,
  pulse,
  plain,
}: {
  children: ReactNode
  label: string
  onClick: () => void
  off?: boolean
  active?: boolean
  danger?: boolean
  pulse?: boolean
  plain?: boolean
}) {
  const cls = [
    'ctrl',
    plain ? 'plain' : '',
    off ? 'off' : '',
    active ? 'active' : '',
    danger ? 'danger' : '',
    pulse ? 'pulse' : '',
  ]
    .filter(Boolean)
    .join(' ')
  return (
    <button className={cls} onClick={onClick} title={label} aria-label={label}>
      {children}
    </button>
  )
}

/** Botão de mic/câmara com chevron para escolher dispositivo (estilo Meet). */
function DeviceControl({
  children,
  label,
  off,
  pulse,
  onToggle,
  open,
  onChevron,
  devices,
  currentId,
  onPick,
  emptyLabel,
}: {
  children: ReactNode
  label: string
  off?: boolean
  pulse?: boolean
  onToggle: () => void
  open: boolean
  onChevron: () => void
  devices: MediaDeviceInfo[]
  currentId: string
  onPick: (id: string) => void
  emptyLabel: string
}) {
  return (
    <div className="device-ctrl">
      <button
        className={['ctrl', off ? 'off' : '', pulse ? 'pulse' : ''].filter(Boolean).join(' ')}
        onClick={onToggle}
        title={label}
        aria-label={label}
      >
        {children}
      </button>
      <button className={open ? 'chevron open' : 'chevron'} onClick={onChevron} aria-label="Escolher dispositivo">
        <ChevronUpIcon />
      </button>
      {open && (
        <div className="device-menu">
          {devices.length === 0 && <div className="device-empty">Sem dispositivos</div>}
          {devices.map((d) => (
            <button
              key={d.deviceId}
              className={d.deviceId === currentId ? 'device-item active' : 'device-item'}
              onClick={() => onPick(d.deviceId)}
            >
              {d.deviceId === currentId ? '● ' : '○ '}
              {d.label || emptyLabel}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

/** Barras animadas tipo Meet quando alguém está a falar. */
function SpeakingBars() {
  return (
    <span className="speaking-bars" aria-hidden>
      <i /><i /><i />
    </span>
  )
}

function RemoteTile({
  peer,
  isHost,
  sinkId,
  speaking,
  onMute,
  onKick,
}: {
  peer: RemotePeer
  isHost: boolean
  sinkId: string
  speaking: boolean
  onMute: () => void
  onKick: () => void
}) {
  const ref = useRef<HTMLVideoElement>(null)
  useEffect(() => {
    if (ref.current) ref.current.srcObject = peer.stream
  }, [peer.stream])
  // Saída de áudio (altifalantes) escolhida nas definições.
  useEffect(() => {
    const el = ref.current as (HTMLVideoElement & { setSinkId?: (id: string) => Promise<void> }) | null
    if (el?.setSinkId) void el.setSinkId(sinkId || '').catch(() => {})
  }, [sinkId, peer.stream])
  const hasVideo = !!peer.stream?.getVideoTracks().length
  const hasAudio = !!peer.stream?.getAudioTracks().length
  return (
    <div className={speaking ? 'tile speaking' : 'tile'}>
      <video ref={ref} autoPlay playsInline style={{ display: hasVideo ? undefined : 'none' }} />
      {!hasVideo && (
        <div className="tile-avatar">
          <span className="avatar-circle">{peer.username.slice(0, 2).toUpperCase()}</span>
        </div>
      )}
      {peer.hand && <span className="hand-badge">✋</span>}
      <span className="tile-name">
        {!hasAudio && <span className="name-mic-off"><MicOffIcon /></span>}
        {hasAudio && speaking && <SpeakingBars />}
        {peer.username}
        {peer.host ? ' · anfitrião' : ''}
      </span>
      {isHost && !peer.host && (
        <div className="host-actions">
          <button title="Silenciar" onClick={onMute}>
            <MicOffIcon />
          </button>
          <button title="Remover da reunião" onClick={onKick}>
            <CloseIcon />
          </button>
        </div>
      )}
    </div>
  )
}
