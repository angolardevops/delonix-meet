/**
 * Painel + resultados para uma lista que o ecrã JÁ carregou inteira (webhooks,
 * chaves de API, destinos, exportações, tabelas da Análise). Se `resource`
 * for dado e o servidor o descrever, usa o servidor; senão filtra as linhas
 * recebidas no browser (e diz isso no ecrã).
 */
import { ReactNode, useMemo } from 'react'
import type { LocalSource } from './local'
import { SearchBar, SearchResults } from './SearchResults'
import { useResourceSearch } from './useResourceSearch'
import type { SearchSchema } from '../../api'
import type { IconName } from '../icons'

export default function ListSearch<T>({
  rows,
  source,
  resource = null,
  orgId,
  ns,
  label,
  emptyIcon,
  emptyTitle,
  renderItems,
  className,
}: {
  rows: T[]
  source: Omit<LocalSource<T>, 'schema'> & { schema: SearchSchema }
  resource?: string | null
  orgId?: string | null
  /** Prefixo do estado na URL (vários painéis no mesmo ecrã). */
  ns: string
  label: string
  emptyIcon?: IconName
  emptyTitle: ReactNode
  renderItems: (items: T[]) => ReactNode
  className?: string
}) {
  // Pelo CONTEÚDO: quem chama costuma passar um `.map(...)` novo a cada render,
  // e uma identidade nova recarregava a pesquisa em ciclo.
  const key = JSON.stringify(rows)
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const fallback = useMemo(() => ({ load: async () => rows, source }), [key, source])
  const rs = useResourceSearch<T>({ resource, orgId, ns, fallback, deps: [key] })
  return (
    <div className={className}>
      <SearchBar rs={rs} label={label} />
      <div className="dx-listsearch__results">
        <SearchResults rs={rs} emptyIcon={emptyIcon} emptyTitle={emptyTitle} renderItems={renderItems} />
      </div>
    </div>
  )
}
