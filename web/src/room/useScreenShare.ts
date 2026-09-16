import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { videoConstraints } from '../media'
import { SCREEN_CONSTRAINTS } from '../webrtc'
import type { LocalMedia } from './useLocalMedia'
import type { RoomCore } from './useRoomCore'

/**
 * Partilha de ecrã e as permissões à volta dela. Parar de partilhar tem de
 * PARAR a captura (R111): é a única parte da sala onde um defeito silencioso é
 * um problema de privacidade.
 */
export function useScreenShare(core: RoomCore, media: LocalMedia) {
  const { t } = useTranslation()
  const { signal, setStatus, sharing, setSharing, presentation, setPresentation } = core
  const [shareAllowed, setShareAllowed] = useState(false)
  const [hostShareOnly, setHostShareOnly] = useState(false)
  /** Pedido de partilha de um não-anfitrião, à espera de decisão. */
  const [shareAsk, setShareAsk] = useState<{ from: string; username: string } | null>(null)
  /** Anfitrião: a quem concedeu a permissão. */
  const [sharePerms, setSharePerms] = useState<Set<string>>(() => new Set())
  /** Pedido feito: a partilha arranca sozinha quando a autorização chegar. */
  const pendingShareRef = useRef(false)
  /**
   * Uma CÂMARA a partilhar em vez do ecrã (segunda fonte: captura HDMI, câmara
   * de documentos). Viaja pelo mesmo caminho da partilha — mesma permissão,
   * mesma paragem — e por isso também só existe em SFU.
   */
  const pendingSourceRef = useRef<MediaStream | null>(null)
  /** O dispositivo da fonte partilhada agora, se a partilha é uma câmara. */
  const [sourceDeviceId, setSourceDeviceId] = useState('')
  const ownShareRef = useRef<MediaStream | null>(null)
  // Os handlers registam-se uma vez: este ref aponta sempre ao `toggleShare`
  // do render actual (sem closure obsoleta de `sharing`/`topology`).
  const toggleShareRef = useRef<() => void>(() => {})
  toggleShareRef.current = () => void toggleShare()

  useEffect(() => {
    const offs = [
      signal.on('room-settings', (m) => setHostShareOnly(m.host_share_only)),
      signal.on('share-granted', (m) => {
        setShareAllowed(m.allowed)
        const wasPending = pendingShareRef.current
        pendingShareRef.current = false
        if (m.allowed && wasPending) {
          setStatus(t('room.estado.partilhaAutorizada'))
          toggleShareRef.current()
          return
        }
        if (!m.allowed && wasPending) {
          largarFontePendente()
          setStatus(t('room.estado.partilhaRecusada'))
          return
        }
        setStatus(m.allowed ? t('room.estado.permissaoPartilhaDada') : t('room.estado.permissaoPartilhaRetirada'))
      }),
      signal.on('share-request', (m) => setShareAsk({ from: m.from, username: m.username })),
      // Aviso fiável: ao parar, limpa já a apresentação desse participante.
      signal.on('presenting', (m) => {
        if (!m.on) setPresentation((p) => (p?.peerId === m.from ? null : p))
      }),
    ]
    return () => offs.forEach((off) => off())
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signal])

  // Sair da sala a partilhar não deixa o browser a capturar.
  useEffect(
    () => () => {
      core.displayStreamRef.current?.getTracks().forEach((tr) => tr.stop())
      core.displayStreamRef.current = null
      // SFU: a apresentação própria (ecrã ou segunda câmara) também se pára.
      ownShareRef.current?.getTracks().forEach((tr) => tr.stop())
      ownShareRef.current = null
      pendingSourceRef.current?.getTracks().forEach((tr) => tr.stop())
      pendingSourceRef.current = null
    },
    [core.displayStreamRef],
  )

  async function toggleShare() {
    const isSfu = core.topology === 'sfu'
    const displayStreamRef = core.displayStreamRef
    const callRef = core.callRef
    if (sharing) {
      if (isSfu) {
        // SFU: o ecrã era uma track adicional — pára-se e retira-se.
        presentation?.stream.getTracks().forEach((t) => t.stop())
        await callRef.current?.stopScreen()
        setPresentation((p) => (p?.peerId === 'me' ? null : p))
      } else {
        // Mesh: o ecrã substituiu a câmara. PARA-SE o que veio do
        // `getDisplayMedia` — aqui não está em `presentation`, e sem isto a
        // captura continuava viva depois de «parar partilha» (R111).
        displayStreamRef.current?.getTracks().forEach((t) => t.stop())
        displayStreamRef.current = null
        const back = (media.bgMode !== 'none' && core.effectRef.current?.output) || core.cameraTrackRef.current
        if (back) await callRef.current?.replaceVideoTrack(back)
        if (core.localVideoRef.current && core.localStreamRef.current) {
          core.localVideoRef.current.srcObject =
            media.bgMode !== 'none' && back !== core.cameraTrackRef.current ? new MediaStream([back!]) : core.localStreamRef.current
        }
      }
      setSharing(false)
      setSourceDeviceId('')
      return
    }
    try {
      const fonte = pendingSourceRef.current
      pendingSourceRef.current = null
      const display = fonte ?? (await navigator.mediaDevices.getDisplayMedia(SCREEN_CONSTRAINTS))
      const screenTrack = display.getVideoTracks()[0]
      screenTrack.contentHint = 'detail' // nitidez de texto antes de fluidez
      if (isSfu) {
        // Track separada: a câmara continua; todos recebem o ecrã à parte.
        await callRef.current?.startScreen(screenTrack, display)
        ownShareRef.current = display
        setPresentation({ peerId: 'me', stream: display })
      } else {
        displayStreamRef.current = display
        await callRef.current?.replaceVideoTrack(screenTrack)
        if (core.localVideoRef.current) core.localVideoRef.current.srcObject = display
        // O mesh não tem para onde enviar o áudio do sistema: o ecrã viaja no
        // lugar da câmara. Pára-se, e DIZ-SE — uma caixa marcada que não faz
        // nada é da família do consentimento vazio do R109.
        const sysAudio = display.getAudioTracks()
        if (sysAudio.length > 0) {
          sysAudio.forEach((t) => t.stop())
          setStatus(t('room.txt.audioDoSistemaSoEmSfu'))
        }
      }
      screenTrack.onended = () => toggleShareRef.current()
      setSourceDeviceId(fonte ? screenTrack.getSettings().deviceId ?? 'fonte' : '')
      setSharing(true)
    } catch {
      /* a pessoa cancelou o selector */
    }
  }

  function largarFontePendente() {
    pendingSourceRef.current?.getTracks().forEach((tr) => tr.stop())
    pendingSourceRef.current = null
  }

  /** O botão: sem autorização pede-a ao anfitrião; com ela, partilha. */
  function requestOrToggleShare() {
    if (!sharing && !core.isHost && !shareAllowed) {
      if (hostShareOnly) {
        largarFontePendente()
        setStatus(t('room.estado.soAnfitriaoPartilha'))
        return
      }
      pendingShareRef.current = true
      signal.send({ type: 'share-request' })
      setStatus(t('room.estado.pedidoPartilhaEnviado'))
      return
    }
    void toggleShare()
  }

  /**
   * Publica uma segunda fonte de vídeo como apresentação. Recebe o stream já
   * aberto (pré-entrada) ou abre o dispositivo pedido (barra da sala).
   */
  async function shareSource(source: MediaStream | string) {
    if (sharing || core.topology !== 'sfu') {
      if (typeof source !== 'string') source.getTracks().forEach((tr) => tr.stop())
      return
    }
    try {
      const stream =
        typeof source === 'string' ? await navigator.mediaDevices.getUserMedia({ video: videoConstraints(source) }) : source
      stream.getVideoTracks().forEach((tr) => (tr.contentHint = 'detail'))
      largarFontePendente()
      pendingSourceRef.current = stream
      requestOrToggleShare()
    } catch {
      setStatus(t('room.estado.naoMudouCamara'))
    }
  }

  function grantShare(peerId: string, allowed: boolean) {
    setSharePerms((s) => {
      const n = new Set(s)
      if (allowed) n.add(peerId)
      else n.delete(peerId)
      return n
    })
    signal.send({ type: 'share-grant', to: peerId, allowed })
  }

  function answerShareRequest(allowed: boolean) {
    if (!shareAsk) return
    grantShare(shareAsk.from, allowed)
    setShareAsk(null)
  }

  return {
    sharing,
    shareAllowed,
    hostShareOnly,
    setHostShareOnly: (on: boolean) => signal.send({ type: 'host-share-only', on }),
    needsPermission: !sharing && !core.isHost && !shareAllowed,
    shareAsk,
    sharePerms,
    requestOrToggleShare,
    shareSource,
    sourceDeviceId,
    grantShare,
    answerShareRequest,
  }
}

export type ScreenShare = ReturnType<typeof useScreenShare>
