/**
 * Linha do tempo de um ou mais dias (vista de dia, de semana, e a semana ao
 * lado do formulário de agendamento). Reuniões sobrepostas partilham a
 * largura da coluna em faixas, em vez de se taparem.
 */
import { useEffect, useMemo, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import type { Meeting } from '../../api'
import { cx } from '../../ui/kit'
import EventChip from './EventChip'
import { fmtTime, localeOf, meetingEnd, meetingStart, pad2, sameDay, ymd } from './dates'

const HOURS = Array.from({ length: 24 }, (_, i) => i)

export interface Draft {
  start: Date
  durationMin: number
  label: string
}

interface Placed {
  m: Meeting
  top: number
  height: number
  lane: number
  lanes: number
}

/** Faixas: cada grupo de reuniões que se tocam divide a largura entre si. */
function layoutDay(list: Meeting[], hourH: number): Placed[] {
  const sorted = [...list].sort((a, b) => a.starts_at.localeCompare(b.starts_at))
  const out: Placed[] = []
  let cluster: Placed[] = []
  let clusterEnd = -Infinity
  let laneEnds: number[] = []
  const flush = () => {
    const lanes = Math.max(1, laneEnds.length)
    for (const p of cluster) p.lanes = lanes
    out.push(...cluster)
    cluster = []
    laneEnds = []
  }
  for (const m of sorted) {
    const s = meetingStart(m).getTime()
    const e = meetingEnd(m).getTime()
    if (s >= clusterEnd) {
      flush()
      clusterEnd = -Infinity
    }
    let lane = laneEnds.findIndex((end) => end <= s)
    if (lane < 0) {
      lane = laneEnds.length
      laneEnds.push(e)
    } else laneEnds[lane] = e
    clusterEnd = Math.max(clusterEnd, e)
    const d = meetingStart(m)
    cluster.push({
      m,
      top: ((d.getHours() * 60 + d.getMinutes()) * hourH) / 60,
      height: Math.max((m.duration_min * hourH) / 60, 20),
      lane,
      lanes: 1,
    })
  }
  flush()
  return out
}

export default function WeekView({
  days,
  byDay,
  onOpen,
  onSlot,
  onDay,
  draft,
  compact,
  scrollToHour = 7,
}: {
  days: Date[]
  byDay: Map<string, Meeting[]>
  onOpen?: (m: Meeting) => void
  onSlot?: (date: Date) => void
  onDay?: (date: Date) => void
  draft?: Draft | null
  compact?: boolean
  scrollToHour?: number
}) {
  const { t, i18n } = useTranslation()
  const locale = localeOf(i18n.language)
  const hourH = compact ? 36 : 52
  const scrollRef = useRef<HTMLDivElement>(null)
  const today = new Date()

  const focusHour = draft ? Math.max(0, draft.start.getHours() - 1) : scrollToHour
  useEffect(() => {
    if (scrollRef.current) scrollRef.current.scrollTop = focusHour * hourH
  }, [focusHour, hourH, days.length])

  const placed = useMemo(() => {
    const map = new Map<string, Placed[]>()
    for (const d of days) map.set(ymd(d), layoutDay(byDay.get(ymd(d)) ?? [], hourH))
    return map
  }, [days, byDay, hourH])

  const nowTop = ((today.getHours() * 60 + today.getMinutes()) * hourH) / 60
  const cols = { gridTemplateColumns: `44px repeat(${days.length}, minmax(0, 1fr))` }

  return (
    <div className={cx('cal-week', compact && 'cal-week--compact', days.length > 1 && 'cal-week--multi')}>
      <div className="cal-week__scroller">
        <div className="cal-week__head" style={cols}>
          <span />
          {days.map((d) => {
            const label = d.toLocaleDateString(locale, { weekday: 'short', day: 'numeric' })
            const isToday = sameDay(d, today)
            return onDay ? (
              <button
                key={ymd(d)}
                type="button"
                className={cx('cal-week__day', isToday && 'is-today')}
                onClick={() => onDay(d)}
                aria-label={d.toLocaleDateString(locale, { weekday: 'long', day: 'numeric', month: 'long' })}
                aria-current={isToday ? 'date' : undefined}
              >
                {label}
              </button>
            ) : (
              <span key={ymd(d)} className={cx('cal-week__day', isToday && 'is-today')}>
                {label}
              </span>
            )
          })}
        </div>
        <div className="cal-week__body" ref={scrollRef}>
          <div className="cal-week__grid" style={{ ...cols, height: hourH * 24 }}>
            <div className="cal-week__gutter">
              {HOURS.map((h) => (
                <span key={h} className="dx-num" style={{ top: h * hourH }}>
                  {h > 0 ? `${pad2(h)}:00` : ''}
                </span>
              ))}
            </div>
            {days.map((d) => {
              const key = ymd(d)
              const isToday = sameDay(d, today)
              const draftHere = draft && sameDay(draft.start, d) ? draft : null
              return (
                <div
                  key={key}
                  className={cx('cal-week__col', isToday && 'is-today')}
                  style={{ backgroundSize: `100% ${hourH}px` }}
                  onClick={
                    onSlot
                      ? (e) => {
                          const rect = e.currentTarget.getBoundingClientRect()
                          const min = Math.floor((((e.clientY - rect.top) / hourH) * 60) / 30) * 30
                          const slot = new Date(d)
                          slot.setHours(Math.floor(min / 60), min % 60, 0, 0)
                          onSlot(slot)
                        }
                      : undefined
                  }
                >
                  {isToday && <div className="cal-week__now" style={{ top: nowTop }} aria-hidden="true" />}
                  {(placed.get(key) ?? []).map((p) => (
                    <EventChip
                      key={p.m.id}
                      meeting={p.m}
                      onOpen={onOpen}
                      variant="block"
                      showTime={p.height > 34}
                      style={{
                        top: p.top,
                        height: p.height,
                        left: `calc(${(p.lane / p.lanes) * 100}% + 2px)`,
                        width: `calc(${100 / p.lanes}% - 4px)`,
                      }}
                    />
                  ))}
                  {draftHere && (
                    <div
                      className="cal-draft"
                      style={{
                        top: ((draftHere.start.getHours() * 60 + draftHere.start.getMinutes()) * hourH) / 60,
                        height: Math.max((draftHere.durationMin * hourH) / 60, 20),
                      }}
                    >
                      <strong>{draftHere.label || t('schedule.form.estaSessao')}</strong>
                      <span className="dx-num">{fmtTime(draftHere.start, locale)}</span>
                    </div>
                  )}
                </div>
              )
            })}
          </div>
        </div>
      </div>
    </div>
  )
}
