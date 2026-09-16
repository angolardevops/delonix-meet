import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { audioConstraints, listDevices, videoConstraints } from '../media'
import type { LocalMedia } from './useLocalMedia'
import type { RoomCore } from './useRoomCore'

/**
 * Pré-entrada: SÓ media local para a pré-visualização. Não cria Signaling nem
 * chamada (R2: nada de ofertas antes de entrar). O stream fica em
 * `core.previewStreamRef` e é ENTREGUE ao efeito de entrada no clique em
 * «Entrar» — não se readquire a câmara (sem piscar, sem «câmara ocupada»).
 */
export function usePrejoin(core: RoomCore, media: LocalMedia, joinIntentRef: { current: boolean }) {
  const { t } = useTranslation()
  const { setStatus } = core
  /** O stream mudou de track: quem mede o nível tem de voltar a ligar-se. */
  const [previewVersion, setPreviewVersion] = useState(0)
  const [previewStream, setPreviewStream] = useState<MediaStream | null>(null)
  const active = core.roomState === 'prejoin'

  useEffect(() => {
    if (!active) return
    let cancelled = false
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
        stream.getTracks().forEach((tr) => tr.stop())
        return
      }
      stream.getVideoTracks().forEach((tr) => (tr.contentHint = 'motion'))
      core.previewStreamRef.current = stream
      core.prejoinPermErrRef.current = permErr
      setPreviewStream(stream)
      const hasVideo = stream.getVideoTracks().length > 0
      media.setHasLocalVideo(hasVideo)
      if (stream.getAudioTracks().length === 0) media.setMicOn(false)
      if (permErr === 'denied' && stream.getTracks().length === 0) setStatus(t('room.estado.permissaoBloqueada'))
      else if (permErr === 'missing') setStatus(t('room.estado.semDispositivoDetectado'))
      else if (stream.getTracks().length === 0) setStatus(t('room.estado.semCamaraNemMicrofone'))
      else if (!hasVideo) setStatus(t('room.estado.camaraIndisponivelEntraAudio'))
      else setStatus('')
      void listDevices()
        .then((d) => {
          if (cancelled) return
          media.setDevices(d)
          media.setMicId(stream.getAudioTracks()[0]?.getSettings().deviceId ?? '')
          media.setCamId(stream.getVideoTracks()[0]?.getSettings().deviceId ?? '')
        })
        .catch(() => {})
    })()
    return () => {
      cancelled = true
      // Handoff: a ENTRAR, o efeito de entrada assume o stream — não se param
      // as tracks. Só se descartam se se sair da pré-entrada sem entrar.
      if (!joinIntentRef.current) {
        core.previewStreamRef.current?.getTracks().forEach((tr) => tr.stop())
        core.previewStreamRef.current = null
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active])

  const attachPreview = useCallback(
    (node: HTMLVideoElement | null) => {
      if (node && node.srcObject !== core.previewStreamRef.current) node.srcObject = core.previewStreamRef.current
    },
    // O `previewVersion` religa o <video> quando a track muda.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [previewStream, previewVersion],
  )

  function toggle(kind: 'mic' | 'cam') {
    const s = core.previewStreamRef.current
    const tr = kind === 'mic' ? s?.getAudioTracks()[0] : s?.getVideoTracks()[0]
    if (!tr) return // sem dispositivo (espectador): nada a alternar
    tr.enabled = !tr.enabled
    if (kind === 'mic') media.setMicOn(tr.enabled)
    else media.setCamOn(tr.enabled)
  }

  async function switchDevice(kind: 'mic' | 'cam', deviceId: string) {
    const s = core.previewStreamRef.current
    if (!s || !deviceId) return
    try {
      const ns = await navigator.mediaDevices.getUserMedia(
        kind === 'mic' ? { audio: audioConstraints(deviceId) } : { video: videoConstraints(deviceId) },
      )
      const nt = kind === 'mic' ? ns.getAudioTracks()[0] : ns.getVideoTracks()[0]
      const old = kind === 'mic' ? s.getAudioTracks()[0] : s.getVideoTracks()[0]
      if (old) {
        nt.enabled = old.enabled // preserva o mute escolhido
        old.stop()
        s.removeTrack(old)
      }
      if (kind === 'cam') nt.contentHint = 'motion'
      s.addTrack(nt)
      if (kind === 'mic') {
        media.setMicId(deviceId)
        media.setMicOn(nt.enabled)
      } else {
        media.setCamId(deviceId)
        media.setHasLocalVideo(true)
        media.setCamOn(nt.enabled)
      }
      setPreviewVersion((n) => n + 1)
    } catch {
      setStatus(t('room.estado.naoTrocouDispositivo'))
    }
  }

  /** Entrar só com áudio: a câmara é libertada ANTES de entrar, não escondida. */
  function dropVideo() {
    const s = core.previewStreamRef.current
    s?.getVideoTracks().forEach((tr) => {
      tr.stop()
      s.removeTrack(tr)
    })
    media.setHasLocalVideo(false)
    media.setCamOn(false)
    setPreviewVersion((n) => n + 1)
  }

  return { previewStream, previewVersion, attachPreview, toggle, switchDevice, dropVideo }
}
