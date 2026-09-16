/**
 * Linha de tempo multi-faixa: régua, marcadores e capítulos, V1/V2/A1/A2/CC com
 * visibilidade e bloqueio, clipes com onda sonora, e as ferramentas aplicadas
 * com o ponteiro. Toda a mudança é uma `Edicao` — nada aqui altera o projecto
 * por outro caminho.
 */
import { useEffect, useMemo, useRef, useState } from 'react'
import type { KeyboardEvent as ReactKeyboardEvent, PointerEvent as ReactPointerEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from '../../ui/icons'
import { cx, IconButton } from '../../ui/kit'
import { relogio, timecode } from '../captions/legendas'
import type { Clip, Edicao, FaixaDeClipe, FaixaId, Projecto } from './projecto'
import { clipsDaFaixa, duracaoDoClip, estadoDaFaixa, faixaDeVideo, fimDoClip, fonte, novoId, pontosDeEdicao } from './projecto'
import type { Leitor } from './useLeitor'

export type Ferramenta = 'seleccionar' | 'lamina' | 'aparar' | 'deslizar' | 'transicao' | 'texto' | 'mascara' | 'audio'

export const ZOOMS = [1, 2, 4, 10, 30, 60, 120] as const
const PX_POR_DIVISAO = 80

function Onda({ picos, c, largura, altura }: { picos: Float32Array | undefined; c: Clip; largura: number; altura: number }) {
  const ref = useRef<HTMLCanvasElement>(null)
  useEffect(() => {
    const cv = ref.current
    if (!cv || !picos) return
    const w = Math.max(1, Math.min(4000, Math.round(largura)))
    const dpr = Math.min(2, window.devicePixelRatio || 1)
    cv.width = w * dpr
    cv.height = altura * dpr
    const ctx = cv.getContext('2d')
    if (!ctx) return
    ctx.scale(dpr, dpr)
    ctx.clearRect(0, 0, w, altura)
    ctx.fillStyle = getComputedStyle(cv).color
    const porSegundo = 20
    const meio = altura / 2
    for (let x = 0; x < w; x++) {
      const tf = c.entrada + (x / w) * (c.saida - c.entrada)
      const tf2 = c.entrada + ((x + 1) / w) * (c.saida - c.entrada)
      let m = 0
      for (let i = Math.floor(tf * porSegundo); i <= Math.floor(tf2 * porSegundo) && i < picos.length; i++) m = Math.max(m, picos[i] ?? 0)
      const h = Math.max(1, Math.min(1, m) * (altura - 4))
      ctx.fillRect(x, meio - h / 2, 1, h)
    }
  }, [picos, c.entrada, c.saida, largura, altura])
  if (!picos) return null
  return <canvas ref={ref} className="ed-clip__wave" style={{ width: Math.round(largura), height: altura }} aria-hidden="true" />
}

const ROTULO_FAIXA: Record<FaixaId, string> = { V1: 'v1', V2: 'v2', A1: 'a1', A2: 'a2', CC: 'cc' }

export default function Timeline({
  projecto: p,
  leitor,
  seleccao,
  onSeleccionar,
  ferramenta,
  ripple,
  aplicar,
  ondas,
  onCapitulo,
  onFerramentaUsada,
}: {
  projecto: Projecto
  leitor: Leitor
  seleccao: string | null
  onSeleccionar: (id: string | null) => void
  ferramenta: Ferramenta
  ripple: boolean
  aplicar: (e: Edicao, chave?: string | null) => void
  ondas: Map<string, Float32Array>
  onCapitulo: () => void
  onFerramentaUsada: (f: Ferramenta, clipId: string | null) => void
}) {
  const { t } = useTranslation()
  const dur = p.clips.reduce((a, c) => Math.max(a, fimDoClip(c)), 0)
  const [zoom, setZoom] = useState<number>(() => ZOOMS.find((z) => (dur / z) * PX_POR_DIVISAO < 1100) ?? 120)
  const pps = PX_POR_DIVISAO / zoom
  const largura = Math.max(600, (dur + zoom * 2) * pps)
  const areaRef = useRef<HTMLDivElement>(null)
  const T = leitor.tempo

  // Ao abrir outro projecto, o zoom ajusta-se para caber.
  const projectoId = p.id
  useEffect(() => {
    setZoom(ZOOMS.find((z) => (dur / z) * PX_POR_DIVISAO < 1100) ?? 120)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectoId])

  // A cabeça de leitura fica à vista enquanto toca.
  useEffect(() => {
    const a = areaRef.current
    if (!a || !leitor.aTocar) return
    const x = T * pps
    if (x < a.scrollLeft || x > a.scrollLeft + a.clientWidth - 40) a.scrollLeft = Math.max(0, x - 80)
  }, [T, pps, leitor.aTocar])

  const regua = useMemo(() => {
    const n = Math.ceil(largura / PX_POR_DIVISAO)
    return Array.from({ length: n }, (_, i) => i * zoom)
  }, [largura, zoom])

  function tempoDoPonteiro(e: { clientX: number }): number {
    const a = areaRef.current
    if (!a) return 0
    const r = a.getBoundingClientRect()
    return Math.max(0, (e.clientX - r.left + a.scrollLeft) / pps)
  }

  function buscarNoFundo(e: ReactPointerEvent<HTMLElement>) {
    if ((e.target as HTMLElement).closest('.ed-clip, button')) return
    const t0 = tempoDoPonteiro(e)
    if (ferramenta === 'texto') {
      aplicar({ tipo: 'texto', texto: { id: novoId('t'), inicio: t0, duracao: 4, texto: t('editor.texto.omisso'), x: 0.5, y: 0.8, tamanho: 0.06 } })
      onFerramentaUsada('texto', null)
    }
    leitor.buscar(t0)
    onSeleccionar(null)
    const alvo = e.currentTarget
    alvo.setPointerCapture(e.pointerId)
    const mover = (ev: PointerEvent) => leitor.buscar(tempoDoPonteiro(ev))
    const largar = () => {
      alvo.removeEventListener('pointermove', mover)
      alvo.removeEventListener('pointerup', largar)
    }
    alvo.addEventListener('pointermove', mover)
    alvo.addEventListener('pointerup', largar)
  }

  function premirClip(e: ReactPointerEvent<HTMLButtonElement>, c: Clip) {
    if (e.button !== 0) return
    e.stopPropagation()
    onSeleccionar(c.id)
    const bloqueada = estadoDaFaixa(p, c.faixa).bloqueada
    if (bloqueada) return
    const t0 = tempoDoPonteiro(e)
    switch (ferramenta) {
      case 'lamina':
        aplicar({ tipo: 'dividir', t: t0, clipIds: [c.id] })
        leitor.buscar(t0)
        return
      case 'transicao':
        aplicar({ tipo: 'transicao', clipId: c.id, transicao: c.transicao ? null : { tipo: 'dissolver', duracao: 0.5 } })
        onFerramentaUsada('transicao', c.id)
        return
      case 'texto':
        aplicar({ tipo: 'texto', texto: { id: novoId('t'), inicio: t0, duracao: 4, texto: t('editor.texto.omisso'), x: 0.5, y: 0.8, tamanho: 0.06 } })
        onFerramentaUsada('texto', c.id)
        return
      case 'mascara':
        if (faixaDeVideo(c.faixa) && !c.mascara) aplicar({ tipo: 'mascara', clipId: c.id, mascara: { x: 0.1, y: 0.1, w: 0.8, h: 0.8 } })
        onFerramentaUsada('mascara', c.id)
        return
      case 'audio':
        onFerramentaUsada('audio', c.id)
        return
      default:
        break
    }
    const alvo = e.currentTarget
    const r = alvo.getBoundingClientRect()
    const borda = e.clientX - r.left < 8 ? 'esq' : r.right - e.clientX < 8 ? 'dir' : null
    const modo = ferramenta === 'deslizar' ? 'deslizar' : ferramenta === 'aparar' || borda ? 'aparar' : 'mover'
    const lado = borda ?? (e.clientX - r.left < r.width / 2 ? 'esq' : 'dir')
    const origem = { ...c }
    const arrasto = novoId('d')
    const x0 = e.clientX
    let ultimoX = x0
    alvo.setPointerCapture(e.pointerId)
    const mover = (ev: PointerEvent) => {
      const delta = (ev.clientX - x0) / pps
      if (modo === 'mover') aplicar({ tipo: 'mover', clipId: c.id, inicio: origem.inicio + delta }, `mover:${arrasto}`)
      else if (modo === 'aparar') {
        if (lado === 'esq') aplicar({ tipo: 'aparar', clipId: c.id, entrada: origem.entrada + delta * origem.velocidade, ripple }, `aparar:${arrasto}`)
        else aplicar({ tipo: 'aparar', clipId: c.id, saida: origem.saida + delta * origem.velocidade, ripple }, `aparar:${arrasto}`)
      } else {
        const passo = ((ultimoX - ev.clientX) / pps) * origem.velocidade
        ultimoX = ev.clientX
        aplicar({ tipo: 'deslizar', clipId: c.id, delta: passo }, `deslizar:${arrasto}`)
      }
    }
    const largar = (ev: PointerEvent) => {
      alvo.removeEventListener('pointermove', mover)
      alvo.removeEventListener('pointerup', largar)
      alvo.removeEventListener('pointercancel', largar)
      if (Math.abs(ev.clientX - x0) < 3) leitor.buscar(t0)
    }
    alvo.addEventListener('pointermove', mover)
    alvo.addEventListener('pointerup', largar)
    alvo.addEventListener('pointercancel', largar)
  }

  function teclaNoClip(e: ReactKeyboardEvent<HTMLButtonElement>, c: Clip) {
    const passo = 1 / p.fps
    if (e.key === 'Delete' || e.key === 'Backspace') {
      e.preventDefault()
      aplicar({ tipo: 'remover', clipIds: [c.id], ripple })
      onSeleccionar(null)
    } else if (e.altKey && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) {
      e.preventDefault()
      aplicar({ tipo: 'mover', clipId: c.id, inicio: c.inicio + (e.key === 'ArrowLeft' ? -passo : passo) })
    }
  }

  const faixas: FaixaId[] = ['V1', 'V2', 'A1', 'A2', 'CC']
  const altura: Record<FaixaId, number> = { V1: 52, V2: 44, A1: 44, A2: 34, CC: 30 }
  const pontos = pontosDeEdicao(p)
  const cues = p.legendas?.cues ?? []

  function nomeDoClip(c: Clip, i: number): string {
    const f = fonte(p, c.fonteId)
    if (c.congelado !== null) return t('editor.clip.congelado')
    return `${t('editor.clip.n', { n: i + 1 })} · ${f?.nome ?? ''}`
  }

  return (
    <section className="ed-tl" aria-label={t('studio.edicao.linhaTempo')}>
      <div className="ed-tl__head">
        <span className="st-timecode st-timecode--on dx-num" data-studio="timecode">
          {timecode(T, p.fps)}
        </span>
        <div className="ed-transport" role="group" aria-label={t('editor.transporte.rotulo')}>
          <IconButton
            icon="chevronLeft"
            label={t('editor.transporte.anterior')}
            onClick={() => leitor.buscar([...pontos].reverse().find((x) => x < T - 0.05) ?? 0)}
          />
          <IconButton
            icon={leitor.aTocar ? 'pause' : 'play'}
            label={leitor.aTocar ? t('editor.transporte.pausa') : t('editor.transporte.tocar')}
            onClick={() => leitor.tocar(!leitor.aTocar)}
            data-studio="tocar"
          />
          <IconButton icon="chevronRight" label={t('editor.transporte.seguinte')} onClick={() => leitor.buscar(pontos.find((x) => x > T + 0.05) ?? dur)} />
        </div>
        <span className="dx-num st-small dx-muted" data-studio="duracao">
          / {relogio(dur)}
        </span>
        <span className="ed-tl__sep" aria-hidden="true" />
        <div className="ed-tl__acts">
          <button type="button" className="ed-chip" onClick={() => aplicar({ tipo: 'marcador', marcador: { id: novoId('m'), t: T, rotulo: relogio(T), tipo: 'marcador' } })}>
            <Icon name="pin" /> {t('editor.linha.marcador')}
          </button>
          <button type="button" className="ed-chip" data-studio="dividir" onClick={() => aplicar({ tipo: 'dividir', t: T, ...(seleccao ? { clipIds: [seleccao] } : {}) })}>
            <Icon name="scissors" /> {t('editor.linha.dividir')}
          </button>
          <button type="button" className="ed-chip" onClick={onCapitulo}>
            <Icon name="list" /> {t('editor.linha.capitulo')}
          </button>
        </div>
        <span className="dx-spacer" />
        <label className="ed-zoom">
          <span className="st-label">{t('editor.linha.zoom')}</span>
          <input
            type="range"
            className="st-range"
            min={0}
            max={ZOOMS.length - 1}
            value={ZOOMS.indexOf(zoom as (typeof ZOOMS)[number])}
            onChange={(e) => setZoom(ZOOMS[Number(e.target.value)])}
            aria-valuetext={t('editor.linha.porDivisao', { s: zoom })}
          />
          <span className="dx-num st-small dx-muted">{t('editor.linha.porDivisao', { s: zoom })}</span>
        </label>
      </div>

      <div className="ed-tl__body">
        <div className="ed-tl__labels">
          <div className="ed-tl__ruler-pad" />
          {faixas.map((f) => {
            const e = estadoDaFaixa(p, f)
            return (
              <div key={f} className="ed-tl__label" style={{ height: altura[f] }}>
                <span className="ed-tl__id dx-num" data-faixa={f}>
                  {f}
                </span>
                <span className="ed-tl__name">{t(`editor.faixas.${ROTULO_FAIXA[f]}`)}</span>
                <IconButton
                  icon={e.visivel ? 'eye' : 'eyeOff'}
                  bare
                  className="ed-flag"
                  label={t(e.visivel ? 'editor.faixas.esconder' : 'editor.faixas.mostrar', { f })}
                  aria-pressed={!e.visivel}
                  onClick={() => aplicar({ tipo: 'faixa', id: f, patch: { visivel: !e.visivel } })}
                />
                <IconButton
                  icon="lock"
                  bare
                  className={cx('ed-flag', e.bloqueada && 'ed-flag--on')}
                  label={t(e.bloqueada ? 'editor.faixas.desbloquear' : 'editor.faixas.bloquear', { f })}
                  aria-pressed={e.bloqueada}
                  onClick={() => aplicar({ tipo: 'faixa', id: f, patch: { bloqueada: !e.bloqueada } })}
                />
              </div>
            )
          })}
        </div>

        <div className="ed-tl__area" ref={areaRef}>
          <div className="ed-tl__content" style={{ width: largura }} onPointerDown={buscarNoFundo}>
            <div className="ed-tl__ruler" aria-hidden="true">
              {regua.map((s) => (
                <span key={s} className="ed-tl__tick dx-num" style={{ left: s * pps }}>
                  {relogio(s)}
                </span>
              ))}
            </div>
            <div className="ed-tl__marks">
              {p.marcadores.map((m) => (
                <button
                  key={m.id}
                  type="button"
                  className={cx('ed-mark', m.tipo === 'capitulo' && 'ed-mark--chapter')}
                  style={{ left: m.t * pps }}
                  title={`${m.rotulo} · ${relogio(m.t)}`}
                  aria-label={t(m.tipo === 'capitulo' ? 'editor.linha.capituloEm' : 'editor.linha.marcadorEm', { r: m.rotulo, t: relogio(m.t) })}
                  onClick={() => leitor.buscar(m.t)}
                  onKeyDown={(e) => {
                    if (e.key === 'Delete' || e.key === 'Backspace') aplicar({ tipo: 'remover-marcador', id: m.id })
                  }}
                >
                  {m.tipo === 'capitulo' ? m.rotulo : ''}
                </button>
              ))}
            </div>

            {faixas.map((f) => {
              const e = estadoDaFaixa(p, f)
              if (f === 'CC') {
                return (
                  <div key={f} className={cx('ed-track ed-track--cc', !e.visivel && 'ed-track--off')} style={{ height: altura[f] }}>
                    {cues.map((q) => (
                      <span key={q.id} className="ed-cue" style={{ left: q.inicio * pps, width: Math.max(2, (q.fim - q.inicio) * pps) }} title={q.texto}>
                        {q.texto}
                      </span>
                    ))}
                  </div>
                )
              }
              const lista = clipsDaFaixa(p, f as FaixaDeClipe)
              return (
                <div
                  key={f}
                  className={cx('ed-track', `ed-track--${faixaDeVideo(f) ? 'video' : 'audio'}`, !e.visivel && 'ed-track--off', e.bloqueada && 'ed-track--locked')}
                  style={{ height: altura[f] }}
                  data-faixa={f}
                >
                  {lista.map((c, i) => {
                    const w = duracaoDoClip(c) * pps
                    const sel = c.id === seleccao
                    return (
                      <button
                        key={c.id}
                        type="button"
                        className={cx('ed-clip', sel && 'ed-clip--sel', c.grupo && 'ed-clip--linked', `ed-clip--tool-${ferramenta}`)}
                        style={{ left: c.inicio * pps, width: Math.max(4, w) }}
                        aria-pressed={sel}
                        aria-label={t('editor.clip.rotulo', { faixa: f, nome: nomeDoClip(c, i), de: relogio(c.inicio), ate: relogio(fimDoClip(c)) })}
                        data-clip={c.id}
                        onPointerDown={(ev) => premirClip(ev, c)}
                        onKeyDown={(ev) => teclaNoClip(ev, c)}
                      >
                        {!faixaDeVideo(f) && <Onda picos={ondas.get(c.fonteId)} c={c} largura={w} altura={altura[f] - 6} />}
                        {c.transicao && <span className="ed-clip__trans" style={{ width: c.transicao.duracao * pps }} aria-hidden="true" />}
                        <span className="ed-clip__name">{nomeDoClip(c, i)}</span>
                        {w > 120 && (
                          <span className="ed-clip__time dx-num">
                            {relogio(c.inicio)} — {relogio(fimDoClip(c))}
                            {c.velocidade !== 1 ? ` · ${c.velocidade.toLocaleString()}×` : ''}
                          </span>
                        )}
                      </button>
                    )
                  })}
                </div>
              )
            })}
            <span className="ed-playhead" style={{ left: T * pps }} aria-hidden="true" />
          </div>
        </div>
      </div>
    </section>
  )
}
