import { CSSProperties, useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  audioConstraints,
  BackgroundEffect,
  Denoiser,
  DeviceSets,
  HeadTracker,
  playTestTone,
  presetBackgrounds,
  videoConstraints,
} from '../media'
import type { RoomCore } from './useRoomCore'

export type BgMode = 'none' | 'blur' | 'image'
export type BlurLevel = 'light' | 'strong'

/**
 * A media LOCAL: microfone, câmara, dispositivos, supressão de ruído, efeitos
 * de fundo e a inclinação 3D. Tudo o que muda o que os outros recebem de mim.
 */
export function useLocalMedia(core: RoomCore) {
  const { t } = useTranslation()
  const { signal, setStatus } = core

  const [micOn, setMicOn] = useState(true)
  const [camOn, setCamOn] = useState(true)
  const [hasLocalVideo, setHasLocalVideo] = useState(true)
  /** Regra do anfitrião «silenciar todos sem voltar a ligar» (R92). */
  const [allowUnmute, setAllowUnmute] = useState(true)
  const [devices, setDevices] = useState<DeviceSets>({ mics: [], cams: [], speakers: [] })
  const [micId, setMicId] = useState('')
  const [camId, setCamId] = useState('')
  const [speakerId, setSpeakerId] = useState('')
  // Supressão de ruído por IA (RNNoise) — LIGADA por omissão. Se falhar, fica
  // a track crua com a supressão nativa do browser.
  const [noiseSuppression, setNoiseSuppression] = useState(true)

  const [bgMode, setBgMode] = useState<BgMode>('none')
  const [bgImageUrl, setBgImageUrl] = useState('')
  const [bgBusy, setBgBusy] = useState(false)
  const [blurLevel, setBlurLevel] = useState<BlurLevel>('strong')
  const presets = useMemo(() => presetBackgrounds(), [])

  /** Volume do que se OUVE (0–100), só neste dispositivo. */
  const [outputVolume, setOutputVolumeState] = useState(() => {
    try {
      const v = Number(localStorage.getItem('dx_out_vol'))
      return Number.isFinite(v) && v > 0 && v <= 100 ? v : 100
    } catch {
      return 100
    }
  })
  const setOutputVolume = useCallback((v: number) => {
    const n = Math.max(0, Math.min(100, Math.round(v)))
    setOutputVolumeState(n)
    try {
      localStorage.setItem('dx_out_vol', String(n))
    } catch {
      /* sem armazenamento: vale para esta sessão */
    }
  }, [])

  const [parallax, setParallax] = useState(false)
  const [tilt, setTilt] = useState({ x: 0, y: 0 })

  useEffect(() => {
    core.bgModeRef.current = bgMode
  }, [bgMode, core.bgModeRef])

  // ── O que o anfitrião impõe. Vem SEMPRE do servidor. ──────────────────────
  useEffect(() => {
    const offs = [
      signal.on('force-muted', () => {
        const track = core.localStreamRef.current?.getAudioTracks()[0]
        if (track) track.enabled = false
        setMicOn(false)
        setStatus(t('room.estado.anfitriaoSilenciouTe'))
      }),
      signal.on('force-cam-off', () => {
        const track = core.localStreamRef.current?.getVideoTracks()[0]
        if (track) track.enabled = false
        setCamOn(false)
        setStatus(t('room.estado.anfitriaoDesligouCamara'))
      }),
      signal.on('muted-all', (m) => {
        const track = core.localStreamRef.current?.getAudioTracks()[0]
        if (track) track.enabled = false
        setMicOn(false)
        setAllowUnmute(m.allow_unmute)
        setStatus(m.allow_unmute ? t('room.estado.anfitriaoSilenciouTodos') : t('room.estado.anfitriaoSilenciouSemVolta'))
      }),
      signal.on('room-settings', (m) => setAllowUnmute(m.allow_unmute ?? true)),
    ]
    return () => offs.forEach((off) => off())
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signal])

  // Difunde o estado de câmara/mic sempre que muda. No SFU o ecrã é track
  // separada; no mesh a partilha substitui a câmara e conta como vídeo ligado.
  useEffect(() => {
    const meshSharing = core.sharing && core.topology !== 'sfu'
    if (core.roomState === 'in') signal.send({ type: 'media', cam: (camOn && hasLocalVideo) || meshSharing, mic: micOn })
  }, [camOn, micOn, hasLocalVideo, core.sharing, core.roomState, core.topology, signal])

  /** Aplica (ou retira) a supressão por IA e devolve a track a enviar. */
  const denoiseMic = useCallback(
    async (rawTrack: MediaStreamTrack, ns: boolean): Promise<MediaStreamTrack> => {
      core.denoiserRef.current?.stop()
      core.denoiserRef.current = null
      core.rawMicRef.current?.stop()
      core.rawMicRef.current = null
      if (!ns) return rawTrack
      try {
        const d = new Denoiser()
        const clean = await d.process(rawTrack)
        clean.enabled = rawTrack.enabled
        core.denoiserRef.current = d
        core.rawMicRef.current = rawTrack // mantém-se viva: alimenta o denoiser
        return clean
      } catch (e) {
        console.warn('[denoise] RNNoise indisponível — supressão nativa do browser', e)
        return rawTrack
      }
    },
    [core.denoiserRef, core.rawMicRef],
  )

  async function toggleMic() {
    // Guarda de INTERFACE: a de servidor é não reencaminhar áudio de quem está
    // silenciado. Esta só evita que a pessoa julgue que está a falar.
    if (!allowUnmute && !micOn && !core.isHost) {
      setStatus(t('room.estado.anfitriaoNaoPermiteMic'))
      return
    }
    core.levelsRef.current?.resume()
    const track = core.localStreamRef.current?.getAudioTracks()[0]
    if (track) {
      track.enabled = !track.enabled
      setMicOn(track.enabled)
      return
    }
    // Sem track (espectador ou mic negado à entrada): adquire agora.
    try {
      const s = await navigator.mediaDevices.getUserMedia({ audio: audioConstraints(micId || undefined) })
      const newTrack = s.getAudioTracks()[0]
      if (!newTrack) throw new Error('sem microfone')
      core.localStreamRef.current?.addTrack(newTrack)
      await core.callRef.current?.replaceAudioTrack(newTrack)
      if (core.localStreamRef.current) core.levelsRef.current?.watch('me', core.localStreamRef.current)
      setMicId(newTrack.getSettings().deviceId ?? '')
      setMicOn(true)
    } catch {
      setStatus(t('room.estado.semAcessoMicrofone'))
    }
  }

  function stopParallax() {
    core.headRef.current?.stop()
    core.headRef.current = null
    setParallax(false)
    setTilt({ x: 0, y: 0 })
  }

  async function toggleCam() {
    const existing = core.localStreamRef.current?.getVideoTracks()[0]
    if (existing && camOn) {
      // DESLIGAR liberta mesmo a câmara — o LED apaga e o encoder pára. O
      // transceiver fica de pé, por isso voltar a ligar não renegoceia.
      setCamOn(false)
      setHasLocalVideo(false)
      await core.callRef.current?.disableVideo()
      if (parallax) stopParallax()
      core.effectRef.current?.stop()
      core.localStreamRef.current?.removeTrack(existing)
      existing.stop()
      core.cameraTrackRef.current = null
      if (core.localVideoRef.current) core.localVideoRef.current.srcObject = core.localStreamRef.current
      return
    }
    try {
      const s = await navigator.mediaDevices.getUserMedia({ video: videoConstraints(camId || undefined) })
      const track = s.getVideoTracks()[0]
      track.contentHint = 'motion'
      core.cameraTrackRef.current = track
      core.localStreamRef.current?.addTrack(track)
      setHasLocalVideo(true)
      setCamOn(true)
      let sendTrack = track
      if (bgMode !== 'none' && core.effectRef.current) sendTrack = await core.effectRef.current.start(track)
      await core.callRef.current?.enableVideo(sendTrack, core.localStreamRef.current ?? s)
      if (core.localVideoRef.current) {
        core.localVideoRef.current.srcObject = bgMode !== 'none' ? new MediaStream([sendTrack]) : core.localStreamRef.current
      }
      setStatus('')
    } catch {
      setStatus(t('room.estado.semAcessoCamara'))
    }
  }

  async function switchMic(deviceId: string, ns = noiseSuppression) {
    try {
      // Escolher UM microfone desfaz a mistura de dois da pré-entrada.
      core.micMixRef.current?.stop()
      core.micMixRef.current = null
      const s = await navigator.mediaDevices.getUserMedia({ audio: audioConstraints(deviceId || undefined) })
      const raw = s.getAudioTracks()[0]
      raw.enabled = micOn
      // O deviceId lê-se ANTES do denoise: a track limpa não o tem.
      const devId = raw.getSettings().deviceId ?? deviceId
      const sendTrack = await denoiseMic(raw, ns)
      sendTrack.enabled = micOn
      const old = core.localStreamRef.current?.getAudioTracks()[0]
      if (old) {
        core.localStreamRef.current?.removeTrack(old)
        old.stop()
      }
      core.localStreamRef.current?.addTrack(sendTrack)
      await core.callRef.current?.replaceAudioTrack(sendTrack)
      if (core.localStreamRef.current) core.levelsRef.current?.watch('me', core.localStreamRef.current)
      setMicId(devId)
    } catch {
      setStatus(t('room.estado.naoMudouMicrofone'))
    }
  }

  async function switchCam(deviceId: string) {
    try {
      const s = await navigator.mediaDevices.getUserMedia({ video: videoConstraints(deviceId || undefined) })
      const newTrack = s.getVideoTracks()[0]
      newTrack.contentHint = 'motion'
      newTrack.enabled = camOn
      const old = core.cameraTrackRef.current
      core.cameraTrackRef.current = newTrack
      if (old) {
        core.localStreamRef.current?.removeTrack(old)
        old.stop()
      }
      core.localStreamRef.current?.addTrack(newTrack)
      setHasLocalVideo(true)
      let sendTrack = newTrack
      if (bgMode !== 'none' && core.effectRef.current) {
        core.effectRef.current.stop()
        sendTrack = await core.effectRef.current.start(newTrack)
      }
      // A partilhar ecrã, a câmara não é o que está a ser enviado.
      if (!core.sharing) {
        await core.callRef.current?.replaceVideoTrack(sendTrack)
        if (core.localVideoRef.current) {
          core.localVideoRef.current.srcObject = bgMode !== 'none' ? new MediaStream([sendTrack]) : core.localStreamRef.current
        }
      }
      setCamId(newTrack.getSettings().deviceId ?? deviceId)
    } catch {
      setStatus(t('room.estado.naoMudouCamara'))
    }
  }

  async function toggleNoiseSuppression() {
    const next = !noiseSuppression
    setNoiseSuppression(next)
    const mix = core.micMixRef.current
    if (!mix) {
      await switchMic(micId, next)
      return
    }
    // Com dois microfones misturados, reprocessa-se a MISTURA — voltar a pedir
    // um só microfone desfazia a escolha da pré-entrada sem aviso.
    try {
      const sendTrack = await denoiseMic(mix.freshOutput(), next)
      sendTrack.enabled = micOn
      const old = core.localStreamRef.current?.getAudioTracks()[0]
      if (old) {
        core.localStreamRef.current?.removeTrack(old)
        old.stop()
      }
      core.localStreamRef.current?.addTrack(sendTrack)
      await core.callRef.current?.replaceAudioTrack(sendTrack)
      if (core.localStreamRef.current) core.levelsRef.current?.watch('me', core.localStreamRef.current)
    } catch {
      setStatus(t('room.estado.naoMudouMicrofone'))
    }
  }

  /**
   * Efeitos de fundo (IA local, nada sai do dispositivo). Trocar entre
   * desfoque e imagem é instantâneo: é o mesmo pipeline.
   */
  async function applyBackground(mode: BgMode, imageUrl?: string, blur: BlurLevel = blurLevel) {
    // Na pré-entrada ainda não há câmara da chamada: o efeito aplica-se à da
    // pré-visualização e segue com ela para a sala.
    const cam =
      core.cameraTrackRef.current ?? (core.roomState === 'prejoin' ? core.previewStreamRef.current?.getVideoTracks()[0] ?? null : null)
    if (bgBusy || !cam) return
    setBgBusy(true)
    try {
      if (mode === 'none') {
        const raw = core.effectRef.current?.stop() ?? cam
        if (!core.sharing && raw) {
          await core.callRef.current?.replaceVideoTrack(raw)
          if (core.localVideoRef.current) core.localVideoRef.current.srcObject = core.localStreamRef.current
        }
        setBgMode('none')
        setBgImageUrl('')
        return
      }
      core.effectRef.current = core.effectRef.current ?? new BackgroundEffect()
      const effect = core.effectRef.current
      effect.blurPx = blur === 'light' ? 10 : 24
      setBlurLevel(blur)
      if (mode === 'image' && imageUrl) await effect.setImage(imageUrl)
      else effect.mode = 'blur'
      if (!effect.started) {
        const processed = await effect.start(cam)
        if (!core.sharing) {
          await core.callRef.current?.replaceVideoTrack(processed)
          if (core.localVideoRef.current) core.localVideoRef.current.srcObject = new MediaStream([processed])
        }
      }
      setBgMode(mode)
      setBgImageUrl(mode === 'image' ? imageUrl ?? '' : '')
    } catch (e) {
      console.warn('[background]', e)
      setStatus(t('room.estado.fundoIndisponivel'))
    } finally {
      setBgBusy(false)
    }
  }

  /** Sem câmara (entrar só com áudio): o efeito pára e o estado volta a «sem fundo». */
  function clearBackground() {
    core.effectRef.current?.stop()
    setBgMode('none')
    setBgImageUrl('')
  }

  function uploadBackground(file: File | null) {
    if (!file) return
    void applyBackground('image', URL.createObjectURL(file))
  }

  /** Sala 3D: segue a cabeça e inclina a grelha. */
  async function toggleParallax() {
    if (parallax) {
      stopParallax()
      return
    }
    if (!core.cameraTrackRef.current) {
      setStatus(t('room.estado.efeito3dPrecisaCamara'))
      return
    }
    try {
      const h = new HeadTracker()
      h.onUpdate = (x, y) => setTilt({ x, y })
      await h.start(new MediaStream([core.cameraTrackRef.current]))
      core.headRef.current = h
      setParallax(true)
      setStatus(t('room.estado.efeito3dLigado'))
    } catch {
      setStatus(t('room.estado.efeito3dIndisponivel'))
    }
  }

  // O overscan (scale 1.2) mantém a área maior do que a moldura: a rotação
  // nunca revela o fundo nos cantos.
  const parallaxStyle: CSSProperties = parallax
    ? {
        transform: `perspective(1400px) rotateY(${tilt.x * 7}deg) rotateX(${tilt.y * -7}deg) scale(1.2) translate(${tilt.x * 2.8}%, ${tilt.y * 2.8}%)`,
        transformOrigin: 'center center',
        transition: 'transform 0.12s cubic-bezier(0.22, 0.61, 0.36, 1)',
        willChange: 'transform',
      }
    : {}

  /**
   * Ref de callback do vídeo local: o `srcObject` não é prop do React e perde-se
   * quando o `<video>` REMONTA (o meu retrato muda da grelha para a fila ao
   * iniciar uma partilha). Ao montar, volta a ligar a câmara ou a saída do efeito.
   */
  const attachLocalVideo = useCallback(
    (node: HTMLVideoElement | null) => {
      core.localVideoRef.current = node
      if (!node || node.srcObject) return
      const eff = core.bgModeRef.current !== 'none' ? core.effectRef.current?.output ?? null : null
      node.srcObject = eff ? new MediaStream([eff]) : core.localStreamRef.current
    },
    [core.localVideoRef, core.bgModeRef, core.effectRef, core.localStreamRef],
  )

  return {
    micOn,
    setMicOn,
    camOn,
    setCamOn,
    hasLocalVideo,
    setHasLocalVideo,
    allowUnmute,
    devices,
    setDevices,
    micId,
    setMicId,
    camId,
    setCamId,
    speakerId,
    setSpeakerId,
    noiseSuppression,
    setNoiseSuppression,
    denoiseMic,
    toggleMic,
    toggleCam,
    switchMic,
    switchCam,
    toggleNoiseSuppression,
    testSpeaker: () => void playTestTone(speakerId),
    bgMode,
    bgImageUrl,
    bgBusy,
    blurLevel,
    presets,
    applyBackground,
    clearBackground,
    uploadBackground,
    outputVolume,
    setOutputVolume,
    parallax,
    parallaxStyle,
    toggleParallax,
    attachLocalVideo,
  }
}

export type LocalMedia = ReturnType<typeof useLocalMedia>
