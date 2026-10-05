/**
 * Menu de contexto — o botão direito do rato.
 *
 * Porque existe: a app não tinha UM `onContextMenu`. Todas as acções de uma
 * linha viviam em botões que só aparecem com o rato por cima (os retratos da
 * sala) ou numa coluna de ícones no fim da linha (as gravações, os contactos),
 * e o botão direito abria o menu do BROWSER por cima da reunião. Quem vem do
 * Zoom, do Explorador ou do Finder tenta o botão direito primeiro.
 *
 * O menu não é só do rato: o browser dispara `contextmenu` também com a tecla
 * ☰ e com ⇧F10, por isso quem navega por teclado chega-lhe pelo mesmo caminho.
 * Dentro dele as setas andam, o Esc fecha e o foco volta a quem o abriu.
 *
 * A posição é uma função pura (`posicaoDoMenu`): um menu aberto no canto
 * inferior direito do ecrã tem de dobrar para dentro, e isso testa-se sem DOM.
 */
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { Icon, type IconName } from './icons'
import { cx } from './kit'

export interface AccaoDeMenu {
  id: string
  label: string
  icon?: IconName
  onPick: () => void
  /** Com valor, a linha é um `menuitemcheckbox` com visto. */
  marcado?: boolean
  /** Acção destrutiva (expulsar, eliminar) — lê-se a vermelho. */
  perigo?: boolean
  disabled?: boolean
}

export interface PontoDoMenu {
  x: number
  y: number
}

export interface TamanhoDoMenu {
  largura: number
  altura: number
}

/**
 * Onde o menu cabe: segue o ponteiro quando cabe, dobra para o lado contrário
 * quando não cabe, e encosta à margem quando nem dobrado cabe (um menu mais
 * alto do que o ecrã num telemóvel).
 */
export function posicaoDoMenu(p: PontoDoMenu, menu: TamanhoDoMenu, ecra: TamanhoDoMenu, margem = 8): PontoDoMenu {
  const eixo = (pos: number, tamanho: number, limite: number) => {
    if (pos + tamanho + margem <= limite) return pos
    if (pos - tamanho >= margem) return pos - tamanho
    return Math.max(margem, limite - tamanho - margem)
  }
  return {
    x: eixo(p.x, menu.largura, ecra.largura),
    y: eixo(p.y, menu.altura, ecra.altura),
  }
}

/** O mínimo de um evento de rato que `abrir` lê. */
export interface EventoDePonteiro {
  clientX: number
  clientY: number
  preventDefault: () => void
}

export interface MenuDeContexto {
  ponto: PontoDoMenu | null
  /** Para o `onContextMenu` de uma linha, de um retrato ou de um cartão. */
  abrir: (e: EventoDePonteiro) => void
  fechar: () => void
}

/**
 * O estado de um menu de contexto. Guarda quem tinha o foco para lho devolver
 * ao fechar — sem isso, o Esc deixava o foco no nada e o Tab recomeçava no
 * topo da página.
 */
export function useMenuDeContexto(): MenuDeContexto {
  const [ponto, setPonto] = useState<PontoDoMenu | null>(null)
  const focoAnterior = useRef<HTMLElement | null>(null)
  const abrir = useCallback((e: EventoDePonteiro) => {
    e.preventDefault()
    focoAnterior.current = (typeof document === 'undefined' ? null : document.activeElement) as HTMLElement | null
    setPonto({ x: e.clientX, y: e.clientY })
  }, [])
  const fechar = useCallback(() => {
    setPonto(null)
    focoAnterior.current?.focus?.()
  }, [])
  return { ponto, abrir, fechar }
}

/**
 * O menu. Não desenha nada sem `ponto` — a página chama-o sempre e é ele que
 * decide, para não haver um `&&` em cada sítio.
 */
export function Menu({
  ponto,
  accoes,
  label,
  onFechar,
  children,
}: {
  ponto: PontoDoMenu | null
  accoes: AccaoDeMenu[]
  /** Nome do menu para quem não vê — «Acções de {{nome}}». */
  label: string
  onFechar: () => void
  /** Cabeçalho opcional (o nome de quem a linha é). */
  children?: ReactNode
}) {
  const ref = useRef<HTMLDivElement>(null)
  const [pos, setPos] = useState<PontoDoMenu | null>(null)

  // Medir e só depois posicionar: a dobra depende do tamanho real do menu, que
  // só se conhece montado. Até lá fica invisível, para não piscar no canto.
  useLayoutEffect(() => {
    if (!ponto) {
      setPos(null)
      return
    }
    const el = ref.current
    if (!el) return
    const r = el.getBoundingClientRect()
    setPos(
      posicaoDoMenu(
        ponto,
        { largura: r.width, altura: r.height },
        { largura: window.innerWidth, altura: window.innerHeight },
      ),
    )
  }, [ponto])

  /**
   * O foco vai para a primeira linha DEPOIS de o menu ter posição — nunca no
   * mesmo efeito que o mede.
   *
   * Medido no browser: enquanto não há `pos` o menu está `visibility: hidden`
   * (para não piscar no canto antes de dobrar), e **um `focus()` num elemento
   * invisível não faz nada**. O menu abria com o foco no `body`: as setas, o
   * Home e o End não andavam, e o Esc fechava-o só porque o ouvinte é da
   * janela. Nenhum teste via isto — não há DOM na bateria.
   */
  useEffect(() => {
    if (!pos) return
    ref.current?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus()
  }, [pos])

  useEffect(() => {
    if (!ponto) return
    const onFora = (e: PointerEvent) => {
      if (!ref.current?.contains(e.target as Node)) onFechar()
    }
    // Fecha com a roda e com o scroll: um menu ancorado a um ponto fica
    // pendurado no sítio errado assim que a lista por baixo dele anda.
    const onAnda = () => onFechar()
    document.addEventListener('pointerdown', onFora, true)
    window.addEventListener('scroll', onAnda, true)
    window.addEventListener('resize', onAnda)
    return () => {
      document.removeEventListener('pointerdown', onFora, true)
      window.removeEventListener('scroll', onAnda, true)
      window.removeEventListener('resize', onAnda)
    }
  }, [ponto, onFechar])

  if (!ponto) return null

  /** Setas, Home e End andam pelas linhas activáveis; Esc e Tab fecham. */
  function aoTeclar(e: React.KeyboardEvent<HTMLDivElement>) {
    if (e.key === 'Escape' || e.key === 'Tab') {
      e.preventDefault()
      onFechar()
      return
    }
    const passos = ['ArrowDown', 'ArrowUp', 'Home', 'End']
    if (!passos.includes(e.key)) return
    e.preventDefault()
    const botoes = Array.from(ref.current?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [])
    if (!botoes.length) return
    const i = botoes.indexOf(document.activeElement as HTMLButtonElement)
    const j =
      e.key === 'Home'
        ? 0
        : e.key === 'End'
          ? botoes.length - 1
          : (i + (e.key === 'ArrowDown' ? 1 : -1) + botoes.length) % botoes.length
    botoes[j < 0 ? 0 : j].focus()
  }

  return (
    <div
      ref={ref}
      className="dx-menu"
      role="menu"
      aria-label={label}
      style={{ left: pos?.x ?? -9999, top: pos?.y ?? -9999, visibility: pos ? 'visible' : 'hidden' }}
      onKeyDown={aoTeclar}
    >
      {children && <div className="dx-menu__head dx-eyebrow">{children}</div>}
      {accoes.map((a) => (
        <button
          key={a.id}
          type="button"
          role={a.marcado === undefined ? 'menuitem' : 'menuitemcheckbox'}
          aria-checked={a.marcado}
          disabled={a.disabled}
          className={cx('dx-menu__item', a.perigo && 'is-danger')}
          onClick={() => {
            onFechar()
            a.onPick()
          }}
        >
          <span className="dx-menu__icon" aria-hidden="true">
            {a.icon && <Icon name={a.icon} size={14} />}
          </span>
          <span className="dx-menu__text">{a.label}</span>
          {a.marcado !== undefined && <span className={cx('dx-menu__check', a.marcado && 'is-on')} aria-hidden="true" />}
        </button>
      ))}
    </div>
  )
}
