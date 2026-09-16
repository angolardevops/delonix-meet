import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button, TextInput } from '../ui/kit'
import type { MeetingTools } from './useMeetingTools'
import { QuestionCard } from './QuestionCard'

/** Perguntas e respostas: toda a gente pergunta e vota; o anfitrião modera. */
export function QaPanel({ tools, isHost }: { tools: MeetingTools; isHost: boolean }) {
  const { t } = useTranslation()
  const [text, setText] = useState('')
  // Em palco primeiro; depois as abertas pelos votos; respondidas; ocultas no fim.
  const peso = (q: (typeof tools.questions)[number]) => (q.spotlight ? 0 : q.hidden ? 3 : q.answered ? 2 : 1)
  const ordenadas = [...tools.questions].sort((a, b) => peso(a) - peso(b) || b.upvotes - a.upvotes)
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
      {ordenadas.map((q) => (
        <QuestionCard key={q.id} q={q} tools={tools} isHost={isHost} />
      ))}
    </div>
  )
}
