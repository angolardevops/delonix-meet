/**
 * Transcrição com instantes RELATIVOS ao vídeo (segmentos): cada linha salta
 * o leitor, e a que está a ser dita fica realçada. Só aparece quando a camada
 * de mapeamento devolve segmentos; sem eles fica a das notas da sala
 * (`RecordingNotes`), que não salta.
 */
import { useTranslation } from 'react-i18next'
import { cx } from '../../ui/kit'
import { formatClock, segmentAt, splitSpeaker } from './libraryData'
import type { SegmentView } from './recordingView'

export default function Transcript({ segments, nowMs, onSeek }: { segments: SegmentView[]; nowMs: number; onSeek: (ms: number) => void }) {
  const { t } = useTranslation()
  const active = segmentAt(segments, nowMs)
  if (segments.length === 0) return <p className="rec-notes__empty">{t('recordings.notas.semTranscricao')}</p>
  return (
    <ol className="rec-transcript rec-transcript--box">
      {segments.map((s, i) => {
        const { speaker, text } = splitSpeaker(s.text)
        return (
          <li key={`${s.startMs}-${i}`}>
            <button
              type="button"
              className={cx('rec-transcript__line', 'rec-transcript__seek', i === active && 'is-active')}
              aria-label={t('recordings.transcricao.saltar', { time: formatClock(s.startMs) })}
              onClick={() => onSeek(s.startMs)}
            >
              <span className="rec-transcript__time dx-num">{formatClock(s.startMs)}</span>
              <span>
                {speaker && <strong>{speaker}:</strong>} {text}
              </span>
            </button>
          </li>
        )
      })}
    </ol>
  )
}
