/**
 * Peças partilhadas pelos ecrãs do estúdio de TV: a barra de topo, os
 * monitores (programa, plano, miniatura), o deslizador, o interruptor, o
 * fader vertical e o medidor.
 *
 * Tudo o que se mexe a cada frame (vídeo, níveis) escreve directamente no DOM
 * a partir de um tique partilhado — um `setState` a 60 Hz por medidor punha a
 * árvore inteira a re-renderizar.
 */
import { ChangeEvent, CSSProperties, HTMLAttributes, MutableRefObject, ReactNode, useEffect, useRef, type ButtonHTMLAttributes } from 'react'
import { useTranslation } from 'react-i18next'
import { useShell } from '../../../components/shellContext'
import type { CompositorDeAula } from '../../../studio/compositor'
import { desenharImagem, desenharPlano, type FontesParaDesenho } from '../../../studio/tv/desenhoDaMesa'
import type { Plano } from '../../../studio/tv/mesa'
import { DelonixSymbol } from '../../../ui/icons'
import { cx, IconButton } from '../../../ui/kit'

// ------------------------------------------------------------------ tique

type Assinante = (agora: number) => void
const assinantes = new Set<Assinante>()
let raf = 0
function girar(agora: number) {
  for (const a of assinantes) a(agora)
  raf = assinantes.size ? requestAnimationFrame(girar) : 0
}
/** Um só `requestAnimationFrame` para todos os monitores e medidores da página. */
export function useTique(fn: Assinante, activo = true): void {
  const ref = useRef(fn)
  ref.current = fn
  useEffect(() => {
    if (!activo) return
    const a: Assinante = (t) => ref.current(t)
    assinantes.add(a)
    if (!raf) raf = requestAnimationFrame(girar)
    return () => {
      assinantes.delete(a)
    }
  }, [activo])
}

// ------------------------------------------------------------------ caixas

/**
 * O CARTÃO dos ecrãs de TV. Vinte e cinco sítios escreviam
 * `className="tv-cartao tv-cartao--nota"` à mão, e um deles escrevia o fundo
 * (`style={{ background: 'var(--raised)' }}`) numa página — o que a regra da
 * casa proíbe, porque o cartão já sabe qual é o seu fundo em cada ecrã.
 *
 * As variantes são as do template: `painel` (a caixa grande de um ecrã),
 * `nota` (texto sem fundo), `tracejado` (um lugar por preencher), `accent` e
 * `live`. O `style` continua aberto para o POSICIONAMENTO de cada sítio
 * (`flex`, `marginTop: auto`), que não é decoração.
 */
export function Cartao({
  variante,
  como: Tag = 'div',
  className,
  children,
  ...rest
}: {
  variante?: 'painel' | 'nota' | 'tracejado' | 'accent' | 'live'
  /** `section` quando a caixa tem título próprio e `aria-labelledby`. */
  como?: 'div' | 'section'
  children?: ReactNode
} & Omit<HTMLAttributes<HTMLElement>, 'children'>) {
  return (
    <Tag className={cx('tv-cartao', variante && `tv-cartao--${variante}`, className)} {...rest}>
      {children}
    </Tag>
  )
}

/** A CABEÇA de um cartão: título à esquerda, e o que levar `tv-dir` à direita. */
export function Cabeca({
  como: Tag = 'div',
  className,
  children,
  ...rest
}: {
  como?: 'div' | 'span'
  children?: ReactNode
} & Omit<HTMLAttributes<HTMLElement>, 'children'>) {
  return (
    <Tag className={cx('tv-cabeca', className)} {...rest}>
      {children}
    </Tag>
  )
}

/**
 * O BOTÃO dos ecrãs de TV — vinte e um sítios com a mesma cadeia de classes.
 * `forte` é a acção principal, `rec` a destrutiva do ar, `mini` o botão de
 * canto em mono, e `aDireita` encosta-o à direita de uma `Cabeca` (o nome não
 * é `dir` porque esse é um atributo do HTML, e sombreá-lo tirava-o a quem
 * precisasse dele).
 */
export function BotaoTv({
  variante,
  mini,
  aDireita,
  className,
  type,
  children,
  ...rest
}: {
  variante?: 'forte' | 'rec' | 'accent'
  mini?: boolean
  /** Encosta à direita dentro de uma `Cabeca` (`tv-dir`). */
  aDireita?: boolean
} & ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      type={type ?? 'button'}
      className={cx('tv-botao', variante && `tv-botao--${variante}`, mini && 'tv-botao--mini', aDireita && 'tv-dir', className)}
      {...rest}
    >
      {children}
    </button>
  )
}

// ------------------------------------------------------------------ topo

export function TopoTv({ titulo, children, onVoltar }: { titulo: string; children?: ReactNode; onVoltar: () => void }) {
  const { t } = useTranslation()
  const { navOpen, setNavOpen } = useShell()
  return (
    <header className="tv-topo">
      <IconButton
        icon="menu"
        bare
        className="tv-topo__burger"
        label={t('shell.abrirNavegacao')}
        aria-expanded={navOpen}
        aria-controls="shell-nav"
        onClick={() => setNavOpen(!navOpen)}
      />
      <button type="button" className="tv-topo__marca" aria-label={t('tv.voltarAoEstudio')} title={t('tv.voltarAoEstudio')} onClick={onVoltar}>
        <DelonixSymbol />
      </button>
      <h1 className="tv-topo__titulo">{titulo}</h1>
      {children}
    </header>
  )
}

export function Espaco() {
  return <span className="tv-espaco" />
}

// ------------------------------------------------------------------ monitores

/**
 * O PROGRAMA: o próprio canvas do compositor, mudado para aqui enquanto o
 * monitor está montado e devolvido ao palco do Estúdio ao sair. Não é uma
 * cópia — é o que vai para o ar e para a gravação, pixel a pixel.
 */
export function MonitorDoPrograma({
  compRef,
  devolverA,
  className,
}: {
  compRef: MutableRefObject<CompositorDeAula | null>
  devolverA: MutableRefObject<HTMLDivElement | null>
  className?: string
}) {
  const host = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const c = compRef.current
    const h = host.current
    if (!c || !h) return
    h.appendChild(c.canvas)
    const casa = devolverA
    return () => {
      if (casa.current && c.canvas.parentElement === h) casa.current.appendChild(c.canvas)
    }
  }, [compRef, devolverA])
  return <div ref={host} className={cx('tv-monitor__imagem tv-monitor__imagem--programa', className)} data-tv="programa" />
}

/** Um plano da mesa desenhado com o MESMO código do programa (ver `desenhoDaMesa.ts`). */
export function MonitorDePlano({ fontes, plano, largura = 640, altura = 360 }: { fontes: FontesParaDesenho; plano: Plano | null; largura?: number; altura?: number }) {
  const ref = useRef<HTMLCanvasElement>(null)
  const planoRef = useRef(plano)
  planoRef.current = plano
  useTique(() => {
    const c = ref.current?.getContext('2d')
    if (c) desenharPlano(c, planoRef.current, fontes, largura, altura)
  })
  return <canvas ref={ref} width={largura} height={altura} className="tv-monitor__imagem" data-tv="previa" />
}

/** Miniatura de uma fonte, a ~15 fps (as miniaturas não precisam de mais). */
export function Miniatura({ imagem, ajuste = 'cover', largura = 320, altura = 180 }: { imagem: () => HTMLVideoElement | HTMLCanvasElement | null; ajuste?: 'cover' | 'contain'; largura?: number; altura?: number }) {
  const ref = useRef<HTMLCanvasElement>(null)
  const ultimo = useRef(0)
  useTique((agora) => {
    if (agora - ultimo.current < 66) return
    ultimo.current = agora
    const c = ref.current?.getContext('2d')
    if (c) desenharImagem(c, imagem(), { x: 0, y: 0, w: largura, h: altura }, ajuste)
  })
  return <canvas ref={ref} width={largura} height={altura} className="tv-mini__imagem" aria-hidden="true" />
}

// ------------------------------------------------------------------ controlos

/** Deslizador horizontal (input range nativo, com a pele do template). */
export function Deslizador({
  rotulo,
  valor,
  min,
  max,
  passo = 1,
  texto,
  onChange,
  disabled,
  title,
  larguraRotulo,
  larguraValor,
  'data-tv': dataTv,
}: {
  rotulo: string
  valor: number
  min: number
  max: number
  passo?: number
  texto: string
  onChange: (v: number) => void
  disabled?: boolean
  title?: string
  larguraRotulo?: number
  larguraValor?: number
  'data-tv'?: string
}) {
  const pct = ((valor - min) / (max - min)) * 100
  return (
    <label className={cx('tv-desl', disabled && 'is-off')} title={title} data-tv={dataTv}>
      <span className="tv-desl__rotulo" style={larguraRotulo ? { width: larguraRotulo } : undefined}>
        {rotulo}
      </span>
      <input
        type="range"
        min={min}
        max={max}
        step={passo}
        value={valor}
        disabled={disabled}
        style={{ '--pct': `${Math.min(100, Math.max(0, pct))}%` } as CSSProperties}
        onChange={(e: ChangeEvent<HTMLInputElement>) => onChange(Number(e.target.value))}
      />
      <span className="tv-desl__valor dx-num" style={larguraValor ? { width: larguraValor } : undefined}>
        {texto}
      </span>
    </label>
  )
}

/** Interruptor quadrado do template (30×17), com `role="switch"`. */
export function Interruptor({ ligado, onChange, rotulo, disabled, pequeno }: { ligado: boolean; onChange: (v: boolean) => void; rotulo: string; disabled?: boolean; pequeno?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={ligado}
      aria-label={rotulo}
      disabled={disabled}
      className={cx('tv-sw', ligado && 'is-on', pequeno && 'tv-sw--sm')}
      onClick={() => onChange(!ligado)}
    >
      <span className="tv-sw__knob" />
    </button>
  )
}

/** Medidor vertical que lê um nível (0–1) a cada frame, sem passar pelo React. */
export function Medidor({ ler, className }: { ler: () => number; className?: string }) {
  const ref = useRef<HTMLSpanElement>(null)
  useTique(() => {
    const el = ref.current
    if (el) el.style.height = `${Math.round(Math.min(1, Math.max(0, ler())) * 100)}%`
  })
  return (
    <span className={cx('tv-medidor', className)} aria-hidden="true">
      <span ref={ref} className="tv-medidor__nivel" />
    </span>
  )
}

/** Barras de nível do monitor (as do template, por cima da imagem), a partir de um nível 0–1. */
export function BarrasDeNivel({ ler, n = 12 }: { ler: () => number; n?: number }) {
  const ref = useRef<HTMLSpanElement>(null)
  useTique(() => {
    const el = ref.current
    if (!el) return
    const v = Math.min(1, Math.max(0, ler()))
    const barras = el.children
    for (let i = 0; i < barras.length; i++) {
      const b = barras[i] as HTMLElement
      // Uma pequena curva: as barras do meio sobem mais, como um espectro de voz.
      const forma = 0.55 + 0.45 * Math.sin((Math.PI * (i + 0.5)) / barras.length)
      const h = Math.max(2, Math.round(40 * v * forma))
      b.style.height = `${h}px`
      b.classList.toggle('is-alto', h > 32)
    }
  })
  return (
    <span ref={ref} className="tv-barras" aria-hidden="true">
      {Array.from({ length: n }, (_, i) => (
        <span key={i} />
      ))}
    </span>
  )
}

/** Fader vertical (input range nativo em modo vertical). */
export function Fader({ valor, onChange, rotulo, activo, mudo, largo }: { valor: number; onChange: (v: number) => void; rotulo: string; activo?: boolean; mudo?: boolean; largo?: boolean }) {
  return (
    <input
      type="range"
      className={cx('tv-fader', activo && 'is-sel', mudo && 'is-mudo', largo && 'tv-fader--largo')}
      min={0}
      max={1}
      step={0.005}
      value={valor}
      aria-label={rotulo}
      onChange={(e) => onChange(Number(e.target.value))}
    />
  )
}

/** Cronómetro que conta desde `desde` (ms epoch), escrito no DOM a cada meio segundo. */
export function Desde({ desde, className }: { desde: number; className?: string }) {
  const ref = useRef<HTMLSpanElement>(null)
  const ultimo = useRef(0)
  useTique((agora) => {
    if (agora - ultimo.current < 250 || !ref.current) return
    ultimo.current = agora
    ref.current.textContent = desde ? duracao((Date.now() - desde) / 1000) : '00:00'
  })
  return <span ref={ref} className={cx('dx-num', className)} />
}

export function duracao(segundos: number): string {
  const s = Math.max(0, Math.floor(segundos))
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const dd = (n: number) => String(n).padStart(2, '0')
  return h ? `${dd(h)}:${dd(m)}:${dd(s % 60)}` : `${dd(m)}:${dd(s % 60)}`
}
