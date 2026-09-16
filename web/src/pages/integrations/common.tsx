/**
 * Peças partilhadas pelos cartões de Integrações: cabeçalho com marca,
 * leitura que distingue «sem permissão» (403) de «falhou», o segredo que só
 * se mostra uma vez, e o formato de data no idioma activo.
 */
import { ReactNode, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError } from '../../api'
import { Icon, IconName } from '../../ui/icons'
import { IconButton } from '../../ui/kit'

/** Resultado de uma leitura que o servidor pode recusar por autorização. */
export type Guarded<T> = { forbidden: true } | { forbidden: false; d: T }

/**
 * O 403 não é uma avaria: é o servidor a dizer quem pode. Converte-o num
 * estado próprio para o cartão explicar porquê; tudo o resto continua a ser
 * erro (e passa pelo `useAsync`).
 */
export function guarded<T>(p: Promise<T>): Promise<Guarded<T>> {
  return p.then(
    (d) => ({ forbidden: false as const, d }),
    (e) => {
      if (e instanceof ApiError && e.status === 403) return { forbidden: true as const }
      throw e
    },
  )
}

export function isForbidden(e: unknown): boolean {
  return e instanceof ApiError && e.status === 403
}

export function localeTag(lang: string): string {
  return lang.startsWith('en') ? 'en-GB' : lang.startsWith('fr') ? 'fr-FR' : 'pt-PT'
}

export function useDateFmt() {
  const { i18n } = useTranslation()
  const tag = localeTag(i18n.language)
  return (iso: string) =>
    new Date(iso).toLocaleString(tag, { day: 'numeric', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit' })
}

/** Cabeçalho de cartão de integração: quadrado com a marca, título, sub, estado. */
export function IntegHead({
  mark,
  icon,
  title,
  sub,
  badge,
}: {
  mark?: string
  icon?: IconName
  title: ReactNode
  sub?: ReactNode
  badge?: ReactNode
}) {
  return (
    <header className="integ-head">
      <span className="integ-mark" aria-hidden="true">
        {icon ? <Icon name={icon} /> : mark}
      </span>
      <div className="integ-head__text">
        <h2 className="integ-head__title">{title}</h2>
        {sub && <div className="integ-head__sub">{sub}</div>}
      </div>
      {badge}
    </header>
  )
}

/** Um segredo acabado de gerar: mostra-se aqui e mais nenhuma vez. */
export function SecretOnce({ value, note }: { value: string; note: ReactNode }) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)
  const timer = useRef<number | undefined>(undefined)
  useEffect(() => () => window.clearTimeout(timer.current), [])
  function copy() {
    void navigator.clipboard?.writeText(value).then(() => {
      setCopied(true)
      window.clearTimeout(timer.current)
      timer.current = window.setTimeout(() => setCopied(false), 1600)
    })
  }
  return (
    <div className="integ-secret" role="status">
      <div className="integ-secret__row">
        <code className="integ-secret__value dx-num">{value}</code>
        <IconButton icon={copied ? 'check' : 'copy'} label={copied ? t('ui.copiado') : t('ui.copiar')} onClick={copy} />
      </div>
      <div className="integ-secret__note">
        <Icon name="alert" size={12} />
        <span>{note}</span>
      </div>
    </div>
  )
}

/** Diálogo de confirmação de uma acção destrutiva. */
export { ConfirmDialog } from './ConfirmDialog'
