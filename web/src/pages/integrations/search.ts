/**
 * Fontes de pesquisa das Integrações. `webhooks` é recurso da fase 2 do
 * contrato: se o servidor o descrever, o painel usa-o; até lá filtra a lista
 * inteira no browser. As chaves de API não estão no contrato.
 */
import type { ApiKeyInfo, Webhook } from '../../api'
import { localSchema } from '../../ui/search/localSchema'

export const webhooksSource = {
  schema: localSchema(
    'webhooks',
    [
      { name: 'kind', type: 'enum', options: ['slack', 'mattermost', 'teams', 'generic'] },
      { name: 'url', type: 'text' },
      { name: 'events', type: 'text' },
      { name: 'active', type: 'bool' },
    ],
    [
      { name: 'active', group: 'state', filter: [['active', 'eq', true]] },
      { name: 'inactive', group: 'state', filter: [['active', 'eq', false]] },
    ],
    { textFields: ['url', 'events'], defaultOrder: [] },
  ),
  get: (h: Webhook, f: string) => (h as unknown as Record<string, unknown>)[f],
  text: (h: Webhook) => `${h.kind} ${h.url} ${h.events}`,
}

export const apiKeysSource = {
  schema: localSchema(
    'api_keys',
    [
      { name: 'name', type: 'text' },
      { name: 'prefix', type: 'text' },
      { name: 'created_at', type: 'datetime' },
      { name: 'last_used_at', type: 'datetime' },
    ],
    [
      { name: 'never_used', group: 'use', filter: [['last_used_at', 'is_not_set']] },
      { name: 'used_30_days', group: 'use', filter: [['last_used_at', 'in_period', 'last_30_days']] },
    ],
    { textFields: ['name', 'prefix'], defaultOrder: ['-created_at'] },
  ),
  get: (k: ApiKeyInfo, f: string) => (k as unknown as Record<string, unknown>)[f],
  text: (k: ApiKeyInfo) => `${k.name} ${k.prefix}`,
}
