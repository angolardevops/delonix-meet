/**
 * Lista ordenada com barra relativa ao primeiro — organizadores e quarentena.
 *
 * Com `searchNs`, leva o painel de pesquisa estilo Odoo: as linhas já vêm
 * agregadas e inteiras do servidor (top N de `/stats` ou da quarentena), por
 * isso filtram-se aqui, e o ecrã diz que é no browser.
 */
import { useTranslation } from 'react-i18next'
import { Avatar } from '../../ui/kit'
import { localSchema } from '../../ui/search/localSchema'
import ListSearch from '../../ui/search/ListSearch'
import { useNumFmt } from './format'

type Row = { key: string; name: string; count: number }

const RANK_SOURCE = {
  schema: localSchema(
    'rank',
    [
      { name: 'name', type: 'text' },
      { name: 'count', type: 'number', aggregates: ['sum'] },
    ],
    [],
    { textFields: ['name'], defaultOrder: ['-count'] },
  ),
  get: (r: Row, f: string) => (f === 'name' ? r.name : f === 'count' ? r.count : null),
  text: (r: Row) => r.name,
}

export function RankList({ rows, label, searchNs }: { rows: Row[]; label: string; searchNs?: string }) {
  const { t } = useTranslation()
  if (!searchNs) return <Ranked rows={rows} label={label} max={Math.max(1, ...rows.map((r) => r.count))} />
  const max = Math.max(1, ...rows.map((r) => r.count))
  return (
    <ListSearch
      rows={rows}
      source={RANK_SOURCE}
      ns={searchNs}
      label={t('search.rotulos.rank')}
      emptyTitle={t('ui.semResultados')}
      className="an-ranksearch"
      renderItems={(items) => <Ranked rows={items} label={label} max={max} />}
    />
  )
}

function Ranked({ rows, label, max }: { rows: Row[]; label: string; max: number }) {
  const { n } = useNumFmt()
  return (
    <ol className="an-rank" aria-label={label}>
      {rows.map((r, i) => (
        <li key={r.key} className="an-rank__row">
          <span className="an-rank__pos dx-num">{i + 1}</span>
          <Avatar name={r.name} size={22} />
          <span className="an-rank__name">{r.name}</span>
          <span className="an-rank__bar" aria-hidden="true">
            <span style={{ width: `${(r.count / max) * 100}%` }} />
          </span>
          <span className="an-rank__count dx-num">{n(r.count)}</span>
        </li>
      ))}
    </ol>
  )
}
