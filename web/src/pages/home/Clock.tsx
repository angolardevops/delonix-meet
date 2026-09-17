/**
 * Saudação e data da barra da Início. Folhas com o seu próprio relógio: o
 * minuto que passa não volta a desenhar a página inteira.
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { fmtTime, localeOf, tzShort } from '../calendar/dates'

function useMinute(): Date {
  const [now, setNow] = useState(() => new Date())
  useEffect(() => {
    const id = window.setInterval(() => setNow(new Date()), 30_000)
    return () => window.clearInterval(id)
  }, [])
  return now
}

export function Greeting({ name }: { name: string }) {
  const { t } = useTranslation()
  const h = useMinute().getHours()
  const key = h < 12 ? 'home.saudacao.manha' : h < 19 ? 'home.saudacao.tarde' : 'home.saudacao.noite'
  return <>{t(key, { nome: name })}</>
}

export function TodayStamp() {
  const { i18n } = useTranslation()
  const now = useMinute()
  const locale = localeOf(i18n.language)
  const day = now.toLocaleDateString(locale, { weekday: 'short', day: 'numeric', month: 'short', year: 'numeric' })
  const tz = tzShort(locale, now)
  return (
    <time dateTime={now.toISOString()}>
      {day} · {fmtTime(now, locale)}
      {tz && ` ${tz}`}
    </time>
  )
}
