import { CSSProperties, PointerEvent as ReactPointerEvent, ReactNode, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon, IconName } from '../ui/icons'
import { Avatar, Button, Toggle, avatarTone, cx, initials } from '../ui/kit'
import type { WbStroke } from '../signaling'
import '../ui/whiteboard.css'
import { corDeMarcador, endireitar, espessura, grausDeInclinacao, limitarPontos, suavizar, type AmostraCaneta, type Pt } from './ink'
import { repartirFila } from './stripCapacity'
import type { RemotePeer } from './useRoomCore'
import { useStripCapacity } from './useStripCapacity'
import { WB_COLORS, type WhiteboardState } from './useWhiteboard'
import { daPagina, desenharObjecto, objectoEm, tamanhoDoTexto, type WbObject } from './wbState'

type Ferramenta = 'ponteiro' | 'caneta' | 'marcador' | 'forma' | 'texto' | 'nota' | 'laser' | 'regua' | 'apagar'
type Forma = NonNullable<WbStroke['shape']>

/** Três espessuras de base (B6). O marcador usa o triplo. */
const ESPESSURAS = [2, 4, 8] as const
const FORMAS: { forma: Forma; icon: IconName; label: string }[] = [
  { forma: 'rect', icon: 'square', label: 'room.quadro.formaRect' },
  { forma: 'ellipse', icon: 'circle', label: 'room.quadro.formaElipse' },
  { forma: 'line', icon: 'minus', label: 'room.quadro.formaLinha' },
  { forma: 'arrow', icon: 'arrowRight', label: 'room.quadro.formaSeta' },
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

const nomeCurto = (nome: string) => {
  const p = nome.trim().split(/\s+/)
  return p.length > 1 ? `${p[0]} ${p[p.length - 1][0]}.` : nome
}

/** Miniatura de uma página (rail PÁGINAS): os objectos dela, à escala. */
function Miniatura({ objectos }: { objectos: WbObject[] }) {
  const ref = useRef<HTMLCanvasElement>(null)
  useEffect(() => {
    const c = ref.current
    const ctx = c?.getContext('2d')
    if (!c || !ctx) return
    ctx.clearRect(0, 0, c.width, c.height)
    for (const o of objectos) desenharObjecto(ctx, o, c.width, c.height, c.width / 900, true)
  }, [objectos])
  return <canvas ref={ref} width={192} height={144} aria-hidden="true" />
}

/** Um retrato pequeno da fila «Na sessão» (ou da fila por baixo do quadro partilhado). */
function SeloPessoa({ nome, micOn, speaking, escrever, grande }: { nome: string; micOn: boolean; speaking: boolean; escrever: boolean; grande: boolean }) {
  const { t } = useTranslation()
  return (
    <div className={cx('rm-wbperson', speaking && 'is-speaking', grande && 'is-big')} style={{ '--tone': avatarTone(nome) } as CSSProperties} title={nome}>
      <span className="rm-wbperson__ini" aria-hidden="true">
        {initials(nome)}
      </span>
      <span className="rm-wbperson__name">
        {speaking && <span className="rm-speakdot" aria-hidden="true" />}
        {escrever ? t('room.quadro.pessoaAEscrever', { nome }) : nomeCurto(nome)}
        {!micOn && <Icon name="ban" size={9} className="dx-icon rm-tile__muted" />}
        {!micOn && <span className="dx-sr-only">{t('room.tile.microfoneDesligado')}</span>}
      </span>
    </div>
  )
}

/**
 * Quadro branco colaborativo (templates DelonixWhiteboard e DelonixBoardShared):
 * objectos com id e autor (traços, formas, texto, notas) por página, cursores
 * e laser de toda a gente, apagar e mover o que é seu (ou tudo, o anfitrião),
 * e quem pode escrever.
 *
 * Duas formas: o quadro (ferramentas · páginas · folha · «Na sessão») e, com
 * uma caneta detectada, os ajustes abertos ou o quadro no palco da emissão, o
 * quadro partilhado (cartões à direita, a fila por baixo).
 */
export function Whiteboard({
  wb,
  me,
  myPeerId,
  isHost,
  micOn,
  meSpeaking,
  peers,
  speaking,
  onOpenPeople,
  stage,
  controls,
}: {
  wb: WhiteboardState
  me: string
  myPeerId: string
  isHost: boolean
  micOn: boolean
  meSpeaking: boolean
  peers: RemotePeer[]
  speaking: Set<string>
  onOpenPeople: () => void
  /** Anfitrião a emitir: o quadro pode ir para o palco da emissão. */
  stage: { destinos: number; onStage: boolean; setStream: (s: MediaStream | null) => void } | null
  /** A barra de controlos da sala, que vive por baixo da folha (template). */
  controls?: ReactNode
}) {
  const { t } = useTranslation()
  const { strokes, registerSnapshot, pages } = wb
  const pagina = pages.current
  const objectos = useMemo(() => daPagina(strokes, pagina), [strokes, pagina])
  const sheetRef = useRef<HTMLDivElement>(null)
  const viewRef = useRef<HTMLDivElement>(null)
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const liveRef = useRef<HTMLCanvasElement>(null)
  const textosRef = useRef<Record<string, HTMLElement | null>>({})
  const drawing = useRef<{ pts: Pt[]; pressoes: number[]; amostras: AmostraCaneta[]; caneta: boolean; inicio: Pt } | null>(null)
  const penSeenAt = useRef(0)
  const [ferramenta, setFerramenta] = useState<Ferramenta>('caneta')
  const [forma, setForma] = useState<Forma>('rect')
  const [color, setColor] = useState(WB_COLORS[1])
  const [base, setBase] = useState<number>(ESPESSURAS[1])
  const [zoom, setZoom] = useState(1)
  const [ajustesAbertos, setAjustesAbertos] = useState(false)
  const [ajustes, setAjustesState] = useState<AjustesCaneta>(lerAjustes)
  const [aEscrever, setAEscrever] = useState(false)
  /** Texto/nota a ser escrito (novo, ou a editar um existente). */
  const [editor, setEditor] = useState<{ id: string | null; kind: 'text' | 'note'; x: number; y: number; valor: string } | null>(null)
  /** Objecto a ser arrastado (deslocamento normalizado ainda não enviado). */
  const [arrasto, setArrasto] = useState<{ id: string; inicio: Pt; dx: number; dy: number } | null>(null)
  const [laser, setLaser] = useState<Pt | null>(null)
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
  const penOn = wb.pen.on
  const partilhado = penOn || ajustesAbertos || !!stage?.onStage
  const fila = useStripCapacity('.rm-wbpeople > .rm-wbperson, .rm-wbpeople > .rm-wbpeople__more')
  const podeEscrever = wb.canWrite

  // ── Pintura ────────────────────────────────────────────────────────────────
  const redraw = useCallback(() => {
    const c = canvasRef.current
    const live = liveRef.current
    const sheet = sheetRef.current
    if (!c || !sheet) return
    const w = sheet.clientWidth
    const h = sheet.clientHeight
    if (c.width !== w || c.height !== h) {
      c.width = w
      c.height = h
    }
    if (live && (live.width !== w || live.height !== h)) {
      live.width = w
      live.height = h
    }
    const ctx = c.getContext('2d')
    if (!ctx) return
    ctx.clearRect(0, 0, c.width, c.height)
    for (const o of objectos) {
      if (o.kind === 'text' || o.kind === 'note') continue
      if (arrasto && o.id === arrasto.id) {
        ctx.save()
        ctx.translate(arrasto.dx * c.width, arrasto.dy * c.height)
        desenharObjecto(ctx, o, c.width, c.height, zoom)
        ctx.restore()
      } else desenharObjecto(ctx, o, c.width, c.height, zoom)
    }
  }, [objectos, zoom, arrasto])

  useEffect(() => {
    redraw()
    const ro = new ResizeObserver(redraw)
    if (sheetRef.current) ro.observe(sheetRef.current)
    return () => ro.disconnect()
  }, [redraw])

  /** A página à vista num canvas opaco — para guardar e para o palco da emissão. */
  const pintarPagina = useCallback(
    (c: HTMLCanvasElement) => {
      const ctx = c.getContext('2d')
      if (!ctx) return
      ctx.fillStyle = '#f7f7f8'
      ctx.fillRect(0, 0, c.width, c.height)
      for (const o of objectos) desenharObjecto(ctx, o, c.width, c.height, c.width / (sheetRef.current?.clientWidth || c.width), true)
    },
    [objectos],
  )

  const snapshot = useCallback((): string | null => {
    const sheet = sheetRef.current
    if (!sheet || objectos.length === 0) return null
    const off = document.createElement('canvas')
    off.width = Math.max(1, Math.round(sheet.clientWidth / zoom))
    off.height = Math.max(1, Math.round(sheet.clientHeight / zoom))
    pintarPagina(off)
    return off.toDataURL('image/png')
  }, [objectos, zoom, pintarPagina])

  useEffect(() => {
    registerSnapshot(snapshot)
    return () => registerSnapshot(null)
  }, [snapshot, registerSnapshot])

  // ── Quadro no palco da emissão: um canvas 16:9 opaco, pintado a cada mudança. ──
  const palcoRef = useRef<HTMLCanvasElement | null>(null)
  useEffect(() => {
    if (stage?.onStage && palcoRef.current) pintarPagina(palcoRef.current)
  }, [stage?.onStage, pintarPagina])
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
    pintarPagina(c)
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

  // ── Entrada ────────────────────────────────────────────────────────────────
  const norm = (e: { clientX: number; clientY: number }): Pt => {
    const r = liveRef.current!.getBoundingClientRect()
    return [Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)), Math.min(1, Math.max(0, (e.clientY - r.top) / r.height))]
  }
  const aspecto = () => {
    const c = canvasRef.current
    return c && c.height ? c.width / c.height : 1
  }
  /** Caixas (normalizadas) dos textos e notas, medidas no DOM — para apanhar o clique. */
  const caixasDeTexto = () => {
    const sheet = sheetRef.current
    const out: Record<string, { w: number; h: number }> = {}
    if (!sheet) return out
    const r = sheet.getBoundingClientRect()
    for (const [id, el] of Object.entries(textosRef.current)) {
      if (!el || !r.width || !r.height) continue
      const b = el.getBoundingClientRect()
      out[id] = { w: b.width / r.width, h: b.height / r.height }
    }
    return out
  }
  const alvo = (p: Pt) => {
    const c = liveRef.current
    return objectoEm(objectos, p, c?.width || 1, c?.height || 1, caixasDeTexto())
  }

  /** O traço em curso (ou a forma a ser arrastada) na camada viva. */
  function pintarVivo(traco: WbStroke) {
    const live = liveRef.current
    const ctx = live?.getContext('2d')
    if (!live || !ctx) return
    ctx.clearRect(0, 0, live.width, live.height)
    desenharObjecto(ctx, traco, live.width, live.height, zoom)
  }
  function limparVivo() {
    const live = liveRef.current
    live?.getContext('2d')?.clearRect(0, 0, live.width, live.height)
  }

  const corActual = ferramenta === 'marcador' ? corDeMarcador(color) : color
  const baseActual = ferramenta === 'marcador' ? base * 3 : base
  const entrada = (tipo: string): 'mouse' | 'pen' | 'touch' => (tipo === 'pen' ? 'pen' : tipo === 'touch' ? 'touch' : 'mouse')

  function editar(o: WbObject | null) {
    if (!podeEscrever || !o?.id || (o.kind !== 'text' && o.kind !== 'note')) return false
    setEditor({ id: o.id, kind: o.kind, x: o.pts[0][0], y: o.pts[0][1], valor: o.text ?? '' })
    return true
  }

  function onDown(e: ReactPointerEvent<HTMLCanvasElement>) {
    if (e.pointerType === 'pen') {
      penSeenAt.current = Date.now()
      if (!penOn) wb.setPen((p) => ({ ...p, on: true }))
    }
    // Rejeição de palma: com uma caneta em uso, o toque da mão não desenha.
    if (ajustes.rejeicaoPalma && e.pointerType === 'touch' && Date.now() - penSeenAt.current < 10_000) return
    const p = norm(e)
    if (ferramenta === 'laser') {
      setLaser(p)
      wb.cursor(p[0], p[1], true, entrada(e.pointerType))
      return
    }
    if (!podeEscrever) return
    if (ferramenta === 'texto' || ferramenta === 'nota') {
      // Sem isto, o `mousedown` que se segue tira o foco ao campo acabado de
      // abrir (o canvas não é focável) e o texto fecha-se vazio.
      e.preventDefault()
      if (editar(alvo(p))) return
      setEditor({ id: null, kind: ferramenta === 'texto' ? 'text' : 'note', x: p[0], y: p[1], valor: '' })
      return
    }
    if (ferramenta === 'apagar') {
      const o = alvo(p)
      if (o?.id) wb.erase(o.id)
      return
    }
    try {
      e.currentTarget.setPointerCapture(e.pointerId)
    } catch {
      /* ponteiro sintético */
    }
    if (ferramenta === 'ponteiro') {
      const o = alvo(p)
      if (o?.id) setArrasto({ id: o.id, inicio: p, dx: 0, dy: 0 })
      return
    }
    drawing.current = { pts: [p], pressoes: [e.pressure || 0.5], amostras: [], caneta: e.pointerType === 'pen', inicio: p }
    setAEscrever(true)
    wb.tocar(me)
  }

  function onMove(e: ReactPointerEvent<HTMLCanvasElement>) {
    const ultimo = norm(e)
    // O cursor (ou o laser) de toda a gente vê-se em todos os ecrãs, anfitrião incluído.
    wb.cursor(ultimo[0], ultimo[1], ferramenta === 'laser' && e.buttons > 0, entrada(e.pointerType))
    if (ferramenta === 'laser') {
      if (e.buttons > 0) setLaser(ultimo)
      return
    }
    if (arrasto) {
      setArrasto((a) => (a ? { ...a, dx: ultimo[0] - a.inicio[0], dy: ultimo[1] - a.inicio[1] } : a))
      return
    }
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
      d.pressoes.push(ev.pressure || 0.5)
    }
    const fim = d.pts[d.pts.length - 1]
    if (ferramenta === 'forma') pintarVivo({ kind: 'shape', shape: forma, pts: [d.inicio, fim], c: corActual, w: baseActual })
    else if (ferramenta === 'regua') pintarVivo({ pts: [d.inicio, fim], c: corActual, w: baseActual })
    else pintarVivo({ pts: d.pts, c: corActual, w: baseActual })
  }

  function onUp() {
    setLaser(null)
    if (arrasto) {
      wb.move(arrasto.id, arrasto.dx, arrasto.dy)
      setArrasto(null)
      return
    }
    const d = drawing.current
    drawing.current = null
    setAEscrever(false)
    limparVivo()
    if (!d || d.pts.length < 2) return
    const fim = d.pts[d.pts.length - 1]
    let obj: WbStroke
    if (ferramenta === 'forma') obj = wb.addObject({ kind: 'shape', shape: forma, pts: [d.inicio, fim], c: corActual, w: baseActual })
    else if (ferramenta === 'regua') obj = wb.addObject({ kind: 'stroke', pts: [d.inicio, fim], c: corActual, w: baseActual })
    else {
      const pts = suavizar(d.pts, ajustes.suavizar)
      const recta = ajustes.endireitar && ferramenta === 'caneta' ? endireitar(pts, aspecto()) : null
      if (recta) {
        // O que se reconhece vira FORMA — um objecto que se move e apaga inteiro.
        const xs = recta.pts.map((q) => q[0])
        const ys = recta.pts.map((q) => q[1])
        const shape: Forma = recta.forma === 'elipse' ? 'ellipse' : recta.forma === 'linha' ? 'line' : 'rect'
        const cantos: [number, number][] =
          shape === 'line' ? [recta.pts[0], recta.pts[recta.pts.length - 1]] : [[Math.min(...xs), Math.min(...ys)], [Math.max(...xs), Math.max(...ys)]]
        obj = wb.addObject({ kind: 'shape', shape, pts: cantos, c: corActual, w: baseActual })
      } else {
        const w = d.caneta ? espessura(baseActual, d.amostras, ajustes.sensibilidade, ajustes.inclinacao) : baseActual
        const limitados = limitarPontos(pts)
        // Pressão POR PONTO (B7) só de uma caneta que a reporta — o rato dá sempre 0,5 —
        // e só quando os pontos não foram reduzidos (senão deixava de corresponder).
        const pressao = d.caneta && wb.pen.pressao && limitados.length === d.pressoes.length ? d.pressoes.map((v) => Math.max(0, Math.min(1, v))) : undefined
        obj = wb.addObject({ kind: 'stroke', pts: limitados, c: corActual, w, ...(pressao ? { p: pressao } : {}) })
      }
    }
    if (stage?.onStage && palcoRef.current) {
      const ctx = palcoRef.current.getContext('2d')
      if (ctx) desenharObjecto(ctx, obj, palcoRef.current.width, palcoRef.current.height, palcoRef.current.width / (sheetRef.current?.clientWidth || 1280))
    }
  }

  function fecharEditor(guardar: boolean) {
    const ed = editor
    setEditor(null)
    if (!ed || !guardar) return
    const texto = ed.valor.trim()
    if (!texto) return
    if (ed.id) wb.updateText(ed.id, texto)
    // No texto, a espessura escolhida é o tamanho da letra (pequeno, normal, título).
    else wb.addObject({ kind: ed.kind, pts: [[ed.x, ed.y]], text: texto, c: ed.kind === 'text' ? color : '#4a4326', w: ed.kind === 'text' ? base : 1 })
  }

  // ── Vista ──────────────────────────────────────────────────────────────────
  const ferramentasNormais: Ferramenta[] = ['ponteiro', 'caneta', 'forma', 'texto', 'nota', 'laser', 'regua', 'apagar']
  const ferramentasPartilhado: Ferramenta[] = ['caneta', 'marcador', 'forma', 'texto', 'nota', 'laser', 'apagar', 'ponteiro']
  const icones: Record<Ferramenta, IconName> = {
    ponteiro: partilhado ? 'move' : 'cursor',
    caneta: 'pen',
    marcador: 'highlighter',
    forma: 'shapes',
    texto: 'text',
    nota: 'note',
    laser: 'laser',
    regua: 'ruler',
    apagar: 'eraser',
  }
  const rotulos: Record<Ferramenta, string> = {
    ponteiro: partilhado ? t('room.quadro.mover') : t('room.quadro.ponteiro'),
    caneta: t('room.quadro.caneta'),
    marcador: t('room.quadro.marcador'),
    forma: t('room.quadro.formas'),
    texto: t('room.quadro.texto'),
    nota: t('room.quadro.nota'),
    laser: t('room.quadro.laser'),
    regua: t('room.quadro.regua'),
    apagar: t('room.quadro.apagar'),
  }
  /** Ferramentas que não escrevem — ficam à mão de quem só pode ver. */
  const soVer = (f: Ferramenta) => f === 'laser'

  // A fila «Na sessão»: eu primeiro, depois quem está na sala.
  const pessoas = [
    { id: 'me', nome: me, micOn, speaking: meSpeaking, escrever: aEscrever },
    ...peers.map((p) => ({ id: p.peerId, nome: p.username, micOn: p.micOn, speaking: speaking.has(p.peerId), escrever: false })),
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
        <SeloPessoa key={p.id} nome={p.nome} micOn={p.micOn} speaking={p.speaking} escrever={p.escrever} grande={partilhado} />
      ))}
      {resto > 0 && (
        <button type="button" className="rm-wbpeople__more" onClick={onOpenPeople} aria-label={t('room.palco.maisPessoasDica', { count: resto })}>
          {t('room.quadro.mais', { n: resto })}
        </button>
      )}
    </div>
  )

  // Quem pode escrever: eu e quem está na sala, com o dispositivo de entrada.
  const escritores = [
    { peerId: myPeerId, nome: me, host: isHost, eu: true },
    ...peers.map((p) => ({ peerId: p.peerId, nome: p.username, host: p.host, eu: false })),
  ].map((p) => ({
    ...p,
    pode: !wb.writers.restricted || p.host || wb.writers.writers.includes(p.peerId),
    caneta: p.eu ? penOn : wb.entradas[p.nome] === 'pen',
  }))

  const cursores = Object.entries(wb.cursors).flatMap(([peerId, c]) => {
    const nome = peers.find((p) => p.peerId === peerId)?.username
    return nome ? [{ peerId, c, nome }] : []
  })

  const numero = (n: number) => String(n + 1).padStart(2, '0')

  const folha = (
    <div className={cx('rm-wb__view', stage?.onStage && 'is-onstage')} ref={viewRef}>
      <div
        className={cx('rm-wb__sheet', `is-${ferramenta}`, !podeEscrever && 'is-readonly')}
        data-theme="light"
        ref={sheetRef}
        style={{ width: `${zoom * 100}%`, height: `${zoom * 100}%` }}
      >
        <canvas ref={canvasRef} />
        {objectos
          .filter((o) => (o.kind === 'text' || o.kind === 'note') && o.id !== editor?.id)
          .map((o) => {
            const desloc = arrasto && arrasto.id === o.id ? arrasto : null
            const estilo: CSSProperties = {
              left: `${(o.pts[0][0] + (desloc?.dx ?? 0)) * 100}%`,
              top: `${(o.pts[0][1] + (desloc?.dy ?? 0)) * 100}%`,
              ...(o.kind === 'text' ? { color: o.c, fontSize: `${tamanhoDoTexto(o.w) * zoom}px` } : { fontSize: `${11.5 * zoom}px` }),
            }
            return (
              <div
                key={o.id}
                ref={(el) => {
                  if (o.id) textosRef.current[o.id] = el
                }}
                className={o.kind === 'note' ? 'rm-wbnote' : 'rm-wbtext'}
                style={estilo}
              >
                <span className="rm-wbnote__text">{o.text}</span>
                {o.kind === 'note' && o.by && <small className="dx-num">{initials(o.by)}</small>}
              </div>
            )
          })}
        <canvas
          ref={liveRef}
          className="rm-wb__live"
          title={ferramenta === 'ponteiro' || ferramenta === 'texto' || ferramenta === 'nota' ? t('room.quadro.editarTextoDica') : undefined}
          onPointerDown={onDown}
          onPointerMove={onMove}
          onPointerUp={onUp}
          onPointerCancel={onUp}
          onPointerLeave={() => setLaser(null)}
          onDoubleClick={(e) => void editar(alvo(norm(e)))}
        />
        {editor && (
          <textarea
            className={cx('rm-wbeditor', editor.kind === 'note' ? 'is-note' : 'is-text')}
            style={{ left: `${editor.x * 100}%`, top: `${editor.y * 100}%`, ...(editor.kind === 'text' ? { color, fontSize: `${tamanhoDoTexto(base) * zoom}px` } : {}) }}
            autoFocus
            rows={editor.kind === 'note' ? 3 : 1}
            value={editor.valor}
            maxLength={2000}
            placeholder={t('room.quadro.escreverTexto')}
            aria-label={editor.kind === 'note' ? t('room.quadro.nota') : t('room.quadro.texto')}
            onChange={(e) => setEditor((ed) => (ed ? { ...ed, valor: e.target.value } : ed))}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault()
                fecharEditor(true)
              } else if (e.key === 'Escape') {
                e.preventDefault()
                e.stopPropagation()
                fecharEditor(false)
              }
            }}
            onBlur={() => fecharEditor(true)}
          />
        )}
        {cursores.map(({ peerId, c, nome }) => (
          <div
            key={peerId}
            className={cx('rm-wbcursor', c.laser && 'is-laser')}
            style={{ left: `${c.x * 100}%`, top: `${c.y * 100}%`, '--tone': avatarTone(nome) } as CSSProperties}
            role="img"
            aria-label={t('room.quadro.cursorDe', { nome })}
          >
            {c.laser ? <span className="rm-wbcursor__dot" aria-hidden="true" /> : <Icon name="cursor" size={16} className="dx-icon rm-wbcursor__arrow" />}
            <span className="rm-wbcursor__name">{c.input === 'pen' ? t('room.quadro.cursorCaneta', { nome }) : nome}</span>
          </div>
        ))}
        {laser && (
          <span className="rm-wbcursor is-laser is-mine" style={{ left: `${laser[0] * 100}%`, top: `${laser[1] * 100}%` }} aria-hidden="true">
            <span className="rm-wbcursor__dot" />
          </span>
        )}
      </div>
      {partilhado ? (
        <>
          <div className="rm-wb__chips" data-theme="light">
            <span className="rm-wb__chip dx-num">{t('room.quadro.paginaDe', { n: numero(pagina), total: numero(pages.count - 1) })}</span>
            {stage?.onStage && <span className="rm-wb__chip is-stage dx-num">{t('room.quadro.noPalcoDestinos', { count: stage.destinos })}</span>}
          </div>
          <div className="rm-wb__actions" data-theme="light">
            <Button size="sm" variant="secondary" busy={wb.saving} disabled={objectos.length === 0} onClick={() => void wb.save()}>
              {t('room.quadro.guardar')}
            </Button>
          </div>
        </>
      ) : (
        <div className="rm-wb__zoom" data-theme="light" role="group" aria-label={t('room.quadro.zoom')}>
          <button type="button" onClick={() => setZoom((z) => Math.max(ZOOM_MIN, z / 1.25))} aria-label={t('room.apresentacao.reduzir')} title={t('room.apresentacao.reduzir')}>
            <Icon name="minus" size={11} />
          </button>
          <button type="button" className="dx-num" onClick={() => setZoom(1)} title={t('room.apresentacao.repor')}>
            {t('room.quadro.paginaZoom', { n: numero(pagina), zoom: Math.round(zoom * 100) })}
          </button>
          <button type="button" onClick={() => setZoom((z) => Math.min(ZOOM_MAX, z * 1.25))} aria-label={t('room.apresentacao.ampliar')} title={t('room.apresentacao.ampliar')}>
            <Icon name="plus" size={11} />
          </button>
        </div>
      )}
    </div>
  )

  const listaFerramentas = partilhado ? ferramentasPartilhado : ferramentasNormais

  return (
    <section
      className={cx('rm-wb', partilhado && 'is-shared')}
      aria-label={wb.openedBy ? t('room.quadro.partilhadoPor', { nome: wb.openedBy }) : t('room.quadro.titulo')}
      data-objects={strokes.length}
    >
      <div className="rm-wb__tools" role="toolbar" aria-label={t('room.quadro.ferramentas')}>
        {listaFerramentas.map((f) => (
          <button
            key={f}
            type="button"
            className={cx('rm-wb__tool', ferramenta === f && 'is-on')}
            aria-pressed={ferramenta === f}
            disabled={!podeEscrever && !soVer(f)}
            onClick={() => setFerramenta(f)}
          >
            <Icon name={icones[f]} size={partilhado ? 17 : 15} />
            <span>{rotulos[f]}</span>
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
        {ferramenta === 'apagar' && isHost && (
          <button type="button" className="rm-wb__subbtn" onClick={wb.clear} disabled={objectos.length === 0} title={t('room.quadro.limparTudo')} aria-label={t('room.quadro.limparTudo')}>
            <Icon name="trash" size={12} />
          </button>
        )}
        {!partilhado && (
          <button type="button" className={cx('rm-wb__tool', ajustesAbertos && 'is-on')} aria-pressed={ajustesAbertos} aria-controls="rm-wb-pen" onClick={() => setAjustesAbertos((v) => !v)}>
            <Icon name="sliders" size={15} />
            <span>{t('room.quadro.ajustes')}</span>
          </button>
        )}

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
              <span style={{ width: 8 + i * 5, height: 8 + i * 5 }} />
            </button>
          ))}
        </div>
      </div>

      {!partilhado && (
        <nav className="rm-wb__pages" aria-label={t('room.quadro.paginas')}>
          <span className="rm-wb__eyebrow">{t('room.quadro.paginas')}</span>
          {Array.from({ length: pages.count }, (_, i) => (
            <button
              key={i}
              type="button"
              className={cx('rm-wbpage', i === pagina && 'is-on')}
              aria-current={i === pagina ? 'page' : undefined}
              aria-label={t('room.quadro.paginaN', { n: i + 1 })}
              // Mudar de página muda-a para toda a gente: anfitrião (o servidor decide).
              disabled={!isHost}
              onClick={() => wb.setPage(i)}
            >
              <Miniatura objectos={i === pagina ? objectos : daPagina(strokes, i)} />
              <span className="dx-num" data-theme="light">{numero(i)}</span>
            </button>
          ))}
          {podeEscrever && (
            <button type="button" className="rm-wbpage is-add" aria-label={t('room.quadro.novaPagina')} title={t('room.quadro.novaPagina')} onClick={wb.addPage}>
              <Icon name="plus" size={13} />
            </button>
          )}
        </nav>
      )}

      <div className="rm-wb__center">
        {!podeEscrever && <p className="rm-wb__readonly">{t('room.quadro.soLeitura')}</p>}
        {folha}
        {partilhado && filaPessoas}
        {controls}
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
                <input
                  type="range"
                  min={0}
                  max={100}
                  value={Math.round(ajustes.sensibilidade * 100)}
                  style={{ '--v': `${Math.round(ajustes.sensibilidade * 100)}%` } as CSSProperties}
                  onChange={(e) => setAjustes({ sensibilidade: Number(e.target.value) / 100 })}
                />
                <span className="dx-num">{Math.round(ajustes.sensibilidade * 100)}%</span>
              </label>
              <label className="rm-wbslider">
                <span>{t('room.quadro.suavizar')}</span>
                <input
                  type="range"
                  min={0}
                  max={100}
                  value={Math.round(ajustes.suavizar * 100)}
                  style={{ '--v': `${Math.round(ajustes.suavizar * 100)}%` } as CSSProperties}
                  onChange={(e) => setAjustes({ suavizar: Number(e.target.value) / 100 })}
                />
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
            <div className="rm-wbcard">
              <div className="rm-wbcard__head">
                <strong>{t('room.quadro.quemPodeEscrever')}</strong>
              </div>
              {escritores.slice(0, 6).map((p) => {
                const badge = !p.pode ? t('room.quadro.soVer') : p.caneta ? t('room.quadro.podeCaneta') : t('room.quadro.podeRato')
                const podeMudar = isHost && !p.host && wb.writers.restricted
                return (
                  <button
                    key={p.peerId || p.nome}
                    type="button"
                    className={cx('rm-wbwriter', !p.pode && 'is-viewer')}
                    disabled={!podeMudar}
                    aria-pressed={podeMudar ? p.pode : undefined}
                    aria-label={podeMudar ? (p.pode ? t('room.quadro.retirarEscrita', { nome: p.nome }) : t('room.quadro.convidar', { nome: p.nome })) : undefined}
                    onClick={() => wb.grant(p.peerId, !p.pode)}
                  >
                    <Avatar name={p.nome} size={22} />
                    <span className="rm-wbwriter__name">{p.nome}</span>
                    <span className={cx('rm-wbwriter__badge dx-num', !p.pode ? 'is-view' : p.caneta ? 'is-pen' : 'is-mouse')}>{badge}</span>
                  </button>
                )
              })}
              {isHost && (
                <button type="button" className="rm-wbopt is-center" onClick={() => wb.setLocked(!wb.writers.restricted)}>
                  {wb.writers.restricted ? t('room.quadro.libertar') : t('room.quadro.restringir')}
                </button>
              )}
            </div>
            {stage?.onStage && (
              <div className="rm-wbcard rm-wbcard--note">
                <strong>{t('room.quadro.aSerEmitido')}</strong>
                <span className="dx-muted">{t('room.quadro.aSerEmitidoTexto')}</span>
              </div>
            )}
            {stage && (
              <Button size="sm" variant={stage.onStage ? 'outline' : 'live'} onClick={alternarPalco}>
                {stage.onStage ? t('room.quadro.tirarDoPalco') : t('room.quadro.porNoPalco')}
              </Button>
            )}
            {ajustesAbertos && !penOn && !stage?.onStage && (
              <Button size="sm" variant="ghost" onClick={() => setAjustesAbertos(false)}>
                {t('room.painel.fechar')}
              </Button>
            )}
          </>
        ) : (
          <>
            {filaPessoas}
            {stage && (
              <div className="rm-wbcard rm-wbcard--stage">
                <strong>{t('room.quadro.noPalco')}</strong>
                <span className="dx-num rm-wbcard__live">{t('room.quadro.aEmitirEm', { count: stage.destinos })}</span>
                <Button size="sm" variant={stage.onStage ? 'outline' : 'live'} onClick={alternarPalco}>
                  {stage.onStage ? t('room.quadro.tirarDoPalco') : t('room.quadro.porNoPalco')}
                </Button>
              </div>
            )}
          </>
        )}
      </aside>
    </section>
  )
}
