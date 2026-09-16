/** Lista ordenada com barra relativa ao primeiro — organizadores e quarentena. */
import { Avatar } from '../../ui/kit'
import { useNumFmt } from './format'

export function RankList({ rows, label }: { rows: { key: string; name: string; count: number }[]; label: string }) {
  const { n } = useNumFmt()
  const max = Math.max(1, ...rows.map((r) => r.count))
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
