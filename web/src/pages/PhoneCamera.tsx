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
import { Alert, Button, cx, Spinner, StatusBadge } from '../ui/kit'
import { Icon } from '../ui/icons'
import { AudioSink } from '../room/AudioSink'
import { useRoomCore } from '../room/useRoomCore'
import { useLocalMedia } from '../room/useLocalMedia'
import { useCallSession } from '../room/useCallSession'
import { ambienteDoBrowser, criarGuardaDeEcra } from '../room/wakeLock'
import '../ui/phoneCamera.css'

/** Capacidades não-padrão que só alguns browsers Android expõem. */
interface CapacidadesAlargadas extends MediaTrackCapabilities {
  zoom?: { min: number; max: number; step: number }
  focusDistance?: { min: number; max: number; step: number }
  exposureCompensation?: { min: number; max: number; step: number }
  torch?: boolean
  exposureMode?: string[]
  focusMode?: string[]
}
interface ConstraintsAlargadas extends MediaTrackConstraintSet {
  zoom?: number
  focusDistance?: number
  exposureCompensation?: number
  torch?: boolean
  exposureMode?: string
  focusMode?: string
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
  // Espelhado por omissão: sempre foi o comportamento (frente ao operador,
  // como um espelho de selfie); o toggle serve para desligar quando a câmara
  // aponta para fora (traseira) e a orientação real é que interessa.
  const [espelhado, setEspelhado] = useState(true)
  const [aeAfBloqueado, setAeAfBloqueado] = useState(false)
  const [aVirar, setAVirar] = useState(false)
  /**
   * Talkback: ouvir o estúdio. DESLIGADO por omissão de propósito — o som sai
   * no altifalante do telefone e volta a entrar pelo microfone que está no ar.
   * O cancelamento de eco ajuda, mas num telefone encostado a uma cena não
   * chega: quem quer retorno põe auscultadores, e é isso que o aviso diz.
   */
  const [talkback, setTalkback] = useState(false)
  const [bateria, setBateria] = useState<{ nivel: number; aCarregar: boolean } | null>(null)
  const [gravando, setGravando] = useState(false)
  /**
   * Os pedaços da gravação de segurança. É uma LISTA porque virar a lente
   * fecha um e abre outro (ver `virarLente`): com um só campo, o pedaço
   * anterior era substituído e o que já estava gravado perdia-se.
   */
  const [ficheiros, setFicheiros] = useState<{ url: string; nome: string }[]>([])
  const [erro, setErro] = useState('')
  const recorderRef = useRef<MediaRecorder | null>(null)
  const chunksRef = useRef<Blob[]>([])

  /**
   * Trocar de lente é pedir OUTRA pista ao aparelho, por isso o espelho tem de
   * a acompanhar: a frontal quer-se espelhada (o operador vê-se como num
   * espelho) e a traseira não, senão o que está escrito na cena aparece ao
   * contrário. Quem quiser o contrário continua a ter o botão ao lado.
   */
  const virarLente = async () => {
    if (aVirar) return
    setAVirar(true)
    // O lado ALVO calcula-se antes: o `media.facing` é estado do React e, lido
    // logo a seguir ao `await`, ainda traz o valor velho.
    const alvo = media.facing === 'user' ? 'environment' : 'user'
    /**
     * A gravação de segurança NÃO sobrevive a virar a lente, e não é escolha
     * nossa: o `switchCam` faz `removeTrack`/`addTrack` no MESMO
     * `MediaStream` sobre que o `MediaRecorder` foi construído, e a
     * especificação do MediaStream Recording diz que mexer nas pistas durante
     * a gravação a termina (`InvalidModificationError`). Antes, isso matava a
     * gravação em silêncio e o botão continuava vermelho.
     *
     * Então fecha-se o pedaço ANTES de virar — fica um ficheiro válido, com
     * tudo o que já se gravou — e abre-se outro depois, na pista nova. Dois
     * ficheiros em vez de um perdido. Juntá-los num só exigia remultiplexar
     * no telefone, que é precisamente o trabalho que esta página não faz.
     */
    const estavaAGravar = gravando
    if (estavaAGravar) pararGravacaoSeguranca()
    try {
      await media.flipCam()
      setEspelhado(alvo === 'user')
    } finally {
      setAVirar(false)
      if (estavaAGravar) iniciarGravacaoSeguranca()
    }
  }

  /**
   * O ecrã não apaga enquanto isto é uma câmara. O `visibilitychange` é
   * parte da correcção e não um extra: o browser larga o lock de ecrã quando
   * a página sai de vista (uma chamada, uma notificação) e **não o devolve**
   * — quem peça só na montagem fica sem ele para o resto da sessão, com o
   * sintoma igual a nunca o ter pedido. A lógica e as provas vivem em
   * `room/wakeLock.ts`.
   *
   * Melhor esforço, como os controlos de câmara: no iOS antigo e no Firefox a
   * API não existe, e aí não há nada a dizer ao operador que ele possa fazer.
   */
  useEffect(() => {
    const guarda = criarGuardaDeEcra(ambienteDoBrowser())
    void guarda.pedir()
    const aoMudarVisibilidade = () => void guarda.pedir()
    document.addEventListener('visibilitychange', aoMudarVisibilidade)
    return () => {
      document.removeEventListener('visibilitychange', aoMudarVisibilidade)
      guarda.parar()
      void guarda.largar()
    }
  }, [])

  /**
   * As capacidades são da LENTE, não do aparelho: a traseira é grande-angular
   * com zoom de 1 a 10×, a frontal costuma ter 1× e mais nada. As dependências
   * eram `[core.cameraTrackRef]` — um objecto `ref`, de identidade estável,
   * por isso isto corria uma vez na montagem e os limites ficavam os da
   * primeira lente para sempre. Depois de virar, o cursor de zoom aplicava
   * `applyConstraints` fora do intervalo real e cada falha dava um alerta.
   *
   * Agora depende do que MUDA quando a lente muda (`facing` e `camId`, ambos
   * estado do `useLocalMedia`, escritos no fim do `switchCam`); a pista
   * continua a ler-se do ref, que é onde ela vive.
   */
  useEffect(() => {
    const track = core.cameraTrackRef.current
    const caps = (track?.getCapabilities?.() ?? null) as CapacidadesAlargadas | null
    setCapacidades(caps)
    const settings = track?.getSettings?.() as ConstraintsAlargadas | undefined
    setZoom(caps?.zoom ? ((settings?.zoom as number) ?? caps.zoom.min) : null)
    setFoco(caps?.focusDistance ? ((settings?.focusDistance as number) ?? caps.focusDistance.min) : null)
    setExposicao(caps?.exposureCompensation ? ((settings?.exposureCompensation as number) ?? 0) : null)
    // A lente nova chega sem o bloqueio que estava na anterior.
    setAeAfBloqueado(false)
    setTorch(false)
  }, [core.cameraTrackRef, media.facing, media.camId])

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

  function alternarAeAfBloqueado() {
    const novo = !aeAfBloqueado
    setAeAfBloqueado(novo)
    const track = core.cameraTrackRef.current
    const settings = track?.getSettings?.() as ConstraintsAlargadas | undefined
    // Ao bloquear leva-se o valor actual — sem isto alguns browsers saltam
    // para o extremo do intervalo em vez de ficar onde a imagem já estava boa.
    void aplicar({
      exposureMode: novo ? 'manual' : 'continuous',
      focusMode: novo ? 'manual' : 'continuous',
      ...(novo && settings?.exposureCompensation !== undefined ? { exposureCompensation: settings.exposureCompensation } : {}),
      ...(novo && settings?.focusDistance !== undefined ? { focusDistance: settings.focusDistance } : {}),
    })
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
        // Cada paragem fecha um PEDAÇO e acrescenta-o à lista, em vez de
        // substituir o anterior: virar a lente fecha um e abre outro, e
        // perder o primeiro era perder justamente o que já estava gravado.
        const blob = new Blob(chunksRef.current, { type: 'video/webm' })
        if (!blob.size) return
        setFicheiros((fs) => [...fs, { url: URL.createObjectURL(blob), nome: `seguranca-${new Date().toISOString().replace(/[:.]/g, '-')}.webm` }])
      }
      /**
       * A gravação de segurança existe para quando o directo falha — por isso
       * uma falha DELA não pode ser silenciosa. Sem este `onerror`, o botão
       * ficava vermelho e o `StatusBadge` continuava a dizer «a gravar no
       * telefone» com a gravação já morta; só se descobria no fim, ao ver o
       * ficheiro acabar no instante em que a coisa correu mal.
       */
      rec.onerror = () => {
        setErro(t('telemovel.segurancaMorreu'))
        recorderRef.current = null
        setGravando(false)
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
      {/* O áudio do estúdio. Sempre montado, silenciado quando o retorno está
          desligado: ligar o som passa a ser instantâneo, sem renegociar nada. */}
      <AudioSink peers={core.peers} sinkId="" mudo={!talkback} />
      <div className={`phonecam__tally ${tally ? 'is-live' : ''}`}>
        {tally ? t('telemovel.tally.noAr') : t('telemovel.tally.emEspera')}
      </div>
      <video ref={media.attachLocalVideo} className={cx('phonecam__preview', espelhado && 'is-mirrored')} autoPlay playsInline muted />

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
        {capacidades?.exposureMode?.includes('manual') && capacidades?.focusMode?.includes('manual') && (
          <Button variant={aeAfBloqueado ? 'primary' : 'secondary'} size="sm" onClick={alternarAeAfBloqueado}>
            {t('telemovel.bloquearAeAf')}
          </Button>
        )}
        <Button variant={media.micOn ? 'secondary' : 'primary'} size="sm" onClick={() => void media.toggleMic()}>
          {media.micOn ? t('telemovel.microfoneLigado') : t('telemovel.microfoneDesligado')}
        </Button>
        <Button variant={talkback ? 'primary' : 'secondary'} size="sm" onClick={() => setTalkback((v) => !v)}>
          {talkback ? t('telemovel.retornoLigado') : t('telemovel.retornoDesligado')}
        </Button>
        {media.facing && (
          <Button variant="secondary" size="sm" disabled={aVirar} onClick={() => void virarLente()}>
            {media.facing === 'user' ? t('telemovel.virarParaTras') : t('telemovel.virarParaFrente')}
          </Button>
        )}
        <Button variant={espelhado ? 'primary' : 'secondary'} size="sm" onClick={() => setEspelhado((v) => !v)}>
          {t('telemovel.espelhar')}
        </Button>
        {talkback && <p className="dx-muted phonecam__semControlo">{t('telemovel.retornoNota')}</p>}
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
          {!gravando &&
            ficheiros.map((f, i) => (
              <a key={f.url} className="dx-btn dx-btn--ghost dx-btn--sm" href={f.url} download={f.nome}>
                {ficheiros.length > 1 ? t('telemovel.descarregarPedaco', { n: i + 1 }) : t('telemovel.descarregarSeguranca')}
              </a>
            ))}
        </div>
        <Button variant="secondary" size="sm" icon="x" onClick={onLeave}>
          {t('telemovel.sair')}
        </Button>
      </div>
      <p className="dx-muted phonecam__nota">{t('telemovel.nota')}</p>
    </div>
  )
}
