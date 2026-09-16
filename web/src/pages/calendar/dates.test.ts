import { describe, expect, it } from 'vitest'
import { calendarHash, fmtBytes, groupByDay, mondayOf, parseCalendarHash, parseYmd, ymd } from './dates'
import type { Meeting } from '../../api'

describe('rotas da agenda', () => {
  // A Início abre «agendar» e o detalhe por link; se a ida e a volta não
  // coincidem, o botão da Início leva a uma agenda que ignora o pedido.
  it('agendar com dia e hora faz ida e volta', () => {
    const h = `#${calendarHash.schedule('2026-09-14', '10:30')}`
    expect(parseCalendarHash(h)).toEqual({ kind: 'schedule', date: '2026-09-14', time: '10:30' })
  })
  it('agendar sem nada, detalhe e ver', () => {
    expect(parseCalendarHash(`#${calendarHash.schedule()}`)).toEqual({ kind: 'schedule', date: null, time: null })
    expect(parseCalendarHash(`#${calendarHash.meeting('3f2a-99')}`)).toEqual({ kind: 'meeting', id: '3f2a-99' })
    expect(parseCalendarHash('#/calendar')).toEqual({ kind: 'browse' })
    expect(parseCalendarHash('#/calendar/qualquer')).toEqual({ kind: 'browse' })
  })
})

describe('datas', () => {
  it('a semana começa à segunda, também a partir de um domingo', () => {
    expect(ymd(mondayOf(new Date(2026, 8, 20)))).toBe('2026-09-14')
    expect(ymd(mondayOf(new Date(2026, 8, 14)))).toBe('2026-09-14')
  })
  it('AAAA-MM-DD é meia-noite LOCAL, não UTC', () => {
    const d = parseYmd('2026-09-14')!
    expect([d.getFullYear(), d.getMonth(), d.getDate(), d.getHours()]).toEqual([2026, 8, 14, 0])
    expect(parseYmd('14/09/2026')).toBeNull()
  })
  it('agrupa por dia e ordena por hora', () => {
    const m = (id: string, iso: string) => ({ id, starts_at: iso, duration_min: 30 }) as Meeting
    const a = new Date(2026, 8, 14, 15).toISOString()
    const b = new Date(2026, 8, 14, 9).toISOString()
    const g = groupByDay([m('a', a), m('b', b)])
    expect(g.get('2026-09-14')?.map((x) => x.id)).toEqual(['b', 'a'])
  })
  it('bytes em unidade legível', () => {
    expect(fmtBytes(1024 ** 3 * 1.84, 'pt-PT')).toEqual({ value: '1,84', unit: 'GB' })
    expect(fmtBytes(0, 'en-GB')).toEqual({ value: '0', unit: 'B' })
  })
})
