import { CSSProperties, ReactNode, RefObject, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  currentUser, downloadRecording, iceServers, inviteToRoom, joinRoom, listRecordings, postQos, postTimings, Recording,
  roomChatHistory, saveMinutesByRoom, saveWhiteboard, searchUsers, translateCaption, uploadRecording, User,
  isAbort,
} from '../api'
import {
  audioConstraints,
  BackgroundEffect,
  Denoiser,
  DeviceSets,
  HeadTracker,
  LevelWatcher,
  listDevices,
  MeetingRecorder,
  playTestTone,
  presetBackgrounds,
  Transcriber,
  videoConstraints,
} from '../media'
import { deriveRoomKey, e2eeSupported, FrameCrypto } from '../e2ee'
import { Btn, SelectCtl } from '../components/ui'
import { BreakoutRoom, PeerInfo, PollView, QaView, Signaling, WbStroke } from '../signaling'
import ThemePicker from '../components/ThemePicker'
import { Countdown, MeetingElapsed, WallClock } from '../room/Clocks'
import { peerColor, RemotePeer, RemoteTile, SpeakingBars } from '../room/RemoteTile'
import { Call, MeshCall, SCREEN_CONSTRAINTS, SfuCall } from '../webrtc'
import type { CallState } from '../callRecovery'
import { backoffDelay } from '../callRecovery'
import { chooseLayers, type LocalConditions, type TileSignal } from '../layerPolicy'
import { LinhaDoTempo } from '../callTimings'
import { makeCallHolderStart } from '../sfuLifecycle'
import { BlurIcon, CamIcon, CamOffIcon, ChartIcon, ChatIcon, CheckIcon, ChevronLeftIcon, ChevronUpIcon, ClockIcon, CloseIcon, CubeIcon, DownloadIcon, EditIcon, EmojiIcon, FullscreenIcon, GridIcon, HandIcon, HangupIcon, HelpIcon, InfoIcon, LockIcon, MicIcon, MicOffIcon, NoteIcon, PeopleIcon, PinIcon, PlusIcon, RecordIcon, RepeatIcon, RowsIcon, SaveIcon, SearchIcon, SendIcon, SettingsIcon, ShareIcon, ShieldIcon, SpeakerIcon, StageIcon, StopIcon, StrokeThickIcon, StrokeThinIcon, TableIcon, TrashIcon, TrophyIcon } from '../icons'

// `RemotePeer` mudou-se para room/RemoteTile.tsx, com o componente que o usa.

/**
 * Cor de fundo determinística por participante: djb2 hash → HSL escuro único.
 * Cada pessoa tem sempre o mesmo tom independentemente da sessão.
 */
/** Renderiza markdown básico em linha: **bold**, *italic*, `code`, @menção. */
function ChatText({ text }: { text: string }) {
  // Divide por tokens de marcação e @mentions.
  const parts = text.split(/(\*\*[^*]+\*\*|\*[^*]+\*|`[^`]+`|@\w+)/g)
  return (
    <>
      {parts.map((part, i) => {
        if (part.startsWith('**') && part.endsWith('**'))
          return <strong key={i}>{part.slice(2, -2)}</strong>
        if (part.startsWith('*') && part.endsWith('*'))
          return <em key={i}>{part.slice(1, -1)}</em>
        if (part.startsWith('`') && part.endsWith('`'))
          return <code key={i} className="chat-code">{part.slice(1, -1)}</code>
        if (part.startsWith('@'))
          return <span key={i} className="chat-mention">{part}</span>
        return <span key={i}>{part}</span>
      })}
    </>
  )
}


/**
 * Grelha estilo Meet: para N mosaicos 16:9 num contentor W×H, escolhe o nº de
 * colunas que maximiza o tamanho de cada mosaico. Linhas incompletas ficam
 * centradas (flex-wrap), sem buracos nem mosaicos esticados.
 */
function useGridLayout(areaRef: RefObject<HTMLDivElement | null>, count: number, active: boolean) {
  const [size, setSize] = useState({ w: 480, h: 270 })
  useEffect(() => {
    const el = areaRef.current
    if (!el || !active) return
    const GAP = 12
    const PAD = 14
    const compute = () => {
      const w = el.clientWidth - PAD * 2
      const h = el.clientHeight - PAD * 2
      if (w <= 0 || h <= 0 || count === 0) return

      // 1 pessoa: preenche TODA a área disponível (estilo Google Meet).
      // O vídeo usa object-fit: cover para manter o ratio da câmara.
      if (count === 1) {
        const nw = Math.floor(w), nh = Math.floor(h)
        setSize((s) => (s.w === nw && s.h === nh ? s : { w: nw, h: nh }))
        return
      }

      const RATIO = 16 / 9
      let best = { w: 480, h: 270, scale: 0 }
      for (let cols = 1; cols <= count; cols++) {
        const rows = Math.ceil(count / cols)
        const cellW = (w - GAP * (cols - 1)) / cols
        const cellH = (h - GAP * (rows - 1)) / rows
        const scale = Math.min(cellW / RATIO, cellH)
        if (scale > best.scale) {
          best = { w: Math.floor(RATIO * scale), h: Math.floor(scale), scale }
        }
      }
      setSize((s) => (s.w === best.w && s.h === best.h ? s : { w: best.w, h: best.h }))
    }
    compute()
    // Fallback: re-calcula depois do layout estabilizar (resolve race conditions
    // onde clientWidth ainda não reflete o tamanho flex final).
    const raf = requestAnimationFrame(compute)
    const ro = new ResizeObserver(compute)
    ro.observe(el)
    return () => { cancelAnimationFrame(raf); ro.disconnect() }
  }, [areaRef, count, active])
  return size
}


interface ChatMsg {
  username: string
  text: string
  own: boolean
  historical?: boolean
}

interface FloatingReaction {
  id: number
  emoji: string
  username: string
}

const REACTION_EMOJIS = ['👍', '❤️', '😂', '🎉', '👏', '😮']

// Emojis para inserir directamente numa mensagem de chat (inspirado no Teams).
const CHAT_EMOJIS = [
  '😀','😂','😍','🥰','😎','🤔','🙏','👍','👎','❤️',
  '🔥','🎉','✅','⚠️','📌','💡','🚀','💪','👏','😭',
  '😅','🤗','😤','💼','📊','📈','⏰','🔔','✍️','🫂',
]
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

type RoomState = 'prejoin' | 'connecting' | 'waiting' | 'denied' | 'kicked' | 'in' | 'e2ee-pass'
type Panel = 'none' | 'chat' | 'people' | 'settings' | 'tools'

// Tira de abas do painel lateral unificado (Pessoas · Chat · Ferramentas),
// estilo Teams/Zoom — troca de painel sem fechar. Pura UI sobre o estado `panel`.
function PanelTabs({
  active,
  onSelect,
  unreadChat,
  total,
}: {
  active: Panel
  onSelect: (p: Panel) => void
  unreadChat: number
  total: number
}) {
  const tabs: { key: Panel; label: string; badge?: number }[] = [
    { key: 'people', label: 'Pessoas', badge: total },
    { key: 'chat', label: 'Chat', badge: unreadChat || undefined },
    { key: 'tools', label: 'Ferramentas' },
  ]
  return (
    <div className="panel-tabs" role="tablist">
      {tabs.map((tb) => (
        <button
          key={tb.key}
          role="tab"
          aria-selected={active === tb.key}
          className={active === tb.key ? 'panel-tab active' : 'panel-tab'}
          onClick={() => onSelect(tb.key)}
        >
          {tb.label}
          {tb.badge ? <span className="panel-tab-badge">{tb.badge}</span> : null}
        </button>
      ))}
    </div>
  )
}

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
  // Pre-join (green room): por omissão entra-se pelo ecrã de preparação.
  // SALTA-SE quando: (a) chamada de voz atendida (o outro lado está à espera do
  // ring) ou (b) rejoin recente (< 60s) — o auto-reconnect por reload (onclose)
  // e as trocas de breakout não devem voltar a mostrar o prejoin.
  const [joinIntent, setJoinIntent] = useState<boolean>(() => {
    const recent = Date.now() - Number(sessionStorage.getItem(`dx_rejoin_${code}`) || 0) < 60_000
    return Boolean(voiceOnly) || recent
  })
  const joinIntentRef = useRef(joinIntent)
  const [roomState, setRoomState] = useState<RoomState>(joinIntent ? 'connecting' : 'prejoin')
  // Stream do preview do prejoin — entregue à chamada no join (handoff), para
  // não readquirir a câmara (evita flicker e "câmara ocupada" em alguns SOs).
  const previewStreamRef = useRef<MediaStream | null>(null)
  const prejoinPermErrRef = useRef('')
  const prejoinVideoRef = useRef<HTMLVideoElement | null>(null)
  const [peers, setPeers] = useState<RemotePeer[]>([])
  const [waitingQueue, setWaitingQueue] = useState<PeerInfo[]>([])
  // Pedidos in-room com diálogo próprio (nada de confirm()/alert() nativos):
  const [ctrlAsk, setCtrlAsk] = useState<{ from: string; username: string } | null>(null)
  const [shareAsk, setShareAsk] = useState<{ from: string; username: string } | null>(null)
  // Partilha pedida por não-anfitrião: arranca sozinha quando o grant chegar.
  const pendingShareRef = useRef(false)
  // Handlers WS registam-se uma vez — este ref aponta sempre ao toggleShare
  // do render atual (evita closure obsoleta de `sharing`/`topology`).
  const toggleShareRef = useRef<() => void>(() => {})
  toggleShareRef.current = () => void toggleShare()
  const [reactions, setReactions] = useState<FloatingReaction[]>([])
  const [chat, setChat] = useState<ChatMsg[]>([])
  const [chatInput, setChatInput] = useState('')
  const [panel, setPanel] = useState<Panel>('none')
  const [pickerOpen, setPickerOpen] = useState(false)
  const [deviceMenu, setDeviceMenu] = useState<'none' | 'mic' | 'cam'>('none')
  const [micOn, setMicOn] = useState(true)
  // Regras de sala impostas pelo anfitrião (R92). Vêm SEMPRE do servidor —
  // nunca se decidem aqui, porque esconder um botão não impede ninguém de
  // enviar a mensagem pelo socket.
  const [chatOn, setChatOn] = useState(true)
  /// O nosso `peer_id` nesta sala. É preciso para saber se uma troca de
  /// anfitrião nos diz respeito — sem ele, `host-changed` não se consegue
  /// interpretar do lado de quem recebe.
  const meuPeerIdRef = useRef<string>('')
  const [allowUnmute, setAllowUnmute] = useState(true)
  const [camOn, setCamOn] = useState(true)
  const [sharing, setSharing] = useState(false)
  const [handRaised, setHandRaised] = useState(false)
  const [hasLocalVideo, setHasLocalVideo] = useState(true)
  const [isHost, setIsHost] = useState(false)
  // Pode admitir convidados em espera: anfitrião OU participante promovido.
  const [canAdmit, setCanAdmit] = useState(false)
  const [status, setStatus] = useState('A ligar…')
  /** Estado da ligação de media — alimenta o indicador de recuperação. */
  const [callState, setCallState] = useState<CallState>('connecting')
  /** Os tempos de estabelecimento só se reportam UMA vez por sessão. */
  const temposEnviados = useRef(false)
  /** Condições locais que alimentam a política de camada (ver layerPolicy.ts). */
  const [conditions, setConditions] = useState<LocalConditions>({})
  const [topology, setTopology] = useState('')
  const [isTraining, setIsTraining] = useState(false)
  const [waitingRoomOn, setWaitingRoomOn] = useState(false)

  // Dispositivos & qualidade
  const [devices, setDevices] = useState<DeviceSets>({ mics: [], cams: [], speakers: [] })
  const [micId, setMicId] = useState('')
  const [camId, setCamId] = useState('')
  const [speakerId, setSpeakerId] = useState('')
  // Supressão de ruído por IA (RNNoise) — LIGADA por defeito (remove teclado/
  // ventoinha muito melhor que a nativa). Fallback seguro: se o RNNoise falhar
  // (wasm indisponível, AudioContext), `denoiseMic` devolve a track CRUA, que
  // mantém a supressão nativa do browser. Desliga-se nas Definições.
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
  const [recNotice, setRecNotice] = useState('') // toast transitório de início de gravação
  const [isInstant, setIsInstant] = useState(false) // chamada instantânea (sem agenda)
  const [recordings, setRecordings] = useState<Recording[]>([])
  const [recBusy, setRecBusy] = useState(false)

  // E2EE
  const [passInput, setPassInput] = useState('')
  const [passTry, setPassTry] = useState(0)
  const [e2eeOn, setE2eeOn] = useState(false)

  // Breakout rooms
  const [breakoutRooms, setBreakoutRooms] = useState<BreakoutRoom[]>([])
  const [breakoutEndsAt, setBreakoutEndsAt] = useState<number | null>(() => {
    const v = sessionStorage.getItem(`dx_bo_ends_${code}`)
    return v ? Number(v) : null
  })
  const [breakoutMinutes, setBreakoutMinutes] = useState(0)
  const returnTo = sessionStorage.getItem(`dx_return_${code}`)

  // UX estilo Meet: cartão "reunião pronta" + painel Fundos e efeitos
  const [readyOpen, setReadyOpen] = useState(
    () => !sessionStorage.getItem(`dx_ready_${code}`),
  )
  // O cartão fecha-se SOZINHO (R87). Antes só saía por clique explícito, e por
  // isso tapava o canto inferior esquerdo do vídeo durante a reunião inteira —
  // aparecia em todas as capturas, com painéis abertos e tudo. No telemóvel
  // ocupava metade do ecrã.
  //
  // Três saídas, e cada uma é um momento em que o cartão deixou de ter razão:
  //   · alguém entrou   — já não é preciso convidar ninguém;
  //   · abriu-se um painel — a pessoa está a fazer outra coisa;
  //   · passaram 20 s   — teve tempo de copiar o link.
  // O `dx_ready_` continua a impedir que volte nesta sessão, tal como antes.
  const dispensarReady = useCallback(() => {
    sessionStorage.setItem(`dx_ready_${code}`, '1')
    setReadyOpen(false)
  }, [code])

  // 20 s a partir do momento em que o cartão aparece. O temporizador é limpo
  // se ele fechar antes por outra razão — senão ficava a escrever no
  // sessionStorage de uma sala que já não está aberta.
  useEffect(() => {
    if (!readyOpen) return
    const t = setTimeout(dispensarReady, 20_000)
    return () => clearTimeout(t)
  }, [readyOpen, dispensarReady])

  // Alguém entrou, ou abriu-se um painel. Nos dois casos o cartão perdeu a
  // razão de ser: já não é preciso convidar, ou a pessoa está noutra coisa.
  useEffect(() => {
    if (!readyOpen) return
    if (peers.length > 0 || panel !== 'none') dispensarReady()
  }, [readyOpen, peers.length, panel, dispensarReady])
  const [fxOpen, setFxOpen] = useState(false)
  const fxPreview = useRef<HTMLVideoElement>(null)
  const [blurLevel, setBlurLevel] = useState<'light' | 'strong'>('strong')
  // Menu "⋮ Mais opções" + preferências de vista (estilo Meet/Zoom)
  const [moreOpen, setMoreOpen] = useState(false)
  const [hideSelf, setHideSelf] = useState(false)
  const [hideNoVideo, setHideNoVideo] = useState(false)
  const [gridPage, setGridPage] = useState(0)
  /** Esta sala já chegou a paginar? (ver o efeito de `video-interest`) */
  const paginatedOnceRef = useRef(false)
  const [fullscreen, setFullscreen] = useState(false)

  useEffect(() => {
    const onFs = () => setFullscreen(!!document.fullscreenElement)
    document.addEventListener('fullscreenchange', onFs)
    return () => document.removeEventListener('fullscreenchange', onFs)
  }, [])

  // Ferramentas de reunião: sondagens, Q&A, temporizador
  const [polls, setPolls] = useState<PollView[]>([])
  const [questions, setQuestions] = useState<QaView[]>([])
  const [meetTimerEndsAt, setMeetTimerEndsAt] = useState<number | null>(null)
  const [serverRec, setServerRec] = useState<{ by: string } | null>(null)
  const [qos, setQos] = useState<import('../webrtc').QosReport | null>(null)
  const [wbOpen, setWbOpen] = useState(false)
  const [wbStrokes, setWbStrokes] = useState<WbStroke[]>([])
  const [secCode, setSecCode] = useState('')
  const [secOpen, setSecOpen] = useState(false)
  const [sttLang, setSttLang] = useState(() => localStorage.getItem('dx_stt_lang') ?? 'pt-PT')
  const [peopleSearch, setPeopleSearch] = useState('')
  // Convidar membros da org para a sala em curso.
  const [inviteOpen, setInviteOpen] = useState(false)
  const [inviteQuery, setInviteQuery] = useState('')
  const [inviteResults, setInviteResults] = useState<User[]>([])
  const [inviteSelected, setInviteSelected] = useState<User[]>([])
  const [inviteBusy, setInviteBusy] = useState(false)
  const [inviteStatus, setInviteStatus] = useState('')
  // Teams: contador de mensagens não lidas no chat quando o painel está fechado.
  const [unreadChat, setUnreadChat] = useState(0)
  // Teams: picker de emoji para inserir na mensagem (diferente das reações flutuantes).
  const [chatEmojiOpen, setChatEmojiOpen] = useState(false)
  // @mentions: query ativa (null = inativo), lista de sugestões filtradas.
  const [mentionQuery, setMentionQuery] = useState<string | null>(null)
  const chatInputRef = useRef<HTMLInputElement>(null)
  // Teams: temporizador de duração da reunião ("00:37" estilo).
  const joinedAtRef = useRef<number>(0)
  // Ref do panel atual para acessar em closures estáticas (signal.on).
  const panelRef = useRef<Panel>('none')
  const e2eeKeyRef = useRef<string | null>(null)
  /** Apresentação (partilha de ecrã) em curso: minha ou de outro peer. */
  const [presentation, setPresentation] = useState<{ peerId: string; stream: MediaStream } | null>(null)
  const [myVotes, setMyVotes] = useState<Record<string, number>>(() => {
    // Recupera os votos desta sala (sobrevive a reloads a meio do quiz).
    try {
      return JSON.parse(sessionStorage.getItem(`dx_votes_${code}`) ?? '{}')
    } catch {
      return {}
    }
  })
  const [myUpvotes, setMyUpvotes] = useState<Record<string, boolean>>({})
  const [pollQ, setPollQ] = useState('')
  const [pollOpts, setPollOpts] = useState<string[]>(['', ''])
  // Quiz: resposta certa (índice em pollOpts) + duração da votação (0 = sem tempo).
  const [pollCorrect, setPollCorrect] = useState<number | null>(null)
  const [pollDur, setPollDur] = useState(0)
  // Popup de sondagem visível a TODOS: dispensados + janela de revelação pós-fecho.
  const [pollDismissed, setPollDismissed] = useState<Record<string, boolean>>({})
  const [revealUntil, setRevealUntil] = useState<Record<string, number>>({})
  const [winnerFx, setWinnerFx] = useState(false)
  const pollPrevOpenRef = useRef<Record<string, boolean>>({})
  const pollCloseSentRef = useRef<Record<string, boolean>>({})
  const [qaInput, setQaInput] = useState('')

  // Relógio de 1s para as contagens de quiz (só com sondagem aberta com prazo
  // ou janela de revelação ativa).

  // Persiste os meus votos por sala: um reload a meio do quiz não pode
  // apagar a memória de em quem votei (senão a festa nunca dispara).
  useEffect(() => {
    try {
      sessionStorage.setItem(`dx_votes_${code}`, JSON.stringify(myVotes))
    } catch { /* storage cheio — segue sem persistir */ }
  }, [myVotes, code])

  // Fecho do quiz: revelação no popup (mesmo que tenha sido dispensado — o
  // resultado merece reaparecer) e festa para quem acertou. Dispara tanto na
  // transição aberta→fechada como quando a sondagem já chega fechada (reload),
  // com guarda em sessionStorage para nunca celebrar duas vezes.
  useEffect(() => {
    for (const p of polls) {
      const was = pollPrevOpenRef.current[p.id]
      const closedNow = was && !p.open
      const arrivedClosed = was === undefined && !p.open
      if (closedNow) {
        setRevealUntil((m) => ({ ...m, [p.id]: Date.now() + 10_000 }))
        setPollDismissed((m) => (m[p.id] ? { ...m, [p.id]: false } : m))
      }
      if ((closedNow || arrivedClosed) && p.correct != null && myVotes[p.id] === p.correct) {
        const guard = `dx_fx_${p.id}`
        if (!sessionStorage.getItem(guard)) {
          sessionStorage.setItem(guard, '1')
          setWinnerFx(true)
          window.setTimeout(() => setWinnerFx(false), 4500)
        }
      }
      pollPrevOpenRef.current[p.id] = p.open
    }
  }, [polls, myVotes])

  // O anfitrião fecha o quiz quando o tempo acaba — o servidor valida (host),
  // revela a resposta certa a todos e persiste o resultado no meeting.
  // Callbacks ESTÁVEIS para os mosaicos (achado 2.3). Antes era uma closure
  // nova por peer e por render — o que anula qualquer `memo` a jusante. Agora
  // há uma identidade só, e o mosaico devolve o `peerId` que recebeu.
  const onTilePin = useCallback((peerId: string) => togglePin(peerId), [])
  const onTileMute = useCallback(
    (peerId: string) => signalRef.current?.send({ type: 'force-mute', to: peerId }),
    [],
  )
  const onTileKick = useCallback(
    (peerId: string) => signalRef.current?.send({ type: 'kick', to: peerId }),
    [],
  )
  // Controlos de anfitrião acrescentados em R92. Todos passam pelo servidor,
  // que revalida — o botão é a conveniência, não a autorização.
  const onTileCam = useCallback(
    (peerId: string) => signalRef.current?.send({ type: 'force-cam', to: peerId }),
    [],
  )
  const onTransferHost = useCallback((peerId: string) => {
    // Passar o bastão é irreversível pelo próprio: só o novo anfitrião o pode
    // devolver. Por isso confirma-se, ao contrário de silenciar.
    if (!window.confirm('Passar o papel de anfitrião a esta pessoa? Só ela to poderá devolver.')) return
    signalRef.current?.send({ type: 'transfer-host', to: peerId })
  }, [])
  const onMuteAll = useCallback((allowUnmute: boolean) => {
    signalRef.current?.send({ type: 'mute-all', allow_unmute: allowUnmute })
  }, [])
  const onChatToggle = useCallback((on: boolean) => {
    signalRef.current?.send({ type: 'chat-toggle', on })
  }, [])

  // Tique de revelação: existe SÓ enquanto houver uma janela aberta. Fora dela
  // não há relógio na raiz — contra o `setPollNow` permanente de antes, que
  // reconciliava a sala 86 400 vezes por dia para um contador de segundos.
  const revelacaoAberta = Object.values(revealUntil).some((t) => t > Date.now())
  const [revTick, setRevTick] = useState(0)
  useEffect(() => {
    if (!revelacaoAberta) return
    const t = setInterval(() => setRevTick((n) => n + 1), 1000)
    return () => clearInterval(t)
  }, [revelacaoAberta])
  void revTick

  // Não usa estado: o fecho automático precisa do TIQUE, não de um render. Era
  // um `setPollNow` por segundo a reconciliar a sala inteira para actualizar um
  // contador (achado 2.1). O intervalo lê o relógio do sistema e dispara.
  const pollsRef = useRef(polls)
  pollsRef.current = polls
  useEffect(() => {
    if (!isHost) return
    const t = setInterval(() => {
      const agora = Math.floor(Date.now() / 1000)
      for (const p of pollsRef.current) {
        if (p.open && p.ends_at && agora >= p.ends_at && !pollCloseSentRef.current[p.id]) {
          pollCloseSentRef.current[p.id] = true
          signalRef.current?.send({ type: 'poll-close', poll: p.id })
        }
      }
    }, 1000)
    return () => clearInterval(t)
  }, [isHost])

  // Controlos de anfitrião (runtime) + legendas CC
  const [roomLocked, setRoomLocked] = useState(false)
  const [hostShareOnly, setHostShareOnly] = useState(false)
  // Partilha: se o anfitrião me concedeu permissão (quando "só anfitrião" está on).
  const [shareAllowed, setShareAllowed] = useState(false)
  // Anfitrião: peers a quem concedeu a permissão de partilhar.
  const [sharePerms, setSharePerms] = useState<Set<string>>(new Set())
  const [ccOn, setCcOn] = useState(false)
  const [caption, setCaption] = useState<{ text: string; at: number } | null>(null)
  // Transcrição no servidor (Whisper) em vez do motor do browser (opt-in).
  const [serverAsr, setServerAsr] = useState(() => localStorage.getItem('dx_asr_server') === '1')
  // Tradução das legendas via LLM local ('' = original, sem tradução).
  const [ccLang, setCcLang] = useState(() => localStorage.getItem('dx_cc_lang') ?? '')
  const ccLangRef = useRef(ccLang)
  const ccSeqRef = useRef(0)
  // Throttle do envio de legendas parciais (interim) aos outros.
  const interimSentAtRef = useRef(0)
  // Debounce/single-flight da tradução de legendas parciais (não sobrecarregar
  // o LLM local com um pedido por cada palavra).
  const interimXlateRef = useRef<{ timer: number | null; busy: boolean }>({ timer: null, busy: false })

  // Relógio dos countdowns (grupos e temporizador) — só corre quando há deadline.
  // Os countdowns passaram para <Countdown>: o tique fica na folha que
  // mostra o número, não na raiz da sala (achado 2.1).

  // Pré-visualização do painel Fundos e efeitos = o mesmo stream que os
  // outros veem (com efeito aplicado, se houver).
  useEffect(() => {
    if (fxOpen && fxPreview.current && localVideo.current) {
      fxPreview.current.srcObject = localVideo.current.srcObject
    }
  }, [fxOpen, bgMode, bgImageUrl, hasLocalVideo])

  // Telemetria QoS por participante (roadmap): amostra a cada 2 s com o
  // painel Participantes aberto — bitrate/perda por peer + RTT/uplink.
  useEffect(() => {
    if (panel !== 'people') return
    const call = callRef.current
    if (!call?.qos) return
    let alive = true
    const tick = async () => {
      const r = await call.qos!().catch(() => null)
      if (alive && r) setQos(r)
    }
    void tick()
    const t = setInterval(tick, 2000)
    return () => { alive = false; clearInterval(t) }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [panel])

  // Reporte de QoS ao servidor (~1/30s durante a chamada): alimenta o cartão
  // "Qualidade das chamadas" do admin. Best-effort — falhas são ignoradas.
  useEffect(() => {
    if (roomState !== 'in') return
    // Amostra-se a cada 5 s e REPORTA-SE a cada sexta (≈30 s). A amostragem
    // mais frequente serve a política de camada, que precisa de reagir a uma
    // rede a degradar em segundos e não em meio minuto; o reporte mantém a
    // mesma cadência de antes, para não multiplicar por seis a escrita na base
    // de dados. Uma só recolha de `getStats()` alimenta as duas coisas.
    //
    // 5 s e não 1 s: o Chrome actualiza as estatísticas ~1×/s, e duas leituras
    // dentro do mesmo intervalo trazem o mesmo carimbo temporal — o delta dá
    // zero e a política reagiria a um falso «sem media» (ver R37).
    let n = 0
    const report = async () => {
      const r = await callRef.current?.qos?.().catch(() => null)
      if (!r) return
      setConditions({
        backgrounded: document.visibilityState === 'hidden',
        cpuLimited: r.limitedBy === 'cpu',
        lossPct: r.lossPct,
        rttMs: r.rttMs ?? undefined,
        // A banda ESTIMADA é do uplink; para o downlink não há estimativa
        // portável. Sem número, a política não orçamenta — inventar um tecto
        // seria pior do que não ter nenhum.
        downlinkKbps: null,
        dataSaver: Boolean((navigator as { connection?: { saveData?: boolean } }).connection?.saveData),
      })
      if (n++ % 6 !== 0) return
      // Envia-se a amostra COMPLETA, não uma média de três números. A média
      // entre publicadores era o que escondia o participante inaudível numa
      // sala em que todos os outros estavam bem — o `extractQuality` agrega
      // pelo PIOR exactamente por isso.
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
    const first = setTimeout(() => void report(), 5_000)
    const t = setInterval(() => void report(), 5_000)
    return () => { clearTimeout(first); clearInterval(t) }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [roomState, code])

  // Código de segurança E2EE (roadmap "E2EE verificável"): SHA-256 da chave
  // da sala em 4 grupos de 5 dígitos — todos os participantes derivam o
  // mesmo código e podem compará-lo verbalmente (estilo Signal).
  useEffect(() => {
    if (!e2eeOn || !e2eeKeyRef.current) return
    void (async () => {
      const raw = Uint8Array.from(atob(e2eeKeyRef.current!), (c) => c.charCodeAt(0))
      const digest = new Uint8Array(await window.crypto.subtle.digest('SHA-256', raw))
      let digits = ''
      for (let i = 0; i < 10 && digits.length < 20; i += 1) {
        const n = (digest[i * 2] << 8) | digest[i * 2 + 1]
        digits += String(n % 100000).padStart(5, '0').slice(0, 20 - digits.length)
      }
      setSecCode(digits.match(/.{5}/g)!.join(' '))
    })()
  }, [e2eeOn])

  // Desbloqueio de áudio: o browser bloqueia o autoplay do áudio remoto até
  // haver interação. A cada gesto (clique/tecla), re-tenta tocar TODOS os
  // <audio>/<video> e retoma AudioContexts — garante que se ouve os outros.
  useEffect(() => {
    const unlock = () => {
      document.querySelectorAll<HTMLMediaElement>('audio, video').forEach((el) => {
        if (el.paused) void el.play().catch(() => {})
      })
    }
    // Tenta já e a cada interação (play() em elemento a tocar é no-op).
    unlock()
    document.addEventListener('pointerdown', unlock)
    document.addEventListener('keydown', unlock)
    return () => {
      document.removeEventListener('pointerdown', unlock)
      document.removeEventListener('keydown', unlock)
    }
  }, [])

  // Legendas: cada frase fica 8s no ecrã e depois desaparece.
  useEffect(() => {
    if (!caption) return
    const t = setTimeout(() => setCaption(null), 8000)
    return () => clearTimeout(t)
  }, [caption])

  // Sincroniza a ref do panel a cada render (para uso em closures estáticas).
  panelRef.current = panel

  // Temporizador de duração da reunião (estilo Teams "00:37").
  // O relógio da duração passou para <MeetingElapsed>: era um setState por
  // segundo na raiz de um componente de 4 254 linhas (achado 2.1). Aqui ficou
  // só a HORA DE INÍCIO, que a folha recebe como prop.
  //
  // Marca-se no RENDER e não num `useEffect` — mesmo idioma do `panelRef` acima.
  // Um efeito só corre DEPOIS deste render, e o <MeetingElapsed> já teria
  // recebido `startedAt` a 0; como escrever numa ref não provoca render, ficava
  // preso nesse zero até a sala renderizar por outro motivo qualquer.
  //
  // O `if` faz a duração contar desde a PRIMEIRA entrada: uma quebra de ligação
  // e o reentrar não repõem o contador a zero.
  if (roomState === 'in' && !joinedAtRef.current) joinedAtRef.current = Date.now()

  // Atalhos de teclado: Ctrl+D = mic, Ctrl+E = câmara (estilo Google Meet).
  // Clica no botão DOM em vez de chamar toggleMic/toggleCam diretamente para
  // evitar closures obsoletas sem precisar de refatorar para useCallback.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement).tagName
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return
      if (e.ctrlKey || e.metaKey) {
        if (e.key === 'd') {
          e.preventDefault()
          document.querySelector<HTMLButtonElement>('.ctrl[aria-label*="microfone"]')?.click()
        } else if (e.key === 'e') {
          e.preventDefault()
          document.querySelector<HTMLButtonElement>('.ctrl[aria-label*="mara"]')?.click()
        }
      }
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [])

  // Conferência / 3D / notas AI
  const [viewMode, setViewMode] = useState<'grid' | 'stage'>('grid')
  // Fixar (pin) um participante no palco: 'me', o peerId, ou null (segue orador).
  const [pinnedId, setPinnedId] = useState<string | null>(null)
  const togglePin = (id: string) => setPinnedId((cur) => (cur === id ? null : id))
  // Layout da apresentação: plateia em baixo (default) ou na lateral direita.
  const [presLayout, setPresLayout] = useState<'bottom' | 'side'>('bottom')
  const [parallax, setParallax] = useState(false)
  const [tilt, setTilt] = useState({ x: 0, y: 0 })
  const [notesOpen, setNotesOpen] = useState(false)
  const [transcribing, setTranscribing] = useState(false)
  // Transcrição PARTILHADA ligada pelo anfitrião: obriga TODOS os clientes a
  // captar o próprio microfone → capta todos os oradores (#6). `scribeBy` é
  // quem a iniciou (para mostrar na UI).
  const [scribeBy, setScribeBy] = useState<string | null>(null)
  const [lines, setLines] = useState<string[]>([])
  const [interim, setInterim] = useState('')
  const [momSaved, setMomSaved] = useState(false)

  const localVideo = useRef<HTMLVideoElement | null>(null)
  const videoAreaRef = useRef<HTMLDivElement>(null)
  const callRef = useRef<Call | null>(null)
  const signalRef = useRef<Signaling | null>(null)
  const localStreamRef = useRef<MediaStream | null>(null)
  const cameraTrackRef = useRef<MediaStreamTrack | null>(null)
  const effectRef = useRef<BackgroundEffect | null>(null)
  // Supressão de ruído por IA (RNNoise): o denoiser + a track crua do mic (para
  // parar corretamente ao trocar de dispositivo/desligar).
  const denoiserRef = useRef<Denoiser | null>(null)
  const rawMicRef = useRef<MediaStreamTrack | null>(null)
  const uploadRef = useRef<HTMLInputElement>(null)
  const headRef = useRef<HeadTracker | null>(null)
  const transcriberRef = useRef<Transcriber | null>(null)
  // Espelham o estado para os callbacks do motor de voz (evita closures obsoletas).
  const transcribingRef = useRef(false)
  const ccOnRef = useRef(false)
  const bgModeRef = useRef<'none' | 'blur' | 'image'>('none')
  const levelsRef = useRef<LevelWatcher | null>(null)
  const deviceChangeRef = useRef<(() => void) | null>(null)
  const recorderRef = useRef<MeetingRecorder | null>(null)
  const talkOverSince = useRef(0)
  const peersRef = useRef<RemotePeer[]>([])
  peersRef.current = peers
  // Refs que espelham estado para callbacks sem closures obsoletas (beforeunload, WS handlers).
  const linesRef = useRef<string[]>([])
  const isHostRef = useRef(false)
  const momSavedRef = useRef(false)
  linesRef.current = lines
  isHostRef.current = isHost
  momSavedRef.current = momSaved

  function floatReaction(emoji: string, username: string) {
    const id = ++reactionSeq
    setReactions((rs) => [...rs, { id, emoji, username }])
    setTimeout(() => setReactions((rs) => rs.filter((r) => r.id !== id)), 3500)
  }

  // O relógio de parede passou para <WallClock> (achado 2.1).

  // ---- Pre-join (green room): SÓ media local para o preview. ----
  // NÃO cria Signaling nem SfuCall (alinhado com R2: nada de ofertas antes de
  // entrar). O stream é entregue ao efeito principal no clique em Entrar.
  useEffect(() => {
    if (roomState !== 'prejoin') return
    let cancelled = false
    const levels = new LevelWatcher((s) => setSpeaking((prev) => (sameSet(prev, s) ? prev : new Set(s))))
    ;(async () => {
      let permErr = ''
      const getMedia = (c: MediaStreamConstraints) =>
        navigator.mediaDevices.getUserMedia(c).catch((e: DOMException) => {
          if (e?.name === 'NotAllowedError' || e?.name === 'SecurityError') permErr = 'denied'
          else if (e?.name === 'NotReadableError' && !permErr) permErr = 'busy'
          else if (e?.name === 'NotFoundError' && !permErr) permErr = 'missing'
          throw e
        })
      const stream = await getMedia({ audio: audioConstraints(), video: videoConstraints() })
        .catch(() => getMedia({ audio: audioConstraints() }))
        .catch(() => new MediaStream())
      if (cancelled) {
        stream.getTracks().forEach((t) => t.stop())
        return
      }
      stream.getVideoTracks().forEach((t) => (t.contentHint = 'motion'))
      previewStreamRef.current = stream
      prejoinPermErrRef.current = permErr
      const hasVideo = stream.getVideoTracks().length > 0
      setHasLocalVideo(hasVideo)
      if (stream.getAudioTracks().length === 0) setMicOn(false)
      if (permErr === 'denied' && stream.getTracks().length === 0)
        setStatus('Câmara/microfone BLOQUEADOS neste site — clica no cadeado 🔒 na barra de endereço, permite Câmara e Microfone, e recarrega a página.')
      else if (permErr === 'missing') setStatus('Não foi detetada câmara nem microfone neste dispositivo.')
      else if (stream.getTracks().length === 0) setStatus('Sem câmara/microfone — vais entrar em modo espectador')
      else if (!hasVideo) setStatus('Câmara indisponível — vais entrar só com áudio.')
      if (prejoinVideoRef.current) prejoinVideoRef.current.srcObject = stream
      levels.watch('me', stream) // indicador "o mic está a captar-te"
      void listDevices().then((d) => {
        if (cancelled) return
        setDevices(d)
        setMicId(stream.getAudioTracks()[0]?.getSettings().deviceId ?? '')
        setCamId(stream.getVideoTracks()[0]?.getSettings().deviceId ?? '')
      })
    })()
    return () => {
      cancelled = true
      levels.close()
      // Handoff: se estamos a ENTRAR, o efeito principal assume o stream — não
      // parar as tracks. Só descarta se saímos do prejoin sem entrar (unmount).
      if (!joinIntentRef.current) {
        previewStreamRef.current?.getTracks().forEach((t) => t.stop())
        previewStreamRef.current = null
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [roomState === 'prejoin'])

  useEffect(() => {
    if (!joinIntent) return // à espera do Entrar do prejoin (nada foi iniciado)
    let cancelled = false
    async function start() {
      try {
        // Camera+mic (máx. resolução), depois só mic, depois modo espectador — nunca bloquear a entrada.
        // Reunião de voz: pede só o microfone (entra sem vídeo). Captura-se o
        // motivo da falha (permissão negada vs. dispositivo ocupado/ausente)
        // para dar uma mensagem acionável em vez de ficar preto/mudo em silêncio.
        let permErr = ''
        const getMedia = (c: MediaStreamConstraints) =>
          navigator.mediaDevices.getUserMedia(c).catch((e: DOMException) => {
            if (e?.name === 'NotAllowedError' || e?.name === 'SecurityError') permErr = 'denied'
            else if (e?.name === 'NotReadableError' && !permErr) permErr = 'busy'
            else if (e?.name === 'NotFoundError' && !permErr) permErr = 'missing'
            throw e
          })
        // Handoff do prejoin: reutiliza o stream do preview (dispositivos e
        // toggles escolhidos lá) em vez de readquirir — sem flicker e sem risco
        // de "câmara ocupada". Sem prejoin (voz/rejoin), adquire como sempre.
        const handoff = previewStreamRef.current
        previewStreamRef.current = null
        if (handoff) permErr = prejoinPermErrRef.current
        const stream =
          handoff ??
          (await getMedia(
            voiceOnly ? { audio: audioConstraints() } : { audio: audioConstraints(), video: videoConstraints() },
          )
            .catch(() => getMedia({ audio: audioConstraints() }))
            .catch(() => new MediaStream()))
        stream.getVideoTracks().forEach((t) => (t.contentHint = 'motion'))
        if (cancelled) {
          stream.getTracks().forEach((t) => t.stop())
          return
        }
        const spectator = stream.getTracks().length === 0
        const hasVideo = stream.getVideoTracks().length > 0
        if (permErr === 'denied' && spectator)
          setStatus('Câmara/microfone BLOQUEADOS neste site — clica no cadeado 🔒 na barra de endereço, permite Câmara e Microfone, e recarrega a página.')
        else if (permErr === 'denied' && !hasVideo)
          setStatus('Câmara bloqueada — clica no cadeado 🔒 na barra de endereço, permite a Câmara, e recarrega.')
        else if (permErr === 'missing')
          setStatus('Não foi detetada câmara nem microfone neste dispositivo.')
        else if (spectator) setStatus('Sem câmara/microfone — modo espectador')
        // Pediu câmara mas só veio áudio: quase sempre a câmara está ocupada
        // por outra app/separador (ex.: testar em duas abas no mesmo PC).
        else if (!voiceOnly && !hasVideo)
          setStatus('Câmara indisponível (em uso por outra app ou separador) — entraste só com áudio. Clica na câmara para tentar ligar.')
        // O indicador de mic reflete a realidade: sem track de áudio (mic negado
        // ou espectador) mostra-se desligado — clicar no mic readquire-o.
        if (stream.getAudioTracks().length === 0) setMicOn(false)
        setHasLocalVideo(hasVideo)
        localStreamRef.current = stream
        cameraTrackRef.current = stream.getVideoTracks()[0] ?? null

        // Supressão de ruído por IA (RNNoise) no arranque: troca a track crua do
        // mic pela limpa ANTES de a chamada começar. Fallback: mantém a crua.
        const initRaw = stream.getAudioTracks()[0]
        const initMicId = initRaw?.getSettings().deviceId ?? ''
        if (initRaw && noiseSuppression) {
          const clean = await denoiseMic(initRaw, true)
          if (clean !== initRaw) {
            // Preserva o mute escolhido no prejoin (a track limpa nasce enabled).
            clean.enabled = initRaw.enabled
            stream.removeTrack(initRaw)
            stream.addTrack(clean)
          }
        }
        if (localVideo.current) localVideo.current.srcObject = stream

        // Deteção de voz local + lista de dispositivos (labels só após permissão).
        const levels = new LevelWatcher((s) => setSpeaking((prev) => (sameSet(prev, s) ? prev : new Set(s))))
        levelsRef.current = levels
        levels.watch('me', stream)
        void listDevices().then((d) => {
          if (cancelled) return
          setDevices(d)
          setMicId(initMicId || stream.getAudioTracks()[0]?.getSettings().deviceId || '')
          setCamId(stream.getVideoTracks()[0]?.getSettings().deviceId ?? '')
        })
        // Guardado em ref para poder ser removido no cleanup: sem isso,
        // cada re-entrada na sala acumulava mais um listener vivo.
        const onDeviceChange = () => void listDevices().then(setDevices).catch(() => {})
        deviceChangeRef.current = onDeviceChange
        navigator.mediaDevices.addEventListener?.('devicechange', onDeviceChange)

        // A linha do tempo começa AQUI e não dentro da SfuCall: o utilizador
        // quis entrar muito antes de existir uma RTCPeerConnection, e é o
        // tempo DELE que interessa medir. Uma medição que começa quando o
        // código está pronto mede o código, não a experiência.
        const tempos = new LinhaDoTempo()
        tempos.marcar('intencao')
        const [{ room, room_token, scheduled }, rtcConfig] = await Promise.all([joinRoom(code), iceServers()])
        tempos.marcar('token')
        setTopology(room.topology)
        setIsTraining(room.format === 'training')
        setIsInstant(scheduled === false) // só marca instantânea se o servidor o confirmar (degrada em falso)
        setWaitingRoomOn(room.waiting_room)
        const amHost = room.owner_id === currentUser()?.id
        setIsHost(amHost)
        setCanAdmit(amHost) // o anfitrião pode admitir; outros só se promovidos

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
          const rawKey = await deriveRoomKey(pass, code)
          // Cópia base64 ANTES do setKey (o buffer é transferido ao worker):
          // é a chave que o anfitrião pode ceder à gravação server-side.
          e2eeKeyRef.current = btoa(String.fromCharCode(...new Uint8Array(rawKey)))
          await crypto.setKey(rawKey)
          setE2eeOn(true)
        }

        const signal = new Signaling(room_token, code)
        signalRef.current = signal
        signal.on('joined', () => tempos.marcar('ws'))
        // A chamada SFU/mesh só arranca DEPOIS de sermos admitidos ('joined').
        // Se negociarmos enquanto estamos na sala de espera, o servidor descarta
        // a oferta e, ao ser admitido, a colisão de renegociação entra em loop e
        // dispara o anti-flood → o convidado é desligado e a página recarrega em
        // ciclo. O holder é preenchido mais abaixo (quando media/crypto existem).
        const callHolder: { start: () => void } = { start: () => {} }
        signal.on('chat', (m) => {
          setChat((c) => [...c, { username: m.username, text: m.text, own: false }])
          if (panelRef.current !== 'chat') setUnreadChat((n) => n + 1)
        })
        signal.on('error', (m) => setStatus(m.message))
        // Este nó vai fechar. Não é um erro nem uma expulsão: a chamada
        // continua a funcionar, e migra-se quando o servidor disser.
        //
        // O JITTER não é enfeite. Um pod a drenar avisa a sala INTEIRA no mesmo
        // instante; sem jitter, vinte pessoas reconectam no mesmo milissegundo
        // e o pod novo leva com a sala toda de uma vez — trocava-se um
        // encerramento ordenado por uma avalanche.
        signal.on('draining', ({ reconnect_in_ms }) => {
          if (cancelled) return
          setStatus('O servidor vai reiniciar — a mudar de nó, sem sair da reunião.')
          const jitter = Math.round(reconnect_in_ms * Math.random())
          sessionStorage.setItem(`dx_rejoin_${code}`, String(Date.now()))
          setTimeout(() => {
            if (!cancelled) location.reload()
          }, reconnect_in_ms + jitter)
        })

        signal.onclose = () => {
          if (cancelled) return // saída intencional da sala
          // O WS caiu (rede/proxy). Sem sinalização não há renegociação → media
          // num sentido só. Reconecta recarregando a sala (rejoin limpo), com
          // guarda anti-loop: se cair outra vez em < 8s, pede recarregar manual.
          const now = Date.now()
          const last = Number(sessionStorage.getItem('dx_reconnect_at') || 0)
          if (now - last > 8000) {
            sessionStorage.setItem('dx_reconnect_at', String(now))
            // Renova o rejoin AGORA (o carimbo do join pode ter > 60s): o reload
            // de reconexão tem de saltar o prejoin e voltar direto à chamada.
            sessionStorage.setItem(`dx_rejoin_${code}`, String(now))
            setStatus('Ligação perdida — a reconectar…')
            setTimeout(() => {
              if (!cancelled) location.reload()
            }, 1500)
          } else {
            setStatus('Ligação instável. Verifica a rede / o proxy do WebSocket e recarrega a página.')
          }
        }

        // Estados da sala de espera / controlo do anfitrião.
        signal.on('waiting', () => setRoomState('waiting'))
        signal.on('denied', () => setRoomState('denied'))
        signal.on('kicked', () => {
          sessionStorage.removeItem(`dx_rejoin_${code}`)
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
        // O anfitrião promoveu-me (ou revogou) o poder de admitir entradas.
        signal.on('admit-role', (m) => {
          setCanAdmit(m.allowed || amHost)
          setStatus(m.allowed ? 'Podes agora admitir convidados da sala de espera' : '')
          if (!m.allowed) setWaitingQueue([])
        })
        // Crachá de co-anfitrião de admissões nos participantes.
        signal.on('peer-role', (m) =>
          setPeers((ps) => ps.map((p) => (p.peerId === m.peer_id ? { ...p, canAdmit: m.can_admit } : p))),
        )

        // Breakout rooms: mover para o grupo (guardando o caminho de volta)
        // ou regressar à sala principal quando o anfitrião encerra.
        signal.on('breakout-move', (m) => {
          if (m.back) {
            sessionStorage.removeItem(`dx_return_${code}`)
            sessionStorage.removeItem(`dx_bo_ends_${code}`)
          } else {
            sessionStorage.setItem(`dx_return_${m.code}`, returnTo ?? code)
            if (m.ends_at) sessionStorage.setItem(`dx_bo_ends_${m.code}`, String(m.ends_at))
            else sessionStorage.removeItem(`dx_bo_ends_${m.code}`)
          }
          onSwitch?.(m.code)
        })
        signal.on('breakouts-created', (m) => {
          setBreakoutRooms(m.rooms)
          setBreakoutEndsAt(m.ends_at)
        })

        // A grelha é orientada ao roster: tile ao entrar, stream quando chegar.
        signal.on('joined', (m) => {
          setRoomState('in')
          // Guarda o lugar (R91). A partir daqui, uma quebra do socket — F5,
          // Wi-Fi que cai, aba que dorme — devolve o mesmo lugar, com o papel
          // de anfitrião e sem voltar à sala de espera.
          if (m.reconnect) Signaling.guardarSegredo(code, m.reconnect)
          meuPeerIdRef.current = m.peer_id
          // Marca o rejoin recente: o reload de reconexão (onclose) e os breakouts
          // saltam o prejoin dentro desta janela (ver init do joinIntent).
          sessionStorage.setItem(`dx_rejoin_${code}`, String(Date.now()))
          callHolder.start() // arranca a chamada SÓ agora (após admissão/entrada direta)
          setStatus(spectator ? 'Sem câmara/microfone — modo espectador' : '')
          // Carrega histórico de chat (best-effort, não bloqueia a sala).
          void roomChatHistory(code).then((history) => {
            if (cancelled) return
            setChat(history.map((h) => ({
              username: h.username,
              text: h.message,
              own: h.user_id === currentUser()?.id,
              historical: true,
            })))
          }).catch(() => {})
          setPeers(
            m.peers.map((p) => ({
              peerId: p.peer_id,
              username: p.username,
              host: p.host,
              hand: p.hand,
              camOn: p.cam ?? true,
              micOn: p.mic ?? true,
              canAdmit: p.can_admit ?? p.host,
              stream: null,
            })),
          )
          void listRecordings(code).then(setRecordings).catch(() => {})
        })
        // Quem reclama o lugar volta com o MESMO peer_id, por isso o
        // `peer-joined` que se segue actualiza a entrada existente em vez de
        // criar outra — e limpa a marca de «a voltar».
        signal.on('peer-joined', (m) =>
          setPeers((ps) => [
            ...ps.filter((p) => p.peerId !== m.peer.peer_id),
            {
              peerId: m.peer.peer_id,
              username: m.peer.username,
              host: m.peer.host,
              hand: m.peer.hand,
              camOn: m.peer.cam ?? true,
              micOn: m.peer.mic ?? true,
              canAdmit: m.peer.can_admit ?? m.peer.host,
              stream: null,
            },
          ]),
        )
        signal.on('peer-left', (m) => {
          levelsRef.current?.unwatch(m.peer_id)
          setPeers((ps) => ps.filter((p) => p.peerId !== m.peer_id))
        })
        // O socket do outro caiu, mas o lugar dele está reservado (R91). O
        // retrato FICA — marcado como a voltar — em vez de desaparecer e
        // reaparecer segundos depois, que é o salto que fazia uma quebra de
        // rede parecer que a pessoa tinha saído e voltado a entrar.
        signal.on('peer-reconnecting', (m) =>
          setPeers((ps) =>
            ps.map((p) => (p.peerId === m.peer_id ? { ...p, reconnecting: true } : p)),
          ),
        )
        signal.on('hand', (m) =>
          setPeers((ps) => ps.map((p) => (p.peerId === m.from ? { ...p, hand: m.raised } : p))),
        )
        // Estado de câmara/mic dos outros: avatar em vez de vídeo preto.
        signal.on('media', (m) =>
          setPeers((ps) => ps.map((p) => (p.peerId === m.from ? { ...p, camOn: m.cam, micOn: m.mic } : p))),
        )
        signal.on('reaction', (m) => floatReaction(m.emoji, m.username))
        signal.on('recording', (m) => {
          setRemoteRecorder(m.active ? m.username : '')
          if (m.active) setRecNotice(`${m.username} começou a gravar a reunião`)
          if (!m.active) void listRecordings(code).then(setRecordings).catch(() => {})
        })
        signal.on('room-settings', (m) => {
          setRoomLocked(m.locked)
          setHostShareOnly(m.host_share_only)
          setChatOn(m.chat_on ?? true)
          setAllowUnmute(m.allow_unmute ?? true)
        })
        signal.on('force-cam-off', () => {
          const track = localStreamRef.current?.getVideoTracks()[0]
          if (track) track.enabled = false
          setCamOn(false)
          setStatus('O anfitrião desligou a tua câmara')
        })
        signal.on('muted-all', (m) => {
          const track = localStreamRef.current?.getAudioTracks()[0]
          if (track) track.enabled = false
          setMicOn(false)
          setAllowUnmute(m.allow_unmute)
          setStatus(
            m.allow_unmute
              ? 'O anfitrião silenciou toda a gente'
              : 'O anfitrião silenciou toda a gente — não te podes voltar a ligar',
          )
        })
        signal.on('host-changed', (m) => {
          setPeers((ps) =>
            ps.map((p) => ({ ...p, host: p.peerId === m.to ? true : p.peerId === m.from ? false : p.host })),
          )
          setIsHost((h) =>
            meuPeerIdRef.current === m.to ? true : meuPeerIdRef.current === m.from ? false : h,
          )
        })
        signal.on('share-granted', (m) => {
          setShareAllowed(m.allowed)
          const wasPending = pendingShareRef.current
          pendingShareRef.current = false
          if (m.allowed && wasPending) {
            // O grant chegou em resposta ao pedido: arranca a partilha já.
            setStatus('O anfitrião autorizou — a iniciar partilha de ecrã…')
            toggleShareRef.current()
            return
          }
          if (!m.allowed && wasPending) {
            setStatus('O anfitrião recusou o pedido de partilha de ecrã')
            return
          }
          setStatus(m.allowed ? 'O anfitrião permitiu-te partilhar o ecrã' : 'A permissão de partilha foi revogada')
        })
        signal.on('polls', (m) => setPolls(m.polls))
        signal.on('qa', (m) => setQuestions(m.questions))
        signal.on('timer', (m) => setMeetTimerEndsAt(m.ends_at))
        signal.on('server-recording', (m) => {
          setServerRec(m.active ? { by: m.by } : null)
          if (m.active) {
            setRecNotice(`${m.by} começou a gravar no servidor`)
          } else if (isHostRef.current && linesRef.current.length > 0 && !momSavedRef.current) {
            // Gravação parou → guardar ata automaticamente sem bloquear o UI
            saveMinutesByRoom(code, buildMoM(linesRef.current), linesRef.current.join('\n'))
              .then(() => { setMomSaved(true); setStatus('Ata guardada automaticamente ao parar a gravação') })
              .catch(() => {})
          }
        })
        // Quadro branco: snapshot ao entrar + traços/limpeza em tempo real.
        signal.on('wb-state', (m) => {
          // Só carrega o conteúdo — NÃO abre sozinho. Abrir no snapshot fazia o
          // quadro reaparecer a cada reload se a sala tivesse traços antigos.
          setWbStrokes(m.strokes)
        })
        signal.on('wb-stroke', (m) => {
          setWbStrokes((st) => [...st, m.stroke])
          setWbOpen(true) // alguém desenhou → o quadro aparece a todos
        })
        signal.on('wb-clear', () => setWbStrokes([]))
        // Alguém fechou o quadro → fecha em todos (#4).
        signal.on('wb-close', () => setWbOpen(false))
        // Aviso fiável de apresentação: ao parar, limpa já a apresentação desse
        // peer nos recetores (sem esperar por eventos de track) (#2).
        signal.on('presenting', (m) => {
          if (!m.on) setPresentation((p) => (p?.peerId === m.from ? null : p))
        })
        // Transcrição de outro participante — junta à transcrição partilhada.
        signal.on('transcript', (m) => {
          const stamp = new Date().toLocaleTimeString('pt-PT', { hour: '2-digit', minute: '2-digit' })
          // A legenda ao vivo só aparece a quem tem CC ligado; as notas
          // acumulam sempre (transcrição partilhada, legendada por orador).
          if (ccOnRef.current) showCaption(m.username, m.text)
          setLines((l) => [...l, `[${stamp}] ${m.username}: ${m.text}`])
        })
        // Legenda PARCIAL de outro orador — atualiza a legenda ao vivo (estilo
        // Meet), sem esperar pelo fim da frase. Efémera: não vai para as notas.
        signal.on('transcript-interim', (m) => {
          if (ccOnRef.current) showCaption(m.username, m.text, true)
        })
        // O anfitrião ligou/desligou a Nota AI partilhada: todos captam o
        // próprio microfone (#6). Segue o estado partilhado localmente.
        signal.on('transcription', (m) => {
          setScribeBy(m.on ? m.by : null)
          setTranscribing(m.on)
          // NÃO abrir o painel de notas nos outros participantes — só quem
          // inicia (o anfitrião, via toggleTranscription) o abre. Aos restantes
          // basta um aviso de que a sua fala está a ser captada (#5).
          if (m.on) setStatus(`Transcrição iniciada por ${m.by} — a tua fala é captada`)
        })

        signal.on('remote-control', (m) => {
          if (m.action === 'request') {
            const who = peersRef.current.find((p) => p.peerId === m.from)?.username ?? 'Alguém'
            setCtrlAsk({ from: m.from, username: who })
          } else if (m.action === 'accept') {
            setStatus('🎮 Pedido aceite — controlo remoto da tela partilhada ativo')
          } else if (m.action === 'deny') {
            setStatus('O pedido de controlo remoto foi recusado')
          }
        })
        // Pedido de partilha de um não-anfitrião → diálogo Permitir/Negar.
        signal.on('share-request', (m) => setShareAsk({ from: m.from, username: m.username }))
        // O apresentador abriu o quadro branco → abre em todos.
        signal.on('wb-open', (m) => {
          setWbOpen(true)
          setStatus(`Quadro branco partilhado por ${m.by}`)
        })

        const callbacks = {
          // Media para um peer desconhecido é descartada — o roster chega
          // sempre primeiro; evita tiles fantasma de m-lines sem publisher.
          onStream: (peerId: string, remote: MediaStream) => {
            // Partilha de ecrã de outro participante: stream "<peer>-screen"
            // vira a apresentação em palco (não substitui a câmara dele).
            if (peerId.endsWith('-screen')) {
              const owner = peerId.slice(0, -7)
              setPresentation({ peerId: owner, stream: remote })
              const clear = () =>
                setPresentation((p) => (p?.peerId === owner ? null : p))
              remote.getVideoTracks().forEach((t) => {
                t.onended = clear
                t.onmute = () => setTimeout(() => { if (t.muted) clear() }, 2000)
              })
              remote.onremovetrack = clear
              return
            }
            levelsRef.current?.watch(peerId, remote)
            setPeers((ps) => ps.map((p) => (p.peerId === peerId ? { ...p, stream: remote } : p)))
          },
          onPeerLeft: (peerId: string) => {
            levelsRef.current?.unwatch(peerId)
            setPresentation((p) => (p?.peerId === peerId ? null : p))
            setPeers((ps) => ps.map((p) => (p.peerId === peerId ? { ...p, stream: null } : p)))
          },
          // Estado da ligação de media (ver callRecovery.ts). O que interessa
          // ao utilizador é saber que ALGUÉM está a tratar do assunto: antes
          // disto, uma quebra de rede deixava o ecrã preto sem uma palavra até
          // ele próprio recarregar a página.
          onState: (st: CallState) => {
            if (cancelled) return
            setCallState(st)
            // Reporta os tempos UMA vez, quando a media aparece. Esperar pelo
            // fim da chamada perderia os números de quem fecha o separador — e
            // quem fecha o separador a meio é precisamente quem teve má
            // experiência, que é a cauda que interessa medir.
            if (st === 'connected' && !temposEnviados.current && tempos.vale_reportar()) {
              temposEnviados.current = true
              void postTimings(code, tempos.resumo()).catch(() => {})
            }
            if (st === 'degraded') setStatus('Ligação instável — a media pode falhar por instantes.')
            else if (st === 'reconnecting' || st === 'recovering') setStatus('A restabelecer a ligação de media…')
            else if (st === 'connected') setStatus('')
            else if (st === 'failed') {
              // Último recurso, e SÓ agora: o recarregar que dantes era a
              // primeira (e única) resposta a qualquer quebra.
              setStatus('Não foi possível restabelecer a media. A reentrar na sala…')
              sessionStorage.setItem(`dx_rejoin_${code}`, String(Date.now()))
              setTimeout(() => {
                if (!cancelled) location.reload()
              }, 2000)
            }
          },
        }
        // Preenche o holder — só arranca quando o handler 'joined' o invocar
        // (após admissão). A guarda (idempotência + não-montar-em-espera = R1/R2)
        // vive em makeCallHolderStart, testada em sfuLifecycle.test.ts.
        callHolder.start = makeCallHolderStart({
          ref: callRef,
          isCancelled: () => cancelled,
          create: () =>
            room.topology === 'sfu'
              ? new SfuCall(signal, stream, rtcConfig, callbacks, crypto, undefined, tempos)
              : new MeshCall(signal, stream, rtcConfig, callbacks, crypto),
        })
      } catch (err) {
        // Uma falha a montar a sala NÃO é terminal. Antes disto, o `catch`
        // pintava a mensagem técnica do erro e parava ali: um corte de seis
        // segundos no servidor — medido — deixava toda a gente na reunião presa
        // em «Erro: Internal Server Error», sem retorno, mesmo depois de o
        // servidor voltar. E «Internal Server Error» não é uma frase que se
        // mostre a alguém numa reunião.
        if (cancelled || isAbort(err)) return
        tentativas += 1
        if (tentativas <= MAX_TENTATIVAS) {
          const espera = backoffDelay(tentativas - 1)
          setStatus(`Sem ligação ao servidor — a tentar de novo (${tentativas}/${MAX_TENTATIVAS})…`)
          setTimeout(() => {
            if (!cancelled) void start()
          }, espera)
          return
        }
        setStatus('Não foi possível ligar ao servidor da reunião. Verifica a ligação e recarrega a página.')
      }
    }
    // As tentativas contam-se FORA do `start`, senão cada nova tentativa
    // reinicia o contador e o recuo nunca cresce.
    let tentativas = 0
    const MAX_TENTATIVAS = 6
    start()
    return () => {
      cancelled = true
      if (deviceChangeRef.current) {
        navigator.mediaDevices.removeEventListener?.('devicechange', deviceChangeRef.current)
        deviceChangeRef.current = null
      }
      levelsRef.current?.close()
      effectRef.current?.stop()
      headRef.current?.stop()
      transcriberRef.current?.stop()
      denoiserRef.current?.stop()
      rawMicRef.current?.stop()
      callRef.current?.hangup()
      localStreamRef.current?.getTracks().forEach((t) => t.stop())
    }
  }, [code, passTry, joinIntent])

  // Guardar ata via fetch keepalive quando o utilizador fecha o tab sem clicar Sair.
  // keepalive: true garante que o pedido completa mesmo após o unload da página.
  useEffect(() => {
    function handleUnload() {
      if (!isHostRef.current || linesRef.current.length === 0 || momSavedRef.current) return
      const token = localStorage.getItem('dlx_token')
      if (!token) return
      fetch(`/api/rooms/${code}/minutes`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({
          minutes: buildMoM(linesRef.current),
          transcript: linesRef.current.join('\n'),
        }),
        keepalive: true,
      })
    }
    window.addEventListener('beforeunload', handleUnload)
    return () => window.removeEventListener('beforeunload', handleUnload)
  }, [code])

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

  // Difunde o estado de câmara/mic sempre que muda (toggle, force-mute,
  // voz→vídeo) — os outros trocam vídeo preto por avatar e acertam o ícone.
  useEffect(() => {
    // No SFU o ecrã é track separada — a câmara mantém o seu estado; no mesh
    // a partilha substitui a câmara, por isso conta como "vídeo ligado".
    const meshSharing = sharing && topology !== 'sfu'
    if (roomState === 'in')
      signalRef.current?.send({ type: 'media', cam: (camOn && hasLocalVideo) || meshSharing, mic: micOn })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [camOn, micOn, hasLocalVideo, sharing, roomState, topology])

  async function toggleMic() {
    // Com «silenciar todos sem voltar a ligar» activo, o botão não liga o
    // microfone. É uma guarda de INTERFACE, e a de servidor é o próprio
    // servidor não reencaminhar áudio de quem está silenciado — esta só evita
    // que a pessoa julgue que está a falar quando não está.
    if (!allowUnmute && !micOn && !isHost) {
      setStatus('O anfitrião não permite voltar a ligar o microfone')
      return
    }
    levelsRef.current?.resume()
    const track = localStreamRef.current?.getAudioTracks()[0]
    if (track) {
      track.enabled = !track.enabled
      setMicOn(track.enabled)
      return
    }
    // Sem track de áudio (entrou em modo espectador ou negou o mic no início):
    // adquire o microfone agora e liga-o à chamada, para poder falar/transcrever.
    try {
      const s = await navigator.mediaDevices.getUserMedia({ audio: audioConstraints(micId || undefined) })
      const newTrack = s.getAudioTracks()[0]
      if (!newTrack) throw new Error('sem microfone')
      localStreamRef.current?.addTrack(newTrack)
      await callRef.current?.replaceAudioTrack(newTrack)
      if (localStreamRef.current) levelsRef.current?.watch('me', localStreamRef.current)
      setMicId(newTrack.getSettings().deviceId ?? '')
      setMicOn(true)
    } catch {
      setStatus('Não foi possível aceder ao microfone — verifica a permissão do browser')
    }
  }

  async function toggleCam() {
    const existing = localStreamRef.current?.getVideoTracks()[0]
    if (existing && camOn) {
      // DESLIGAR: liberta mesmo a câmara — o LED apaga e o encoder pára.
      // `enabled = false` (o que se fazia antes) mantinha a captura e a
      // codificação a correr e o indicador do sistema aceso, o que num
      // produto que se vende por soberania de dados não é aceitável.
      // O transceiver fica de pé (`replaceTrack(null)`), por isso voltar a
      // ligar não renegoceia nem perde o simulcast.
      setCamOn(false)
      setHasLocalVideo(false)
      await callRef.current?.disableVideo()
      if (parallax) {
        headRef.current?.stop()
        headRef.current = null
        setParallax(false)
        setTilt({ x: 0, y: 0 })
      }
      effectRef.current?.stop()
      localStreamRef.current?.removeTrack(existing)
      existing.stop()
      cameraTrackRef.current = null
      if (localVideo.current) localVideo.current.srcObject = localStreamRef.current
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
      setStatus('Efeito 3D ligado — move a cabeça e sente a profundidade da sala')
    } catch {
      setStatus('Efeito 3D indisponível neste dispositivo')
    }
  }

  /** Notas AI (só anfitrião): liga/desliga a transcrição PARTILHADA. Difunde a
   * todos via WS → cada cliente capta o próprio microfone (#6). O estado local
   * é atualizado pelo eco do servidor no handler `transcription`. */
  function toggleTranscription() {
    if (!isHost) return // só o anfitrião controla a transcrição partilhada
    setNotesOpen(true)
    signalRef.current?.send({ type: 'transcription-toggle', on: !transcribing })
  }

  // Motor de voz do browser (Web Speech; Whisper WASM como fallback no Firefox).
  // Corre um ÚNICO motor sempre que as legendas (CC) OU as notas estiverem
  // ativas — enquanto falo, transcreve e difunde a frase, e os outros com CC
  // ligado veem a minha legenda. Um só motor porque a Web Speech não permite
  // várias instâncias em simultâneo.
  useEffect(() => {
    transcribingRef.current = transcribing
  }, [transcribing])
  useEffect(() => {
    ccOnRef.current = ccOn
  }, [ccOn])
  useEffect(() => {
    ccLangRef.current = ccLang
    localStorage.setItem('dx_cc_lang', ccLang)
  }, [ccLang])

  /** Mostra a legenda já (original) e, com tradução ativa, substitui pela
   *  versão traduzida do LLM local — só se ainda for a linha mais recente (seq),
   *  para traduções lentas não taparem falas novas. `interim` (parcial): a
   *  tradução é debounced e single-flight, senão bombardeávamos o Ollama com um
   *  pedido por palavra; a versão final (não-interim) traduz sempre. */
  function showCaption(prefix: string, text: string, interim = false) {
    const seq = ++ccSeqRef.current
    setCaption({ text: `${prefix}: ${text}`, at: Date.now() })
    const target = ccLangRef.current
    if (!target) return
    const applyTranslation = () => {
      const x = interimXlateRef.current
      x.busy = true
      void translateCaption(text, target)
        .then((r) => {
          if (ccSeqRef.current === seq) setCaption({ text: `${prefix}: ${r.text}`, at: Date.now() })
        })
        .catch(() => {})
        .finally(() => { x.busy = false })
    }
    if (!interim) {
      applyTranslation()
      return
    }
    // Parcial: agenda a tradução ~400ms depois da última palavra; se já houver
    // um pedido em curso, deixa-o terminar (single-flight) e a final corrige.
    const x = interimXlateRef.current
    if (x.timer != null) window.clearTimeout(x.timer)
    x.timer = window.setTimeout(() => {
      x.timer = null
      if (!x.busy) applyTranslation()
    }, 400)
  }
  useEffect(() => {
    const want = (ccOn || transcribing) && roomState === 'in'
    if (want && !transcriberRef.current) {
      const t = new Transcriber()
      t.onFinal = (text) => {
        const stamp = new Date().toLocaleTimeString('pt-PT', { hour: '2-digit', minute: '2-digit' })
        if (ccOnRef.current) showCaption('eu', text)
        setInterim('')
        // Difunde a frase para os outros montarem legenda/transcrição partilhada.
        signalRef.current?.send({ type: 'transcript', text })
        // Só acumula nas notas quando a transcrição está ligada (CC é efémera).
        if (transcribingRef.current) setLines((l) => [...l, `[${stamp}] eu: ${text}`])
      }
      t.onInterim = (text) => {
        setInterim(text)
        // Legenda ao vivo (estilo Meet) enquanto falo, se o CC estiver ligado.
        if (ccOnRef.current && text) showCaption('eu', text)
        // Difunde a legenda PARCIAL aos outros (throttle ~250ms) — é isto que
        // acaba com a sensação de "standby": eles veem a fala em tempo real, sem
        // esperar pelo fim da frase. Só quando a transcrição partilhada está on.
        if (transcribingRef.current && text) {
          const now = Date.now()
          if (now - interimSentAtRef.current >= 250) {
            interimSentAtRef.current = now
            signalRef.current?.send({ type: 'transcript-interim', text })
          }
        }
      }
      t.onError = (message) => {
        setStatus(message)
        setInterim('')
        transcriberRef.current?.stop()
        transcriberRef.current = null
        setTranscribing(false)
        setCcOn(false)
      }
      t.start(sttLang, localStreamRef.current)
      transcriberRef.current = t
      if (!localStreamRef.current?.getAudioTracks().length)
        setInterim('(à espera do microfone — se nada aparecer, verifica a permissão do browser)')
    } else if (!want && transcriberRef.current) {
      transcriberRef.current.stop()
      transcriberRef.current = null
      setInterim('')
    }
  }, [ccOn, transcribing, sttLang, roomState, serverAsr])

  // Pára o motor ao sair da sala (desmontagem).
  useEffect(
    () => () => {
      transcriberRef.current?.stop()
      transcriberRef.current = null
    },
    [],
  )

  // Toast transitório de início de gravação — some ao fim de 6s (o banner
  // persistente `anyoneRecording` continua a indicar que a gravação está ativa).
  useEffect(() => {
    if (!recNotice) return
    const t = setTimeout(() => setRecNotice(''), 6000)
    return () => clearTimeout(t)
  }, [recNotice])

  useEffect(() => {
    bgModeRef.current = bgMode
  }, [bgMode])

  // Ref de callback do vídeo local: o `srcObject` não é uma prop do React, por
  // isso perde-se quando o elemento <video> é REMONTADO (ex.: o meu tile muda da
  // grelha para a plateia ao iniciar a partilha de ecrã). Ao (re)montar, volta a
  // ligar o stream da câmara (ou a saída do efeito de fundo) — sem isto o tile
  // fica preto mesmo com a câmara ligada.
  const attachLocalVideo = useCallback((node: HTMLVideoElement | null) => {
    localVideo.current = node
    if (!node || node.srcObject) return
    const eff = bgModeRef.current !== 'none' ? effectRef.current?.output ?? null : null
    node.srcObject = eff ? new MediaStream([eff]) : localStreamRef.current
  }, [])

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

  /** Sair da sala. Se houver transcrição por guardar, grava a ata AUTOMATICAMENTE
   *  antes de sair (#7) — o anfitrião não perde as notas ao encerrar. */
  async function leaveRoom() {
    // SAIR não é CAIR: quem sai de propósito larga o lugar, para não ficar a
    // ocupar sítio na sala durante a janela de graça. É a única diferença
    // entre os dois casos, e é o cliente que a sabe.
    Signaling.esquecerSegredo(code)
    sessionStorage.removeItem(`dx_rejoin_${code}`) // saída intencional → próximo acesso volta ao prejoin
    if (isHost && lines.length > 0 && !momSaved) {
      setStatus('A guardar a ata antes de sair…')
      try {
        await saveMinutesByRoom(code, buildMoM(lines), lines.join('\n'))
      } catch {
        /* não bloqueia a saída se a gravação falhar */
      }
    }
    onLeave()
  }

  // ---- Handlers do pre-join (operam SÓ no stream de preview; sem SfuCall) ----
  function prejoinJoin() {
    joinIntentRef.current = true // antes do setState: o cleanup do prejoin lê isto (handoff)
    setJoinIntent(true)
    setRoomState('connecting')
  }
  function prejoinToggle(kind: 'mic' | 'cam') {
    const s = previewStreamRef.current
    const t = kind === 'mic' ? s?.getAudioTracks()[0] : s?.getVideoTracks()[0]
    if (!t) return // sem dispositivo (espectador) — nada a alternar
    t.enabled = !t.enabled
    if (kind === 'mic') setMicOn(t.enabled)
    else setCamOn(t.enabled)
  }
  async function prejoinSwitch(kind: 'mic' | 'cam', deviceId: string) {
    const s = previewStreamRef.current
    if (!s || !deviceId) return
    try {
      const ns = await navigator.mediaDevices.getUserMedia(
        kind === 'mic' ? { audio: audioConstraints(deviceId) } : { video: videoConstraints(deviceId) },
      )
      const nt = kind === 'mic' ? ns.getAudioTracks()[0] : ns.getVideoTracks()[0]
      const old = kind === 'mic' ? s.getAudioTracks()[0] : s.getVideoTracks()[0]
      if (old) {
        nt.enabled = old.enabled // preserva o mute do toggle
        old.stop()
        s.removeTrack(old)
      }
      if (kind === 'cam') nt.contentHint = 'motion'
      s.addTrack(nt)
      if (kind === 'mic') setMicId(deviceId)
      else {
        setCamId(deviceId)
        setHasLocalVideo(true)
      }
      // Track trocada → re-liga o <video> do preview.
      if (prejoinVideoRef.current) prejoinVideoRef.current.srcObject = s
    } catch {
      setStatus('Não foi possível trocar de dispositivo')
    }
  }

  /** Troca o microfone e/ou reaplica a supressão de ruído. */
  /** Aplica (ou remove) a supressão por IA e devolve a track a enviar à chamada.
   *  Para o denoiser/track crua anteriores. Fallback: track crua se o RNNoise falhar. */
  async function denoiseMic(rawTrack: MediaStreamTrack, ns: boolean): Promise<MediaStreamTrack> {
    denoiserRef.current?.stop()
    denoiserRef.current = null
    rawMicRef.current?.stop()
    rawMicRef.current = null
    if (!ns) return rawTrack
    try {
      const d = new Denoiser()
      const clean = await d.process(rawTrack)
      clean.enabled = rawTrack.enabled
      denoiserRef.current = d
      rawMicRef.current = rawTrack // mantém viva (alimenta o denoiser)
      return clean
    } catch (e) {
      console.warn('[denoise] RNNoise indisponível — supressão nativa do browser', e)
      return rawTrack
    }
  }

  async function switchMic(deviceId: string, ns = noiseSuppression) {
    try {
      // Supressão NATIVA do browser sempre ligada (robusta); `ns` controla só a
      // camada de IA (RNNoise) aplicada em denoiseMic.
      const s = await navigator.mediaDevices.getUserMedia({ audio: audioConstraints(deviceId || undefined) })
      const raw = s.getAudioTracks()[0]
      raw.enabled = micOn
      const devId = raw.getSettings().deviceId ?? deviceId // capturar ANTES do denoise (a track limpa não tem deviceId)
      const sendTrack = await denoiseMic(raw, ns)
      sendTrack.enabled = micOn
      const old = localStreamRef.current?.getAudioTracks()[0]
      if (old) {
        localStreamRef.current?.removeTrack(old)
        old.stop()
      }
      localStreamRef.current?.addTrack(sendTrack)
      await callRef.current?.replaceAudioTrack(sendTrack)
      if (localStreamRef.current) levelsRef.current?.watch('me', localStreamRef.current)
      setMicId(devId)
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
  async function applyBackground(
    mode: 'none' | 'blur' | 'image',
    imageUrl?: string,
    blur: 'light' | 'strong' = blurLevel,
  ) {
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
      effect.blurPx = blur === 'light' ? 10 : 24
      setBlurLevel(blur)
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
    const isSfu = topology === 'sfu'
    if (sharing) {
      if (isSfu) {
        // SFU: o ecrã era uma track adicional — basta removê-la.
        presentation?.stream.getTracks().forEach((t) => t.stop())
        await callRef.current?.stopScreen()
        setPresentation((p) => (p?.peerId === 'me' ? null : p))
      } else {
        // Mesh: o ecrã substituiu a câmara — repor a track.
        const back = (bgMode !== 'none' && effectRef.current?.output) || cameraTrackRef.current
        if (back) await callRef.current?.replaceVideoTrack(back)
        if (localVideo.current && localStreamRef.current) {
          localVideo.current.srcObject =
            bgMode !== 'none' && back !== cameraTrackRef.current ? new MediaStream([back!]) : localStreamRef.current
        }
      }
      setSharing(false)
      return
    }
    try {
      const display = await navigator.mediaDevices.getDisplayMedia(SCREEN_CONSTRAINTS)
      const screenTrack = display.getVideoTracks()[0]
      screenTrack.contentHint = 'detail' // privilegiar nitidez de texto sobre framerate
      if (isSfu) {
        // Track separada: a câmara continua; todos (e a gravação) recebem
        // o ecrã como "apresentação" própria.
        await callRef.current?.startScreen(screenTrack, display)
        setPresentation({ peerId: 'me', stream: display })
      } else {
        await callRef.current?.replaceVideoTrack(screenTrack)
        if (localVideo.current) localVideo.current.srcObject = display
      }
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
      setRecNotice('Começaste a gravar a reunião')
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

  const mentionSuggestions = mentionQuery !== null
    ? peers.map((p) => p.username).filter((n) => n.toLowerCase().startsWith(mentionQuery)).slice(0, 5)
    : []

  // Pesquisa de membros da org para o convite.
  useEffect(() => {
    if (inviteQuery.trim().length < 2) { setInviteResults([]); return }
    const t = setTimeout(() => void searchUsers(inviteQuery).then(setInviteResults).catch(() => {}), 250)
    return () => clearTimeout(t)
  }, [inviteQuery])

  async function sendInvites() {
    if (inviteSelected.length === 0 || inviteBusy) return
    setInviteBusy(true)
    try {
      const { ringing, offline } = await inviteToRoom(code, inviteSelected.map((u) => u.id))
      setInviteStatus(`A chamar ${ringing.length} pessoa(s)…${offline.length > 0 ? ` (${offline.length} offline)` : ''}`)
      setInviteSelected([])
      setInviteQuery('')
      setTimeout(() => { setInviteOpen(false); setInviteStatus('') }, 2500)
    } catch {
      setInviteStatus('Erro ao convidar. Tenta novamente.')
    } finally {
      setInviteBusy(false)
    }
  }

  function completeMention(name: string) {
    const at = chatInput.lastIndexOf('@')
    if (at === -1) return
    const completed = chatInput.slice(0, at) + '@' + name + ' '
    setChatInput(completed)
    setMentionQuery(null)
    chatInputRef.current?.focus()
  }

  function sendChat() {
    const text = chatInput.trim()
    if (!text) return
    // Com o chat fechado, o servidor recusa (R92) — a mensagem seria engolida
    // em silêncio e o eco local abaixo faria a pessoa acreditar que tinha
    // enviado. Recusar aqui é sobre DIZER-LHE, não sobre segurança.
    if (!chatOn && !isHost) {
      setStatus('O anfitrião fechou o chat')
      return
    }
    signalRef.current?.send({ type: 'chat', text })
    setChat((c) => [...c, { username: 'eu', text, own: true }])
    setChatInput('')
    setMentionQuery(null)
  }

  function admit(peerId: string, ok: boolean) {
    signalRef.current?.send({ type: ok ? 'admit' : 'deny', to: peerId })
    setWaitingQueue((q) => q.filter((p) => p.peer_id !== peerId))
  }

  // Preferências de vista: esconder o meu tile e/ou participantes sem vídeo.
  const filteredPeers = hideNoVideo
    ? peers.filter((p) => !!p.stream?.getVideoTracks().length && p.camOn)
    : peers
  // Paginação: acima de TILES_PER_PAGE ninguém consegue ver (nem o browser
  // descodificar) mais tiles com proveito. O áudio NÃO é paginado — vive no
  // AudioSink e ouve-se toda a gente esteja em que página estiver.
  const pageCount = Math.max(1, Math.ceil(filteredPeers.length / TILES_PER_PAGE))
  const page = Math.min(gridPage, pageCount - 1)
  const visiblePeers =
    filteredPeers.length > TILES_PER_PAGE
      ? filteredPeers.slice(page * TILES_PER_PAGE, page * TILES_PER_PAGE + TILES_PER_PAGE)
      : filteredPeers
  const showSelf = !hideSelf
  const total = visiblePeers.length + (showSelf ? 1 : 0)
  const tileSize = useGridLayout(videoAreaRef, total, roomState === 'in')
  const meSpeaking = speaking.has('me') && micOn
  const anyoneRecording = recording || !!remoteRecorder

  // Conferência: o orador ativo (a falar) vai para o palco — inclui-me a MIM
  // (spotlight estilo Meet). Se um remoto fala, é ele; senão se eu falo, sou eu;
  // senão fica o primeiro participante, ou eu próprio se estou sozinho.
  // Pin tem prioridade: fixa o tile escolhido no palco (não troca com o orador).
  const pinnedPeer = pinnedId && pinnedId !== 'me' ? peers.find((p) => p.peerId === pinnedId) ?? null : null
  const pinnedSelf = pinnedId === 'me'
  const remoteSpeaker = pinnedPeer ?? (pinnedSelf ? null : peers.find((p) => speaking.has(p.peerId)) ?? null)
  const stagePeer = pinnedPeer ?? remoteSpeaker ?? peers[0] ?? null
  // Palco em mim: fixei-me, ou sou o orador ativo, ou não há mais ninguém.
  const stageOnSelf = pinnedSelf || (!pinnedPeer && !remoteSpeaker && (speaking.has('me') || peers.length === 0))
  // Com pin ativo, força o modo palco (é o efeito de "não trocar toda a hora").
  const effectiveViewMode: 'grid' | 'stage' = pinnedId ? 'stage' : viewMode
  // Transformação 3D (parallax) aplicada à área de vídeo. O scale(1.14) é
  // overscan: mantém a área sempre maior que a moldura, por isso a rotação/
  // deslocamento nunca revela o fundo escuro nos cantos. Deslocamento em %
  // (relativo ao próprio tamanho) para acompanhar o overscan.
  const parallaxStyle: CSSProperties = parallax
    ? {
        // Ângulos e deslocamento maiores (4→7deg, 1.6→2.8%) para o efeito ser
        // claramente percetível; o overscan sobe em conjunto (1.14→1.2) para a
        // rotação continuar a não revelar o fundo nos cantos.
        transform: `perspective(1400px) rotateY(${tilt.x * 7}deg) rotateX(${tilt.y * -7}deg) scale(1.2) translate(${tilt.x * 2.8}%, ${tilt.y * 2.8}%)`,
        transformOrigin: 'center center',
        transition: 'transform 0.12s cubic-bezier(0.22, 0.61, 0.36, 1)',
        willChange: 'transform',
      }
    : {}

  // De quem precisamos MESMO de vídeo agora. Vai ao SFU (`video-interest`),
  // que deixa de enviar o resto — é aqui que a paginação poupa banda a sério,
  // e não apenas ciclos de decoder. Inclui sempre quem está no palco e quem
  // está fixado, mesmo que caiam fora da página atual.
  const videoInterest = useMemo(() => {
    // Sem paginação ativa o interesse é TODA a gente. Isto é essencial: se a
    // sala encolher abaixo do limiar e deixássemos simplesmente de enviar, o
    // servidor ficaria com a última página em memória e quem estivesse fora
    // dela nunca mais receberia vídeo.
    const source = filteredPeers.length > TILES_PER_PAGE ? visiblePeers : filteredPeers
    const ids = new Set(source.map((p) => p.peerId))
    if (stagePeer) ids.add(stagePeer.peerId)
    if (pinnedPeer) ids.add(pinnedPeer.peerId)
    return [...ids].sort()
  }, [visiblePeers, filteredPeers, stagePeer, pinnedPeer])

  // QUE qualidade pedir de cada um. A decisão vive em `layerPolicy.ts` (pura e
  // testada); aqui só se recolhem os sinais que só o cliente conhece — o
  // tamanho a que cada tile está MESMO a ser desenhado, quem está em palco ou
  // fixado, e quem está a partilhar ecrã.
  //
  // Antes disto o servidor adivinhava a camada pelo NÚMERO DE PARTICIPANTES:
  // numa sala de doze, o orador em palco a ocupar 70% do ecrã recebia `q`.
  const videoQuality = useMemo(() => {
    const stageW = Math.round(window.innerWidth * 0.7)
    const tiles: TileSignal[] = videoInterest.map((peerId) => {
      const emPalco = effectiveViewMode === 'stage' && stagePeer?.peerId === peerId
      return {
        peerId,
        // Em palco o tile ocupa a área toda; na grelha vale a largura medida
        // pelo `useGridLayout`, que é a largura real em px de CSS.
        widthPx: emPalco ? stageW : tileSize.w,
        pinned: pinnedPeer?.peerId === peerId,
        onStage: emPalco,
        speaking: speaking.has(peerId),
        presenting: presentation?.peerId === peerId,
      }
    })
    return chooseLayers(tiles, conditions)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [videoInterest.join(','), tileSize.w, effectiveViewMode, stagePeer?.peerId, pinnedPeer?.peerId, presentation?.peerId, speaking, conditions])

  useEffect(() => {
    if (roomState !== 'in' || topology !== 'sfu') return
    // R23 continua válido para o INTERESSE: assim que uma sala pagina uma vez,
    // passa a informar sempre — inclusive quando encolhe e deixa de paginar,
    // senão o servidor ficava preso na última página.
    if (filteredPeers.length > TILES_PER_PAGE) paginatedOnceRef.current = true
    // Mas a QUALIDADE informa-se desde o início, pagine-se ou não: numa sala de
    // três pessoas o servidor servia `f` a toda a gente, miniaturas incluídas, e
    // é precisamente aí que a sugestão poupa banda. A lista de `peers` quando
    // não se pagina é «toda a gente», que é o default do servidor — por isso
    // enviar sempre é um superconjunto seguro do comportamento antigo.
    signalRef.current?.send({ type: 'video-interest', peers: videoInterest, quality: videoQuality })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [videoInterest.join(','), JSON.stringify(videoQuality), roomState, topology, filteredPeers.length])

  const talkOverNames = useMemo(() => {
    if (!talkOver) return ''
    const names = [...speaking].map((id) => (id === 'me' ? 'tu' : peers.find((p) => p.peerId === id)?.username ?? '')).filter(Boolean)
    return names.join(' e ')
  }, [talkOver, speaking, peers])

  // ---- Pre-join (green room): preview + dispositivos ANTES de entrar. ----
  if (roomState === 'prejoin') {
    const me = currentUser()?.username ?? 'eu'
    return (
      <div className="room-page prejoin-page">
        <div className="prejoin-card">
          <h2 className="prejoin-title">Pronto para entrar?</h2>
          <p className="prejoin-code">Sala <code>{code}</code></p>
          <div className="prejoin-preview">
            {hasLocalVideo && camOn ? (
              <video
                // Callback ref: o <video> monta DEPOIS do stream chegar (hasLocalVideo)
                // — liga o preview aqui, qualquer que seja a ordem (cf. attachLocalVideo).
                ref={(node) => {
                  prejoinVideoRef.current = node
                  if (node && !node.srcObject) node.srcObject = previewStreamRef.current
                }}
                autoPlay
                playsInline
                muted
                className="prejoin-video"
              />
            ) : (
              <div className="prejoin-avatar">
                <span className="tile-avatar" style={{ background: peerColor(me) }}>
                  {me.slice(0, 1).toUpperCase()}
                </span>
                <small>{hasLocalVideo ? 'Câmara desligada' : 'Sem câmara'}</small>
              </div>
            )}
            {micOn && speaking.has('me') && <span className="prejoin-mic-live" title="O microfone está a captar-te"><MicIcon /></span>}
            <div className="prejoin-toggles">
              <button
                className={micOn ? 'ctrl' : 'ctrl off'}
                onClick={() => prejoinToggle('mic')}
                title={micOn ? 'Desligar microfone' : 'Ligar microfone'}
                aria-pressed={!micOn}
              >
                {micOn ? <MicIcon /> : <MicOffIcon />}
              </button>
              <button
                className={camOn && hasLocalVideo ? 'ctrl' : 'ctrl off'}
                onClick={() => prejoinToggle('cam')}
                title={camOn ? 'Desligar câmara' : 'Ligar câmara'}
                aria-pressed={!camOn}
              >
                {camOn && hasLocalVideo ? <CamIcon /> : <CamOffIcon />}
              </button>
            </div>
          </div>

          <div className="prejoin-devices">
            <label className="dev-chip">
              <MicIcon />
              <select value={micId} onChange={(e) => void prejoinSwitch('mic', e.target.value)}>
                {devices.mics.length === 0 && <option value="">Microfone</option>}
                {devices.mics.map((d) => (
                  <option key={d.deviceId} value={d.deviceId}>{d.label || 'Microfone'}</option>
                ))}
              </select>
            </label>
            <label className="dev-chip">
              <CamIcon />
              <select value={camId} onChange={(e) => void prejoinSwitch('cam', e.target.value)}>
                {devices.cams.length === 0 && <option value="">Câmara</option>}
                {devices.cams.map((d) => (
                  <option key={d.deviceId} value={d.deviceId}>{d.label || 'Câmara'}</option>
                ))}
              </select>
            </label>
            <button className="dev-chip icon" title="Testar os altifalantes" onClick={() => void playTestTone(speakerId)}>
              <SpeakerIcon /> Testar som
            </button>
          </div>

          {status && <p className="prejoin-status">{status}</p>}

          <div className="prejoin-actions">
            <button className="btn-ghost small" onClick={() => onLeave()}>Cancelar</button>
            <div className="prejoin-actions-right">
              {/* Vista de gestão do anfitrião; convidados são reencaminhados de volta. */}
              <button className="btn-ghost small" onClick={() => (location.hash = `/lobby/${code}`)}>
                Sala de espera
              </button>
              <button className="prejoin-join" onClick={prejoinJoin}>
                Entrar agora
              </button>
            </div>
          </div>
        </div>
      </div>
    )
  }

  if (roomState === 'e2ee-pass') {
    return (
      <div className="waiting-page">
        <h2><LockIcon /> Reunião encriptada de ponta a ponta</h2>
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
      {/* Barra de topo estilo Meet: info à esquerda, alertas/participantes à direita. */}
      <header className="room-topbar">
        <div className="rt-left">
          <WallClock />
          <span className="rt-sep">|</span>
          <span className="rt-code" title="Código da reunião">{code}</span>
          <button
            className="rt-info"
            title="Detalhes da reunião"
            aria-label="Detalhes da reunião"
            onClick={() => setPanel(panel === 'people' ? 'none' : 'people')}
          ><InfoIcon /></button>
          {isInstant && (
            <span className="rt-chip instant" title="Chamada instantânea — sala virtual; só a gravação é guardada">
              <ClockIcon /> Instantânea
            </span>
          )}
          {isTraining && <span className="rt-chip">Formação</span>}
          {e2eeOn && (
            <button className="rt-chip e2ee" title="Encriptação de ponta a ponta ativa" onClick={() => setSecOpen((v) => !v)}>
              <LockIcon /> E2EE
            </button>
          )}
        </div>
        <div className="rt-right">
          {canAdmit && waitingQueue.length > 0 && (
            <button
              className="waiting-pill"
              onClick={() => setPanel(panel === 'people' ? 'none' : 'people')}
              title="Convidados à espera de admissão"
            >
              <PeopleIcon />
              {waitingQueue.length} {waitingQueue.length === 1 ? 'convidado a aguardar' : 'convidados a aguardar'}
            </button>
          )}
          <button
            className="rt-count"
            onClick={() => setPanel(panel === 'people' ? 'none' : 'people')}
            title="Participantes"
            aria-label={`${total} participantes`}
          >
            <span className="rt-avatar" aria-hidden>{(currentUser()?.username ?? '?').slice(0, 1).toUpperCase()}</span>
            <span className="rt-count-n">{total}</span>
          </button>
        </div>
      </header>
      <div className="room-body">
        {roomState === 'waiting' && (
          <div className="waiting-overlay">
            <div className="spinner" />
            <h2>À espera que o anfitrião te deixe entrar…</h2>
            <p className="muted">Podes preparar a câmara e o microfone entretanto.</p>
          </div>
        )}

        {/* Áudio de TODOS os participantes, independente do que a grelha
            mostra (ver AudioSink). Fora do `video-area` de propósito. */}
        <AudioSink peers={peers} sinkId={speakerId} />

        <div className="video-area" ref={videoAreaRef}>
        {(() => {
          // Dimensões explícitas só na grelha multi-tile; solo e palco usam CSS.
          const isSolo = total === 1
          const tileStyle: CSSProperties | undefined =
            effectiveViewMode === 'stage' || presentation || isSolo ? undefined : { width: tileSize.w, height: tileSize.h }
          // A câmara aparece quando há vídeo local e está ligada. Em mesh a
          // partilhar, o próprio <video> mostra o ecrã (substitui a câmara); no
          // SFU o ecrã é um tile à parte, por isso aqui mostra-se a câmara ou,
          // se não houver, o avatar (nunca um retângulo preto).
          const showSelfVideo = (hasLocalVideo && camOn) || (sharing && topology !== 'sfu')
          const selfTile = (
            <div
              className={meSpeaking ? 'tile speaking' : 'tile'}
              data-peer="local"
              style={tileStyle}
              onDoubleClick={() => togglePin('me')}
              title="Duplo-clique para fixar/desafixar no palco"
            >
              <video
                ref={attachLocalVideo}
                autoPlay
                muted
                playsInline
                className={hasLocalVideo && !sharing && bgMode === 'none' ? 'mirror' : undefined}
                style={{ display: showSelfVideo ? undefined : 'none' }}
              />
              {!showSelfVideo && (
                <div className="tile-avatar" style={{ background: peerColor(currentUser()?.username ?? 'eu') }}>
                  <span className="avatar-circle" style={{ background: 'rgba(0,0,0,0.25)' }}>EU</span>
                </div>
              )}
              <button
                className={pinnedId === 'me' ? 'tile-pin pinned' : 'tile-pin'}
                onClick={() => togglePin('me')}
                title={pinnedId === 'me' ? 'Desafixar do palco' : 'Fixar no palco'}
              >
                <PinIcon />
              </button>
              {handRaised && <span className="hand-badge"><HandIcon /></span>}
              {/* Indicador de mic muted no canto superior direito (estilo Meet). */}
              {!micOn && (
                <span className="tile-mic-status" aria-label="microfone desativado">
                  <MicOffIcon />
                </span>
              )}
              <span className="tile-name">
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
              speaking={speaking.has(p.peerId)}
              style={tileStyle}
              pinned={pinnedId === p.peerId}
              onPin={onTilePin}
              onMute={onTileMute}
              onKick={onTileKick}
            />
          )

          // Apresentação (partilha de ecrã em track separada): palco com o
          // ecrã em grande e a plateia (câmaras) numa fila por baixo.
          if (presentation) {
            const presenter =
              presentation.peerId === 'me' ? 'Estás a apresentar' :
              `${peers.find((p) => p.peerId === presentation.peerId)?.username ?? '?'} está a apresentar`
            // Não me mostro a mim na plateia quando SOU eu a apresentar (o meu
            // ecrã já é o conteúdo principal — a minha câmara seria redundante).
            const audienceSelf = !hideSelf && presentation.peerId !== 'me'
            return (
              <div className={presLayout === 'side' ? 'stage-wrap pres-side' : 'stage-wrap'} style={parallaxStyle}>
                <div className="stage-main">
                  <PresentationTile
                    stream={presentation.stream}
                    label={presenter}
                    own={presentation.peerId === 'me'}
                    onRequestControl={() => signalRef.current?.send({ type: 'remote-control', to: presentation.peerId, action: 'request', payload: null })}
                  />
                  <button
                    className="pres-layout-btn"
                    onClick={() => setPresLayout((l) => (l === 'bottom' ? 'side' : 'bottom'))}
                    title={presLayout === 'bottom' ? 'Participantes na lateral' : 'Participantes em baixo'}
                  >
                    {presLayout === 'bottom' ? '⧉ Lateral' : '⧉ Em baixo'}
                  </button>
                </div>
                <div className="stage-audience">
                  {audienceSelf && selfTile}
                  {visiblePeers.map(remoteTile)}
                </div>
              </div>
            )
          }

          if (effectiveViewMode === 'stage' && (stageOnSelf || stagePeer)) {
            // Palco em mim (sou o orador ou estou sozinho): a minha câmara em
            // grande, os restantes na plateia. Senão, o orador remoto no palco.
            if (stageOnSelf || !stagePeer) {
              return (
                <div className="stage-wrap" style={parallaxStyle}>
                  <div className="stage-main">{selfTile}</div>
                  <div className="stage-audience">{visiblePeers.map(remoteTile)}</div>
                </div>
              )
            }
            const audience = visiblePeers.filter((p) => p.peerId !== stagePeer.peerId)
            return (
              <div className="stage-wrap" style={parallaxStyle}>
                <div className="stage-main">{remoteTile(stagePeer)}</div>
                <div className="stage-audience">
                  {showSelf && selfTile}
                  {audience.map(remoteTile)}
                </div>
              </div>
            )
          }
          return (
            <>
              <div
                className={isSolo ? 'video-grid video-grid--solo' : 'video-grid'}
                style={isSolo ? parallaxStyle : {
                  ['--tw' as string]: `${tileSize.w}px`,
                  ['--th' as string]: `${tileSize.h}px`,
                  ...parallaxStyle,
                }}
              >
                {showSelf && selfTile}
                {visiblePeers.map(remoteTile)}
              </div>
              {pageCount > 1 && (
                <div className="grid-pager" role="navigation" aria-label="Páginas de participantes">
                  <button
                    onClick={() => setGridPage((p) => Math.max(0, p - 1))}
                    disabled={page === 0}
                    title="Página anterior"
                    aria-label="Página anterior"
                  >
                    ‹
                  </button>
                  <span className="mono">
                    {page + 1} / {pageCount}
                  </span>
                  <button
                    onClick={() => setGridPage((p) => Math.min(pageCount - 1, p + 1))}
                    disabled={page >= pageCount - 1}
                    title="Página seguinte"
                    aria-label="Página seguinte"
                  >
                    ›
                  </button>
                  <small className="muted">
                    {filteredPeers.length} participantes · ouves todos
                  </small>
                </div>
              )}
            </>
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

        {ccOn && caption && (
          <div className="cc-overlay" aria-live="polite">
            {caption.text}
          </div>
        )}

        {/* Festa de quem acerta no quiz: taça + fogo de artifício (~4.5s). */}
        {winnerFx && (
          <div className="winner-overlay" aria-hidden>
            {Array.from({ length: 14 }, (_, i) => (
              <span key={i} className={`fw p${i % 7}`} style={{ animationDelay: `${(i % 5) * 0.25}s` }} />
            ))}
            <div className="winner-card">
              <span className="winner-trophy"><TrophyIcon /></span>
              <strong>Acertaste!</strong>
            </div>
          </div>
        )}

        {wbOpen && (
          <Whiteboard
            strokes={wbStrokes}
            onStroke={(stroke) => {
              setWbStrokes((st) => [...st, stroke])
              signalRef.current?.send({ type: 'wb-stroke', stroke })
            }}
            onClear={() => {
              setWbStrokes([])
              signalRef.current?.send({ type: 'wb-clear' })
            }}
            onSave={async (pngBase64) => {
              try {
                await saveWhiteboard(`Quadro · ${code}`, code, pngBase64)
                setStatus('Quadro guardado na biblioteca')
              } catch {
                setStatus('Não foi possível guardar o quadro')
              }
            }}
            onClose={() => { setWbOpen(false); signalRef.current?.send({ type: 'wb-close' }) }}
          />
        )}

        {/* Barra de dispositivos flutuante ao fundo do vídeo (estilo Google Meet):
            o chevron do mic mostra mic+altifalante; o da câmara mostra câmara+fundo. */}
        {deviceMenu !== 'none' && (
          <div className="device-chips">
            {deviceMenu === 'mic' ? (
              <>
                <label className="dev-chip">
                  <MicIcon />
                  <select value={micId} onChange={(e) => void switchMic(e.target.value)}>
                    {devices.mics.length === 0 && <option>Microfone</option>}
                    {devices.mics.map((d) => (
                      <option key={d.deviceId} value={d.deviceId}>{d.label || 'Microfone'}</option>
                    ))}
                  </select>
                </label>
                <label className="dev-chip">
                  <span className="dev-chip-emoji"><SpeakerIcon /></span>
                  <select value={speakerId} onChange={(e) => setSpeakerId(e.target.value)}>
                    <option value="">Predefinido do sistema</option>
                    {devices.speakers.map((d) => (
                      <option key={d.deviceId} value={d.deviceId}>{d.label || 'Altifalante'}</option>
                    ))}
                  </select>
                </label>
                <button className="dev-chip icon" title="Testar os altifalantes" onClick={() => void playTestTone(speakerId)}>
                  <SpeakerIcon />
                </button>
                <button
                  className="dev-chip icon"
                  title="Definições"
                  onClick={() => { setPanel('settings'); setDeviceMenu('none') }}
                >
                  <SettingsIcon />
                </button>
              </>
            ) : (
              <>
                <label className="dev-chip">
                  <CamIcon />
                  <select value={camId} onChange={(e) => void switchCam(e.target.value)}>
                    {devices.cams.length === 0 && <option>Câmara</option>}
                    {devices.cams.map((d) => (
                      <option key={d.deviceId} value={d.deviceId}>{d.label || 'Câmara'}</option>
                    ))}
                  </select>
                </label>
                <button
                  className={bgMode === 'blur' ? 'dev-chip toggle on' : 'dev-chip toggle'}
                  disabled={bgBusy || !hasLocalVideo}
                  onClick={() => void applyBackground(bgMode === 'blur' ? 'none' : 'blur')}
                >
                  {bgMode === 'blur' ? '✓ ' : ''}Esbater fundo
                </button>
                <button className="dev-chip" onClick={() => { setFxOpen(true); setDeviceMenu('none') }}>
                  Fundos e efeitos
                </button>
                <button
                  className="dev-chip icon"
                  title="Definições"
                  onClick={() => { setPanel('settings'); setDeviceMenu('none') }}
                >
                  <SettingsIcon />
                </button>
              </>
            )}
          </div>
        )}

        {/* Cartão "reunião pronta" (link de convite): SÓ para o anfitrião —
            um convidado não é dono da reunião e não deve ser convidado a
            partilhar o link como se a tivesse criado. */}
        {readyOpen && isHost && roomState === 'in' && (
          <div className="ready-card">
            <div className="ready-head">
              <h3>A tua reunião está pronta.</h3>
              <button
                className="panel-close"
                onClick={dispensarReady}
              >
                <CloseIcon />
              </button>
            </div>
            <button
              className="ready-add-btn"
              onClick={() => {
                setPanel('people')
                dispensarReady()
              }}
            >
              <PeopleIcon /> Adicionar participantes
            </button>
            <p className="muted small ready-or">Ou partilhe este link da reunião com as outras pessoas que quer incluir na reunião.</p>
            <div className="ready-link">
              <span className="mono">{`${location.host}/#/r/${code}`}</span>
              <button
                className="icon-btn"
                title="Copiar link"
                onClick={(e) => {
                  void navigator.clipboard.writeText(`${location.origin}/#/r/${code}`)
                  const el = e.currentTarget
                  el.textContent = '✓'
                  setTimeout(() => { el.textContent = '⧉' }, 1500)
                }}
              >
                ⧉
              </button>
            </div>
            <p className="muted small ready-note">
              🛡 {waitingRoomOn
                ? 'As pessoas que utilizarem este link terão de pedir autorização para participar.'
                : 'Quem tiver o link e sessão iniciada entra diretamente.'}
            </p>
            <p className="muted small">A participar como <strong>{currentUser()?.username ?? 'eu'}</strong></p>
          </div>
        )}

        {panel === 'chat' && (
          <aside className="side-panel">
            <div className="panel-head">
              <PanelTabs active={panel} onSelect={(p) => { setPanel(p); if (p === 'chat') setUnreadChat(0) }} unreadChat={unreadChat} total={total} />
              <button className="panel-close" onClick={() => setPanel('none')}><CloseIcon /></button>
            </div>
            <p className="chat-notice-bar"><ChatIcon /> Mensagens guardadas durante a reunião. Usa @ para mencionar alguém.</p>
            <div className="chat-messages">
              {chat.length === 0 && (
                <div className="chat-empty-notice">
                  <strong>Ainda sem mensagens</strong>
                  <p>As mensagens ficam visíveis durante a reunião. Quem entrar depois não as vê.</p>
                </div>
              )}
              {chat.map((m, i) => (
                <>
                  {!m.historical && i > 0 && chat[i - 1].historical && (
                    <div key={`div-${i}`} className="chat-history-divider">— início desta sessão —</div>
                  )}
                  <div key={i} className={m.own ? 'chat-msg own' : 'chat-msg'}>
                    <strong>{m.username}</strong> <ChatText text={m.text} />
                  </div>
                </>
              ))}
            </div>
            <div className="chat-compose">
              {mentionSuggestions.length > 0 && (
                <div className="mention-suggestions">
                  {mentionSuggestions.map((name) => (
                    <button key={name} className="mention-item" onMouseDown={(e) => { e.preventDefault(); completeMention(name) }}>
                      @{name}
                    </button>
                  ))}
                </div>
              )}
              {chatEmojiOpen && (
                <div className="chat-emoji-picker">
                  {CHAT_EMOJIS.map((e) => (
                    <button
                      key={e}
                      onClick={() => { setChatInput((t) => t + e); setChatEmojiOpen(false) }}
                    >
                      {e}
                    </button>
                  ))}
                </div>
              )}
              <div className="chat-input">
                <button
                  className={chatEmojiOpen ? 'chat-emoji-btn active' : 'chat-emoji-btn'}
                  title="Emojis"
                  onClick={() => setChatEmojiOpen((v) => !v)}
                >
                  <EmojiIcon />
                </button>
                <input
                  ref={chatInputRef}
                  value={chatInput}
                  onChange={(e) => {
                    const val = e.target.value
                    setChatInput(val)
                    // Detecta @menção: palavra após @ sem espaços.
                    const at = val.lastIndexOf('@')
                    if (at !== -1 && !val.slice(at + 1).includes(' ')) {
                      setMentionQuery(val.slice(at + 1).toLowerCase())
                    } else {
                      setMentionQuery(null)
                    }
                  }}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') { sendChat(); setChatEmojiOpen(false); setMentionQuery(null) }
                    if (e.key === 'Escape') { setChatEmojiOpen(false); setMentionQuery(null) }
                    if (e.key === 'Tab' && mentionSuggestions.length > 0) {
                      e.preventDefault()
                      completeMention(mentionSuggestions[0])
                    }
                  }}
                  placeholder="Escreve uma mensagem…"
                />
                <button className="chat-send-btn" onClick={() => { sendChat(); setChatEmojiOpen(false) }} title="Enviar (Enter)"><SendIcon /></button>
              </div>
            </div>
          </aside>
        )}

        {panel === 'people' && (
          <aside className="side-panel">
            <div className="panel-head">
              <PanelTabs active={panel} onSelect={(p) => { setPanel(p); if (p === 'chat') setUnreadChat(0) }} unreadChat={unreadChat} total={total} />
              <button className="panel-close" onClick={() => setPanel('none')}><CloseIcon /></button>
            </div>
            <button
              className="btn-sm invite-btn"
              onClick={() => { setInviteOpen(true); setInviteQuery(''); setInviteSelected([]); setInviteStatus('') }}
            >
              + Convidar membros
            </button>
            {inviteOpen && (
              <div className="invite-modal">
                <div className="invite-modal-head">
                  <span>Convidar para a reunião</span>
                  <button className="panel-close" onClick={() => setInviteOpen(false)}><CloseIcon /></button>
                </div>
                <input
                  autoFocus
                  placeholder="Pesquisar por nome ou email…"
                  value={inviteQuery}
                  onChange={(e) => setInviteQuery(e.target.value)}
                />
                {inviteResults.length > 0 && (
                  <div className="invite-member-list">
                    {inviteResults
                      .filter((u) => !inviteSelected.some((s) => s.id === u.id) && u.id !== currentUser()?.id)
                      .map((u) => (
                        <button
                          key={u.id}
                          className="invite-member-item"
                          onClick={() => { setInviteSelected((prev) => [...prev, u]); setInviteQuery('') }}
                        >
                          <span className="avatar-circle small">{u.username.slice(0, 2).toUpperCase()}</span>
                          <span>
                            <strong>{u.username}</strong>
                            <small>{u.email}</small>
                          </span>
                        </button>
                    ))}
                  </div>
                )}
                {inviteSelected.length > 0 && (
                  <div className="invite-selected">
                    {inviteSelected.map((u) => (
                      <span key={u.id} className="invite-chip">
                        {u.username}
                        <button onClick={() => setInviteSelected((prev) => prev.filter((s) => s.id !== u.id))}>×</button>
                      </span>
                    ))}
                  </div>
                )}
                {inviteStatus && <p className="invite-status">{inviteStatus}</p>}
                <button
                  className="btn-sm"
                  disabled={inviteSelected.length === 0 || inviteBusy}
                  onClick={() => void sendInvites()}
                >
                  {inviteBusy ? 'A chamar…' : `Chamar ${inviteSelected.length > 0 ? `(${inviteSelected.length})` : ''}`}
                </button>
              </div>
            )}
            {isHost && (
              // Acções sobre a SALA INTEIRA (R92). Ficam por cima da lista
              // porque se aplicam a toda a gente e não a uma linha — pô-las
              // dentro da lista faria parecer que agem sobre a pessoa ao lado.
              <div className="people-host-actions">
                <button
                  className="ghost-btn"
                  title="Silenciar toda a gente. Cada pessoa pode voltar a ligar-se."
                  onClick={() => onMuteAll(true)}
                >
                  <MicOffIcon /> Silenciar todos
                </button>
                <button
                  className="ghost-btn"
                  title="Silenciar toda a gente e impedir que se voltem a ligar sozinhos."
                  onClick={() => onMuteAll(false)}
                >
                  <MicOffIcon /> …e não deixar voltar a ligar
                </button>
                <button
                  className="ghost-btn"
                  title={chatOn ? 'Fechar o chat a quem não é anfitrião' : 'Reabrir o chat a toda a gente'}
                  onClick={() => onChatToggle(!chatOn)}
                >
                  <ChatIcon /> {chatOn ? 'Fechar o chat' : 'Reabrir o chat'}
                </button>
              </div>
            )}
            <div className="people-search">
              <span className="people-search-icon"><SearchIcon /></span>
              <input
                type="text"
                placeholder="Pesquisar participantes…"
                value={peopleSearch}
                onChange={(e) => setPeopleSearch(e.target.value)}
                autoComplete="off"
              />
            </div>
            <div className="people-list">
              <div className="person-row">
                <span className="avatar-circle small" style={{ background: peerColor(currentUser()?.username ?? 'eu') }}>EU</span>
                <span className="person-name">
                  <span className="pn-name">eu{isHost ? ' · anfitrião' : ''}</span>
                  {qos && (
                    <small className="qos-line mono" title={`Delonix Call Quality Score: ${qos.score}/100${qos.turnRelay ? ' · via TURN relay' : ''}${qos.limitedBy === 'cpu' ? ' · encoder travado por CPU' : ''}`}>
                      {qos.score}/100 · ↑ {qos.upKbps} kbps
                      {qos.rttMs != null ? ` · RTT ${qos.rttMs} ms` : ''}
                      {qos.turnRelay ? ' · relay' : ''}
                    </small>
                  )}
                </span>
                {micOn ? (meSpeaking ? <SpeakingBars /> : <MicIcon />) : <span className="danger-ic"><MicOffIcon /></span>}
              </div>
              {peers.filter((p) => !peopleSearch || p.username.toLowerCase().includes(peopleSearch.toLowerCase())).map((p) => (
                <div key={p.peerId} className="person-row">
                  <span className="avatar-circle small" style={{ background: peerColor(p.username) }}>{p.username.slice(0, 2).toUpperCase()}</span>
                  <span className="person-name">
                    <span className="pn-name">
                      {p.username}
                      {p.host ? ' · anfitrião' : p.canAdmit ? ' · admite entradas' : ''}
                      {p.is_pstn ? ' · 📞 PSTN' : p.is_bot ? ' · 🤖 AI Bot' : ''}
                    </span>
                    {qos?.byPeer[p.peerId] && (
                      <small className={qos.byPeer[p.peerId].lossPct > 5 ? 'qos-line mono qos-bad' : 'qos-line mono'}>
                        ↓ {qos.byPeer[p.peerId].kbps} kbps · perda {qos.byPeer[p.peerId].lossPct}%
                        {qos.byPeer[p.peerId].jitterMs > 30 ? ` · jitter ${qos.byPeer[p.peerId].jitterMs} ms` : ''}
                        {qos.byPeer[p.peerId].freezeMs > 0 ? ` · ${Math.round(qos.byPeer[p.peerId].freezeMs)} ms congelado` : ''}
                      </small>
                    )}
                  </span>
                  {speaking.has(p.peerId) && <SpeakingBars />}
                  {isHost && !p.host && (
                    <button
                      className="share-grant-btn"
                      title="Desligar a câmara desta pessoa"
                      onClick={() => onTileCam(p.peerId)}
                    >
                      <CamOffIcon />
                    </button>
                  )}
                  {isHost && !p.host && (
                    <button
                      className="share-grant-btn"
                      title="Passar o papel de anfitrião a esta pessoa"
                      onClick={() => onTransferHost(p.peerId)}
                    >
                      <TrophyIcon />
                    </button>
                  )}
                  {isHost && !p.host && (
                    <button
                      className="share-grant-btn"
                      title={p.canAdmit ? 'Revogar poder de admitir entradas' : 'Permitir que admita convidados da sala de espera'}
                      onClick={() =>
                        signalRef.current?.send({ type: 'promote-admit', to: p.peerId, allowed: !p.canAdmit })
                      }
                    >
                      <ShieldIcon />{p.canAdmit ? <CheckIcon /> : <PlusIcon />}
                    </button>
                  )}
                  {isHost && !p.host && (
                    <button
                      className="share-grant-btn"
                      title={sharePerms.has(p.peerId) ? 'Revogar permissão de partilha' : 'Permitir que partilhe ecrã'}
                      onClick={() => {
                        const next = !sharePerms.has(p.peerId)
                        setSharePerms((s) => {
                          const n = new Set(s)
                          if (next) n.add(p.peerId)
                          else n.delete(p.peerId)
                          return n
                        })
                        signalRef.current?.send({ type: 'share-grant', to: p.peerId, allowed: next })
                      }}
                    >
                      <ShareIcon />{sharePerms.has(p.peerId) ? <CheckIcon /> : <PlusIcon />}
                    </button>
                  )}
                </div>
              ))}
            </div>
            {isHost && (
              <div className="host-controls-box">
                <h4>Controlos do anfitrião</h4>
                <label className="host-toggle">
                  <input
                    type="checkbox"
                    checked={roomLocked}
                    onChange={(e) => signalRef.current?.send({ type: 'room-lock', locked: e.target.checked })}
                  />
                  <span>
                    <strong>Bloquear reunião</strong>
                    <small>Ninguém entra sem ser admitido, mesmo com o link</small>
                  </span>
                </label>
                <label className="host-toggle">
                  <input
                    type="checkbox"
                    checked={hostShareOnly}
                    onChange={(e) => signalRef.current?.send({ type: 'host-share-only', on: e.target.checked })}
                  />
                  <span>
                    <strong>Só o anfitrião partilha ecrã</strong>
                    <small>Restringe a partilha de ecrã ao anfitrião</small>
                  </span>
                </label>
              </div>
            )}
            {isHost && isTraining && (
              <div className="breakout-box">
                <h4>Salas de grupo</h4>
                {breakoutRooms.length === 0 ? (
                  <>
                    <div className="breakout-create">
                      <span className="muted small">Dividir participantes em</span>
                      {[2, 3, 4].map((n) => (
                        <button
                          key={n}
                          onClick={() =>
                            signalRef.current?.send({
                              type: 'breakouts-create',
                              count: n,
                              minutes: breakoutMinutes || null,
                            })
                          }
                        >
                          {n}
                        </button>
                      ))}
                      <span className="muted small">grupos</span>
                    </div>
                    <label className="breakout-timer-row">
                      <span className="muted small">Duração:</span>
                      <select value={breakoutMinutes} onChange={(e) => setBreakoutMinutes(Number(e.target.value))}>
                        <option value={0}>sem limite</option>
                        {[5, 10, 15, 20, 30, 45, 60].map((m) => (
                          <option key={m} value={m}>{m} min</option>
                        ))}
                      </select>
                      <span className="muted small">(no fim, todos voltam à principal)</span>
                    </label>
                  </>
                ) : (
                  <>
                    {breakoutEndsAt && (
                      <p className="breakout-countdown">
                        <ClockIcon /> Termina em <Countdown endsAt={breakoutEndsAt} render={(txt) => <strong>{txt}</strong>} />
                      </p>
                    )}
                    {breakoutRooms.map((b) => (
                      <div key={b.code} className="breakout-group">
                        <div className="breakout-group-head">
                          <input
                            className="breakout-name"
                            defaultValue={b.label}
                            maxLength={60}
                            title="Renomear grupo (Enter para guardar)"
                            onKeyDown={(e) => {
                              if (e.key === 'Enter') (e.target as HTMLInputElement).blur()
                            }}
                            onBlur={(e) => {
                              const label = e.target.value.trim()
                              if (label && label !== b.label)
                                signalRef.current?.send({ type: 'breakout-rename', code: b.code, label })
                            }}
                          />
                          <button
                            className="btn-sm ghost"
                            title="Visitar este grupo"
                            onClick={() => {
                              sessionStorage.setItem(`dx_return_${b.code}`, code)
                              onSwitch?.(b.code)
                            }}
                          >
                            Visitar
                          </button>
                        </div>
                        {b.people.length === 0 && <p className="muted small">Vazio</p>}
                        {b.people.map((name) => (
                          <div key={name} className="breakout-person">
                            <span className="avatar-circle small">{name.slice(0, 2).toUpperCase()}</span>
                            <span className="breakout-person-name">{name}</span>
                            <select
                              className="breakout-move"
                              value={b.code}
                              title="Mover para…"
                              onChange={(e) =>
                                signalRef.current?.send({ type: 'breakout-move-user', name, code: e.target.value })
                              }
                            >
                              {breakoutRooms.map((o) => (
                                <option key={o.code} value={o.code}>{o.label}</option>
                              ))}
                              <option value={code}>&larr; Principal</option>
                            </select>
                          </div>
                        ))}
                      </div>
                    ))}
                    <div className="breakout-actions">
                      <button className="btn-sm ghost" onClick={() => signalRef.current?.send({ type: 'breakout-add' })}>
                        + Adicionar grupo
                      </button>
                      <button
                        className="admit-no breakout-close"
                        onClick={() => signalRef.current?.send({ type: 'breakouts-close' })}
                      >
                        Retornar todos à principal
                      </button>
                    </div>
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

        {panel === 'tools' && (
          <aside className="side-panel tools-panel">
            <div className="panel-head">
              <PanelTabs active={panel} onSelect={(p) => { setPanel(p); if (p === 'chat') setUnreadChat(0) }} unreadChat={unreadChat} total={total} />
              <button className="panel-close" onClick={() => setPanel('none')}><CloseIcon /></button>
            </div>

            <section className="tool-section">
              <h4><ClockIcon /> Temporizador</h4>
              {meetTimerEndsAt ? (
                <div className="timer-row">
                  <Countdown endsAt={meetTimerEndsAt} render={(txt) => <strong className="mono timer-big">{txt}</strong>} />
                  {isHost && (
                    <button className="btn-sm ghost" onClick={() => signalRef.current?.send({ type: 'timer-clear' })}>
                      Limpar
                    </button>
                  )}
                </div>
              ) : isHost ? (
                <div className="timer-presets">
                  {[5, 10, 15, 30, 60].map((m) => (
                    <Btn key={m} variant="ghost" onClick={() => signalRef.current?.send({ type: 'timer-set', minutes: m })}>
                      {m} min
                    </Btn>
                  ))}
                </div>
              ) : (
                <p className="muted small">O anfitrião pode definir um temporizador visível para todos.</p>
              )}
            </section>

            <section className="tool-section">
              <h4><ChartIcon /> Sondagens</h4>
              {isHost && (
                <div className="poll-create">
                  <input
                    placeholder="Pergunta…"
                    maxLength={200}
                    value={pollQ}
                    onChange={(e) => setPollQ(e.target.value)}
                  />
                  {pollOpts.map((o, i) => (
                    <div key={i} className="poll-opt-edit">
                      <input
                        placeholder={`Opção ${i + 1}`}
                        maxLength={80}
                        value={o}
                        onChange={(e) => setPollOpts(pollOpts.map((x, j) => (j === i ? e.target.value : x)))}
                      />
                      <label
                        className={pollCorrect === i ? 'poll-correct-pick on' : 'poll-correct-pick'}
                        title="Marcar como resposta certa (modo quiz)"
                      >
                        <input
                          type="radio"
                          name="poll-correct"
                          checked={pollCorrect === i}
                          onChange={() => setPollCorrect(i)}
                        />
                        <CheckIcon />
                      </label>
                    </div>
                  ))}
                  <div className="poll-create-actions">
                    {pollOpts.length < 6 && (
                      <Btn variant="ghost" onClick={() => setPollOpts([...pollOpts, ''])}>
                        + opção
                      </Btn>
                    )}
                    <SelectCtl
                      className="poll-dur"
                      title="Duração da votação (quiz com tempo)"
                      value={pollDur}
                      onChange={(e) => setPollDur(Number(e.target.value))}
                    >
                      <option value={0}>Sem tempo</option>
                      <option value={30}>30 s</option>
                      <option value={60}>1 min</option>
                      <option value={120}>2 min</option>
                      <option value={300}>5 min</option>
                    </SelectCtl>
                    {pollCorrect != null && (
                      <Btn variant="ghost" title="Sondagem normal (sem resposta certa)" onClick={() => setPollCorrect(null)}>
                        limpar certa
                      </Btn>
                    )}
                    <button
                      className="btn-sm"
                      disabled={!pollQ.trim() || pollOpts.filter((o) => o.trim()).length < 2}
                      onClick={() => {
                        // O índice da certa refere-se à lista FILTRADA (opções
                        // vazias saem — remapeia para não apontar à errada).
                        const opts = pollOpts.map((o, i) => ({ o: o.trim(), i })).filter((x) => x.o)
                        const correctIdx = pollCorrect != null ? opts.findIndex((x) => x.i === pollCorrect) : -1
                        signalRef.current?.send({
                          type: 'poll-create',
                          question: pollQ,
                          options: opts.map((x) => x.o),
                          correct_option: correctIdx >= 0 ? correctIdx : null,
                          duration_secs: pollDur || null,
                        })
                        setPollQ('')
                        setPollOpts(['', ''])
                        setPollCorrect(null)
                        setPollDur(0)
                      }}
                    >
                      {pollCorrect != null ? 'Lançar quiz' : 'Lançar sondagem'}
                    </button>
                  </div>
                </div>
              )}
              {polls.length === 0 && <p className="muted small">Ainda sem sondagens.</p>}
              {[...polls].reverse().map((p) => {
                const total = p.counts.reduce((a, b) => a + b, 0)
                const revealed = !p.open && p.correct != null
                const remaining =
                  p.open && p.ends_at ? Math.max(0, Math.ceil((p.ends_at - Date.now()) / 1000)) : null
                return (
                  <div key={p.id} className="poll-card">
                    <div className="poll-head">
                      <strong>{p.question}</strong>
                      <span className="muted small">
                        {p.by} · {total} voto{total === 1 ? '' : 's'}{p.open ? '' : ' · encerrada'}
                        {remaining != null && <span className="poll-countdown mono"> · <ClockIcon /> {remaining}s</span>}
                      </span>
                    </div>
                    {p.options.map((opt, i) => {
                      const pct = total ? Math.round((p.counts[i] / total) * 100) : 0
                      const mine = myVotes[p.id] === i
                      const cls = [
                        'poll-opt',
                        mine ? 'mine' : '',
                        revealed && i === p.correct ? 'correct' : '',
                        revealed && mine && i !== p.correct ? 'wrong' : '',
                      ].filter(Boolean).join(' ')
                      return (
                        <button
                          key={i}
                          className={cls}
                          disabled={!p.open || (p.ends_at != null && Date.now() > p.ends_at)}
                          onClick={() => {
                            signalRef.current?.send({ type: 'poll-vote', poll: p.id, option: i })
                            setMyVotes({ ...myVotes, [p.id]: i })
                          }}
                        >
                          <span className="poll-bar" style={{ width: `${pct}%` }} />
                          <span className="poll-opt-label">
                            {revealed && i === p.correct ? '✅ ' : mine ? '● ' : ''}{opt}
                          </span>
                          <span className="poll-opt-count mono">{p.counts[i]} · {pct}%</span>
                        </button>
                      )
                    })}
                    {revealed && (
                      <p className="poll-quiz-totals">
                        <span className="ok"><CheckIcon /> {p.total_right} certa{p.total_right === 1 ? '' : 's'}</span>
                        {' · '}
                        <span className="bad"><CloseIcon /> {p.total_wrong} errada{p.total_wrong === 1 ? '' : 's'}</span>
                      </p>
                    )}
                    {isHost && p.open && (
                      <button className="link small-link" onClick={() => signalRef.current?.send({ type: 'poll-close', poll: p.id })}>
                        Encerrar sondagem
                      </button>
                    )}
                  </div>
                )
              })}
            </section>

            <section className="tool-section">
              <h4><HelpIcon /> Perguntas e respostas</h4>
              <form
                className="qa-form"
                onSubmit={(e) => {
                  e.preventDefault()
                  if (!qaInput.trim()) return
                  signalRef.current?.send({ type: 'qa-ask', text: qaInput })
                  setQaInput('')
                }}
              >
                <input
                  placeholder="Faz uma pergunta…"
                  maxLength={300}
                  value={qaInput}
                  onChange={(e) => setQaInput(e.target.value)}
                />
                <Btn disabled={!qaInput.trim()}>Enviar</Btn>
              </form>
              {questions.length === 0 && <p className="muted small">Ainda sem perguntas.</p>}
              {questions.map((q) => (
                <div key={q.id} className={q.answered ? 'qa-card answered' : 'qa-card'}>
                  <div className="qa-main">
                    <span className="qa-text">{q.text}</span>
                    <small className="muted">{q.by}{q.answered ? ' · ✓ respondida' : ''}</small>
                  </div>
                  <div className="qa-actions">
                    <button
                      className={myUpvotes[q.id] ? 'qa-vote mine' : 'qa-vote'}
                      title="Votar nesta pergunta"
                      onClick={() => {
                        signalRef.current?.send({ type: 'qa-upvote', id: q.id })
                        setMyUpvotes({ ...myUpvotes, [q.id]: !myUpvotes[q.id] })
                      }}
                    >
                      👍 {q.upvotes}
                    </button>
                    {isHost && (
                      <button
                        className="qa-vote"
                        title={q.answered ? 'Reabrir' : 'Marcar como respondida'}
                        onClick={() => signalRef.current?.send({ type: 'qa-answered', id: q.id })}
                      >
                        {q.answered ? <RepeatIcon /> : <CheckIcon />}
                      </button>
                    )}
                  </div>
                </div>
              ))}
            </section>
          </aside>
        )}

        {panel === 'settings' && (
          <aside className="side-panel">
            <div className="panel-head">
              <h3>Definições</h3>
              <button className="panel-close" onClick={() => setPanel('none')}><CloseIcon /></button>
            </div>
            <div className="settings-body">
              {/* Ordem do template: Tema primeiro, depois dispositivos, ruído e fundo. */}
              <div className="bg-section">
                <span className="set-label">Tema</span>
                <small className="muted">Escolhe o aspeto da aplicação.</small>
                <ThemePicker />
              </div>
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

              <label className="set-label">
                Traduzir legendas (IA local)
                <select value={ccLang} onChange={(e) => setCcLang(e.target.value)}>
                  <option value="">Sem tradução — idioma original</option>
                  <option value="pt">Português</option>
                  <option value="en">English</option>
                  <option value="fr">Français</option>
                  <option value="es">Español</option>
                  <option value="de">Deutsch</option>
                </select>
                <small className="muted">
                  As legendas (CC) chegam no idioma original e são substituídas
                  pela tradução do LLM local — nada sai do servidor.
                </small>
              </label>

              <label className="set-toggle">
                <input type="checkbox" checked={noiseSuppression} onChange={() => void toggleNoiseSuppression()} />
                <span>
                  Supressão de ruído (IA)
                  <small>RNNoise remove teclado, ventoinha e ruído de fundo — muito além da supressão do browser.</small>
                </span>
              </label>

              <label className="set-toggle">
                <input
                  type="checkbox"
                  checked={serverAsr}
                  onChange={(e) => {
                    setServerAsr(e.target.checked)
                    localStorage.setItem('dx_asr_server', e.target.checked ? '1' : '0')
                  }}
                />
                <span>
                  Transcrição no servidor (mais precisa)
                  <small>
                    Whisper no teu servidor em vez do reconhecimento do browser — muito mais
                    preciso e soberano (o áudio não sai do datacenter). Aplica-se ao ligar as
                    legendas/notas a seguir.
                  </small>
                </span>
              </label>
              <div className="bg-section">
                <span className="set-label">Fundo {bgBusy ? '· a aplicar…' : ''}</span>
                <small className="muted">
                  IA local segmenta a pessoa; o fundo real nunca fica visível para os outros.
                </small>
                {!hasLocalVideo && (
                  <small className="hint-warn">
                    Liga a câmara para escolher um fundo — os efeitos precisam de vídeo.
                  </small>
                )}
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

        {fxOpen && (
          <aside className="side-panel fx-panel">
            <div className="panel-head">
              <h3>Fundos e efeitos</h3>
              <button className="panel-close" onClick={() => setFxOpen(false)}><CloseIcon /></button>
            </div>
            <div className="fx-preview-wrap">
              <video ref={fxPreview} autoPlay muted playsInline className={bgMode === 'none' ? 'mirror' : undefined} />
            </div>
            <p className="muted small">
              A IA segmenta-te localmente — o teu fundo real nunca é transmitido. {bgBusy ? 'A aplicar…' : ''}
            </p>
            {!hasLocalVideo && (
              <p className="hint-warn small">Liga a câmara para usar efeitos de fundo.</p>
            )}

            <span className="set-label">Efeito esbatido</span>
            <div className="fx-row">
              <button
                className={bgMode === 'none' ? 'fx-opt selected' : 'fx-opt'}
                disabled={bgBusy || !hasLocalVideo}
                onClick={() => void applyBackground('none')}
                title="Sem efeito"
              >
                Ø
              </button>
              <button
                className={bgMode === 'blur' && blurLevel === 'light' ? 'fx-opt selected' : 'fx-opt'}
                disabled={bgBusy || !hasLocalVideo}
                onClick={() => void applyBackground('blur', undefined, 'light')}
                title="Desfoque leve"
              >
                <BlurIcon />
                <small>leve</small>
              </button>
              <button
                className={bgMode === 'blur' && blurLevel === 'strong' ? 'fx-opt selected' : 'fx-opt'}
                disabled={bgBusy || !hasLocalVideo}
                onClick={() => void applyBackground('blur', undefined, 'strong')}
                title="Desfoque forte"
              >
                <BlurIcon />
                <small>forte</small>
              </button>
              <button
                className="fx-opt"
                disabled={bgBusy || !hasLocalVideo}
                onClick={() => uploadRef.current?.click()}
                title="Carregar imagem de fundo"
              >
                ＋
                <small>imagem</small>
              </button>
            </div>

            <span className="set-label">Fundos</span>
            <div className="fx-gallery">
              {presets.map((p) => (
                <button
                  key={p.name}
                  className={bgMode === 'image' && bgImageUrl === p.url ? 'fx-bg selected' : 'fx-bg'}
                  disabled={bgBusy || !hasLocalVideo}
                  onClick={() => void applyBackground('image', p.url)}
                  title={p.name}
                >
                  <img src={p.url} alt={p.name} />
                </button>
              ))}
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
              Transcrição partilhada: o <strong>anfitrião</strong> inicia a Nota AI e <strong>todos</strong> os
              participantes passam a transcrever o próprio microfone — as frases aparecem aqui legendadas por
              orador (capta toda a gente, não só quem iniciou). Ao terminar, a ata (MoM) é gerada e guardada.
            </p>
            {scribeBy && !isHost && (
              <p className="muted small"><MicIcon /> Transcrição ativa (iniciada por {scribeBy}). A tua fala está a ser captada.</p>
            )}
            <div className="notes-body">
              {lines.length === 0 && !interim && <p className="muted small">Ativa a transcrição para começar…</p>}
              {lines.map((l, i) => (
                <div key={i} className="note-line">{l}</div>
              ))}
              {interim && <div className="note-line interim">{interim}…</div>}
            </div>
            <div className="notes-actions">
              <select
                className="stt-lang"
                value={sttLang}
                disabled={transcribing}
                title="Idioma da transcrição"
                onChange={(e) => {
                  setSttLang(e.target.value)
                  localStorage.setItem('dx_stt_lang', e.target.value)
                }}
              >
                <option value="pt-PT">🇵🇹 Português</option>
                <option value="en-US">🇬🇧 English</option>
                <option value="es-ES">🇪🇸 Español</option>
                <option value="fr-FR">🇫🇷 Français</option>
                <option value="de-DE">🇩🇪 Deutsch</option>
                <option value="it-IT">🇮🇹 Italiano</option>
              </select>
              <div className="notes-btns">
                <button
                  className={transcribing ? 'stt-toggle rec' : 'stt-toggle'}
                  onClick={toggleTranscription}
                  disabled={!isHost}
                  title={isHost ? 'Iniciar/parar a transcrição partilhada' : 'Só o anfitrião controla a transcrição'}
                >
                  {transcribing ? <span className="rec-dot" aria-hidden /> : <MicIcon />}
                  {transcribing ? 'Parar transcrição' : 'Iniciar transcrição'}
                </button>
                <button
                  className="stt-save"
                  disabled={lines.length === 0}
                  title={lines.length === 0 ? 'Sem transcrição para guardar' : 'Gerar e guardar a ata (MoM)'}
                  onClick={() => void saveMinutes()}
                >
                  {momSaved ? '✓ Guardada' : <><NoteIcon /> Guardar ata</>}
                </button>
              </div>
            </div>
          </aside>
        )}
      </div>

      {/* Avisos da reunião — faixa própria por baixo do vídeo (NUNCA sobre o
          vídeo): o vídeo encolhe para os acomodar. Cartões estilo Meet. */}
      <div className="room-notices">
        {canAdmit && waitingQueue.length > 0 && (
          <div className="admit-card" role="dialog" aria-label="Sala de espera">
            <div className="admit-card-head">
              <span className="admit-card-title">Sala de espera</span>
              {waitingQueue.length > 1 && (
                <button className="admit-all-link" onClick={() => waitingQueue.forEach((p) => admit(p.peer_id, true))}>
                  Admitir todos ({waitingQueue.length})
                </button>
              )}
            </div>
            {waitingQueue.map((p) => (
              <div key={p.peer_id} className="admit-row">
                <span className="admit-avatar" aria-hidden>{p.username.slice(0, 1).toUpperCase()}</span>
                <span className="admit-name">
                  <strong>{p.username}</strong>
                  <small>quer entrar</small>
                </span>
                <button className="admit-deny" onClick={() => admit(p.peer_id, false)}>Recusar</button>
                <button className="admit-accept" onClick={() => admit(p.peer_id, true)}>Admitir</button>
              </div>
            ))}
          </div>
        )}

        {ctrlAsk && (
          <div className="admit-card" role="dialog" aria-label="Pedido de controlo remoto">
            <div className="admit-row">
              <span className="admit-avatar" aria-hidden><CubeIcon /></span>
              <span className="admit-name">
                <strong>{ctrlAsk.username}</strong>
                <small>pede controlo remoto da tua tela partilhada</small>
              </span>
              <button
                className="admit-deny"
                onClick={() => {
                  signalRef.current?.send({ type: 'remote-control', to: ctrlAsk.from, action: 'deny', payload: null })
                  setCtrlAsk(null)
                }}
              >
                Recusar
              </button>
              <button
                className="admit-accept"
                onClick={() => {
                  signalRef.current?.send({ type: 'remote-control', to: ctrlAsk.from, action: 'accept', payload: null })
                  setCtrlAsk(null)
                }}
              >
                Aceitar
              </button>
            </div>
          </div>
        )}

        {shareAsk && isHost && (
          <div className="admit-card" role="dialog" aria-label="Pedido de partilha de ecrã">
            <div className="admit-row">
              <span className="admit-avatar" aria-hidden><ShareIcon /></span>
              <span className="admit-name">
                <strong>{shareAsk.username}</strong>
                <small>quer partilhar o ecrã</small>
              </span>
              <button
                className="admit-deny"
                onClick={() => {
                  signalRef.current?.send({ type: 'share-grant', to: shareAsk.from, allowed: false })
                  setShareAsk(null)
                }}
              >
                Negar
              </button>
              <button
                className="admit-accept"
                onClick={() => {
                  signalRef.current?.send({ type: 'share-grant', to: shareAsk.from, allowed: true })
                  setShareAsk(null)
                }}
              >
                Permitir
              </button>
            </div>
          </div>
        )}

        {/* Sondagem/quiz publicada a TODOS: cartão flutuante para votar sem
            abrir o painel; após o fecho mostra a revelação por uns segundos. */}
        {(() => {
          const p = [...polls]
            .reverse()
            .find((x) => !pollDismissed[x.id] && (x.open || (revealUntil[x.id] ?? 0) > Date.now()))
          if (!p) return null
          const total = p.counts.reduce((a, b) => a + b, 0)
          const revealed = !p.open && p.correct != null
          const remaining =
            p.open && p.ends_at ? Math.max(0, Math.ceil((p.ends_at - Date.now()) / 1000)) : null
          return (
            <div className="admit-card poll-popup" role="dialog" aria-label="Sondagem">
              <div className="admit-card-head">
                <span className="admit-card-title">
                  {p.correct != null || revealed ? '🏅 Quiz' : '📊 Sondagem'} · {p.by}
                  {remaining != null && <span className="poll-countdown mono"> · <ClockIcon /> {remaining}s</span>}
                </span>
                <button
                  className="panel-close"
                  aria-label="Dispensar"
                  onClick={() => setPollDismissed((m) => ({ ...m, [p.id]: true }))}
                >
                  <CloseIcon />
                </button>
              </div>
              <strong className="poll-popup-q">{p.question}</strong>
              {p.options.map((opt, i) => {
                const pct = total ? Math.round((p.counts[i] / total) * 100) : 0
                const mine = myVotes[p.id] === i
                const cls = [
                  'poll-opt',
                  mine ? 'mine' : '',
                  revealed && i === p.correct ? 'correct' : '',
                  revealed && mine && i !== p.correct ? 'wrong' : '',
                ].filter(Boolean).join(' ')
                return (
                  <button
                    key={i}
                    className={cls}
                    disabled={!p.open || (p.ends_at != null && Date.now() > p.ends_at)}
                    onClick={() => {
                      signalRef.current?.send({ type: 'poll-vote', poll: p.id, option: i })
                      setMyVotes({ ...myVotes, [p.id]: i })
                    }}
                  >
                    <span className="poll-bar" style={{ width: `${pct}%` }} />
                    <span className="poll-opt-label">
                      {revealed && i === p.correct ? '✅ ' : mine ? '● ' : ''}{opt}
                    </span>
                    <span className="poll-opt-count mono">{p.counts[i]} · {pct}%</span>
                  </button>
                )
              })}
              {revealed && (
                <p className="poll-quiz-totals">
                  <span className="ok"><CheckIcon /> {p.total_right} certa{p.total_right === 1 ? '' : 's'}</span>
                  {' · '}
                  <span className="bad"><CloseIcon /> {p.total_wrong} errada{p.total_wrong === 1 ? '' : 's'}</span>
                </p>
              )}
            </div>
          )
        })()}

        {recNotice && (
          <div className="toast rec-start-toast" role="status">
            <span className="rec-dot big" />
            <span><strong>{recNotice}</strong>. Todos os participantes foram notificados.</span>
          </div>
        )}

        {serverRec && (
          <div className="toast rec-toast">
            <span className="rec-dot" />
            <span><strong>{serverRec.by}</strong> ativou a gravação no servidor — o vídeo (webm) fica na biblioteca ao terminar.</span>
          </div>
        )}

        {anyoneRecording && (
          <div className="toast rec-toast">
            <span className="rec-dot" />
            <span>
              {recording ? 'Estás a gravar esta reunião' : `${remoteRecorder} está a gravar esta reunião`} — todos os
              participantes têm acesso à gravação no painel «Participantes».
            </span>
          </div>
        )}

        {talkOver && (
          <div className="toast talk-toast">
            <span aria-hidden><MicIcon /></span>
            <span>
              {talkOverNames ? `${talkOverNames} estão a falar ao mesmo tempo` : 'Duas pessoas estão a falar ao mesmo tempo'} —
              dá espaço para cada um terminar.
            </span>
          </div>
        )}
      </div>

      <footer className="controls-bar">
        <div className="bar-left">
          {/* Hora/código/E2EE estão agora na barra de topo (estilo Meet). Aqui
              ficam só os indicadores dinâmicos da sessão. */}
          {roomState === 'in' && <MeetingElapsed startedAt={joinedAtRef.current} />}
          {secOpen && secCode && (
            <span className="sec-code" onClick={() => setSecOpen(false)} title="Código de segurança da sala">
              <ShieldIcon /> <strong className="mono">{secCode}</strong> — igual em todos os participantes se ninguém estiver a intercetar
            </span>
          )}
          {returnTo && (
            <button
              className="return-main"
              title="Regressar à sala principal"
              onClick={() => {
                sessionStorage.removeItem(`dx_return_${code}`)
                sessionStorage.removeItem(`dx_bo_ends_${code}`)
                onSwitch?.(returnTo)
              }}
            >
              <ChevronLeftIcon /> Sala principal
            </button>
          )}
          {returnTo && breakoutEndsAt && (
            <span className="room-topo breakout-chip" title="Tempo restante neste grupo">
              <ClockIcon /> <Countdown endsAt={breakoutEndsAt} render={(txt) => <>{txt}</>} />
            </span>
          )}
          {meetTimerEndsAt && (
            <Countdown
              endsAt={meetTimerEndsAt}
              render={(txt, restam) => (
                <span
                  className={restam <= 60 ? 'room-topo breakout-chip timer-low' : 'room-topo breakout-chip'}
                  title="Temporizador da reunião"
                >
                  ⏳ {txt}
                </span>
              )}
            />
          )}
          {presentation && (
            <span className="room-topo presenter-chip" title="Apresentação em curso">
              🖥 {presentation.peerId === 'me'
                ? 'A apresentar'
                : `${peers.find((p) => p.peerId === presentation.peerId)?.username ?? ''} • apresenta`}
            </span>
          )}
          {/* Indicador de recuperação de media. Existe porque uma quebra de
              rede dava ECRÃ PRETO SEM DIAGNÓSTICO: o utilizador não tinha como
              distinguir «a plataforma está a tratar disto» de «isto avariou».
              Só aparece quando há mesmo algo a assinalar. */}
          {callState !== 'connected' && callState !== 'connecting' && callState !== 'disconnected' && (
            <span
              className="room-status"
              role="status"
              aria-live="polite"
              title={`Ligação de media: ${callState}`}
            >
              {callState === 'degraded' ? '◐ ligação instável' : '◌ a restabelecer…'}
            </span>
          )}
          {status && <span className="room-status">{status}</span>}
        </div>

        <div className="bar-center">
          {/* Dois grupos visuais: DISPOSITIVOS (o que os outros recebem de mim)
              e SESSÃO (o que faço na reunião). Só agrupamento — nenhuma lógica
              de media depende desta estrutura. */}
          <div className="ctrl-group ctrl-group-devices">
          <DeviceControl
            label={micOn ? 'Desativar microfone (Ctrl+D)' : 'Ativar microfone (Ctrl+D)'}
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
            label={camOn ? 'Desativar câmara (Ctrl+E)' : 'Ativar câmara (Ctrl+E)'}
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
          </div>
          <div className="ctrl-group ctrl-group-session">
          <Ctrl
            label={ccOn ? 'Desativar legendas automáticas' : 'Legendas automáticas (CC) — transcreve a voz em tempo real'}
            active={ccOn}
            onClick={() => setCcOn((v) => !v)}
          >
            <span className="cc-badge">CC</span>
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
            label={
              !isHost && !shareAllowed && !sharing
                ? 'Partilhar ecrã (requer autorização do anfitrião)'
                : 'Partilhar ecrã'
            }
            active={sharing}
            onClick={() => {
              // Não-anfitrião sem grant: pede autorização ao anfitrião e a
              // partilha arranca sozinha quando o grant chegar.
              if (!sharing && !isHost && !shareAllowed) {
                if (hostShareOnly) {
                  setStatus('Só o anfitrião pode partilhar o ecrã nesta reunião')
                  return
                }
                pendingShareRef.current = true
                signalRef.current?.send({ type: 'share-request' })
                setStatus('Pedido de partilha enviado ao anfitrião — aguarda autorização')
                return
              }
              void toggleShare()
            }}
          >
            <ShareIcon />
          </Ctrl>
          <Ctrl label={handRaised ? 'Baixar a mão' : 'Levantar a mão'} active={handRaised} onClick={toggleHand}>
            <HandIcon />
          </Ctrl>
          <Ctrl
            label={recording ? 'Parar gravação' : 'Gravar reunião'}
            active={recording}
            danger={recording}
            onClick={() => void toggleRecording()}
          >
            {recording ? <StopIcon /> : <RecordIcon />}
          </Ctrl>
          <div className="picker-wrap">
            {moreOpen && (
              <div className="device-menu more-menu">
                <button
                  className="device-item"
                  onClick={() => {
                    setViewMode('grid')
                    setPinnedId(null)
                    setMoreOpen(false)
                  }}
                >
                  <span style={{ opacity: effectiveViewMode === 'grid' && !presentation ? 1 : 0.4, marginRight: 4 }}><CheckIcon /></span><GridIcon /> Grelha
                </button>
                <button
                  className="device-item"
                  onClick={() => {
                    setViewMode('stage')
                    setMoreOpen(false)
                  }}
                >
                  <span style={{ opacity: effectiveViewMode === 'stage' && !presentation ? 1 : 0.4, marginRight: 4 }}><CheckIcon /></span><StageIcon /> Orador em palco
                </button>
                {presentation && <>
                  <button
                    className="device-item"
                    onClick={() => { setPresLayout('bottom'); setMoreOpen(false) }}
                  >
                    <span style={{ opacity: presLayout === 'bottom' ? 1 : 0.4, marginRight: 4 }}><CheckIcon /></span><RowsIcon /> Apresentação — plateia em baixo
                  </button>
                  <button
                    className="device-item"
                    onClick={() => { setPresLayout('side'); setMoreOpen(false) }}
                  >
                    <span style={{ opacity: presLayout === 'side' ? 1 : 0.4, marginRight: 4 }}><CheckIcon /></span><TableIcon /> Apresentação — lado a lado
                  </button>
                </>}
                <div className="device-sep" />
                <button
                  className="device-item"
                  onClick={() => {
                    if (document.fullscreenElement) void document.exitFullscreen()
                    else void document.documentElement.requestFullscreen()
                    setMoreOpen(false)
                  }}
                >
                  <FullscreenIcon /> {fullscreen ? 'Sair de ecrã inteiro' : 'Ecrã inteiro'}
                </button>
                <button
                  className="device-item"
                  onClick={() => {
                    setFxOpen(true)
                    setMoreOpen(false)
                  }}
                >
                  <BlurIcon /> Fundos e efeitos
                </button>
                {isHost && topology === 'sfu' && (
                  <button
                    className="device-item"
                    onClick={() => {
                      setMoreOpen(false)
                      if (serverRec) {
                        signalRef.current?.send({ type: 'server-record', active: false })
                        return
                      }
                      let key: string | null = null
                      if (e2eeOn) {
                        // E2EE: gravar exige ceder a chave ao servidor — só
                        // com consentimento explícito do anfitrião.
                        const ok = window.confirm(
                          'Esta reunião é encriptada de ponta a ponta.\n\n' +
                            'Para gravar no servidor, a chave desta sala é entregue ao servidor ' +
                            'APENAS durante a gravação, e o ficheiro fica legível na biblioteca.\n\n' +
                            'Autorizar e começar a gravar?',
                        )
                        if (!ok || !e2eeKeyRef.current) return
                        key = e2eeKeyRef.current
                      }
                      signalRef.current?.send({ type: 'server-record', active: true, e2ee_key: key })
                    }}
                  >
                    {serverRec ? '☁ Parar gravação no servidor' : '☁ Gravar no servidor (webm)'}
                  </button>
                )}
                <button className="device-item" onClick={() => setHideSelf((v) => !v)}>
                  {hideSelf ? '👁 Mostrar o meu vídeo' : '🙈 Ocultar o meu vídeo'}
                </button>
                <button className="device-item" onClick={() => setHideNoVideo((v) => !v)}>
                  {hideNoVideo ? '👥 Mostrar participantes sem vídeo' : '🫥 Ocultar participantes sem vídeo'}
                </button>
                <button
                  className="device-item"
                  onClick={() => void toggleParallax()}
                >
                  <span style={{ opacity: parallax ? 1 : 0.4, marginRight: 4 }}><CheckIcon /></span><CubeIcon /> Efeito de sala 3D
                </button>
                <button
                  className="device-item device-action"
                  onClick={() => {
                    setPanel('settings')
                    setMoreOpen(false)
                  }}
                >
                  <SettingsIcon /> Definições
                </button>
              </div>
            )}
            <Ctrl label="Mais opções" active={moreOpen} onClick={() => setMoreOpen(!moreOpen)}>
              <span className="more-dots">⋮</span>
            </Ctrl>
          </div>
          </div>
          <button className="ctrl hangup" onClick={() => void leaveRoom()} title="Sair da chamada">
            <HangupIcon />
          </button>
        </div>

        <div className="bar-right">
          <Ctrl
            plain
            label="Quadro branco colaborativo"
            active={wbOpen}
            onClick={() => {
              const next = !wbOpen
              setWbOpen(next)
              if (next && (sharing || isHost)) signalRef.current?.send({ type: 'wb-open' })
              if (!next) signalRef.current?.send({ type: 'wb-close' })
            }}
          >
            <span className="tools-badge"><EditIcon /></span>
          </Ctrl>
          <Ctrl
            plain
            label="Ferramentas de reunião (sondagens, Q&A, temporizador)"
            active={panel === 'tools'}
            onClick={() => setPanel(panel === 'tools' ? 'none' : 'tools')}
          >
            <span className="tools-badge"><SettingsIcon /></span>
          </Ctrl>
          <Ctrl plain label="Notas AI / Ata (MoM)" active={notesOpen} onClick={() => setNotesOpen(!notesOpen)}>
            <NoteIcon />
            {transcribing && <span className="badge live dot" aria-label="a transcrever" />}
          </Ctrl>
          <Ctrl plain label="Participantes e gravações" active={panel === 'people'} onClick={() => setPanel(panel === 'people' ? 'none' : 'people')}>
            <PeopleIcon />
            <span className="badge">{total}</span>
          </Ctrl>
          <Ctrl plain label="Chat" active={panel === 'chat'} onClick={() => { setPanel(panel === 'chat' ? 'none' : 'chat'); setUnreadChat(0) }}>
            <ChatIcon />
            {unreadChat > 0 && <span className="badge unread-badge">{unreadChat > 9 ? '9+' : unreadChat}</span>}
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
    <button className={cls} onClick={onClick} data-tip={label} aria-label={label}>
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
  extra,
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
  extra?: ReactNode
}) {
  // Selagem: o menu de dispositivos abre agora como barra horizontal
  // flutuante ao fundo do vídeo (estilo Google Meet) — ver DeviceChipsBar.
  void devices
  void currentId
  void onPick
  void emptyLabel
  void extra
  return (
    <div className="device-ctrl">
      <button
        className={['ctrl', off ? 'off' : '', pulse ? 'pulse' : ''].filter(Boolean).join(' ')}
        onClick={onToggle}
        data-tip={label}
        aria-label={label}
      >
        {children}
      </button>
      <button className={open ? 'chevron open' : 'chevron'} onClick={onChevron} data-tip="Escolher dispositivo" aria-label="Escolher dispositivo">
        <ChevronUpIcon />
      </button>
    </div>
  )
}


const WB_COLORS = ['#202124', '#C8201D', '#EDA33B', '#1c8a4d', '#2b6fd8']

/**
 * Quadro branco colaborativo: canvas em overlay, traços com coordenadas
 * normalizadas (0..1) difundidos via signaling; redesenha ao redimensionar.
 */
function Whiteboard({
  strokes,
  onStroke,
  onClear,
  onSave,
  onClose,
}: {
  strokes: WbStroke[]
  onStroke: (s: WbStroke) => void
  onClear: () => void
  onSave: (pngBase64: string) => void | Promise<void>
  onClose: () => void
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const wrapRef = useRef<HTMLDivElement>(null)
  const drawing = useRef<WbStroke | null>(null)
  const [color, setColor] = useState(WB_COLORS[0])
  const [width, setWidth] = useState(3)

  const drawStroke = (ctx: CanvasRenderingContext2D, s: WbStroke, W: number, H: number) => {
    if (s.pts.length < 2) return
    ctx.strokeStyle = s.c
    ctx.lineWidth = s.w
    ctx.lineCap = 'round'
    ctx.lineJoin = 'round'
    ctx.beginPath()
    ctx.moveTo(s.pts[0][0] * W, s.pts[0][1] * H)
    for (const [x, y] of s.pts.slice(1)) ctx.lineTo(x * W, y * H)
    ctx.stroke()
  }

  const redraw = () => {
    const c = canvasRef.current
    const wrap = wrapRef.current
    if (!c || !wrap) return
    c.width = wrap.clientWidth
    c.height = wrap.clientHeight
    const ctx = c.getContext('2d')!
    ctx.clearRect(0, 0, c.width, c.height)
    for (const s of strokes) drawStroke(ctx, s, c.width, c.height)
  }

  useEffect(() => {
    redraw()
    const ro = new ResizeObserver(redraw)
    if (wrapRef.current) ro.observe(wrapRef.current)
    return () => ro.disconnect()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [strokes])

  const norm = (e: React.PointerEvent): [number, number] => {
    const r = canvasRef.current!.getBoundingClientRect()
    return [(e.clientX - r.left) / r.width, (e.clientY - r.top) / r.height]
  }

  const saved = useRef(false)
  /** Compõe fundo branco opaco + traços num PNG base64 (o canvas em si é transparente). */
  const snapshot = (): string | null => {
    const c = canvasRef.current
    if (!c || strokes.length === 0) return null
    const off = document.createElement('canvas')
    off.width = c.width
    off.height = c.height
    const ctx = off.getContext('2d')!
    ctx.fillStyle = '#ffffff'
    ctx.fillRect(0, 0, off.width, off.height)
    for (const s of strokes) drawStroke(ctx, s, off.width, off.height)
    return off.toDataURL('image/png')
  }
  const save = async () => {
    const png = snapshot()
    if (png) { saved.current = true; await onSave(png) }
  }
  const closeAndSave = async () => {
    // Auto-guardar na biblioteca ao fechar, se houver conteúdo por guardar.
    if (!saved.current && strokes.length > 0) await save()
    onClose()
  }

  return (
    <div className="wb-overlay" ref={wrapRef}>
      <canvas
        ref={canvasRef}
        onPointerDown={(e) => {
          try { e.currentTarget.setPointerCapture(e.pointerId) } catch { /* pointer sintético */ }
          drawing.current = { pts: [norm(e)], c: color, w: width }
        }}
        onPointerMove={(e) => {
          const s = drawing.current
          if (!s) return
          const p = norm(e)
          const last = s.pts[s.pts.length - 1]
          if (Math.abs(p[0] - last[0]) + Math.abs(p[1] - last[1]) < 0.002) return
          s.pts.push(p)
          const c = canvasRef.current!
          drawStroke(c.getContext('2d')!, { ...s, pts: s.pts.slice(-2) }, c.width, c.height)
        }}
        onPointerUp={() => {
          const s = drawing.current
          drawing.current = null
          if (s && s.pts.length >= 2) onStroke(s)
        }}
      />
      <div className="wb-toolbar">
        {WB_COLORS.map((c) => (
          <button
            key={c}
            className={c === color ? 'wb-color sel' : 'wb-color'}
            style={{ background: c }}
            onClick={() => setColor(c)}
            title="Cor"
          />
        ))}
        <button className={width === 3 ? 'wb-tool sel' : 'wb-tool'} onClick={() => setWidth(3)} title="Traço fino"><StrokeThinIcon /></button>
        <button className={width === 8 ? 'wb-tool sel' : 'wb-tool'} onClick={() => setWidth(8)} title="Traço grosso"><StrokeThickIcon /></button>
        <button className="wb-tool" onClick={() => void save()} title="Guardar na biblioteca de quadros"><SaveIcon /></button>
        <button className="wb-tool" onClick={onClear} title="Limpar o quadro para todos"><TrashIcon /></button>
        <button className="wb-tool" onClick={() => void closeAndSave()} title="Fechar (guarda automaticamente)"><CloseIcon /></button>
      </div>
    </div>
  )
}

/** Ecrã partilhado em palco (track separada da câmara). */
function PresentationTile({ stream, label, own, onRequestControl }: { stream: MediaStream; label: string; own?: boolean; onRequestControl?: () => void }) {
  // Callback-ref: liga o stream sempre que o <video> (re)monta e força o play —
  // o autoplay de vídeo NÃO-mudo (a apresentação remota) é bloqueado por alguns
  // browsers e ficava preto. Silencia-se sempre (o áudio do ecrã vem noutra
  // track/tile), o que garante o autoplay, e re-tenta ao chegar metadados.
  const attach = useCallback(
    (node: HTMLVideoElement | null) => {
      if (!node) return
      if (node.srcObject !== stream) node.srcObject = stream
      const tryPlay = () => node.play().catch(() => {})
      tryPlay()
      node.onloadedmetadata = tryPlay
      if (import.meta.env.DEV) {
        const vt = stream.getVideoTracks()[0]
        console.debug('[present] attach', label, 'video?', !!vt, vt && { muted: vt.muted, enabled: vt.enabled, state: vt.readyState })
      }
    },
    [stream, label],
  )
  // Áudio do ecrã partilhado (separador com som): reproduzido num <audio>
  // dedicado — o <video> fica mudo para o autoplay nunca ser bloqueado, mas o
  // som do ecrã não se perde. (Na própria apresentação não se reproduz o áudio
  // para não haver eco.)
  const attachAudio = useCallback(
    (node: HTMLAudioElement | null) => {
      if (!node || own) return
      if (node.srcObject !== stream) node.srcObject = stream
      node.play().catch(() => {})
    },
    [stream, own],
  )

  // ---- Zoom & pan estilo Meet na tela partilhada ----
  // Roda do rato/duplo-clique amplia (centrado no cursor); com zoom, arrasta-se
  // para navegar. O pan é limitado para as margens do vídeo nunca entrarem.
  const viewportRef = useRef<HTMLDivElement | null>(null)
  const [zoom, setZoomState] = useState(1)
  const [pan, setPanState] = useState({ x: 0, y: 0 })
  const [panning, setPanning] = useState(false)
  const zoomRef = useRef(1)
  const panRef = useRef({ x: 0, y: 0 })
  const dragRef = useRef<{ sx: number; sy: number; px: number; py: number } | null>(null)

  const applyZoom = useCallback((next: number, cx?: number, cy?: number) => {
    const el = viewportRef.current
    if (!el) return
    const rect = el.getBoundingClientRect()
    const z0 = zoomRef.current
    const z = Math.min(4, Math.max(1, next))
    let { x, y } = panRef.current
    // Zoom centrado no cursor: o ponto por baixo do rato fica no sítio.
    if (cx != null && cy != null && z !== z0) {
      const ox = cx - rect.left - rect.width / 2
      const oy = cy - rect.top - rect.height / 2
      x = (x - ox) * (z / z0) + ox
      y = (y - oy) * (z / z0) + oy
    }
    const maxX = (rect.width * (z - 1)) / 2
    const maxY = (rect.height * (z - 1)) / 2
    x = z === 1 ? 0 : Math.min(maxX, Math.max(-maxX, x))
    y = z === 1 ? 0 : Math.min(maxY, Math.max(-maxY, y))
    zoomRef.current = z
    panRef.current = { x, y }
    setZoomState(z)
    setPanState({ x, y })
  }, [])

  // Listener nativo: o onWheel do React é passive → sem preventDefault a
  // página fazia scroll em vez de zoom.
  useEffect(() => {
    const el = viewportRef.current
    if (!el) return
    const onWheel = (e: WheelEvent) => {
      e.preventDefault()
      const factor = e.deltaY < 0 ? 1.15 : 1 / 1.15
      applyZoom(zoomRef.current * factor, e.clientX, e.clientY)
    }
    el.addEventListener('wheel', onWheel, { passive: false })
    return () => el.removeEventListener('wheel', onWheel)
  }, [applyZoom])

  // Nova apresentação → repõe o zoom.
  useEffect(() => {
    zoomRef.current = 1
    panRef.current = { x: 0, y: 0 }
    setZoomState(1)
    setPanState({ x: 0, y: 0 })
  }, [stream])

  return (
    <div className="tile presentation">
      <div
        ref={viewportRef}
        className={zoom > 1 ? (panning ? 'pres-viewport panning' : 'pres-viewport pannable') : 'pres-viewport'}
        onPointerDown={(e) => {
          if (zoomRef.current <= 1) return
          e.currentTarget.setPointerCapture?.(e.pointerId)
          dragRef.current = { sx: e.clientX, sy: e.clientY, px: panRef.current.x, py: panRef.current.y }
          setPanning(true)
        }}
        onPointerMove={(e) => {
          const d = dragRef.current
          const el = viewportRef.current
          if (!d || !el) return
          const rect = el.getBoundingClientRect()
          const z = zoomRef.current
          const maxX = (rect.width * (z - 1)) / 2
          const maxY = (rect.height * (z - 1)) / 2
          const x = Math.min(maxX, Math.max(-maxX, d.px + (e.clientX - d.sx)))
          const y = Math.min(maxY, Math.max(-maxY, d.py + (e.clientY - d.sy)))
          panRef.current = { x, y }
          setPanState({ x, y })
        }}
        onPointerUp={() => { dragRef.current = null; setPanning(false) }}
        onPointerCancel={() => { dragRef.current = null; setPanning(false) }}
        onDoubleClick={(e) => applyZoom(zoomRef.current > 1 ? 1 : 2, e.clientX, e.clientY)}
      >
        <video
          ref={attach}
          autoPlay
          playsInline
          muted
          style={{
            transform: `translate(${pan.x}px, ${pan.y}px) scale(${zoom})`,
            transition: panning ? 'none' : 'transform 0.15s ease-out',
          }}
        />
      </div>
      {!own && <audio ref={attachAudio} autoPlay />}
      <span className="tile-name"><ShareIcon /> {label}</span>
      <div className="pres-zoom-ctrls" role="group" aria-label="Zoom da apresentação">
        <button title="Reduzir" onClick={() => applyZoom(zoomRef.current / 1.25)}>−</button>
        <span className="mono">{Math.round(zoom * 100)}%</span>
        <button title="Ampliar" onClick={() => applyZoom(zoomRef.current * 1.25)}>+</button>
        {zoom > 1 && <button title="Repor (100%)" onClick={() => applyZoom(1)}>⟲</button>}
      </div>
      <button
        className="pres-fs-btn"
        title="Ecrã inteiro"
        onClick={() => {
          const el = document.documentElement
          if (document.fullscreenElement) void document.exitFullscreen()
          else void el.requestFullscreen?.()
        }}
      >
        <FullscreenIcon />
      </button>
      {!own && onRequestControl && (
        <button
          className="remote-ctrl-btn"
          title="Solicitar Controlo Remoto"
          onClick={onRequestControl}
          style={{ position: 'absolute', bottom: '10px', left: '10px', zIndex: 10, background: 'var(--accent)', color: 'white', border: 'none', padding: '6px 12px', borderRadius: '4px', cursor: 'pointer' }}
        >
          <CubeIcon /> Solicitar Controlo
        </button>
      )}
    </div>
  )
}

/**
 * Atualizador de "quem está a falar" que só troca o estado quando o CONJUNTO
 * muda. O `LevelWatcher` dispara a cada 180 ms; `setSpeaking(new Set(s))`
 * criava sempre uma identidade nova, pelo que a sala inteira (componente de
 * milhares de linhas, N tiles de vídeo) re-renderizava ~5,5×/s do princípio ao
 * fim da reunião, mesmo em silêncio absoluto — CPU roubado aos decoders.
 */
function sameSet(a: Set<string>, b: Set<string>): boolean {
  if (a.size !== b.size) return false
  for (const v of a) if (!b.has(v)) return false
  return true
}

/**
 * Tiles de vídeo por página. Acima disto o browser descodifica fluxos que
 * ninguém consegue ver: numa sala de 40 eram 39 decoders a correr para desenhar
 * retângulos de 100 px. Com a paginação o cliente só pede (`video-interest`) o
 * vídeo da página visível e o SFU deixa mesmo de o enviar.
 */
const TILES_PER_PAGE = 24

/** Barras animadas tipo Meet quando alguém está a falar. */

/**
 * Reprodução do ÁUDIO de todos os participantes, num sítio só, SEMPRE montado.
 *
 * Antes, cada `<audio>` vivia dentro do `RemoteTile`. Como a grelha esconde
 * tiles (opção «Ocultar participantes sem vídeo», palco, futura paginação),
 * esconder um tile DESMONTAVA o `<audio>` e a pessoa deixava de se ouvir — com
 * a câmara desligada a ser o caso mais comum, ativar essa opção deixava o
 * utilizador praticamente surdo, sem qualquer pista da causa.
 *
 * Regra: o que está no ecrã é decisão de layout; o que se ouve não pode
 * depender do layout.
 */
function AudioSink({ peers, sinkId }: { peers: RemotePeer[]; sinkId: string }) {
  return (
    <div className="audio-sink" aria-hidden style={{ display: 'none' }}>
      {peers.map((p) => (
        <PeerAudio key={p.peerId} stream={p.stream} sinkId={sinkId} />
      ))}
    </div>
  )
}

function PeerAudio({ stream, sinkId }: { stream: MediaStream | null; sinkId: string }) {
  const ref = useRef<HTMLAudioElement>(null)
  useEffect(() => {
    const el = ref.current
    if (!el || el.srcObject === stream) return
    el.srcObject = stream
    void el.play().catch(() => {})
  }, [stream])
  // Saída de áudio (altifalantes) escolhida nas definições.
  useEffect(() => {
    const el = ref.current as (HTMLAudioElement & { setSinkId?: (id: string) => Promise<void> }) | null
    if (el?.setSinkId) void el.setSinkId(sinkId || '').catch(() => {})
  }, [sinkId, stream])
  return <audio ref={ref} autoPlay />
}

