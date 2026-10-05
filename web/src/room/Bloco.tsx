/**
 * O BLOCO dos painéis da sala: `<section>` com um título de ícone + texto +
 * (opcionalmente) algo à direita, e o corpo por baixo.
 *
 * Porque existe: treze sítios escreviam a mesma `<section className="rm-block"
 * aria-labelledby="rm-xx-h">` com um `<h3 id="rm-xx-h">` a seguir, e o `id`
 * era uma constante escrita à mão. Dois painéis montados ao mesmo tempo (as
 * definições e as pessoas, em ecrã largo) davam dois `id` iguais no documento
 * — e um `aria-labelledby` que aponta para um `id` repetido lê o PRIMEIRO,
 * que pode ser o título do outro painel. O `useId` acaba com isso.
 *
 * As classes são as mesmas (`rm-block`, `rm-block__title`): isto arruma a
 * marcação, não muda o aspecto.
 */
import { useId, type HTMLAttributes, type ReactNode } from 'react'
import { Icon, type IconName } from '../ui/icons'
import { cx } from '../ui/kit'

export function Bloco({
  icon,
  titulo,
  meta,
  accent,
  className,
  children,
  ...rest
}: {
  icon?: IconName
  titulo: ReactNode
  /** O que vai à DIREITA do título, depois do espaçador (contagem, estado). */
  meta?: ReactNode
  /** Bloco em destaque — a sala de espera com gente à porta. */
  accent?: boolean
  children: ReactNode
} & Omit<HTMLAttributes<HTMLElement>, 'title' | 'children'>) {
  const id = useId()
  return (
    <section className={cx('rm-block', accent && 'rm-block--accent', className)} aria-labelledby={id} {...rest}>
      <h3 id={id} className="rm-block__title">
        {icon && <Icon name={icon} size={13} />}
        {titulo}
        {meta != null && meta !== false && (
          <>
            <span className="dx-spacer" />
            {meta}
          </>
        )}
      </h3>
      {children}
    </section>
  )
}
