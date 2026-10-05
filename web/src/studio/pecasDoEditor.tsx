/**
 * As peças repetidas do EDITOR — a linha de tempo, as legendas e as
 * exportações partilham-nas.
 *
 * Porque existem: `ed-btn` estava escrito à mão em vinte e dois sítios e
 * `ed-card` em treze, cada um com a sua cadeia de modificadores
 * (`'ed-btn ed-btn--sm ed-btn--danger'`), e meia dúzia deles juntava-os com
 * `cx()` para ligar um estado. Um botão do editor passa a ser um botão do
 * editor, com o estado em propriedades — e quem acrescentar um modificador
 * acrescenta-o AQUI, não numa página.
 *
 * O kit `dx-btn` não serve: o editor é denso de propósito (26 px de altura
 * contra 32, mono a 10,5 px) e vive sobre a coluna `raised` do template. As
 * classes ficam as mesmas; isto arruma a marcação, não muda o aspecto.
 */
import type { AnchorHTMLAttributes, ButtonHTMLAttributes, HTMLAttributes, ReactNode } from 'react'
import { cx } from '../ui/kit'

type VarianteDoBotao = 'primary' | 'on' | 'sel' | 'danger'

/** Botão do editor. `sm` é a altura curta (22 px), para barras apertadas. */
export function BotaoEd({
  variante,
  sm,
  className,
  type,
  children,
  ...rest
}: {
  variante?: VarianteDoBotao | false
  sm?: boolean
} & ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button type={type ?? 'button'} className={cx('ed-btn', sm && 'ed-btn--sm', variante && `ed-btn--${variante}`, className)} {...rest}>
      {children}
    </button>
  )
}

/** O mesmo botão como LIGAÇÃO (descarregar um ficheiro, abrir uma ajuda). */
export function LigacaoEd({ sm, className, children, ...rest }: { sm?: boolean } & AnchorHTMLAttributes<HTMLAnchorElement>) {
  return (
    <a className={cx('ed-btn', sm && 'ed-btn--sm', className)} {...rest}>
      {children}
    </a>
  )
}

/**
 * Cartão do editor. `accent` destaca-o, `end` encosta-o ao fim da coluna e
 * `link` torna-o clicável — essa quer `como="button"`.
 */
export function CartaoEd({
  variante,
  como: Tag = 'div',
  className,
  children,
  ...rest
}: {
  variante?: 'accent' | 'end' | 'link' | false
  como?: 'div' | 'section' | 'button'
  children?: ReactNode
} & Omit<HTMLAttributes<HTMLElement>, 'children'>) {
  return (
    <Tag
      {...(Tag === 'button' ? { type: 'button' as const } : {})}
      className={cx('ed-card', variante && `ed-card--${variante}`, className)}
      {...rest}
    >
      {children}
    </Tag>
  )
}

/** O título de um cartão do editor. */
export function TituloEd({
  como: Tag = 'div',
  className,
  children,
  ...rest
}: {
  como?: 'div' | 'span' | 'h3'
  children?: ReactNode
} & Omit<HTMLAttributes<HTMLElement>, 'children'>) {
  return (
    <Tag className={cx('ed-card__title', className)} {...rest}>
      {children}
    </Tag>
  )
}
