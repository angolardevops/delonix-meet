/**
 * Datas da agenda e da Início. A semana começa à segunda; tudo em hora local
 * do browser. Os nomes de dias e meses vêm do `Intl` no idioma escolhido —
 * nunca de uma lista escrita à mão.
 */
import type { Meeting } from '../../api'

export const startOfDay = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate())
export const addDays = (d: Date, n: number) => {
  const x = new Date(d)
  x.setDate(x.getDate() + n)
  return x
}
export const sameDay = (a: Date, b: Date) => a.toDateString() === b.toDateString()
export const mondayOf = (d: Date) => {
  const x = startOfDay(d)
  return addDays(x, -((x.getDay() + 6) % 7))
}
export const pad2 = (n: number) => String(n).padStart(2, '0')
export const ymd = (d: Date) => `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`
export const hhmm = (d: Date) => `${pad2(d.getHours())}:${pad2(d.getMinutes())}`

/** `YYYY-MM-DD` → data local à meia-noite (o `new Date('2026-09-14')` seria UTC). */
export function parseYmd(s: string): Date | null {
  const m = s.match(/^(\d{4})-(\d{2})-(\d{2})$/)
  return m ? new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3])) : null
}

export function localeOf(lang: string): string {
  if (lang.startsWith('en')) return 'en-GB'
  if (lang.startsWith('fr')) return 'fr-FR'
  return 'pt-PT'
}

/**
 * «14 Set» — dia e mês abreviado, como no template. O `toLocaleDateString`
 * com `month: 'short'` dá «14/09» em pt-PT; aqui o mês vem por extenso
 * abreviado, sem o ponto final e com maiúscula.
 */
export function fmtDayMonth(d: Date, locale: string): string {
  const month = new Intl.DateTimeFormat(locale, { month: 'short' }).format(d).replace(/\.$/, '')
  return [d.getDate(), month.charAt(0).toLocaleUpperCase(locale) + month.slice(1)].join(' ')
}

export const meetingStart = (m: Meeting) => new Date(m.starts_at)
export const meetingEnd = (m: Meeting) => new Date(new Date(m.starts_at).getTime() + m.duration_min * 60_000)

export function fmtTime(d: Date, locale: string) {
  return d.toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' })
}

/** Abreviatura do fuso do browser («WAT», «GMT+1»), ou vazio se o Intl não a der. */
export function tzShort(locale: string, d = new Date()): string {
  try {
    return new Intl.DateTimeFormat(locale, { timeZoneName: 'short' }).formatToParts(d).find((p) => p.type === 'timeZoneName')?.value ?? ''
  } catch {
    return ''
  }
}

/** Nomes curtos dos dias, segunda primeiro. */
export function weekdayNames(locale: string, style: 'short' | 'narrow' = 'short'): string[] {
  const monday = mondayOf(new Date(2026, 0, 5))
  return Array.from({ length: 7 }, (_, i) => addDays(monday, i).toLocaleDateString(locale, { weekday: style }))
}

/** Reuniões agrupadas por dia (`YYYY-MM-DD`), cada lista por hora. */
export function groupByDay(meetings: Meeting[]): Map<string, Meeting[]> {
  const map = new Map<string, Meeting[]>()
  for (const m of meetings) {
    const k = ymd(meetingStart(m))
    const list = map.get(k)
    if (list) list.push(m)
    else map.set(k, [m])
  }
  for (const list of map.values()) list.sort((a, b) => a.starts_at.localeCompare(b.starts_at))
  return map
}

/** Rotas internas da agenda (o `App` só olha para `#/calendar…`). */
export const calendarHash = {
  browse: () => '/calendar',
  schedule: (date?: string, time?: string) =>
    `/calendar/new${date ? `?d=${date}${time ? `&t=${time.replace(':', '')}` : ''}` : ''}`,
  meeting: (id: string) => `/calendar/m/${id}`,
}

export type CalendarRoute =
  | { kind: 'browse' }
  | { kind: 'schedule'; date: string | null; time: string | null }
  | { kind: 'meeting'; id: string }

export function parseCalendarHash(hash: string): CalendarRoute {
  const s = hash.match(/^#\/calendar\/new(?:\?d=(\d{4}-\d{2}-\d{2})(?:&t=(\d{4}))?)?$/)
  if (s) return { kind: 'schedule', date: s[1] ?? null, time: s[2] ? `${s[2].slice(0, 2)}:${s[2].slice(2)}` : null }
  const m = hash.match(/^#\/calendar\/m\/([A-Za-z0-9-]+)$/)
  if (m) return { kind: 'meeting', id: m[1] }
  return { kind: 'browse' }
}

export function fmtBytes(b: number, locale: string): { value: string; unit: string } {
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let v = b
  let i = 0
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i++
  }
  return { value: v.toLocaleString(locale, { maximumFractionDigits: v < 10 && i > 0 ? 2 : 1 }), unit: units[i] }
}
