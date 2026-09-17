import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ROTAS_POR_PORTAR } from '../capabilities'
import { ApiError, currentUser, getRoom, isAbort, listMeetings, netProbe, roomWaiting, type WaitingPeer } from '../api'
import { SONDAGENS, type AmostraRede } from './qualidadePrevista'
import { audioConstraints, listDevices, videoConstraints } from '../media'
import { MicMix } from './micMix'
import { reuniaoMaisProxima } from './salaInfo'
import type { LocalMedia } from './useLocalMedia'
import type { RoomCore } from './useRoomCore'

/** O que a pré-entrada sabe da sessão ANTES de entrar — só por REST, nunca por sinalização (R2). */
export interface PrejoinInfo {
  name: string
  /** Sou o dono da sala: entro como anfitrião. */
  owner: boolean
  topology: string
  /** Hora marcada (ISO) da reunião agendada nesta sala, se houver. */
  startsAt: string | null
}

/**
 * Pré-entrada: SÓ media local para a pré-visualização. Não cria Signaling nem
 * chamada (R2: nada de ofertas antes de entrar). O stream fica em
 * `core.previewStreamRef` e é ENTREGUE ao efeito de entrada no clique em
 * «Entrar» — não se readquire a câmara (sem piscar, sem «câmara ocupada»).
 *
 * O mesmo vale para o que se escolhe aqui e continua na sala: o fundo (efeito
 * em `core.effectRef`), a segunda fonte de vídeo (`core.secondSourceRef`) e a
 * mistura de dois microfones (`core.micMixRef`).
 */
export function usePrejoin(core: RoomCore, media: LocalMedia, joinIntentRef: { current: boolean }) {
  const { t } = useTranslation()
  const { setStatus, code } = core
  /** O stream mudou de track: quem mede o nível tem de voltar a ligar-se. */
  const [previewVersion, setPreviewVersion] = useState(0)
  const [previewStream, setPreviewStream] = useState<MediaStream | null>(null)
  const [info, setInfo] = useState<PrejoinInfo | null>(null)
  const [second, setSecond] = useState<{ deviceId: string; stream: MediaStream } | null>(null)
  const [mix, setMix] = useState<{ deviceId: string; mix: MicMix } | null>(null)
  const active = core.roomState === 'prejoin'
  /** Sondagens de rede contra este servidor (qualidade prevista). */
  const [rede, setRede] = useState<{ amostras: AmostraRede[]; estado: 'a-medir' | 'feito' | 'erro' }>({ amostras: [], estado: 'a-medir' })
  const [medicao, setMedicao] = useState(0)
  /** Quem já está à porta — só para quem admite (o servidor responde 403 aos outros). */
  const [waiting, setWaiting] = useState<WaitingPeer[] | null>(null)

  useEffect(() => {
    if (!active || !ROTAS_POR_PORTAR.netProbe) return
    const ctrl = new AbortController()
    let cancelado = false
    setRede({ amostras: [], estado: 'a-medir' })
    void (async () => {
      const amostras: AmostraRede[] = []
      for (let i = 0; i < SONDAGENS; i++) {
        try {
          const r = await netProbe(128 * 1024, ctrl.signal)
          if (cancelado) return
          amostras.push({ downKbps: r.download_kbps, upKbps: r.upload_kbps })
          setRede({ amostras: [...amostras], estado: i === SONDAGENS - 1 ? 'feito' : 'a-medir' })
        } catch (e) {
          if (cancelado || isAbort(e)) return
          // Travão do servidor (30/min) ou rede: fica o que já se mediu.
          setRede({ amostras: [...amostras], estado: amostras.length ? 'feito' : 'erro' })
          return
        }
      }
    })()
    return () => {
      cancelado = true
      ctrl.abort()
    }
  }, [active, medicao])

  useEffect(() => {
    if (!active || !ROTAS_POR_PORTAR.roomWaiting) return
    let parar = false
    let id = 0
    const espreitar = () => {
      void roomWaiting(code)
        .then((l) => {
          if (!parar) setWaiting(l)
        })
        .catch((e) => {
          // 403/404: esta pessoa não admite — não se volta a perguntar.
          if (e instanceof ApiError && (e.status === 403 || e.status === 404)) {
            parar = true
            window.clearInterval(id)
          }
          if (!isAbort(e)) setWaiting(null)
        })
    }
    espreitar()
    id = window.setInterval(espreitar, 5000)
    return () => {
      parar = true
      window.clearInterval(id)
    }
  }, [active, code])

  // A sessão: nome, dono, topologia e hora marcada. Melhor esforço — sem isto
  // a pré-entrada continua a servir, só com menos contexto.
  useEffect(() => {
    if (!active) return
    const ctrl = new AbortController()
    let cancelled = false
    void getRoom(code)
      .then(async (room) => {
        if (cancelled) return
        const base: PrejoinInfo = {
          name: room.name,
          owner: room.owner_id === currentUser()?.id,
          topology: room.topology,
          startsAt: null,
        }
        setInfo(base)
        const meetings = await listMeetings(ctrl.signal).catch((e) => {
          if (!isAbort(e)) console.warn('[prejoin] reuniões indisponíveis', e)
          return []
        })
        if (cancelled) return
        const m = reuniaoMaisProxima(meetings, code, Date.now())
        if (m) setInfo({ ...base, startsAt: m.starts_at })
      })
      .catch(() => {})
    return () => {
      cancelled = true
      ctrl.abort()
    }
  }, [active, code])

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
        core.micMixRef.current?.stop()
        core.micMixRef.current = null
        core.secondSourceRef.current?.getTracks().forEach((tr) => tr.stop())
        core.secondSourceRef.current = null
        core.effectRef.current?.stop()
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active])

  // O <video> mostra o que os outros vão receber: com fundo, a saída do efeito.
  const shownRef = useRef<{ key: string; stream: MediaStream | null }>({ key: '', stream: null })
  const attachPreview = useCallback(
    (node: HTMLVideoElement | null) => {
      if (!node) return
      const fx = media.bgMode !== 'none' ? core.effectRef.current?.output ?? null : null
      const key = fx ? `fx:${fx.id}` : `raw:${previewVersion}`
      if (shownRef.current.key !== key) {
        shownRef.current = { key, stream: fx ? new MediaStream([fx]) : core.previewStreamRef.current }
      }
      if (node.srcObject !== shownRef.current.stream) node.srcObject = shownRef.current.stream
    },
    // O `previewVersion` religa o <video> quando a track muda; o fundo, quando o efeito arranca.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [previewStream, previewVersion, media.bgMode, media.bgImageUrl, media.bgBusy],
  )

  const attachSecond = useCallback(
    (node: HTMLVideoElement | null) => {
      if (node && node.srcObject !== (second?.stream ?? null)) node.srcObject = second?.stream ?? null
    },
    [second],
  )

  function toggle(kind: 'mic' | 'cam') {
    const s = core.previewStreamRef.current
    const tr = kind === 'mic' ? s?.getAudioTracks()[0] : s?.getVideoTracks()[0]
    if (!tr) return // sem dispositivo (espectador): nada a alternar
    tr.enabled = !tr.enabled
    if (kind === 'mic') media.setMicOn(tr.enabled)
    else media.setCamOn(tr.enabled)
  }

  function stopSecond() {
    core.secondSourceRef.current?.getTracks().forEach((tr) => tr.stop())
    core.secondSourceRef.current = null
    setSecond(null)
  }

  /** Desfaz a mistura: o primeiro microfone volta a ser a track do stream. */
  function unmix() {
    const s = core.previewStreamRef.current
    const cur = core.micMixRef.current
    if (!s || !cur) return
    const primary = cur.inputs[0].clone()
    primary.enabled = cur.output.enabled
    s.removeTrack(cur.output)
    cur.stop()
    core.micMixRef.current = null
    s.addTrack(primary)
    setMix(null)
    setPreviewVersion((n) => n + 1)
  }

  async function switchDevice(kind: 'mic' | 'cam', deviceId: string) {
    const s = core.previewStreamRef.current
    if (!s || !deviceId) return
    if (kind === 'mic' && core.micMixRef.current) unmix()
    // A câmara escolhida como principal deixa de poder ser a segunda fonte.
    if (kind === 'cam' && second?.deviceId === deviceId) stopSecond()
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
        // O fundo segue a câmara nova.
        const fx = core.effectRef.current
        if (fx?.started && media.bgMode !== 'none') {
          fx.stop()
          await fx.start(nt)
        }
      }
      setPreviewVersion((n) => n + 1)
    } catch {
      setStatus(t('room.estado.naoTrocouDispositivo'))
    }
  }

  /** Segunda fonte de vídeo (ex.: captura HDMI dos diapositivos). Entra na sala como apresentação. */
  async function toggleSecondCam(deviceId: string) {
    if (second?.deviceId === deviceId) {
      stopSecond()
      return
    }
    if (!deviceId || deviceId === media.camId) return
    stopSecond()
    try {
      const s = await navigator.mediaDevices.getUserMedia({ video: videoConstraints(deviceId) })
      // Diapositivos e quadros: nitidez antes de fluidez, como numa partilha de ecrã.
      s.getVideoTracks().forEach((tr) => (tr.contentHint = 'detail'))
      core.secondSourceRef.current = s
      setSecond({ deviceId, stream: s })
    } catch {
      setStatus(t('room.estado.naoTrocouDispositivo'))
    }
  }

  /** Segundo microfone activo ao mesmo tempo que o primeiro, misturado localmente. */
  async function toggleSecondMic(deviceId: string) {
    if (mix?.deviceId === deviceId) {
      unmix()
      return
    }
    if (core.micMixRef.current) unmix()
    const s = core.previewStreamRef.current
    const primary = s?.getAudioTracks()[0]
    if (!s || !primary || !deviceId || deviceId === media.micId) return
    try {
      const ns = await navigator.mediaDevices.getUserMedia({ audio: audioConstraints(deviceId) })
      const extra = ns.getAudioTracks()[0]
      const m = new MicMix([primary, extra])
      m.output.enabled = primary.enabled
      primary.enabled = true
      s.removeTrack(primary)
      s.addTrack(m.output)
      core.micMixRef.current = m
      setMix({ deviceId, mix: m })
      setPreviewVersion((n) => n + 1)
    } catch {
      setStatus(t('room.estado.naoTrocouDispositivo'))
    }
  }

  /** Streams só para medir o nível de CADA microfone da mistura. */
  const mixLevels = useMemo(
    () => (mix ? [new MediaStream([mix.mix.inputs[0]]), new MediaStream([mix.mix.inputs[1]])] : null),
    [mix],
  )

  /** Entrar só com áudio: a câmara é libertada ANTES de entrar, não escondida. */
  function dropVideo() {
    const s = core.previewStreamRef.current
    s?.getVideoTracks().forEach((tr) => {
      tr.stop()
      s.removeTrack(tr)
    })
    stopSecond()
    media.clearBackground()
    media.setHasLocalVideo(false)
    media.setCamOn(false)
    setPreviewVersion((n) => n + 1)
  }

  return {
    info,
    rede,
    medirDeNovo: () => setMedicao((n) => n + 1),
    waiting,
    previewStream,
    previewVersion,
    attachPreview,
    toggle,
    switchDevice,
    dropVideo,
    second,
    attachSecond,
    toggleSecondCam,
    mixDeviceId: mix?.deviceId ?? '',
    mixLevels,
    toggleSecondMic,
  }
}

export type Prejoin = ReturnType<typeof usePrejoin>
