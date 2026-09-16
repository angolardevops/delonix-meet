/**
 * As duas peças que cada ecrã compõe: a barra (painel + paginador + aviso de
 * fonte local) e os resultados (a carregar · erro · vazio · lista ou grupos).
 */
import { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon, IconName } from '../icons'
import { Alert, Button, cx, Empty, Skeleton } from '../kit'
import GroupedResults, { AggregateFormat } from './GroupedResults'
import { EMPTY_SEARCH, isEmptySearch } from './model'
import Pager from './Pager'
import SearchPanel from './SearchPanel'
import type { ResourceSearch } from './useResourceSearch'

export function SearchBar<T>({
  rs,
  label,
  placeholder,
  className,
  children,
}: {
  rs: ResourceSearch<T>
  label: string
  placeholder?: string
  className?: string
  /** Controlos da lista à direita do paginador (vista Lista/Grelha…). */
  children?: ReactNode
}) {
  const { t } = useTranslation()
  if (rs.mode === 'error') {
    return (
      <Alert tone="danger">
        {rs.error}{' '}
        <Button size="sm" variant="secondary" icon="refresh" onClick={rs.reload}>
          {t('ui.tentarDeNovo')}
        </Button>
      </Alert>
    )
  }
  if (!rs.schema) {
    return (
      <div className={cx('dx-searchbar', className)} aria-busy="true">
        <Skeleton h={32} w="min(720px, 100%)" />
      </div>
    )
  }
  return (
    <div className={cx('dx-searchbar', className)} data-search-mode={rs.mode}>
      <SearchPanel
        schema={rs.schema}
        state={rs.search}
        onChange={rs.setSearch}
        favorites={rs.favorites}
        label={label}
        placeholder={placeholder}
        aside={
          <>
            {rs.mode === 'local' && (
              <span className="dx-search-note" title={t('search.estado.localDica')}>
                <Icon name="info" size={12} /> {t('search.estado.local')}
              </span>
            )}
            <Pager list={rs.list} />
            {children}
          </>
        }
      />
    </div>
  )
}

export function SearchResults<T>({
  rs,
  renderItems,
  emptyIcon = 'search',
  emptyTitle,
  emptyText,
  emptyAction,
  formatAggregate,
  skeleton,
}: {
  rs: ResourceSearch<T>
  renderItems: (items: T[]) => ReactNode
  emptyIcon?: IconName
  /** Colecção vazia SEM pesquisa (nada existe ainda). */
  emptyTitle: ReactNode
  emptyText?: ReactNode
  emptyAction?: ReactNode
  formatAggregate?: AggregateFormat
  skeleton?: ReactNode
}) {
  const { t } = useTranslation()
  const st = rs.list.state
  const loadingSkeleton = skeleton ?? (
    <div style={{ display: 'grid', gap: 8 }} aria-busy="true">
      {[0, 1, 2, 3].map((i) => (
        <Skeleton key={i} h={36} />
      ))}
    </div>
  )
  if (rs.mode === 'error') return null
  if (rs.mode === 'loading' || st.s === 'loading') return <>{loadingSkeleton}</>
  if (st.s === 'error') {
    return (
      <Alert tone="danger">
        {st.msg}{' '}
        <Button size="sm" variant="secondary" icon="refresh" onClick={rs.list.reload}>
          {t('ui.tentarDeNovo')}
        </Button>
      </Alert>
    )
  }
  const d = st.d
  if (d.total === 0 && d.items.length === 0) {
    return isEmptySearch(rs.search) ? (
      <Empty icon={emptyIcon} title={emptyTitle} action={emptyAction}>
        {emptyText}
      </Empty>
    ) : (
      <Empty
        icon="search"
        title={t('search.estado.vazio')}
        action={
          <Button size="sm" variant="secondary" onClick={() => rs.setSearch({ ...EMPTY_SEARCH, pageSize: rs.search.pageSize })}>
            {t('search.painel.limpar')}
          </Button>
        }
      />
    )
  }
  if (rs.search.groupBy.length && d.groups && rs.fetcher && rs.schema) {
    return (
      <GroupedResults
        fetcher={rs.fetcher}
        schema={rs.schema}
        query={{ ...rs.list.query, page_token: null }}
        env={d}
        levels={rs.search.groupBy}
        renderItems={renderItems}
        formatAggregate={formatAggregate}
      />
    )
  }
  return <>{renderItems(d.items)}</>
}
