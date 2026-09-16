/**
 * Dispositivos antes da próxima reunião — o bloco «Dispositivos» do template
 * de telemóvel (câmara com resolução, microfone com nível em dB).
 *
 * Só liga a câmara e o microfone quando a pessoa carrega no botão: abrir a
 * Início não pode acender a luz da câmara. Os números são os que o browser
 * dá — a resolução é a da faixa aberta (getSettings), o nível é o RMS medido
 * do microfone em dBFS — e fecha tudo ao sair ou ao carregar outra vez.
 */
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from '../../ui/icons'
import { Alert, Button } from '../../ui/kit'

interface Opened {
  stream: MediaStream
  camLabel: string
  camRes: string | null
  micLabel: string
}

export function resolutionLabel(settings: { width?: number; height?: number }): string | null {
  const h = settings.height
  const w = settings.width
  if (!h || !w) return null
  // «1080p» lê-se pelo lado MENOR: um telemóvel ao alto dá 1080×1920.
  return `${Math.min(w, h)}p`
}

function Level({ stream }: { stream: MediaStream }) {
  const { t } = useTranslation()
  const barRef = useRef<HTMLSpanElement>(null)
  const dbRef = useRef<HTMLSpanElement>(null)
  useEffect(() => {
    const track = stream.getAudioTracks()[0]
    if (!track) return
    let ctx: AudioContext
    try {
      ctx = new AudioContext()
    } catch {
      return
    }
    const source = ctx.createMediaStreamSource(new MediaStream([track]))
    const analyser = ctx.createAnalyser()
    analyser.fftSize = 512
    source.connect(analyser)
    const buf = new Float32Array(analyser.fftSize)
    let raf = 0
    let last = 0
    const tick = (now: number) => {
      raf = requestAnimationFrame(tick)
      if (now - last < 80) return
      last = now
      analyser.getFloatTimeDomainData(buf)
      let sum = 0
      for (let i = 0; i < buf.length; i++) sum += buf[i] * buf[i]
      const rms = Math.sqrt(sum / buf.length)
      const db = rms > 0 ? Math.max(-60, 20 * Math.log10(rms)) : -60
      if (barRef.current) barRef.current.style.width = `${Math.round(((db + 60) / 60) * 100)}%`
      if (dbRef.current) dbRef.current.textContent = db <= -60 ? t('consola.dispositivos.silencio') : `${Math.round(db)} dB`
    }
    raf = requestAnimationFrame(tick)
    return () => {
      cancelAnimationFrame(raf)
      source.disconnect()
      void ctx.close()
    }
  }, [stream, t])
  return (
    <span className="home-dev__level">
      <span className="dx-meter dx-meter--success">
        <span ref={barRef} style={{ width: 0 }} />
      </span>
      <span ref={dbRef} className="dx-num dx-muted" data-testid="device-db">
        —
      </span>
    </span>
  )
}

export default function DeviceCheck() {
  const { t } = useTranslation()
  const [open, setOpen] = useState<Opened | null>(null)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const videoRef = useRef<HTMLVideoElement>(null)
  const openRef = useRef<Opened | null>(null)
  openRef.current = open

  useEffect(() => () => openRef.current?.stream.getTracks().forEach((tr) => tr.stop()), [])

  useEffect(() => {
    if (videoRef.current && open) videoRef.current.srcObject = open.stream
  }, [open])

  async function start() {
    setErr('')
    setBusy(true)
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true, video: true })
      const cam = stream.getVideoTracks()[0]
      const mic = stream.getAudioTracks()[0]
      setOpen({
        stream,
        camLabel: cam?.label || t('consola.dispositivos.camara'),
        camRes: cam ? resolutionLabel(cam.getSettings()) : null,
        micLabel: mic?.label || t('consola.dispositivos.microfone'),
      })
    } catch (e) {
      const name = (e as { name?: string }).name
      setErr(name === 'NotAllowedError' ? t('consola.dispositivos.negado') : t('consola.dispositivos.erro'))
    } finally {
      setBusy(false)
    }
  }

  function stop() {
    open?.stream.getTracks().forEach((tr) => tr.stop())
    setOpen(null)
  }

  if (!open) {
    return (
      <div className="home-dev">
        <Button size="sm" variant="ghost" icon="sliders" busy={busy} onClick={() => void start()} data-testid="device-check">
          {t('consola.dispositivos.testar')}
        </Button>
        {err && <Alert tone="warning">{err}</Alert>}
      </div>
    )
  }
  return (
    <div className="home-dev home-dev--open" aria-label={t('consola.dispositivos.titulo')} role="group">
      <video ref={videoRef} className="home-dev__preview" autoPlay muted playsInline aria-hidden="true" />
      <ul className="home-dev__list" role="list">
        <li>
          <Icon name="video" size={14} />
          <span className="home-dev__name">{open.camLabel}</span>
          {open.camRes && <span className="dx-num dx-muted" data-testid="device-res">{open.camRes}</span>}
        </li>
        <li>
          <Icon name="mic" size={14} />
          <span className="home-dev__name">{open.micLabel}</span>
          <Level stream={open.stream} />
        </li>
      </ul>
      <Button size="sm" variant="ghost" icon="x" onClick={stop}>
        {t('consola.dispositivos.fechar')}
      </Button>
    </div>
  )
}
