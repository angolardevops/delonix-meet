/**
 * Lista agrupada: cabeçalhos expansíveis com contagem e agregados, até três
 * níveis. Os grupos são PREGUIÇOSOS como no contrato (§2.3): o servidor só
 * devolve o primeiro nível; abrir um grupo pede a mesma lista com o `filter`
 * que o grupo trouxe e o `group_by` que falta. Dentro do último nível, as
 * linhas vêm paginadas pelo cursor do próprio grupo.
 */
import { ReactNode, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, Domain, DomainNode, isAbort, ListEnvelope, ListGroup, ListQuery, SearchSchema } from '../../api'
import { Async } from '../../components/AsyncSection'
import { Icon } from '../icons'
import { Alert, Button, IconButton, Skeleton } from '../kit'
import { SchemaLabels, useSchemaLabels } from './labels'
import type { ListFetcher } from './useSearch'

export type AggregateFormat = (field: string, kind: string, value: number) => string

function asList(f: Domain | null | undefined): DomainNode[] {
  if (!f) return []
  if (Array.isArray(f) && typeof f[0] !== 'string') return f as DomainNode[]
  return [f as DomainNode]
}

export function groupTitle(g: ListGroup, level: string, schema: SearchSchema, L: SchemaLabels, t: (k: string) => string): string {
  if (g.key === null) return t('search.grupos.semValor')
  const [name, gran] = level.split(':')
  const f = schema.fields.find((x) => x.name === name)
  if (f?.type === 'datetime') return L.dateKey(g.key, gran ?? 'month')
  if (f?.type === 'enum') return L.option(name, g.key)
  if (f?.type === 'bool') return g.key === 'true' ? t('search.personalizado.sim') : t('search.personalizado.nao')
  return g.label ?? g.key
}

export default function GroupedResults<T>({
  fetcher,
  schema,
  query,
  env,
  levels,
  renderItems,
  formatAggregate,
  depth = 0,
}: {
  fetcher: ListFetcher<T>
  schema: SearchSchema
  /** A pesquisa de onde estes grupos vieram (sem cursor). */
  query: ListQuery
  env: ListEnvelope<T>
  /** O `group_by` completo; `levels[depth]` é o campo destes grupos. */
  levels: string[]
  renderItems: (items: T[]) => ReactNode
  formatAggregate?: AggregateFormat
  depth?: number
}) {
  const { t } = useTranslation()
  const [extra, setExtra] = useState<ListGroup[]>([])
  const [nextToken, setNextToken] = useState(env.next_groups_page_token ?? null)
  const [busy, setBusy] = useState(false)
  useEffect(() => {
    setExtra([])
    setNextToken(env.next_groups_page_token ?? null)
  }, [env])
  const groups = [...(env.groups ?? []), ...extra]

  async function more() {
    if (!nextToken) return
    setBusy(true)
    try {
      const d = await fetcher({ ...query, page_size: 1, page_token: null, groups_page_token: nextToken }, new AbortController().signal)
      setExtra((x) => [...x, ...(d.groups ?? [])])
      setNextToken(d.next_groups_page_token ?? null)
    } finally {
      setBusy(false)
    }
  }

  if (groups.length === 0) return null
  return (
    <div className="dx-groups" data-depth={depth} role="list">
      {groups.map((g) => (
        <GroupRow
          key={`${g.key}`}
          group={g}
          fetcher={fetcher}
          schema={schema}
          query={query}
          levels={levels}
          depth={depth}
          renderItems={renderItems}
          formatAggregate={formatAggregate}
        />
      ))}
      {nextToken && (
        <Button size="sm" variant="ghost" busy={busy} onClick={() => void more()} className="dx-groups__more">
          {t('search.grupos.maisGrupos')}
        </Button>
      )}
    </div>
  )
}

function GroupRow<T>({
  group: g,
  fetcher,
  schema,
  query,
  levels,
  depth,
  renderItems,
  formatAggregate,
}: {
  group: ListGroup
  fetcher: ListFetcher<T>
  schema: SearchSchema
  query: ListQuery
  levels: string[]
  depth: number
  renderItems: (items: T[]) => ReactNode
  formatAggregate?: AggregateFormat
}) {
  const { t, i18n } = useTranslation()
  const L = useSchemaLabels(schema)
  const [open, setOpen] = useState(false)
  const title = groupTitle(g, levels[depth] ?? '', schema, L, t)
  const aggs = Object.entries(g.aggregates ?? {}).flatMap(([field, kinds]) =>
    Object.entries(kinds).map(([kind, value]) => ({
      field,
      kind,
      text: `${kind === 'avg' ? t('search.grupos.media', { campo: L.field(field) }) : t('search.grupos.soma', { campo: L.field(field) })} ${
        formatAggregate ? formatAggregate(field, kind, value) : value.toLocaleString(i18n.language, { maximumFractionDigits: 1 })
      }`,
    })),
  )
  return (
    <div className="dx-group" role="listitem" data-group-key={g.key ?? ''}>
      <button type="button" className="dx-group__head" aria-expanded={open} onClick={() => setOpen((o) => !o)} style={{ paddingLeft: 10 + depth * 18 }}>
        <Icon name={open ? 'chevronDown' : 'chevronRight'} size={13} />
        <span className="dx-group__title">{title}</span>
        <span className="dx-group__count dx-num">{t('search.grupos.registos', { count: g.count })}</span>
        <span className="dx-spacer" />
        {aggs.map((a) => (
          <span key={`${a.field}-${a.kind}`} className="dx-group__agg dx-num">
            {a.text}
          </span>
        ))}
      </button>
      {open && <GroupBody group={g} fetcher={fetcher} schema={schema} query={query} levels={levels} depth={depth} renderItems={renderItems} formatAggregate={formatAggregate} />}
    </div>
  )
}

function GroupBody<T>({
  group: g,
  fetcher,
  schema,
  query,
  levels,
  depth,
  renderItems,
  formatAggregate,
}: {
  group: ListGroup
  fetcher: ListFetcher<T>
  schema: SearchSchema
  query: ListQuery
  levels: string[]
  depth: number
  renderItems: (items: T[]) => ReactNode
  formatAggregate?: AggregateFormat
}) {
  const { t } = useTranslation()
  const sub: ListQuery = { ...query, filter: [...asList(query.filter), ...asList(g.filter)], group_by: g.group_by, page_token: null, groups_page_token: null }
  const key = JSON.stringify(sub)
  const [tokens, setTokens] = useState<(string | null)[]>([null])
  const [idx, setIdx] = useState(0)
  const [state, setState] = useState<Async<ListEnvelope<T>>>({ s: 'loading' })
  const [nonce, setNonce] = useState(0)

  useEffect(() => {
    const ctrl = new AbortController()
    setState({ s: 'loading' })
    fetcher({ ...sub, page_token: tokens[idx] ?? null }, ctrl.signal)
      .then((d) => {
        if (ctrl.signal.aborted) return
        setState({ s: 'ready', d })
        setTokens((ts) => {
          const c = ts.slice(0, idx + 1)
          if (d.next_page_token) c[idx + 1] = d.next_page_token
          return c
        })
      })
      .catch((e) => {
        if (!isAbort(e)) setState({ s: 'error', msg: apiErrorMessage(e, t('search.estado.erro')) })
      })
    return () => ctrl.abort()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, idx, nonce])

  if (state.s === 'loading') {
    return (
      <div className="dx-group__body" aria-busy="true">
        <Skeleton h={28} />
      </div>
    )
  }
  if (state.s === 'error') {
    return (
      <div className="dx-group__body">
        <Alert tone="danger">
          {state.msg}{' '}
          <Button size="sm" variant="secondary" icon="refresh" onClick={() => setNonce((n) => n + 1)}>
            {t('ui.tentarDeNovo')}
          </Button>
        </Alert>
      </div>
    )
  }
  const d = state.d
  if (g.group_by.length && d.groups?.length) {
    return <GroupedResults fetcher={fetcher} schema={schema} query={sub} env={d} levels={levels} depth={depth + 1} renderItems={renderItems} formatAggregate={formatAggregate} />
  }
  const size = query.page_size ?? 50
  return (
    <div className="dx-group__body">
      {renderItems(d.items)}
      {(idx > 0 || d.next_page_token) && (
        <div className="dx-group__pager">
          <span className="dx-num dx-muted">
            {idx * size + 1}-{idx * size + d.items.length} / {d.total}
          </span>
          <IconButton icon="chevronLeft" bare label={t('search.paginador.anterior')} disabled={idx === 0} onClick={() => setIdx((i) => i - 1)} />
          <IconButton icon="chevronRight" bare label={t('search.paginador.seguinte')} disabled={!d.next_page_token} onClick={() => setIdx((i) => i + 1)} />
        </div>
      )}
    </div>
  )
}
