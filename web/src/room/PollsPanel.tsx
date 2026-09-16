import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from '../ui/icons'
import { Button, Field, IconButton, Select, TextInput, cx } from '../ui/kit'
import { Countdown } from './Clocks'
import { PollCard } from './PollCard'
import type { MeetingTools } from './useMeetingTools'

const TIMER_PRESETS = [5, 10, 15, 30, 60]
const POLL_DURATIONS = [0, 30, 60, 120, 300]

/** Composição de uma sondagem ou quiz (anfitrião). */
function PollComposer({ tools, autoFocus }: { tools: MeetingTools; autoFocus?: boolean }) {
  const { t } = useTranslation()
  const [question, setQuestion] = useState('')
  const [options, setOptions] = useState<string[]>(['', ''])
  const [correct, setCorrect] = useState<number | null>(null)
  const [duration, setDuration] = useState(0)
  const validas = options.filter((o) => o.trim()).length
  return (
    <form
      className="rm-compose"
      onSubmit={(e) => {
        e.preventDefault()
        if (!question.trim() || validas < 2) return
        tools.createPoll({ question, options, correct, durationSecs: duration })
        setQuestion('')
        setOptions(['', ''])
        setCorrect(null)
        setDuration(0)
      }}
    >
      <Field label={t('room.sondagens.pergunta')} htmlFor="rm-poll-q">
        <TextInput id="rm-poll-q" autoFocus={autoFocus} maxLength={200} value={question} onChange={(e) => setQuestion(e.target.value)} />
      </Field>
      <fieldset className="rm-compose__opts">
        <legend className="dx-field__label">{t('room.sondagens.opcoes')}</legend>
        {options.map((o, i) => (
          <div key={i} className="rm-compose__opt">
            <TextInput
              aria-label={t('room.sondagens.opcaoN', { n: i + 1 })}
              placeholder={t('room.sondagens.opcaoN', { n: i + 1 })}
              maxLength={80}
              value={o}
              onChange={(e) => setOptions(options.map((x, j) => (j === i ? e.target.value : x)))}
            />
            <button
              type="button"
              className={cx('rm-compose__right', correct === i && 'is-on')}
              aria-pressed={correct === i}
              aria-label={t('room.sondagens.marcarCerta', { n: i + 1 })}
              title={t('room.sondagens.marcarCerta', { n: i + 1 })}
              onClick={() => setCorrect(correct === i ? null : i)}
            >
              <Icon name="check" size={12} />
            </button>
            {options.length > 2 && (
              <IconButton
                icon="minus"
                bare
                label={t('room.sondagens.removerOpcao', { n: i + 1 })}
                onClick={() => {
                  setOptions(options.filter((_, j) => j !== i))
                  if (correct === i) setCorrect(null)
                  else if (correct != null && correct > i) setCorrect(correct - 1)
                }}
              />
            )}
          </div>
        ))}
      </fieldset>
      <div className="rm-compose__row">
        {options.length < 6 && (
          <Button size="sm" variant="ghost" icon="plus" onClick={() => setOptions([...options, ''])}>
            {t('room.sondagens.adicionarOpcao')}
          </Button>
        )}
        <span className="dx-spacer" />
        <Select aria-label={t('room.sondagens.duracao')} value={duration} onChange={(e) => setDuration(Number(e.target.value))}>
          {POLL_DURATIONS.map((d) => (
            <option key={d} value={d}>
              {d === 0 ? t('room.sondagens.semTempo') : t('room.sondagens.duracaoSegundos', { n: d })}
            </option>
          ))}
        </Select>
      </div>
      <p className="dx-muted rm-compose__hint">{correct != null ? t('room.sondagens.dicaQuiz') : t('room.sondagens.dicaSondagem')}</p>
      <Button type="submit" variant="primary" size="sm" disabled={!question.trim() || validas < 2}>
        {correct != null ? t('room.sondagens.lancarQuiz') : t('room.sondagens.lancar')}
      </Button>
    </form>
  )
}

export function PollsPanel({
  tools,
  isHost,
  present,
  focusComposer,
}: {
  tools: MeetingTools
  isHost: boolean
  /** Quantos estão na sala («11 de 13»). */
  present: number
  /** Veio do atalho «Nova sondagem» do chat: o cursor vai para a pergunta. */
  focusComposer?: boolean
}) {
  const { t } = useTranslation()
  return (
    <div className="rm-scroll">
      <section className="rm-block" aria-labelledby="rm-timer-h">
        <h3 id="rm-timer-h" className="rm-block__title">
          <Icon name="hourglass" size={13} />
          {t('room.temporizador.titulo')}
        </h3>
        {tools.timerEndsAt ? (
          <div className="rm-timer">
            <Countdown endsAt={tools.timerEndsAt} render={(txt) => <strong className="dx-num rm-timer__big">{txt}</strong>} />
            {isHost && (
              <Button size="sm" variant="outline" onClick={tools.clearTimer}>
                {t('room.temporizador.limpar')}
              </Button>
            )}
          </div>
        ) : isHost ? (
          <div className="dx-chips">
            {TIMER_PRESETS.map((m) => (
              <button key={m} type="button" className="dx-chip dx-num" onClick={() => tools.setTimer(m)}>
                {t('room.temporizador.minutos', { n: m })}
              </button>
            ))}
          </div>
        ) : (
          <p className="dx-muted">{t('room.temporizador.soAnfitriao')}</p>
        )}
      </section>

      {isHost && (
        <section className="rm-block" aria-labelledby="rm-poll-new">
          <h3 id="rm-poll-new" className="rm-block__title">
            <Icon name="plus" size={13} />
            {t('room.sondagens.nova')}
          </h3>
          <PollComposer tools={tools} autoFocus={focusComposer} />
        </section>
      )}

      <section className="rm-block" aria-labelledby="rm-poll-list">
        <h3 id="rm-poll-list" className="rm-block__title">
          <Icon name="poll" size={13} />
          {t('room.painel.sondagens')}
        </h3>
        {tools.polls.length === 0 && <p className="dx-muted">{t('room.sondagens.vazio')}</p>}
        {[...tools.polls].reverse().map((p) => (
          <PollCard
            key={p.id}
            poll={p}
            myVote={tools.myVotes[p.id]}
            isHost={isHost}
            onVote={(i) => tools.vote(p.id, i)}
            onClose={() => tools.closePoll(p.id)}
            present={present}
          />
        ))}
      </section>
    </div>
  )
}
