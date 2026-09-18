/**
 * "Telemóvel como câmara": o telefone do próprio operador do Estúdio, aberto
 * no SEU browser, ligado à mesma sala de convidados já usada para pôr
 * pessoas no palco (compositor.ts::definirConvidados) — não é um transporte
 * novo, é o MESMO caminho SFU/WebRTC de sempre, só que a página aqui é
 * mínima: pré-visualização, tally, controlos de câmara (melhor esforço —
 * dependem do que o aparelho/browser expõem) e gravação de segurança local.
 *
 * Fica fora desta versão: emparelhamento por QR, âmbito por departamento e
 * a app nativa — o telefone entra pelo mesmo link/código de sala que
 * qualquer convidado, sem conta especial.
 */
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Alert, Button, Spinner, StatusBadge } from '../ui/kit'
import { Icon } from '../ui/icons'
import { useRoomCore } from '../room/useRoomCore'
import { useLocalMedia } from '../room/useLocalMedia'
import { useCallSession } from '../room/useCallSession'
import '../ui/phoneCamera.css'

/** Capacidades não-padrão que só alguns browsers Android expõem. */
interface CapacidadesAlargadas extends MediaTrackCapabilities {
  zoom?: { min: number; max: number; step: number }
  focusDistance?: { min: number; max: number; step: number }
  exposureCompensation?: { min: number; max: number; step: number }
  torch?: boolean
}
interface ConstraintsAlargadas extends MediaTrackConstraintSet {
  zoom?: number
  focusDistance?: number
  exposureCompensation?: number
  torch?: boolean
}

export default function PhoneCamera({ code, onLeave }: { code: string; onLeave: () => void }) {
  const { t } = useTranslation()
  const core = useRoomCore(code, 'connecting')
  const media = useLocalMedia(core)
  const session = useCallSession(core, media, { voiceOnly: false, onLeave })

  // Sem pré-entrada: o telefone liga-se logo — é um aparelho, não uma pessoa a decidir.
  useEffect(() => {
    session.join()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const [tally, setTally] = useState(false)
  useEffect(() => core.signal.on('tally', (m) => setTally(m.live)), [core.signal])

  if (core.roomState === 'connecting' || core.roomState === 'prejoin') {
    return (
      <div className="phonecam-wait" role="status">
        <Spinner />
        <span>{t('telemovel.aLigar')}</span>
      </div>
    )
  }
  if (core.roomState === 'waiting') {
    return (
      <div className="phonecam-wait" role="status">
        <Icon name="clock" />
        <span>{t('telemovel.naEspera')}</span>
      </div>
    )
  }
  if (core.roomState === 'denied' || core.roomState === 'kicked' || core.roomState === 'notfound' || core.roomState === 'e2ee-pass') {
    return (
      <div className="phonecam-wait" role="alert">
        <Icon name="alert" />
        <span>{t(`telemovel.erro.${core.roomState}`)}</span>
      </div>
    )
  }

  return <OperadorDeCamara code={code} core={core} media={media} tally={tally} onLeave={session.leave} />
}

function OperadorDeCamara({
  core,
  media,
  tally,
  onLeave,
}: {
  code: string
  core: ReturnType<typeof useRoomCore>
  media: ReturnType<typeof useLocalMedia>
  tally: boolean
  onLeave: () => void
}) {
  const { t } = useTranslation()
  const [capacidades, setCapacidades] = useState<CapacidadesAlargadas | null>(null)
  const [zoom, setZoom] = useState<number | null>(null)
  const [foco, setFoco] = useState<number | null>(null)
  const [exposicao, setExposicao] = useState<number | null>(null)
  const [torch, setTorch] = useState(false)
  const [bateria, setBateria] = useState<{ nivel: number; aCarregar: boolean } | null>(null)
  const [gravando, setGravando] = useState(false)
  const [ficheiroSeguranca, setFicheiroSeguranca] = useState<{ url: string; nome: string } | null>(null)
  const [erro, setErro] = useState('')
  const recorderRef = useRef<MediaRecorder | null>(null)
  const chunksRef = useRef<Blob[]>([])

  useEffect(() => {
    const track = core.cameraTrackRef.current
    const caps = (track?.getCapabilities?.() ?? null) as CapacidadesAlargadas | null
    setCapacidades(caps)
    const settings = track?.getSettings?.() as ConstraintsAlargadas | undefined
    if (caps?.zoom) setZoom((settings?.zoom as number) ?? caps.zoom.min)
    if (caps?.focusDistance) setFoco((settings?.focusDistance as number) ?? caps.focusDistance.min)
    if (caps?.exposureCompensation) setExposicao((settings?.exposureCompensation as number) ?? 0)
  }, [core.cameraTrackRef])

  useEffect(() => {
    const nav = navigator as Navigator & { getBattery?: () => Promise<EventTarget & { level: number; charging: boolean; addEventListener: (e: string, cb: () => void) => void }> }
    if (!nav.getBattery) return
    let bat: Awaited<ReturnType<NonNullable<typeof nav.getBattery>>> | null = null
    nav
      .getBattery()
      .then((b) => {
        bat = b
        const ler = () => setBateria({ nivel: Math.round(b.level * 100), aCarregar: b.charging })
        ler()
        b.addEventListener('levelchange', ler)
        b.addEventListener('chargingchange', ler)
      })
      .catch(() => {})
    return () => {
      /* a API não dá forma de desligar os listeners sem guardar `ler`; a aba fecha com a página */
      void bat
    }
  }, [])

  async function aplicar(patch: ConstraintsAlargadas) {
    const track = core.cameraTrackRef.current
    if (!track) return
    try {
      await track.applyConstraints({ advanced: [patch] })
    } catch {
      setErro(t('telemovel.erroControlo'))
    }
  }

  function iniciarGravacaoSeguranca() {
    const stream = core.localStreamRef.current
    if (!stream) return
    setErro('')
    try {
      chunksRef.current = []
      const rec = new MediaRecorder(stream, { mimeType: MediaRecorder.isTypeSupported('video/webm;codecs=vp8,opus') ? 'video/webm;codecs=vp8,opus' : 'video/webm' })
      rec.ondataavailable = (e) => e.data.size && chunksRef.current.push(e.data)
      rec.onstop = () => {
        const blob = new Blob(chunksRef.current, { type: 'video/webm' })
        setFicheiroSeguranca({ url: URL.createObjectURL(blob), nome: `seguranca-${new Date().toISOString().replace(/[:.]/g, '-')}.webm` })
      }
      rec.start(1000)
      recorderRef.current = rec
      setGravando(true)
    } catch {
      setErro(t('telemovel.erroGravacao'))
    }
  }
  function pararGravacaoSeguranca() {
    recorderRef.current?.stop()
    recorderRef.current = null
    setGravando(false)
  }

  return (
    <div className="phonecam">
      <div className={`phonecam__tally ${tally ? 'is-live' : ''}`}>
        {tally ? t('telemovel.tally.noAr') : t('telemovel.tally.emEspera')}
      </div>
      <video ref={media.attachLocalVideo} className="phonecam__preview" autoPlay playsInline muted />

      {erro && <Alert tone="danger">{erro}</Alert>}

      <div className="phonecam__controls">
        {capacidades?.zoom && zoom !== null && (
          <label className="phonecam__slider">
            <span>{t('telemovel.zoom')}</span>
            <input
              type="range"
              min={capacidades.zoom.min}
              max={capacidades.zoom.max}
              step={capacidades.zoom.step}
              value={zoom}
              onChange={(e) => {
                const v = Number(e.target.value)
                setZoom(v)
                void aplicar({ zoom: v })
              }}
            />
          </label>
        )}
        {capacidades?.focusDistance && foco !== null && (
          <label className="phonecam__slider">
            <span>{t('telemovel.foco')}</span>
            <input
              type="range"
              min={capacidades.focusDistance.min}
              max={capacidades.focusDistance.max}
              step={capacidades.focusDistance.step}
              value={foco}
              onChange={(e) => {
                const v = Number(e.target.value)
                setFoco(v)
                void aplicar({ focusDistance: v })
              }}
            />
          </label>
        )}
        {capacidades?.exposureCompensation && exposicao !== null && (
          <label className="phonecam__slider">
            <span>{t('telemovel.exposicao')}</span>
            <input
              type="range"
              min={capacidades.exposureCompensation.min}
              max={capacidades.exposureCompensation.max}
              step={capacidades.exposureCompensation.step}
              value={exposicao}
              onChange={(e) => {
                const v = Number(e.target.value)
                setExposicao(v)
                void aplicar({ exposureCompensation: v })
              }}
            />
          </label>
        )}
        {capacidades?.torch && (
          <Button
            variant={torch ? 'primary' : 'secondary'}
            size="sm"
            onClick={() => {
              const novo = !torch
              setTorch(novo)
              void aplicar({ torch: novo })
            }}
          >
            {t('telemovel.lanterna')}
          </Button>
        )}
        {!capacidades?.zoom && !capacidades?.focusDistance && !capacidades?.exposureCompensation && (
          <p className="dx-muted phonecam__semControlo">{t('telemovel.semControlosAvancados')}</p>
        )}
      </div>

      <div className="phonecam__footer">
        {bateria && (
          <span className="dx-muted dx-num phonecam__bateria">
            {bateria.nivel}% {bateria.aCarregar ? t('telemovel.aCarregar') : ''}
          </span>
        )}
        <div className="phonecam__seguranca">
          <Button
            variant={gravando ? 'danger' : 'secondary'}
            size="sm"
            icon={gravando ? 'stop' : 'circle'}
            onClick={gravando ? pararGravacaoSeguranca : iniciarGravacaoSeguranca}
          >
            {gravando ? t('telemovel.pararSeguranca') : t('telemovel.gravarSeguranca')}
          </Button>
          {gravando && <StatusBadge tone="record">{t('telemovel.aGravarLocal')}</StatusBadge>}
          {ficheiroSeguranca && !gravando && (
            <a className="dx-btn dx-btn--ghost dx-btn--sm" href={ficheiroSeguranca.url} download={ficheiroSeguranca.nome}>
              {t('telemovel.descarregarSeguranca')}
            </a>
          )}
        </div>
        <Button variant="secondary" size="sm" icon="x" onClick={onLeave}>
          {t('telemovel.sair')}
        </Button>
      </div>
      <p className="dx-muted phonecam__nota">{t('telemovel.nota')}</p>
    </div>
  )
}
