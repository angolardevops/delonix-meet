import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from '../ui/icons'
import { Avatar, Button, IconButton, Tag, TextInput, cx } from '../ui/kit'
import type { MeetingTools } from './useMeetingTools'

/** Perguntas e respostas: toda a gente pergunta e vota; o anfitrião modera. */
export function QaPanel({ tools, isHost }: { tools: MeetingTools; isHost: boolean }) {
  const { t } = useTranslation()
  const [text, setText] = useState('')
  const pendentes = tools.questions.filter((q) => !q.answered)
  const respondidas = tools.questions.filter((q) => q.answered)
  return (
    <div className="rm-scroll">
      <form
        className="rm-qa__form"
        onSubmit={(e) => {
          e.preventDefault()
          if (!text.trim()) return
          tools.ask(text)
          setText('')
        }}
      >
        <TextInput
          placeholder={t('room.perguntas.placeholder')}
          aria-label={t('room.perguntas.placeholder')}
          maxLength={300}
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
        <Button type="submit" variant="primary" size="sm" disabled={!text.trim()} icon="send">
          {t('room.perguntas.enviar')}
        </Button>
      </form>
      {tools.questions.length === 0 && (
        <div className="rm-panel__empty">
          <strong>{t('room.perguntas.vazio')}</strong>
          <span className="dx-muted">{t('room.perguntas.vazioTexto')}</span>
        </div>
      )}
      {[...pendentes, ...respondidas].map((q) => (
        <article key={q.id} className={cx('rm-qa', q.answered && 'is-answered')}>
          <div className="rm-qa__head">
            <Avatar name={q.by} size={22} />
            <strong>{q.by}</strong>
            {q.answered && <Tag tone="success">{t('room.perguntas.respondida')}</Tag>}
          </div>
          <p className="rm-qa__text">{q.text}</p>
          <div className="rm-qa__actions">
            <button
              type="button"
              className={cx('rm-vote', tools.myUpvotes[q.id] && 'is-on')}
              aria-pressed={!!tools.myUpvotes[q.id]}
              aria-label={t('room.perguntas.votar', { count: q.upvotes })}
              onClick={() => tools.upvote(q.id)}
            >
              <Icon name="thumbUp" size={12} />
              <span className="dx-num">{q.upvotes}</span>
            </button>
            {isHost && (
              <IconButton
                icon={q.answered ? 'undo' : 'check'}
                label={q.answered ? t('room.perguntas.reabrir') : t('room.perguntas.marcarRespondida')}
                onClick={() => tools.markAnswered(q.id)}
              />
            )}
          </div>
        </article>
      ))}
    </div>
  )
}
