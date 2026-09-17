/**
 * Peças partilhadas por Contactos e Administração: a organização escolhida,
 * a leitura de uma recusa do servidor e a formatação de datas no idioma.
 */
import { useEffect, useMemo, useState } from 'react'
import type { TFunction } from 'i18next'
import { useTranslation } from 'react-i18next'
import { intlLocale } from '../../i18n'
import { ApiError, apiErrorMessage, myOrgs, OrgSummary } from '../../api'
import { useAsync } from '../../components/AsyncSection'
import { useShell } from '../../components/shellContext'

/**
 * O servidor recusou por permissão. Os handlers de org devolvem 401 a um
 * membro que não é admin (`require_admin` → `Unauthorized`) e 404 a quem não
 * é membro; o `request` renova a sessão e volta a tentar, e só depois disso o
 * erro chega aqui — por isso um 401 que chega a este ponto é recusa, não sessão.
 */
export function isRefused(e: unknown): boolean {
  return e instanceof ApiError && (e.status === 401 || e.status === 403)
}

export function orgErrorMessage(e: unknown, t: TFunction, fallbackKey: string): string {
  if (isRefused(e)) return t('org.erro.recusado')
  return apiErrorMessage(e, t(fallbackKey))
}

/** Converte a recusa num erro legível antes de chegar ao `useAsync`. */
export function refusalAware<T>(p: Promise<T>, t: TFunction): Promise<T> {
  return p.catch((e) => {
    if (isRefused(e)) throw new Error(t('org.erro.recusado'))
    throw e
  })
}

export function useLocaleTag(): string {
  const { i18n } = useTranslation()
  return intlLocale(i18n.language)
}

/**
 * Lista de organizações recarregável (o `useShell().orgs` não se recarrega
 * depois de criar uma organização ou mudar as definições) e a escolhida, que
 * começa na organização activa do Shell.
 */
export function useOrgSelection() {
  const shell = useShell()
  const { t } = useTranslation()
  const list = useAsync((signal) => refusalAware(myOrgs(signal), t), [])
  const [orgId, setOrgId] = useState<string | null>(shell.org?.id ?? null)
  const orgs: OrgSummary[] = list.state.s === 'ready' ? list.state.d : []
  const org = useMemo(() => orgs.find((o) => o.id === orgId) ?? orgs[0] ?? null, [orgs, orgId])
  useEffect(() => {
    if (!orgId && shell.org) setOrgId(shell.org.id)
  }, [orgId, shell.org])
  return { list, orgs, org, setOrgId }
}

export function formatDateTime(iso: string, locale: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  return d.toLocaleString(locale, { day: '2-digit', month: 'short', hour: '2-digit', minute: '2-digit', second: '2-digit' })
}

export function formatAgo(iso: string | null | undefined, locale: string): string | null {
  if (!iso) return null
  const ms = Date.now() - new Date(iso).getTime()
  if (Number.isNaN(ms)) return null
  const mins = Math.round(ms / 60000)
  const rtf = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' })
  if (mins < 60) return rtf.format(-Math.max(mins, 0), 'minute')
  if (mins < 60 * 24) return rtf.format(-Math.round(mins / 60), 'hour')
  if (mins < 60 * 24 * 30) return rtf.format(-Math.round(mins / 1440), 'day')
  return new Date(iso).toLocaleDateString(locale, { day: 'numeric', month: 'short', year: 'numeric' })
}

export function formatBytes(b: number, locale: string): string {
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let v = b
  let i = 0
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i++
  }
  return `${v.toLocaleString(locale, { maximumFractionDigits: i >= 3 ? 2 : 0 })} ${units[i]}`
}
