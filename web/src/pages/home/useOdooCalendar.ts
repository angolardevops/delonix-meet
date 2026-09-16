/**
 * A agenda está mesmo ligada ao calendário do Odoo?
 *
 * O template escreve «Calendário Odoo» em todo o lado. Aqui só se diz quando é
 * verdade, e é verdade quando as duas metades existem:
 *  - a integração Odoo da organização está activa (o Odoo cria e actualiza
 *    reuniões pela API v1 — meetings_v1.rs);
 *  - há um webhook activo, subscrito a «meeting.created», que aponta para o
 *    MESMO host do Odoo (é assim que uma reunião criada aqui chega lá).
 * As duas leituras são de administrador; para os outros o servidor recusa e a
 * resposta é «não sei», que no ecrã é não dizer nada.
 */
import { getOdooConfig, listWebhooks } from '../../api'
import { useAsync } from '../../components/AsyncSection'
import { useShell } from '../../components/shellContext'

function hostOf(url: string | null | undefined): string | null {
  if (!url) return null
  try {
    return new URL(url).host.toLowerCase()
  } catch {
    return null
  }
}

export function odooCalendarLinked(
  odoo: { odoo_enabled: boolean; odoo_url: string | null },
  hooks: { url: string; events: string; active: boolean }[],
): boolean {
  const host = odoo.odoo_enabled ? hostOf(odoo.odoo_url) : null
  if (!host) return false
  return hooks.some(
    (h) => h.active && hostOf(h.url) === host && h.events.split(',').some((e) => e.trim() === 'meeting.created'),
  )
}

export function useOdooCalendar(): boolean {
  const { org, isAdmin } = useShell()
  const orgId = isAdmin ? org?.id : undefined
  const { state } = useAsync(async () => {
    if (!orgId) return false
    try {
      const [cfg, hooks] = await Promise.all([getOdooConfig(orgId), listWebhooks(orgId)])
      return odooCalendarLinked(cfg, hooks)
    } catch {
      return false
    }
  }, [orgId])
  return state.s === 'ready' && state.d
}
