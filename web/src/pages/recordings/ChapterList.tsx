/**
 * Lista de capítulos (ou de cenas por tempo) nas duas formas do template:
 * `rows` — tempo + título, no painel da biblioteca; `thumbs` — miniatura +
 * título + tempo, no leitor. Clicar salta o vídeo; o capítulo em curso fica
 * realçado. Não sabe de onde vêm os capítulos: recebe `ChapterView`.
 */
import { useTranslation } from 'react-i18next'
import { cx } from '../../ui/kit'
import { thumbBackground } from './format'
import { formatClock } from './libraryData'
import type { ChapterView } from './recordingView'

export default function ChapterList({
  chapters,
  active,
  onSeek,
  variant,
  frames,
}: {
  chapters: ChapterView[]
  active: number
  onSeek: (ms: number) => void
  variant: 'rows' | 'thumbs'
  frames?: (string | null)[]
}) {
  const { t } = useTranslation()
  return (
    <ol className={cx('rec-chapters', `rec-chapters--${variant}`)}>
      {chapters.map((c, i) => (
        <li key={c.id}>
          <button
            type="button"
            className={cx('rec-chapter', i === active && 'is-active')}
            aria-current={i === active ? 'true' : undefined}
            aria-label={t('recordings.capitulos.saltar', { title: c.title, time: formatClock(c.tMs) })}
            onClick={() => onSeek(c.tMs)}
          >
            {variant === 'thumbs' ? (
              <>
                <span className="rec-chapter__img" style={frames?.[i] ? undefined : { background: thumbBackground(`${c.id}`) }} aria-hidden="true">
                  {frames?.[i] && <img src={frames[i]!} alt="" />}
                </span>
                <span className="rec-chapter__text">
                  <span className="rec-chapter__title">{c.title}</span>
                  <span className="rec-chapter__time dx-num">{formatClock(c.tMs)}</span>
                </span>
              </>
            ) : (
              <>
                <span className="rec-chapter__time dx-num">{formatClock(c.tMs)}</span>
                <span className="rec-chapter__title">{c.title}</span>
              </>
            )}
          </button>
        </li>
      ))}
    </ol>
  )
}
