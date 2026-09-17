/** Vista de mês (6 × 7) e vista de ano (12 meses em miniatura). */
import { useTranslation } from 'react-i18next'
import type { Meeting } from '../../api'
import { cx } from '../../ui/kit'
import EventChip from './EventChip'
import { addDays, localeOf, mondayOf, sameDay, weekdayNames, ymd } from './dates'

const MAX_CHIPS = 3

export function MonthView({
  cursor,
  byDay,
  onOpen,
  onNewOnDay,
  onShowDay,
}: {
  cursor: Date
  byDay: Map<string, Meeting[]>
  onOpen: (m: Meeting) => void
  onNewOnDay: (d: Date) => void
  onShowDay: (d: Date) => void
}) {
  const { t, i18n } = useTranslation()
  const locale = localeOf(i18n.language)
  const start = mondayOf(new Date(cursor.getFullYear(), cursor.getMonth(), 1))
  const days = Array.from({ length: 42 }, (_, i) => addDays(start, i))
  const today = new Date()
  return (
    <div className="cal-month" role="grid" aria-label={cursor.toLocaleDateString(locale, { month: 'long', year: 'numeric' })}>
      <div className="cal-month__head" role="row">
        {weekdayNames(locale).map((w) => (
          <span key={w} role="columnheader" aria-label={w}>
            <span className="cal-month__wd-long">{w}</span>
            <span className="cal-month__wd-short" aria-hidden="true">
              {w.slice(0, 3)}
            </span>
          </span>
        ))}
      </div>
      <div className="cal-month__grid">
        {days.map((d) => {
          const events = byDay.get(ymd(d)) ?? []
          const isToday = sameDay(d, today)
          const out = d.getMonth() !== cursor.getMonth()
          return (
            <div key={ymd(d)} role="gridcell" className={cx('cal-month__cell', out && 'is-out', isToday && 'is-today')}>
              <button
                type="button"
                className="cal-month__num dx-num"
                onClick={() => onNewOnDay(d)}
                aria-label={t('schedule.vista.agendarNoDia', {
                  dia: d.toLocaleDateString(locale, { weekday: 'long', day: 'numeric', month: 'long' }),
                })}
                aria-current={isToday ? 'date' : undefined}
              >
                {d.getDate()}
              </button>
              <div className="cal-month__events">
                {events.slice(0, MAX_CHIPS).map((m) => (
                  <EventChip key={m.id} meeting={m} onOpen={onOpen} variant="chip" />
                ))}
                {events.length > MAX_CHIPS && (
                  <button type="button" className="cal-month__more" onClick={() => onShowDay(d)}>
                    {t('schedule.vista.mais', { count: events.length - MAX_CHIPS })}
                  </button>
                )}
              </div>
              {events.length > 0 && (
                <button
                  type="button"
                  className="cal-month__dots"
                  onClick={() => onShowDay(d)}
                  aria-label={t('schedule.vista.reunioesNoDia', { count: events.length })}
                >
                  {events.slice(0, 3).map((m) => (
                    <span key={m.id} aria-hidden="true" />
                  ))}
                </button>
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}

export function YearView({
  year,
  byDay,
  onMonth,
  onShowDay,
}: {
  year: number
  byDay: Map<string, Meeting[]>
  onMonth: (d: Date) => void
  onShowDay: (d: Date) => void
}) {
  const { t, i18n } = useTranslation()
  const locale = localeOf(i18n.language)
  const narrow = weekdayNames(locale, 'narrow')
  const today = new Date()
  return (
    <div className="cal-year">
      {Array.from({ length: 12 }, (_, mi) => {
        const first = new Date(year, mi, 1)
        const start = mondayOf(first)
        const days = Array.from({ length: 42 }, (_, i) => addDays(start, i))
        return (
          <section key={mi} className="cal-year__month">
            <button type="button" className="cal-year__name" onClick={() => onMonth(first)}>
              {first.toLocaleDateString(locale, { month: 'long' })}
            </button>
            <div className="cal-year__grid">
              {narrow.map((w, i) => (
                <span key={i} className="cal-year__wd" aria-hidden="true">
                  {w}
                </span>
              ))}
              {days.map((d) => {
                if (d.getMonth() !== mi) return <span key={ymd(d)} />
                const n = byDay.get(ymd(d))?.length ?? 0
                const cls = cx('cal-year__day dx-num', n > 0 && 'has-events', sameDay(d, today) && 'is-today')
                return n > 0 ? (
                  <button
                    key={ymd(d)}
                    type="button"
                    className={cls}
                    onClick={() => onShowDay(d)}
                    aria-label={`${d.toLocaleDateString(locale, { day: 'numeric', month: 'long' })}: ${t('schedule.vista.reunioesNoDia', { count: n })}`}
                  >
                    {d.getDate()}
                  </button>
                ) : (
                  <span key={ymd(d)} className={cls}>
                    {d.getDate()}
                  </span>
                )
              })}
            </div>
          </section>
        )
      })}
    </div>
  )
}
