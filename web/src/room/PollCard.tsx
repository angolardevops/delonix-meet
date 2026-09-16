import { useTranslation } from 'react-i18next'
import type { PollView } from '../signaling'
import { Icon } from '../ui/icons'
import { cx } from '../ui/kit'
import { SecondsLeft } from './Clocks'

/**
 * Uma sondagem (ou quiz) com os números do servidor. Serve o cartão que
 * aparece a todos e a lista do painel.
 */
export function PollCard({
  poll,
  myVote,
  isHost,
  onVote,
  onClose,
  compact,
}: {
  poll: PollView
  myVote: number | undefined
  isHost: boolean
  onVote: (option: number) => void
  onClose?: () => void
  compact?: boolean
}) {
  const { t } = useTranslation()
  const total = poll.counts.reduce((a, b) => a + b, 0)
  const revealed = !poll.open && poll.correct != null
  const quiz = poll.correct != null || revealed
  return (
    <div className={cx('rm-poll', compact && 'is-compact')}>
      <div className="rm-poll__head">
        <span className={cx('rm-poll__kind', poll.open && 'is-open')}>
          <Icon name={quiz ? 'trophy' : 'poll'} size={11} />
          {quiz ? t('room.sondagens.quiz') : t('room.sondagens.sondagem')}
          {' · '}
          {poll.open ? t('room.sondagens.aberta') : t('room.sondagens.encerrada')}
        </span>
        <span className="dx-spacer" />
        {poll.open && poll.ends_at != null && (
          <SecondsLeft
            endsAtMs={poll.ends_at}
            render={(s) => (
              <span className="dx-num rm-poll__left">
                <Icon name="clock" size={11} />
                {t('room.sondagens.segundos', { n: s })}
              </span>
            )}
          />
        )}
        <span className="dx-num dx-muted">{t('room.sondagens.votos', { count: total })}</span>
      </div>
      <strong className="rm-poll__q">{poll.question}</strong>
      <span className="dx-muted rm-poll__by">{t('room.sondagens.por', { nome: poll.by })}</span>
      <div className="rm-poll__opts">
        {poll.options.map((opt, i) => {
          const pct = total ? Math.round((poll.counts[i] / total) * 100) : 0
          const mine = myVote === i
          const certa = revealed && i === poll.correct
          const errada = revealed && mine && i !== poll.correct
          return (
            <button
              key={i}
              type="button"
              className={cx('rm-poll__opt', mine && 'is-mine', certa && 'is-right', errada && 'is-wrong')}
              disabled={!poll.open || (poll.ends_at != null && Date.now() > poll.ends_at)}
              aria-pressed={mine}
              onClick={() => onVote(i)}
            >
              <span className="rm-poll__row">
                <span className="rm-poll__label">
                  {certa ? <Icon name="check" size={11} /> : mine ? <Icon name="record" size={11} /> : null}
                  {opt}
                </span>
                <span className="dx-num">{pct}%</span>
              </span>
              <span className="rm-poll__bar" aria-hidden="true">
                <span style={{ width: `${pct}%` }} />
              </span>
            </button>
          )
        })}
      </div>
      {revealed && (
        <p className="rm-poll__totals dx-num">
          <span className="is-right">
            <Icon name="check" size={11} />
            {t('room.sondagens.certas', { count: poll.total_right })}
          </span>
          <span className="is-wrong">
            <Icon name="x" size={11} />
            {t('room.sondagens.erradas', { count: poll.total_wrong })}
          </span>
        </p>
      )}
      {isHost && poll.open && onClose && (
        <button type="button" className="rm-link" onClick={onClose}>
          {t('room.sondagens.encerrar')}
        </button>
      )}
    </div>
  )
}
