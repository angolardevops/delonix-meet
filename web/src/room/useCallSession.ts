import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError, currentUser, iceServers, isAbort, joinRoom, postTimings } from '../api'
import { audioConstraints, LevelWatcher, listDevices, videoConstraints } from '../media'
import { deriveRoomKey, e2eeSupported, FrameCrypto } from '../e2ee'
import { Signaling } from '../signaling'
import { MeshCall, SfuCall } from '../webrtc'
import { backoffDelay, type CallState } from '../callRecovery'
import { LinhaDoTempo } from '../callTimings'
import { makeCallHolderStart } from '../sfuLifecycle'
import type { LocalMedia } from './useLocalMedia'
import type { RoomCore } from './useRoomCore'

/** A entrada salta a pré-entrada? Voz atendida, ou reentrada recente (< 60 s). */
export function entradaDirecta(code: string, voiceOnly: boolean): boolean {
  const recent = Date.now() - Number(sessionStorage.getItem(`dx_rejoin_${code}`) || 0) < 60_000
  return Boolean(voiceOnly) || recent
}

/**
 * A SESSÃO: token, E2EE, Signaling, chamada SFU/mesh, admissão, recuperação e
 * saída. É o dono do efeito de entrada — o resto da sala regista-se no
 * barramento (`core.signal`) e recebe as mensagens quando o socket existir.
 */
export function useCallSession(
  core: RoomCore,
  media: LocalMedia,
  opts: { voiceOnly: boolean; onLeave: () => void; onSwitch?: (code: string) => void },
) {
  const { t } = useTranslation()
  const { code, signal: bus, setStatus, setRoomState, setPeers, setPresentation } = core
  const { voiceOnly, onSwitch } = opts

  const [joinIntent, setJoinIntent] = useState<boolean>(() => entradaDirecta(code, voiceOnly))
  const joinIntentRef = useRef(joinIntent)
  /** Entrou-se «só com áudio» pela pré-entrada: não é uma câmara avariada. */
  const audioOnlyRef = useRef(voiceOnly)
  const [passTry, setPassTry] = useState(0)
  const [callState, setCallState] = useState<CallState>('connecting')
  const temposEnviados = useRef(false)
  const [isTraining, setIsTraining] = useState(false)
  const [isInstant, setIsInstant] = useState(false)
  const [waitingRoomOn, setWaitingRoomOn] = useState(false)
  const [roomName, setRoomName] = useState('')
  const [canAdmit, setCanAdmit] = useState(false)
  const [e2eeOn, setE2eeOn] = useState(false)
  const [secCode, setSecCode] = useState('')
  /**
   * Esta conta já está na reunião noutro dispositivo (R114). Quem entra em
   * segundo lugar entra MUDO nos dois sentidos, e é avisado de porquê.
   */
  const [companion, setCompanion] = useState(false)

  // ── Efeito de entrada ──────────────────────────────────────────────────────
  useEffect(() => {
    if (!joinIntent) return // à espera do «Entrar» da pré-entrada
    let cancelled = false
    let signal: Signaling | null = null
    let tentativas = 0
    const MAX_TENTATIVAS = 6

    async function start() {
      try {
        let permErr = ''
        const getMedia = (c: MediaStreamConstraints) =>
          navigator.mediaDevices.getUserMedia(c).catch((e: DOMException) => {
            if (e?.name === 'NotAllowedError' || e?.name === 'SecurityError') permErr = 'denied'
            else if (e?.name === 'NotReadableError' && !permErr) permErr = 'busy'
            else if (e?.name === 'NotFoundError' && !permErr) permErr = 'missing'
            throw e
          })
        // Handoff da pré-entrada: reutiliza o stream (dispositivos e toggles
        // escolhidos lá). Sem pré-entrada (voz, reentrada), adquire.
        // Numa nova tentativa (rede em baixo) reaproveita-se o que já se tinha.
        const retomado = tentativas > 0 ? core.localStreamRef.current : null
        const handoff = retomado ?? core.previewStreamRef.current
        core.previewStreamRef.current = null
        if (handoff && !retomado) permErr = core.prejoinPermErrRef.current
        const stream =
          handoff ??
          (await getMedia(voiceOnly ? { audio: audioConstraints() } : { audio: audioConstraints(), video: videoConstraints() })
            .catch(() => getMedia({ audio: audioConstraints() }))
            .catch(() => new MediaStream()))
        stream.getVideoTracks().forEach((tr) => (tr.contentHint = 'motion'))
        if (cancelled) {
          stream.getTracks().forEach((tr) => tr.stop())
          return
        }
        const spectator = stream.getTracks().length === 0
        const hasVideo = stream.getVideoTracks().length > 0
        if (permErr === 'denied' && spectator) setStatus(t('room.estado.permissaoBloqueada'))
        else if (permErr === 'denied' && !hasVideo) setStatus(t('room.estado.camaraBloqueadaEntraAudio'))
        else if (permErr === 'missing') setStatus(t('room.estado.semDispositivoDetectado'))
        else if (spectator) setStatus(t('room.estado.modoEspectadorSemMedia'))
        // Pediu câmara e só veio áudio: quase sempre está ocupada por outra app.
        else if (!audioOnlyRef.current && !hasVideo) setStatus(t('room.estado.camaraEmUso'))
        if (stream.getAudioTracks().length === 0) media.setMicOn(false)
        media.setHasLocalVideo(hasVideo)
        core.localStreamRef.current = stream
        core.cameraTrackRef.current = stream.getVideoTracks()[0] ?? null

        // RNNoise ANTES de a chamada começar. Fallback: mantém a crua.
        const initRaw = stream.getAudioTracks()[0]
        const initMicId = initRaw?.getSettings().deviceId ?? ''
        if (initRaw && media.noiseSuppression && !retomado) {
          const clean = await media.denoiseMic(initRaw, true)
          if (clean !== initRaw) {
            clean.enabled = initRaw.enabled // preserva o mute da pré-entrada
            stream.removeTrack(initRaw)
            stream.addTrack(clean)
          }
        }
        if (core.localVideoRef.current) core.localVideoRef.current.srcObject = stream

        core.levelsRef.current?.close()
        const levels = new LevelWatcher(core.updateSpeaking)
        core.levelsRef.current = levels
        levels.watch('me', stream)
        void listDevices()
          .then((d) => {
            if (cancelled) return
            media.setDevices(d)
            media.setMicId(initMicId || stream.getAudioTracks()[0]?.getSettings().deviceId || '')
            media.setCamId(stream.getVideoTracks()[0]?.getSettings().deviceId ?? '')
          })
          .catch(() => {})
        navigator.mediaDevices.addEventListener?.('devicechange', onDeviceChange)

        // A linha do tempo começa AQUI: é o tempo de quem quis entrar que
        // interessa medir, não o do código.
        const tempos = new LinhaDoTempo()
        tempos.marcar('intencao')
        const [{ room, room_token, scheduled }, rtcConfig] = await Promise.all([joinRoom(code), iceServers()])
        if (cancelled) return
        tempos.marcar('token')
        core.roomTokenRef.current = room_token
        core.setTopology(room.topology)
        setRoomName(room.name)
        setIsTraining(room.format === 'training')
        setIsInstant(scheduled === false) // só se o servidor o confirmar
        setWaitingRoomOn(room.waiting_room)
        const amHost = room.owner_id === currentUser()?.id
        core.setIsHost(amHost)
        setCanAdmit(amHost)

        // E2EE: a chave deriva-se da frase ANTES de ligar e nunca sai daqui.
        let crypto: FrameCrypto | undefined
        if (room.e2ee) {
          if (!e2eeSupported()) {
            setStatus(t('room.estado.e2eeNaoSuportado'))
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
          // é a chave que o anfitrião pode ceder à gravação no servidor.
          core.e2eeKeyRef.current = btoa(String.fromCharCode(...new Uint8Array(rawKey)))
          await crypto.setKey(rawKey)
          setE2eeOn(true)
        }

        signal = new Signaling(room_token, code)
        const s = signal
        s.on('joined', () => tempos.marcar('ws'))
        // A chamada só arranca DEPOIS de `joined`: negociar na sala de espera
        // levava a colisões de renegociação e ao anti-flood (R1/R2).
        const callHolder: { start: () => void } = { start: () => {} }

        // A outra sessão desta conta saiu — já não há com quem fazer eco (R114).
        s.on('companion_ended', () => {
          setCompanion(false)
          setStatus(t('room.companion.oOutroSaiu'))
        })
        s.on('error', (m) => setStatus(m.message))
        // Este nó vai fechar. O JITTER evita que a sala inteira reconecte no
        // mesmo milissegundo e caia de uma vez em cima do pod novo.
        s.on('draining', ({ reconnect_in_ms }) => {
          if (cancelled) return
          setStatus(t('room.estado.servidorVaiReiniciar'))
          const jitter = Math.round(reconnect_in_ms * Math.random())
          sessionStorage.setItem(`dx_rejoin_${code}`, String(Date.now()))
          setTimeout(() => {
            if (!cancelled) location.reload()
          }, reconnect_in_ms + jitter)
        })
        s.onclose = () => {
          if (cancelled) return // saída intencional
          // Sem sinalização não há renegociação: reentra-se recarregando, com
          // guarda anti-ciclo — se cair outra vez em < 8 s, pede-se à pessoa.
          const now = Date.now()
          const last = Number(sessionStorage.getItem('dx_reconnect_at') || 0)
          if (now - last > 8000) {
            sessionStorage.setItem('dx_reconnect_at', String(now))
            sessionStorage.setItem(`dx_rejoin_${code}`, String(now))
            setStatus(t('room.estado.ligacaoPerdidaAReconectar'))
            setTimeout(() => {
              if (!cancelled) location.reload()
            }, 1500)
          } else {
            setStatus(t('room.estado.ligacaoInstavel'))
          }
        }
        s.on('waiting', () => setRoomState('waiting'))
        s.on('denied', () => setRoomState('denied'))
        s.on('kicked', () => {
          sessionStorage.removeItem(`dx_rejoin_${code}`)
          setRoomState('kicked')
          core.callRef.current?.hangup()
        })
        s.on('admit-role', (m) => {
          setCanAdmit(m.allowed || amHost)
          setStatus(m.allowed ? t('room.estado.podesAdmitir') : '')
        })
        s.on('host-changed', (m) => {
          setPeers((ps) => ps.map((p) => ({ ...p, host: p.peerId === m.to ? true : p.peerId === m.from ? false : p.host })))
          core.setIsHost((h) => (core.meuPeerIdRef.current === m.to ? true : core.meuPeerIdRef.current === m.from ? false : h))
        })
        // Salas paralelas: mover para o grupo (guardando o caminho de volta) ou
        // regressar à principal quando o anfitrião encerra.
        s.on('breakout-move', (m) => {
          const returnTo = sessionStorage.getItem(`dx_return_${code}`)
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
        s.on('joined', (m) => {
          setRoomState('in')
          // Guarda o lugar (R91): F5 ou Wi-Fi a cair devolvem o mesmo lugar.
          if (m.reconnect) Signaling.guardarSegredo(code, m.reconnect)
          // O servidor é que sabe que a outra sessão é minha. Entrar mudo é a
          // decisão segura: um eco estraga a reunião a TODA a gente.
          if (m.companion) {
            setCompanion(true)
            const mic = core.localStreamRef.current?.getAudioTracks()[0]
            if (mic) mic.enabled = false
            media.setMicOn(false)
          }
          core.meuPeerIdRef.current = m.peer_id
          sessionStorage.setItem(`dx_rejoin_${code}`, String(Date.now()))
          callHolder.start() // SÓ agora, depois da admissão
          setStatus(spectator ? t('room.estado.modoEspectador') : '')
        })

        const callbacks = {
          onStream: (peerId: string, remote: MediaStream) => {
            // «<peer>-screen» é uma apresentação — não substitui a câmara dele.
            if (peerId.endsWith('-screen')) {
              const owner = peerId.slice(0, -7)
              setPresentation({ peerId: owner, stream: remote })
              const clear = () => setPresentation((p) => (p?.peerId === owner ? null : p))
              remote.getVideoTracks().forEach((tr) => {
                tr.onended = clear
                tr.onmute = () =>
                  setTimeout(() => {
                    if (tr.muted) clear()
                  }, 2000)
              })
              remote.onremovetrack = clear
              return
            }
            core.levelsRef.current?.watch(peerId, remote)
            setPeers((ps) => ps.map((p) => (p.peerId === peerId ? { ...p, stream: remote } : p)))
          },
          onPeerLeft: (peerId: string) => {
            core.levelsRef.current?.unwatch(peerId)
            setPresentation((p) => (p?.peerId === peerId ? null : p))
            setPeers((ps) => ps.map((p) => (p.peerId === peerId ? { ...p, stream: null } : p)))
          },
          // Uma quebra de rede deixava o ecrã preto SEM uma palavra. O que
          // interessa é saber que alguém está a tratar do assunto.
          onState: (st: CallState) => {
            if (cancelled) return
            setCallState(st)
            // Os tempos reportam-se UMA vez, quando a media aparece: quem fecha
            // o separador a meio é justamente a cauda que interessa medir.
            if (st === 'connected' && !temposEnviados.current && tempos.vale_reportar()) {
              temposEnviados.current = true
              void postTimings(code, tempos.resumo()).catch(() => {})
            }
            if (st === 'degraded') setStatus(t('room.estado.mediaInstavel'))
            else if (st === 'reconnecting' || st === 'recovering') setStatus(t('room.estado.aRestabelecer'))
            else if (st === 'connected') setStatus('')
            else if (st === 'failed') {
              setStatus(t('room.estado.naoRestabeleceu'))
              sessionStorage.setItem(`dx_rejoin_${code}`, String(Date.now()))
              setTimeout(() => {
                if (!cancelled) location.reload()
              }, 2000)
            }
          },
        }
        callHolder.start = makeCallHolderStart({
          ref: core.callRef,
          isCancelled: () => cancelled,
          create: () =>
            room.topology === 'sfu'
              ? new SfuCall(s, stream, rtcConfig, callbacks, crypto, undefined, tempos)
              : new MeshCall(s, stream, rtcConfig, callbacks, crypto),
        })
        // Só agora o resto da sala passa a ouvir: os handlers da sessão (acima)
        // correm primeiro em cada mensagem.
        bus.attach(s)
      } catch (err) {
        // Uma falha a montar a sala NÃO é terminal: um corte de seis segundos no
        // servidor deixava toda a gente presa num «Internal Server Error».
        if (cancelled || isAbort(err)) return
        // Uma sala que não existe não volta a existir por se insistir: dizê-lo
        // já, em vez de seis tentativas a fingir que é a rede.
        if (err instanceof ApiError && err.status === 404) {
          core.setRoomState('notfound')
          return
        }
        tentativas += 1
        if (tentativas <= MAX_TENTATIVAS) {
          setStatus(t('room.estado.semLigacaoATentar', { n: tentativas, total: MAX_TENTATIVAS }))
          setTimeout(() => {
            if (!cancelled) void start()
          }, backoffDelay(tentativas - 1))
          return
        }
        setStatus(t('room.estado.naoFoiPossivelLigar'))
      }
    }

    const onDeviceChange = () => void listDevices().then(media.setDevices).catch(() => {})
    void start()
    return () => {
      cancelled = true
      navigator.mediaDevices?.removeEventListener?.('devicechange', onDeviceChange)
      if (signal) bus.detach(signal)
      // Sem chamada (sala de espera, recusa) ninguém fecha o socket por nós.
      if (!core.callRef.current) signal?.close()
      core.levelsRef.current?.close()
      core.levelsRef.current = null
      core.effectRef.current?.stop()
      core.headRef.current?.stop()
      core.denoiserRef.current?.stop()
      core.denoiserRef.current = null
      core.rawMicRef.current?.stop()
      core.rawMicRef.current = null
      core.callRef.current?.hangup()
      core.callRef.current = null
      core.localStreamRef.current?.getTracks().forEach((tr) => tr.stop())
      core.localStreamRef.current = null
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [code, passTry, joinIntent])

  // Código de segurança E2EE: SHA-256 da chave em grupos de 5 dígitos — todos
  // derivam o mesmo e podem compará-lo em voz alta.
  useEffect(() => {
    if (!e2eeOn || !core.e2eeKeyRef.current) return
    void (async () => {
      const raw = Uint8Array.from(atob(core.e2eeKeyRef.current!), (c) => c.charCodeAt(0))
      const digest = new Uint8Array(await window.crypto.subtle.digest('SHA-256', raw))
      let digits = ''
      for (let i = 0; i < 10 && digits.length < 20; i += 1) {
        const n = (digest[i * 2] << 8) | digest[i * 2 + 1]
        digits += String(n % 100000).padStart(5, '0').slice(0, 20 - digits.length)
      }
      setSecCode(digits.match(/.{5}/g)!.join(' '))
    })()
  }, [e2eeOn, core.e2eeKeyRef])

  // O browser bloqueia o autoplay do áudio remoto até haver um gesto: a cada
  // clique ou tecla, re-tenta tocar tudo o que esteja parado.
  useEffect(() => {
    const unlock = () => {
      document.querySelectorAll<HTMLMediaElement>('audio, video').forEach((el) => {
        if (el.paused) void el.play().catch(() => {})
      })
    }
    unlock()
    document.addEventListener('pointerdown', unlock)
    document.addEventListener('keydown', unlock)
    return () => {
      document.removeEventListener('pointerdown', unlock)
      document.removeEventListener('keydown', unlock)
    }
  }, [])

  const join = useCallback(
    (audioOnly = false) => {
      joinIntentRef.current = true // ANTES do setState: o cleanup da pré-entrada lê isto
      if (audioOnly) audioOnlyRef.current = true
      setJoinIntent(true)
      setRoomState('connecting')
    },
    [setRoomState],
  )

  function submitPassphrase(pass: string) {
    const p = pass.trim()
    if (!p) return
    sessionStorage.setItem(`dx_e2ee_${code}`, p)
    setRoomState('connecting')
    setPassTry((n) => n + 1)
  }

  /**
   * Sair. SAIR não é CAIR: quem sai de propósito larga o lugar reservado. As
   * notas por guardar gravam-se antes (`beforeLeave`), sem bloquear a saída.
   */
  async function leave(beforeLeave?: () => Promise<void>) {
    Signaling.esquecerSegredo(code)
    sessionStorage.removeItem(`dx_rejoin_${code}`)
    try {
      await beforeLeave?.()
    } catch {
      /* não bloqueia a saída */
    }
    opts.onLeave()
  }

  function dismissCompanion() {
    // A decisão é da pessoa. O que não pode é o eco ser surpresa: diz-se o que
    // fazer ao outro dispositivo.
    setCompanion(false)
    setStatus(t('room.companion.silenciaOOutro'))
  }

  return {
    joinIntent,
    joinIntentRef,
    join,
    submitPassphrase,
    leave,
    callState,
    isTraining,
    isInstant,
    waitingRoomOn,
    roomName,
    canAdmit,
    e2eeOn,
    secCode,
    companion,
    dismissCompanion,
  }
}

export type CallSession = ReturnType<typeof useCallSession>
