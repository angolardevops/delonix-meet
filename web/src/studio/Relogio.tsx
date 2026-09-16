/**
 * Relógio de parede do topo («WAT 16:07»): a hora local e o fuso abreviado
 * que o próprio browser devolve. Tica a cada 15 s numa folha própria, para a
 * página não se redesenhar por causa de um minuto.
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

export function horaComFuso(d: Date, locale: string): string {
  const partes = new Intl.DateTimeFormat(locale, { hour: '2-digit', minute: '2-digit', hour12: false, timeZoneName: 'short' }).formatToParts(d)
  const fuso = partes.find((p) => p.type === 'timeZoneName')?.value ?? ''
  const hora = partes.find((p) => p.type === 'hour')?.value ?? ''
  const minuto = partes.find((p) => p.type === 'minute')?.value ?? ''
  return `${fuso} ${hora}:${minuto}`.trim()
}

export default function Relogio({ className }: { className?: string }) {
  const { t, i18n } = useTranslation()
  const [agora, setAgora] = useState(() => new Date())
  useEffect(() => {
    const id = setInterval(() => setAgora(new Date()), 15_000)
    return () => clearInterval(id)
  }, [])
  return (
    <time className={className} dateTime={agora.toISOString()} aria-label={t('studio.topo.relogio')}>
      {horaComFuso(agora, i18n.language)}
    </time>
  )
}
