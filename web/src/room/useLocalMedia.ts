import { CSSProperties, useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { readJoinPrefs } from '../accountPrefs'
import {
  audioConstraints,
  BackgroundEffect,
  Denoiser,
  DeviceSets,
  type Facing,
  facingOf,
  HeadTracker,
  playTestTone,
  presetBackgrounds,
  videoConstraints,
  withNativeNoiseSuppression,
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
  /**
   * Para que lado a câmara actual aponta, quando o browser o diz. `undefined`
   * num computador — e é por isso que serve para decidir se o botão de virar
   * aparece: só faz sentido onde há frente e trás.
   */
  const [facing, setFacing] = useState<Facing | undefined>(undefined)
  const [speakerId, setSpeakerId] = useState('')
  // Supressão de ruído por IA (RNNoise) — LIGADA por omissão, ou o que a
  // pessoa escolheu em Definições ➜ Conta ➜ Preferências ao entrar. Se
  // falhar, o microfone volta à supressão nativa do browser (ver `denoiseMic`).
  const [noiseSuppression, setNoiseSuppression] = useState(() => readJoinPrefs().noiseSuppression)

  const [bgMode, setBgMode] = useState<BgMode>(() => (readJoinPrefs().blurByDefault ? 'blur' : 'none'))
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
      // Webinar (A-09): o SFU reteve o que esta pessoa publicou — está fora de
      // palco. Desliga-se o que ia a caminho em vez de o deixar a mandar media
      // que ninguém recebe.
      signal.on('publish-refused', () => {
        const stream = core.localStreamRef.current
        stream?.getAudioTracks().forEach((tr) => (tr.enabled = false))
        stream?.getVideoTracks().forEach((tr) => (tr.enabled = false))
        setMicOn(false)
        setCamOn(false)
        setStatus(t('room.webinar.publicacaoRetida'))
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
    // Só `signal`: estes handlers registam-se uma vez por LIGAÇÃO e o cleanup
    // tira-os. Com `t` ou `setStatus` nas dependências, trocar de língua a meio
    // de uma chamada desmontava e reregistava todos os handlers — e os eventos
    // que chegassem no intervalo perdiam-se. O `core.localStreamRef` é um ref:
    // estável por definição, e o ESLint só o acusa por vir dentro de `core`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signal])

  // Difunde o estado de câmara/mic sempre que muda. No SFU o ecrã é track
  // separada; no mesh a partilha substitui a câmara e conta como vídeo ligado.
  useEffect(() => {
    const meshSharing = core.sharing && core.topology !== 'sfu'
    if (core.roomState === 'in') signal.send({ type: 'media', cam: (camOn && hasLocalVideo) || meshSharing, mic: micOn })
  }, [camOn, micOn, hasLocalVideo, core.sharing, core.roomState, core.topology, signal])

  /**
   * Aplica (ou retira) a cadeia de voz e devolve a track a ENVIAR.
   *
   * É a ÚNICA porta por onde um microfone chega à chamada, e garante que há
   * sempre UMA supressão de ruído — nem duas, nem nenhuma:
   *
   *  - `ns` ligado: o RNNoise trata do ruído e a supressão nativa do browser
   *    sai (duas em série não limpam o dobro). Se o RNNoise não carregar, a
   *    nativa VOLTA — é o recuo, e é o que impede um microfone sem supressão;
   *  - `ns` desligado: o processamento nativo do browser, como sempre.
   *
   * Quem abre o microfone pede-o já no estado certo
   * (`audioConstraints(id, !ns)`), e por isso no caminho normal nada se reabre.
   * Quando não bate certo (a opção mudou na pré-entrada depois de o microfone
   * estar aberto, ou o RNNoise falhou), o `withNativeNoiseSuppression` reabre
   * o microfone — e a track devolvida pode ser OUTRA que não a recebida.
   *
   * `source: 'mix'` é a mistura de dois microfones: não tem dispositivo que se
   * possa reabrir, e as duas entradas já vêm com a supressão nativa ligada
   * (ver `toggleSecondMic` na pré-entrada).
   */
  const denoiseMic = useCallback(
    async (rawTrack: MediaStreamTrack, ns: boolean, source: 'device' | 'mix' = 'device'): Promise<MediaStreamTrack> => {
      core.denoiserRef.current?.stop()
      core.denoiserRef.current = null
      core.rawMicRef.current?.stop()
      core.rawMicRef.current = null
      const native = (track: MediaStreamTrack, on: boolean) =>
        source === 'mix' ? Promise.resolve(track) : withNativeNoiseSuppression(track, on)
      if (!ns) return native(rawTrack, true)
      const raw = await native(rawTrack, false)
      try {
        const d = new Denoiser()
        const clean = await d.process(raw)
        clean.enabled = raw.enabled
        core.denoiserRef.current = d
        core.rawMicRef.current = raw // mantém-se viva: alimenta o denoiser
        return clean
      } catch (e) {
        console.warn('[denoise] RNNoise indisponível — supressão nativa do browser', e)
        return native(raw, true)
      }
    },
    [core.denoiserRef, core.rawMicRef],
  )

  /** Desliga o microfone e a câmara sem os pedir de volta — quem assiste a um
   *  webinar (A-09). Quem for promovido liga-os pelos controlos, como sempre. */
  function pararDePublicar() {
    const stream = core.localStreamRef.current
    stream?.getAudioTracks().forEach((tr) => (tr.enabled = false))
    stream?.getVideoTracks().forEach((tr) => (tr.enabled = false))
    setMicOn(false)
    setCamOn(false)
  }

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
    // Sem track (espectador ou mic negado à entrada): adquire agora — e pela
    // MESMA cadeia de voz que os outros caminhos. Este entrava com a track
    // crua mesmo com a supressão por IA ligada: quem ligava o microfone tarde
    // soava diferente de quem entrava com ele.
    try {
      const s = await navigator.mediaDevices.getUserMedia({ audio: audioConstraints(micId || undefined, !noiseSuppression) })
      const raw = s.getAudioTracks()[0]
      if (!raw) throw new Error('sem microfone')
      // O deviceId lê-se ANTES do denoise: a track limpa não o tem.
      const devId = raw.getSettings().deviceId ?? ''
      const newTrack = await denoiseMic(raw, noiseSuppression)
      core.localStreamRef.current?.addTrack(newTrack)
      await core.callRef.current?.replaceAudioTrack(newTrack)
      if (core.localStreamRef.current) core.levelsRef.current?.watch('me', core.localStreamRef.current)
      setMicId(devId)
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
      // Sem `camId` ainda (primeira vez), pede-se o lado que a pessoa tinha —
      // ou nenhum, e o browser escolhe. No telemóvel é aqui que o `facing`
      // fica a saber-se.
      const s = await navigator.mediaDevices.getUserMedia({
        video: videoConstraints(camId || undefined, facing),
      })
      const track = s.getVideoTracks()[0]
      track.contentHint = 'motion'
      core.cameraTrackRef.current = track
      core.localStreamRef.current?.addTrack(track)
      setHasLocalVideo(true)
      setCamOn(true)
      setFacing(facingOf(track))
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
      // Com a cadeia de voz (`ns`), o microfone pede-se SEM a supressão nativa.
      // Se for o MESMO microfone que já está aberto, o Chromium devolve-o com
      // o processamento que tinha (uma fonte por microfone) — quem acerta é o
      // `denoiseMic`, a seguir.
      const s = await navigator.mediaDevices.getUserMedia({ audio: audioConstraints(deviceId || undefined, !ns) })
      const raw = s.getAudioTracks()[0]
      raw.enabled = micOn
      // O deviceId lê-se ANTES do denoise: a track limpa não o tem.
      const devId = raw.getSettings().deviceId ?? deviceId
      // A pista que se estava a enviar pára ANTES de acertar a nova, e não
      // depois: sem RNNoise ela É a pista do microfone, e enquanto viver
      // prende-lhe o processamento — a nova reabria com a supressão antiga.
      // Só se pára depois de o microfone novo ter aberto (acima): se o pedido
      // falhar, a chamada fica com o microfone que tinha.
      const old = core.localStreamRef.current?.getAudioTracks()[0]
      if (old) {
        core.localStreamRef.current?.removeTrack(old)
        old.stop()
      }
      const sendTrack = await denoiseMic(raw, ns)
      sendTrack.enabled = micOn
      core.localStreamRef.current?.addTrack(sendTrack)
      await core.callRef.current?.replaceAudioTrack(sendTrack)
      if (core.localStreamRef.current) core.levelsRef.current?.watch('me', core.localStreamRef.current)
      setMicId(devId)
    } catch {
      setStatus(t('room.estado.naoMudouMicrofone'))
    }
  }

  /**
   * Troca a câmara por `deviceId`, ou — quando o `deviceId` vem vazio e há um
   * `facing` — pelo LADO para que ela aponta.
   *
   * O caminho do lado existe para o telemóvel: lá, o `deviceId` não é de
   * confiança (etiquetas vazias antes da permissão, ids que rodam entre
   * sessões) e o `facingMode` é o que o browser sabe responder.
   */
  async function switchCam(deviceId: string, facing?: Facing) {
    try {
      const s = await navigator.mediaDevices.getUserMedia({
        video: videoConstraints(deviceId || undefined, facing),
      })
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
      setFacing(facingOf(newTrack))
    } catch {
      setStatus(t('room.estado.naoMudouCamara'))
    }
  }

  /**
   * Vira a câmara: frontal ↔ traseira. É o gesto que falta no telemóvel, e a
   * razão de ele não existir antes é que o código só sabia escolher câmaras por
   * `deviceId` — que no telemóvel não diz para onde elas apontam.
   *
   * Pede o lado CONTRÁRIO ao da pista actual. Se o browser não disser o lado
   * (um computador), assume-se que se está na frontal e pede-se a traseira: no
   * pior caso o `ideal` devolve a mesma câmara e nada se estraga.
   */
  async function flipCam() {
    const actual = facingOf(core.cameraTrackRef.current) ?? 'user'
    await switchCam('', actual === 'user' ? 'environment' : 'user')
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
      const sendTrack = await denoiseMic(mix.freshOutput(), next, 'mix')
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
    pararDePublicar,
    toggleCam,
    switchMic,
    switchCam,
    flipCam,
    facing,
    setFacing,
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
