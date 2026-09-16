/**
 * Gravações recentes em cartões. Uma gravação falhada NÃO é clicável (R59):
 * não há nada para abrir, e o cartão diz porquê em vez de fingir um vídeo.
 */
import { useTranslation } from 'react-i18next'
import { recordingsLibrary, RecordingItem } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { useShell } from '../../components/shellContext'
import { Icon } from '../../ui/icons'
import { Empty, Skeleton, StatusBadge } from '../../ui/kit'
import { fmtBytes, localeOf } from '../calendar/dates'

const MAX = 4

export default function RecentRecordings() {
  const { t, i18n } = useTranslation()
  const { navigate } = useShell()
  const locale = localeOf(i18n.language)
  const { state, reload } = useAsync(async (signal) => {
    const all = await recordingsLibrary(signal)
    return [...all].sort((a, b) => b.created_at.localeCompare(a.created_at)).slice(0, MAX)
  }, [])

  const skeleton = (
    <div className="home-recs" aria-busy="true">
      {Array.from({ length: MAX }, (_, i) => (
        <div key={i} className="home-rec">
          <div className="home-rec__thumb" />
          <div className="home-rec__body">
            <Skeleton h={11} w="80%" />
            <Skeleton h={9} w="50%" />
          </div>
        </div>
      ))}
    </div>
  )

  function body(r: RecordingItem) {
    const failed = r.status !== 'ready'
    const size = fmtBytes(r.size_bytes, locale)
    return (
      <>
        <span className="home-rec__thumb">
          {failed ? (
            <StatusBadge tone="record" icon="alert">
              {t('home.gravacoes.falhou')}
            </StatusBadge>
          ) : (
            <>
              <Icon name="play" size={18} />
              <span className="home-rec__chip dx-num">
                {size.value} {size.unit}
              </span>
            </>
          )}
        </span>
        <span className="home-rec__body">
          <strong>{r.filename.replace(/\.(webm|mp4|mkv)$/i, '')}</strong>
          <small>
            {new Date(r.created_at).toLocaleDateString(locale, { day: 'numeric', month: 'short' })}
            {' · '}
            {failed ? r.failure_reason || t('home.gravacoes.semMedia') : <span className="dx-num">{r.room_code}</span>}
          </small>
        </span>
      </>
    )
  }

  return (
    <section className="home-section" aria-labelledby="home-gravacoes">
      <div className="home-section__head">
        <h2 id="home-gravacoes">{t('home.gravacoes.titulo')}</h2>
        <span className="dx-spacer" />
        <a className="home-link" href="#/recordings">
          {t('home.gravacoes.biblioteca')}
        </a>
      </div>
      <AsyncSection state={state} onRetry={reload} skeleton={skeleton}>
        {(rs) =>
          rs.length === 0 ? (
            <div className="home-panel">
              <Empty icon="film" title={t('home.gravacoes.vazio')}>
                {t('home.gravacoes.vazioDica')}
              </Empty>
            </div>
          ) : (
            <ul className="home-recs" role="list">
              {rs.map((r) => (
                <li key={r.id}>
                  {r.status === 'ready' ? (
                    <button type="button" className="home-rec" onClick={() => navigate('recordings')}>
                      {body(r)}
                    </button>
                  ) : (
                    <div className="home-rec home-rec--failed">{body(r)}</div>
                  )}
                </li>
              ))}
            </ul>
          )
        }
      </AsyncSection>
    </section>
  )
}
