import { CSSProperties, PointerEvent as ReactPointerEvent, useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon, IconName } from '../ui/icons'
import { Button, Toggle, avatarTone, cx, initials } from '../ui/kit'
import type { WbStroke } from '../signaling'
import {
  corDeMarcador,
  endireitar,
  espessura,
  grausDeInclinacao,
  limitarPontos,
  pontosDaForma,
  suavizar,
  type AmostraCaneta,
  type Forma,
  type Pt,
} from './ink'
import { repartirFila } from './stripCapacity'
import type { RemotePeer } from './useRoomCore'
import { useStripCapacity } from './useStripCapacity'
import { WB_COLORS, type WhiteboardState } from './useWhiteboard'

type Ferramenta = 'caneta' | 'marcador' | 'forma' | 'regua'

/** Três espessuras de base (B6). O marcador usa o triplo. */
const ESPESSURAS = [2, 4, 8] as const
const FORMAS: { forma: Forma; icon: IconName; label: string }[] = [
  { forma: 'rect', icon: 'square', label: 'room.quadro.formaRect' },
  { forma: 'elipse', icon: 'circle', label: 'room.quadro.formaElipse' },
  { forma: 'seta', icon: 'arrowRight', label: 'room.quadro.formaSeta' },
]
const ZOOM_MIN = 0.5
const ZOOM_MAX = 3

interface AjustesCaneta {
  sensibilidade: number
  suavizar: number
  rejeicaoPalma: boolean
  inclinacao: boolean
  endireitar: boolean
}
const AJUSTES_OMISSAO: AjustesCaneta = { sensibilidade: 0.72, suavizar: 0.48, rejeicaoPalma: true, inclinacao: true, endireitar: false }

function lerAjustes(): AjustesCaneta {
  try {
    return { ...AJUSTES_OMISSAO, ...JSON.parse(localStorage.getItem('dx_caneta') ?? '{}') }
  } catch {
    return AJUSTES_OMISSAO
  }
}

function desenhar(ctx: CanvasRenderingContext2D, s: WbStroke, W: number, H: number, escala: number) {
  if (s.pts.length < 2) return
  ctx.strokeStyle = s.c
  ctx.lineWidth = s.w * escala
  ctx.lineCap = 'round'
  ctx.lineJoin = 'round'
  ctx.beginPath()
  ctx.moveTo(s.pts[0][0] * W, s.pts[0][1] * H)
  for (const [x, y] of s.pts.slice(1)) ctx.lineTo(x * W, y * H)
  ctx.stroke()
}

/** Um selo da fila «Na sessão»: iniciais, nome curto, microfone. */
function SeloPessoa({ nome, micOn, speaking, escrever }: { nome: string; micOn: boolean; speaking: boolean; escrever?: string }) {
  const { t } = useTranslation()
  const curto = nome.split(/\s+/).length > 1 ? `${nome.split(/\s+/)[0]} ${nome.split(/\s+/).slice(-1)[0][0]}.` : nome
  return (
    <div className={cx('rm-wbperson', speaking && 'is-speaking')} style={{ '--tone': avatarTone(nome) } as CSSProperties} title={nome}>
      <span className="rm-wbperson__ini" aria-hidden="true">
        {initials(nome)}
      </span>
      <span className="rm-wbperson__name">
        {!micOn && <Icon name="micOff" size={9} className="dx-icon rm-tile__muted" />}
        {!micOn && <span className="dx-sr-only">{t('room.tile.microfoneDesligado')}</span>}
        {escrever ? `${curto} · ${escrever}` : curto}
      </span>
    </div>
  )
}

/**
 * Quadro branco colaborativo: traços com coordenadas normalizadas (0..1),
 * difundidos pela sinalização e redesenhados a cada redimensionamento.
 *
 * Duas formas, as dos dois templates: o quadro (ferramentas + fila «Na
 * sessão») e, com uma caneta detectada ou os ajustes abertos, o quadro
 * partilhado (cartões da caneta à direita, a fila por baixo).
 */
export function Whiteboard({
  wb,
  me,
  micOn,
  meSpeaking,
  peers,
  speaking,
  onOpenPeople,
  stage,
}: {
  wb: WhiteboardState
  me: string
  micOn: boolean
  meSpeaking: boolean
  peers: RemotePeer[]
  speaking: Set<string>
  onOpenPeople: () => void
  /** Anfitrião a emitir: o quadro pode ir para o palco da emissão. */
  stage: { destinos: number; onStage: boolean; setStream: (s: MediaStream | null) => void } | null
}) {
  const { t } = useTranslation()
  const { strokes, addStroke: onStroke, clear: onClear, registerSnapshot } = wb
  const viewRef = useRef<HTMLDivElement>(null)
  const sheetRef = useRef<HTMLDivElement>(null)
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const liveRef = useRef<HTMLCanvasElement>(null)
  const drawing = useRef<{ pts: Pt[]; amostras: AmostraCaneta[]; caneta: boolean; inicio: Pt } | null>(null)
  const penSeenAt = useRef(0)
  const [ferramenta, setFerramenta] = useState<Ferramenta>('caneta')
  const [forma, setForma] = useState<Forma>('rect')
  const [color, setColor] = useState(WB_COLORS[0])
  const [base, setBase] = useState<number>(ESPESSURAS[1])
  const [zoom, setZoom] = useState(1)
  const penOn = wb.pen.on
  const [ajustesAbertos, setAjustesAbertos] = useState(false)
  const [ajustes, setAjustesState] = useState<AjustesCaneta>(lerAjustes)
  const [aEscrever, setAEscrever] = useState(false)
  const setAjustes = (patch: Partial<AjustesCaneta>) =>
    setAjustesState((a) => {
      const n = { ...a, ...patch }
      try {
        localStorage.setItem('dx_caneta', JSON.stringify(n))
      } catch {
        /* sem armazenamento: vale para esta sessão */
      }
      return n
    })
  const partilhado = penOn || ajustesAbertos
  const fila = useStripCapacity('.rm-wbpeople > .rm-wbperson, .rm-wbpeople > .rm-wbpeople__more')

  const redraw = useCallback(() => {
    const c = canvasRef.current
    const live = liveRef.current
    const sheet = sheetRef.current
    if (!c || !sheet) return
    c.width = sheet.clientWidth
    c.height = sheet.clientHeight
    if (live) {
      live.width = c.width
      live.height = c.height
    }
    const ctx = c.getContext('2d')
    if (!ctx) return
    ctx.clearRect(0, 0, c.width, c.height)
    for (const s of strokes) desenhar(ctx, s, c.width, c.height, zoom)
  }, [strokes, zoom])

  useEffect(() => {
    redraw()
    const ro = new ResizeObserver(redraw)
    if (sheetRef.current) ro.observe(sheetRef.current)
    return () => ro.disconnect()
  }, [redraw])

  /** Fundo branco opaco + traços num PNG (o canvas em si é transparente). */
  const snapshot = useCallback((): string | null => {
    const c = canvasRef.current
    if (!c || strokes.length === 0) return null
    const off = document.createElement('canvas')
    off.width = c.width / zoom
    off.height = c.height / zoom
    const ctx = off.getContext('2d')
    if (!ctx) return null
    ctx.fillStyle = '#ffffff'
    ctx.fillRect(0, 0, off.width, off.height)
    for (const s of strokes) desenhar(ctx, s, off.width, off.height, 1)
    return off.toDataURL('image/png')
  }, [strokes, zoom])

  useEffect(() => {
    registerSnapshot(snapshot)
    return () => registerSnapshot(null)
  }, [snapshot, registerSnapshot])

  // ── Quadro no palco da emissão: um canvas 16:9 opaco, pintado a cada traço. ──
  const palcoRef = useRef<HTMLCanvasElement | null>(null)
  const pintarPalco = useCallback(() => {
    const c = palcoRef.current
    const ctx = c?.getContext('2d')
    if (!c || !ctx) return
    ctx.fillStyle = '#ffffff'
    ctx.fillRect(0, 0, c.width, c.height)
    for (const s of strokes) desenhar(ctx, s, c.width, c.height, c.width / 1280)
  }, [strokes])
  useEffect(() => {
    if (stage?.onStage) pintarPalco()
  }, [stage?.onStage, pintarPalco])
  // Fechar o quadro tira-o do palco. O setter lê-se por ref: é uma função nova
  // a cada render, e um efeito dependente dela tirava o quadro logo a seguir.
  const setStageRef = useRef(stage?.setStream)
  setStageRef.current = stage?.setStream
  useEffect(
    () => () => {
      if (palcoRef.current) {
        setStageRef.current?.(null)
        palcoRef.current = null
      }
    },
    [],
  )
  function alternarPalco() {
    if (!stage) return
    if (stage.onStage) {
      palcoRef.current = null
      stage.setStream(null)
      return
    }
    const c = document.createElement('canvas')
    c.width = 1280
    c.height = 720
    palcoRef.current = c
    pintarPalco()
    stage.setStream(c.captureStream(15))
  }

  // Ctrl + roda: zoom (a página não faz scroll por baixo).
  useEffect(() => {
    const el = viewRef.current
    if (!el) return
    const onWheel = (e: WheelEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return
      e.preventDefault()
      setZoom((z) => Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, z * (e.deltaY < 0 ? 1.1 : 1 / 1.1))))
    }
    el.addEventListener('wheel', onWheel, { passive: false })
    return () => el.removeEventListener('wheel', onWheel)
  }, [])

  const norm = (e: { clientX: number; clientY: number }): Pt => {
    const r = canvasRef.current!.getBoundingClientRect()
    return [Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)), Math.min(1, Math.max(0, (e.clientY - r.top) / r.height))]
  }
  const aspecto = () => {
    const c = canvasRef.current
    return c && c.height ? c.width / c.height : 1
  }

  /** O traço em curso (ou a forma a ser arrastada) na camada viva. */
  function pintarVivo(traco: WbStroke) {
    const live = liveRef.current
    const ctx = live?.getContext('2d')
    if (!live || !ctx) return
    ctx.clearRect(0, 0, live.width, live.height)
    desenhar(ctx, traco, live.width, live.height, zoom)
  }

  const corActual = ferramenta === 'marcador' ? corDeMarcador(color) : color
  const baseActual = ferramenta === 'marcador' ? base * 3 : base

  function onDown(e: ReactPointerEvent<HTMLCanvasElement>) {
    if (e.pointerType === 'pen') {
      penSeenAt.current = Date.now()
      if (!penOn) wb.setPen((p) => ({ ...p, on: true }))
    }
    // Rejeição de palma: com uma caneta em uso, o toque da mão não desenha.
    if (ajustes.rejeicaoPalma && e.pointerType === 'touch' && Date.now() - penSeenAt.current < 10_000) return
    try {
      e.currentTarget.setPointerCapture(e.pointerId)
    } catch {
      /* ponteiro sintético */
    }
    const p = norm(e)
    drawing.current = { pts: [p], amostras: [], caneta: e.pointerType === 'pen', inicio: p }
    setAEscrever(true)
  }

  function onMove(e: ReactPointerEvent<HTMLCanvasElement>) {
    const d = drawing.current
    if (!d) return
    const eventos = (e.nativeEvent as PointerEvent).getCoalescedEvents?.() ?? [e.nativeEvent]
    for (const ev of eventos) {
      if (d.caneta) {
        d.amostras.push({ pressao: ev.pressure, inclinacao: grausDeInclinacao(ev.tiltX, ev.tiltY) })
        if (ev.pressure > 0 && ev.pressure !== 0.5 && !wb.pen.pressao) wb.setPen({ on: true, pressao: true })
      }
      const p = norm(ev)
      if (ferramenta === 'forma' || ferramenta === 'regua') {
        d.pts = [d.inicio, p]
        continue
      }
      const last = d.pts[d.pts.length - 1]
      if (Math.abs(p[0] - last[0]) + Math.abs(p[1] - last[1]) < 0.002) continue
      d.pts.push(p)
    }
    const fim = d.pts[d.pts.length - 1]
    const pts =
      ferramenta === 'forma' ? pontosDaForma(forma, d.inicio, fim, aspecto()) : ferramenta === 'regua' ? [d.inicio, fim] : d.pts
    pintarVivo({ pts, c: corActual, w: baseActual })
  }

  function onUp() {
    const d = drawing.current
    drawing.current = null
    setAEscrever(false)
    const live = liveRef.current
    live?.getContext('2d')?.clearRect(0, 0, live.width, live.height)
    if (!d || d.pts.length < 2) return
    const fim = d.pts[d.pts.length - 1]
    let pts: Pt[]
    if (ferramenta === 'forma') pts = pontosDaForma(forma, d.inicio, fim, aspecto())
    else if (ferramenta === 'regua') pts = [d.inicio, fim]
    else {
      pts = suavizar(d.pts, ajustes.suavizar)
      if (ajustes.endireitar && ferramenta === 'caneta') pts = endireitar(pts, aspecto())?.pts ?? pts
    }
    const w = d.caneta ? espessura(baseActual, d.amostras, ajustes.sensibilidade, ajustes.inclinacao) : baseActual
    const traco: WbStroke = { pts: limitarPontos(pts), c: corActual, w }
    onStroke(traco)
    if (stage?.onStage && palcoRef.current) {
      const ctx = palcoRef.current.getContext('2d')
      if (ctx) desenhar(ctx, traco, palcoRef.current.width, palcoRef.current.height, palcoRef.current.width / 1280)
    }
  }

  const ferramentas: { id: Ferramenta; icon: IconName; label: string }[] = [
    { id: 'caneta', icon: 'pen', label: t('room.quadro.caneta') },
    { id: 'marcador', icon: 'highlighter', label: t('room.quadro.marcador') },
    { id: 'forma', icon: 'shapes', label: t('room.quadro.formas') },
    { id: 'regua', icon: 'ruler', label: t('room.quadro.regua') },
  ]

  // A fila «Na sessão»: eu primeiro, depois quem está na sala.
  const pessoas = [
    { id: 'me', nome: me, micOn, speaking: meSpeaking, escrever: aEscrever ? t('room.quadro.aEscrever') : undefined },
    ...peers.map((p) => ({ id: p.peerId, nome: p.username, micOn: p.micOn, speaking: speaking.has(p.peerId), escrever: undefined })),
  ]
  const { mostrar, resto } = repartirFila(pessoas.length, fila.capacity)

  const filaPessoas = (
    <div className="rm-wbpeople" ref={fila.ref} aria-label={t('room.quadro.naSessao', { count: pessoas.length })}>
      {!partilhado && (
        <span className="rm-wbpeople__head dx-eyebrow" data-strip-head>
          {t('room.quadro.naSessao', { count: pessoas.length })}
        </span>
      )}
      {pessoas.slice(0, mostrar).map((p) => (
        <SeloPessoa key={p.id} nome={p.nome} micOn={p.micOn} speaking={p.speaking} escrever={p.escrever} />
      ))}
      {resto > 0 && (
        <button type="button" className="rm-wbpeople__more" onClick={onOpenPeople} aria-label={t('room.palco.maisPessoasDica', { count: resto })}>
          +{resto}
        </button>
      )}
    </div>
  )

  const cartaoPalco = stage && (
    <div className="rm-wbcard rm-wbcard--stage">
      <strong>{t('room.quadro.noPalco')}</strong>
      <span className="dx-num rm-wbcard__live">{t('room.quadro.aEmitirEm', { count: stage.destinos })}</span>
      <Button size="sm" variant={stage.onStage ? 'outline' : 'live'} onClick={alternarPalco}>
        {stage.onStage ? t('room.quadro.tirarDoPalco') : t('room.quadro.porNoPalco')}
      </Button>
    </div>
  )

  return (
    <section className={cx('rm-wb', partilhado && 'is-shared')} aria-label={wb.openedBy ? t('room.quadro.partilhadoPor', { nome: wb.openedBy }) : t('room.quadro.titulo')}>
      <div className="rm-wb__tools" role="toolbar" aria-label={t('room.quadro.ferramentas')}>
        {ferramentas.map((f) => (
          <button
            key={f.id}
            type="button"
            className={cx('rm-wb__tool', ferramenta === f.id && 'is-on')}
            aria-pressed={ferramenta === f.id}
            onClick={() => setFerramenta(f.id)}
          >
            <Icon name={f.icon} size={16} />
            <span>{f.label}</span>
          </button>
        ))}
        {ferramenta === 'forma' && (
          <div className="rm-wb__sub" role="radiogroup" aria-label={t('room.quadro.formas')}>
            {FORMAS.map((f) => (
              <button
                key={f.forma}
                type="button"
                role="radio"
                aria-checked={forma === f.forma}
                aria-label={t(f.label)}
                title={t(f.label)}
                className={cx('rm-wb__subbtn', forma === f.forma && 'is-on')}
                onClick={() => setForma(f.forma)}
              >
                <Icon name={f.icon} size={12} />
              </button>
            ))}
          </div>
        )}
        <button type="button" className="rm-wb__tool" onClick={onClear} disabled={strokes.length === 0}>
          <Icon name="eraser" size={16} />
          <span>{t('room.quadro.limpar')}</span>
        </button>
        <button
          type="button"
          className={cx('rm-wb__tool', ajustesAbertos && 'is-on')}
          aria-pressed={ajustesAbertos}
          aria-controls="rm-wb-pen"
          onClick={() => setAjustesAbertos((v) => !v)}
        >
          <Icon name="sliders" size={16} />
          <span>{t('room.quadro.ajustes')}</span>
        </button>

        <div className="rm-wb__colors" role="radiogroup" aria-label={t('room.quadro.cor')}>
          {WB_COLORS.map((c, i) => (
            <button
              key={c}
              type="button"
              role="radio"
              aria-checked={c === color}
              aria-label={t('room.quadro.corN', { n: i + 1 })}
              className={cx('rm-wb__swatch', c === color && 'is-on')}
              style={{ background: c }}
              onClick={() => setColor(c)}
            />
          ))}
        </div>
        <div className="rm-wb__widths" role="radiogroup" aria-label={t('room.quadro.espessura')}>
          {ESPESSURAS.map((w, i) => (
            <button
              key={w}
              type="button"
              role="radio"
              aria-checked={base === w}
              aria-label={t('room.quadro.espessuraN', { n: i + 1 })}
              title={t('room.quadro.espessuraN', { n: i + 1 })}
              className={cx('rm-wb__width', base === w && 'is-on')}
              onClick={() => setBase(w)}
            >
              <span style={{ width: 6 + i * 6, height: 6 + i * 6 }} />
            </button>
          ))}
        </div>
      </div>

      <div className="rm-wb__center">
        <div className="rm-wb__viewwrap">
        <div className={cx('rm-wb__view', stage?.onStage && 'is-onstage')} ref={viewRef}>
          <div className="rm-wb__sheet" data-theme="light" ref={sheetRef} style={{ width: `${zoom * 100}%`, height: `${zoom * 100}%` }}>
            <canvas ref={canvasRef} />
            <canvas
              ref={liveRef}
              className="rm-wb__live"
              onPointerDown={onDown}
              onPointerMove={onMove}
              onPointerUp={onUp}
              onPointerCancel={onUp}
            />
          </div>
          </div>
          {stage?.onStage && <span className="rm-wb__badge rm-wb__badge--stage dx-num">{t('room.quadro.noPalcoDestinos', { count: stage.destinos })}</span>}
          <div className="rm-wb__zoom" role="group" aria-label={t('room.quadro.zoom')}>
            <button type="button" onClick={() => setZoom((z) => Math.max(ZOOM_MIN, z / 1.25))} aria-label={t('room.apresentacao.reduzir')} title={t('room.apresentacao.reduzir')}>
              <Icon name="minus" size={12} />
            </button>
            <button type="button" className="dx-num" onClick={() => setZoom(1)} title={t('room.apresentacao.repor')}>
              {Math.round(zoom * 100)}%
            </button>
            <button type="button" onClick={() => setZoom((z) => Math.min(ZOOM_MAX, z * 1.25))} aria-label={t('room.apresentacao.ampliar')} title={t('room.apresentacao.ampliar')}>
              <Icon name="plus" size={12} />
            </button>
          </div>
        </div>
        {partilhado && filaPessoas}
      </div>

      <aside className="rm-wb__side" id="rm-wb-pen">
        {partilhado ? (
          <>
            <div className={cx('rm-wbcard', penOn && 'is-on')}>
              <div className="rm-wbcard__head">
                <strong>{t('room.quadro.caneta')}</strong>
                <span className={cx('rm-wbcard__state dx-num', penOn && 'is-on')}>{penOn ? t('room.quadro.ligada') : t('room.quadro.semCaneta')}</span>
              </div>
              <label className="rm-wbslider">
                <span>{t('room.quadro.pressao')}</span>
                <input type="range" min={0} max={100} value={Math.round(ajustes.sensibilidade * 100)} onChange={(e) => setAjustes({ sensibilidade: Number(e.target.value) / 100 })} />
                <span className="dx-num">{Math.round(ajustes.sensibilidade * 100)}%</span>
              </label>
              <label className="rm-wbslider">
                <span>{t('room.quadro.suavizar')}</span>
                <input type="range" min={0} max={100} value={Math.round(ajustes.suavizar * 100)} onChange={(e) => setAjustes({ suavizar: Number(e.target.value) / 100 })} />
                <span className="dx-num">{Math.round(ajustes.suavizar * 100)}%</span>
              </label>
              <Toggle label={t('room.quadro.rejeicaoPalma')} checked={ajustes.rejeicaoPalma} onChange={(e) => setAjustes({ rejeicaoPalma: e.target.checked })} />
              <Toggle label={t('room.quadro.inclinacaoEspessura')} checked={ajustes.inclinacao} onChange={(e) => setAjustes({ inclinacao: e.target.checked })} />
            </div>
            <div className="rm-wbcard">
              <div className="rm-wbcard__head">
                <strong>{t('room.quadro.tintaInteligente')}</strong>
                <span className="dx-num dx-muted">{t('room.quadro.local')}</span>
              </div>
              <button type="button" className={cx('rm-wbopt', ajustes.endireitar && 'is-on')} aria-pressed={ajustes.endireitar} onClick={() => setAjustes({ endireitar: !ajustes.endireitar })}>
                {ajustes.endireitar && <Icon name="check" size={11} />}
                {t('room.quadro.endireitarFormas')}
              </button>
            </div>
            {stage?.onStage && (
              <div className="rm-wbcard rm-wbcard--note">
                <strong>{t('room.quadro.aSerEmitido')}</strong>
                <span className="dx-muted">{t('room.quadro.aSerEmitidoTexto')}</span>
              </div>
            )}
            {cartaoPalco}
          </>
        ) : (
          <>
            {filaPessoas}
            {cartaoPalco}
          </>
        )}
      </aside>
    </section>
  )
}
