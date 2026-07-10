import { useTranslation } from 'react-i18next'

type RoadItem = { t: string; cat: string; done?: boolean }
type RoadCol = { phase: string; when: string; items: RoadItem[] }

const CAT_COLORS: Record<string, string> = {
  media: '#3d7dd8',
  ent: '#C8201D',
  collab: '#3ddc97',
  plat: '#9a6ab0',
  rel: '#f5b942',
}

/** Roadmap do produto — colunas Agora/Próximo/Depois, com o que já está feito. */
export default function Roadmap() {
  const { t } = useTranslation()
  const cols = t('road.cols', { returnObjects: true }) as RoadCol[]
  return (
    <div className="page">
      <header className="page-head">
        <h1>{t('road.title')}</h1>
        <p className="muted">{t('road.sub')}</p>
      </header>
      <div className="road-grid">
        {cols.map((c) => (
          <section key={c.phase} className="road-col">
            <header className="road-col-head">
              <h2>{c.phase}</h2>
              <span className="muted small mono">{c.when}</span>
            </header>
            {c.items.map((it) => (
              <div key={it.t} className={it.done ? 'road-item done' : 'road-item'}>
                <span className="road-cat" style={{ background: CAT_COLORS[it.cat] ?? 'var(--muted)' }} />
                <span className="road-text">{it.t}</span>
                <span className="road-tags">
                  <em>{t(`road.cats.${it.cat}`)}</em>
                  {it.done && <strong className="road-done">✓ {t('road.done')}</strong>}
                </span>
              </div>
            ))}
          </section>
        ))}
      </div>
    </div>
  )
}
