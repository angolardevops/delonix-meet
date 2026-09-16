/**
 * Lista: de hoje em diante (incluindo as que já acabaram hoje), por dia. É a
 * vista por omissão num ecrã estreito — uma grelha de sete colunas não cabe
 * em 375 px.
 */
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import type { Meeting } from '../../api'
import { Icon } from '../../ui/icons'
import { Button, Empty, IconButton } from '../../ui/kit'
import { InviteStatus } from './Respond'
import { fmtTime, localeOf, meetingEnd, meetingStart, startOfDay, ymd } from './dates'

export default function ListView({
  meetings,
  onOpen,
  onEnter,
  onDelete,
  entering,
  onSchedule,
}: {
  meetings: Meeting[]
  onOpen: (m: Meeting) => void
  onEnter: (m: Meeting) => void
  onDelete: (m: Meeting) => void
  entering: string | null
  onSchedule: () => void
}) {
  const { t, i18n } = useTranslation()
  const locale = localeOf(i18n.language)
  const groups = useMemo(() => {
    const from = ymd(startOfDay(new Date()))
    const map = new Map<string, Meeting[]>()
    for (const m of [...meetings].sort((a, b) => a.starts_at.localeCompare(b.starts_at))) {
      const k = ymd(meetingStart(m))
      if (k < from) continue
      const list = map.get(k)
      if (list) list.push(m)
      else map.set(k, [m])
    }
    return [...map.entries()]
  }, [meetings])

  if (groups.length === 0) {
    return (
      <div className="cal-panel">
        <Empty
          icon="calendar"
          title={t('schedule.lista.vazio')}
          action={
            <Button size="sm" icon="plus" onClick={onSchedule}>
              {t('schedule.accoes.agendar')}
            </Button>
          }
        />
      </div>
    )
  }

  return (
    <div className="cal-list">
      {groups.map(([day, list]) => (
        <section key={day} className="cal-list__day">
          <h2>{meetingStart(list[0]).toLocaleDateString(locale, { weekday: 'long', day: 'numeric', month: 'long' })}</h2>
          <ul role="list">
            {list.map((m) => (
              <li key={m.id} className="cal-list__item">
                <span className="cal-list__time dx-num">
                  {fmtTime(meetingStart(m), locale)}
                  <small>{fmtTime(meetingEnd(m), locale)}</small>
                </span>
                <button type="button" className="cal-list__main" onClick={() => onOpen(m)}>
                  <strong>
                    <Icon name={m.kind === 'voice' ? 'phone' : 'video'} size={12} />
                    {m.title}
                    {m.recurrence_freq && <Icon name="repeat" size={11} />}
                  </strong>
                  <small>
                    {t('schedule.evento.duracao', { n: m.duration_min })}
                    {' · '}
                    {m.is_owner ? t('schedule.evento.organizasTu') : t('schedule.evento.organizadaPor', { nome: m.owner_name })}
                    {m.room_name && ` · ${m.room_name}`}
                  </small>
                </button>
                {!m.is_owner && <InviteStatus status={m.my_status} />}
                <span className="cal-list__actions">
                  <Button size="sm" variant="outline" busy={entering === m.id} onClick={() => onEnter(m)}>
                    {m.is_owner ? t('schedule.evento.iniciar') : t('schedule.evento.entrar')}
                  </Button>
                  {m.is_owner && <IconButton icon="trash" label={t('schedule.evento.eliminar')} onClick={() => onDelete(m)} />}
                </span>
              </li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  )
}
