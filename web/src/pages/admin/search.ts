/**
 * Fontes de pesquisa da Administração e dos Contactos.
 *
 * - `members`: recurso do servidor; sem ele, `GET /employees` inteiro.
 * - organizações e grupos: o servidor ainda não os descreve (nem na fase 2 do
 *   contrato) — lista inteira no browser.
 * - auditoria: SEM alternativa local. A lista de sempre é cortada (`limit`),
 *   e filtrar uma amostra cortada dava contagens falsas.
 */
import { Employee, Group, listEmployees, listGroups, OrgSummary } from '../../api'
import { localSchema } from '../../ui/search/localSchema'
import type { LocalFallback } from '../../ui/search/useResourceSearch'

export function membersFallback(orgId: string): LocalFallback<Employee> {
  return {
    load: () => listEmployees(orgId),
    source: {
      schema: localSchema(
        'members',
        [
          { name: 'username', type: 'text' },
          { name: 'email', type: 'text' },
          { name: 'title', type: 'text', groupable: true },
          { name: 'role', type: 'enum', options: ['admin', 'member'] },
          { name: 'branch', type: 'ref' },
          { name: 'last_active', type: 'datetime' },
        ],
        [
          { name: 'admins', group: 'role', filter: [['role', 'eq', 'admin']] },
          { name: 'members', group: 'role', filter: [['role', 'eq', 'member']] },
          { name: 'without_branch', group: 'branch', filter: [['branch', 'is_not_set']] },
          { name: 'active_7_days', group: 'period', filter: [['last_active', 'in_period', 'last_7_days']] },
        ],
        { textFields: ['username', 'email', 'title'], defaultOrder: ['username'] },
      ),
      get: (m, f) => (f === 'branch' ? m.branch_name : (m as unknown as Record<string, unknown>)[f]),
      text: (m) => `${m.username} ${m.email} ${m.title ?? ''} ${m.branch_name ?? ''}`,
    },
  }
}

export function orgsFallback(orgs: OrgSummary[]): LocalFallback<OrgSummary> {
  return {
    load: async () => orgs,
    source: {
      schema: localSchema(
        'orgs',
        [
          { name: 'name', type: 'text' },
          { name: 'domain', type: 'text' },
          { name: 'role', type: 'enum', options: ['admin', 'member'] },
          { name: 'member_count', type: 'number', aggregates: ['sum'] },
          { name: 'retention_days', type: 'number' },
        ],
        [
          { name: 'admin', group: 'role', filter: [['role', 'eq', 'admin']] },
          { name: 'member', group: 'role', filter: [['role', 'eq', 'member']] },
        ],
        { textFields: ['name', 'domain'], defaultOrder: ['name'] },
      ),
      get: (o, f) => (f === 'domain' ? (o.domain || o.slug) : (o as unknown as Record<string, unknown>)[f]),
      text: (o) => `${o.name} ${o.domain ?? ''} ${o.slug}`,
    },
  }
}

export function groupsFallback(orgId: string): LocalFallback<Group> {
  return {
    load: () => listGroups(orgId),
    source: {
      schema: localSchema(
        'groups',
        [
          { name: 'name', type: 'text' },
          { name: 'member_count', type: 'number', aggregates: ['sum'] },
        ],
        [],
        { textFields: ['name'], defaultOrder: ['name'] },
      ),
      get: (g, f) => (g as unknown as Record<string, unknown>)[f],
      text: (g) => g.name,
    },
  }
}
