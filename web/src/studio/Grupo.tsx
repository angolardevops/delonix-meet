/**
 * O GRUPO da coluna do Estúdio: `<section>` com um título em mono maiúsculo e,
 * quando há, uma acção encostada à direita.
 *
 * O mesmo que o `room/Bloco.tsx` faz para os painéis da sala, e pela mesma
 * razão: seis sítios escreviam `aria-labelledby="st-xx-h"` com o `<h2
 * id="st-xx-h">` a seguir, e o `id` era uma constante à mão. O `useId` acaba
 * com a possibilidade de dois iguais no documento.
 *
 * O `grupo` escreve `data-studio-grupo`, que é por onde o e2e agarra a coluna
 * — não pelo título, que muda com a língua.
 */
import { useId, type HTMLAttributes, type ReactNode } from 'react'
import { cx } from '../ui/kit'

export function Grupo({
  grupo,
  titulo,
  accao,
  className,
  children,
  ...rest
}: {
  /** Identidade estável para o e2e: `data-studio-grupo="fonte"`. */
  grupo: string
  titulo: ReactNode
  /** Encostada à direita do título — passa a haver uma `<header>`. */
  accao?: ReactNode
  children: ReactNode
} & Omit<HTMLAttributes<HTMLElement>, 'title' | 'children'>) {
  const id = useId()
  const h2 = (
    <h2 id={id} className="st-group__title">
      {titulo}
    </h2>
  )
  return (
    <section className={cx('st-group', className)} data-studio-grupo={grupo} aria-labelledby={id} {...rest}>
      {accao ? (
        <header className="st-group__head">
          {h2}
          <span className="dx-spacer" />
          {accao}
        </header>
      ) : (
        h2
      )}
      {children}
    </section>
  )
}
