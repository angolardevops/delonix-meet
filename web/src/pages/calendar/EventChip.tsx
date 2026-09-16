/**
 * Uma reunião dentro de uma grelha (mês, semana, dia). O resumo que a agenda
 * antiga mostrava num popover ao passar o rato vai no nome acessível e no
 * `title`: chega ao rato, ao teclado e ao leitor de ecrã da mesma maneira.
 */
import { CSSProperties } from 'react'
import { useTranslation } from 'react-i18next'
import type { Meeting } from '../../api'
import { Icon } from '../../ui/icons'
import { cx } from '../../ui/kit'
import { fmtTime, localeOf, meetingEnd, meetingStart } from './dates'

export function useMeetingSummary() {
  const { t, i18n } = useTranslation()
  const locale = localeOf(i18n.language)
  return (m: Meeting) =>
    t('schedule.evento.resumo', {
      titulo: m.title,
      dia: meetingStart(m).toLocaleDateString(locale, { weekday: 'short', day: 'numeric', month: 'short' }),
      inicio: fmtTime(meetingStart(m), locale),
      fim: fmtTime(meetingEnd(m), locale),
      quem: m.is_owner ? t('schedule.evento.organizasTu') : t('schedule.evento.organizadaPor', { nome: m.owner_name }),
    })
}

export default function EventChip({
  meeting: m,
  onOpen,
  variant,
  style,
  showTime = true,
}: {
  meeting: Meeting
  onOpen?: (m: Meeting) => void
  variant: 'chip' | 'block'
  style?: CSSProperties
  showTime?: boolean
}) {
  const { i18n } = useTranslation()
  const summary = useMeetingSummary()
  const locale = localeOf(i18n.language)
  const label = summary(m)
  const className = cx(
    'cal-ev',
    `cal-ev--${variant}`,
    m.kind === 'voice' && 'cal-ev--voice',
    m.my_status === 'pending' && 'cal-ev--pending',
  )
  const inner = (
    <>
      <span className="cal-ev__title">
        {m.recurrence_freq && <Icon name="repeat" size={10} />}
        {m.title}
      </span>
      {showTime && <span className="cal-ev__time dx-num">{fmtTime(meetingStart(m), locale)}</span>}
    </>
  )
  // Sem acção (a semana ao lado do formulário): só leitura, não um botão inerte.
  if (!onOpen) {
    return (
      <div className={className} style={style} title={label}>
        {inner}
      </div>
    )
  }
  return (
    <button
      type="button"
      className={className}
      style={style}
      title={label}
      aria-label={label}
      onClick={(e) => {
        e.stopPropagation()
        onOpen(m)
      }}
    >
      {inner}
    </button>
  )
}
