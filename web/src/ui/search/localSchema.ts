/**
 * Schemas LOCAIS, na forma exacta do `/api/search/schemas/{resource}`, para as
 * listas cujo recurso o servidor ainda não descreve. Os operadores por tipo
 * são os da tabela do contrato (§2.2) — o painel não oferece nada que o
 * servidor, quando tiver o recurso, fosse recusar.
 */
import type { Domain, SearchFieldType, SearchOperator, SearchSchema, SearchSchemaField } from '../../api'

export const OPERATORS: Record<SearchFieldType, SearchOperator[]> = {
  text: ['eq', 'ne', 'contains', 'not_contains', 'starts_with', 'in', 'not_in', 'is_set', 'is_not_set'],
  enum: ['eq', 'ne', 'in', 'not_in'],
  number: ['eq', 'ne', 'lt', 'lte', 'gt', 'gte', 'between', 'is_set', 'is_not_set'],
  datetime: ['lt', 'lte', 'gt', 'gte', 'between', 'in_period', 'is_set', 'is_not_set'],
  bool: ['eq'],
  user: ['eq', 'ne', 'in', 'not_in', 'is_set', 'is_not_set'],
  ref: ['eq', 'ne', 'in', 'not_in', 'is_set', 'is_not_set'],
}

export const PERIODS = [
  'today',
  'yesterday',
  'this_week',
  'last_week',
  'this_month',
  'last_month',
  'this_quarter',
  'last_quarter',
  'this_year',
  'last_year',
  'last_7_days',
  'last_30_days',
  'next_7_days',
  'past',
  'future',
]

export interface LocalField {
  name: string
  type: SearchFieldType
  /** Por omissão: agrupável se enum/bool/datetime/user/ref. */
  groupable?: boolean
  sortable?: boolean
  aggregates?: string[]
  options?: string[]
}

export function localSchema(
  resource: string,
  fields: LocalField[],
  filters: { name: string; group: string; filter: Domain }[],
  opts: { textFields: string[]; defaultOrder?: string[] },
): SearchSchema {
  return {
    resource,
    label: resource,
    collection: '',
    org_scoped: false,
    timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    text_search: { fields: opts.textFields, typo_tolerant: false },
    fields: fields.map(
      (f): SearchSchemaField => ({
        name: f.name,
        label: f.name,
        type: f.type,
        // Utilizadores e referências filtram-se por identidade; localmente o
        // valor é o nome, por isso só se agrupam (sem condição «igual a <uuid>»).
        operators: f.type === 'user' || f.type === 'ref' ? ['is_set', 'is_not_set'] : OPERATORS[f.type],
        filterable: true,
        sortable: f.sortable ?? f.type !== 'bool',
        groupable: f.groupable ?? ['enum', 'bool', 'datetime', 'user', 'ref'].includes(f.type),
        granularities: f.type === 'datetime' ? ['day', 'week', 'month', 'quarter', 'year'] : undefined,
        aggregates: f.aggregates ?? [],
        options: f.options?.map((value) => ({ value, label: value })),
      }),
    ),
    filters: filters.map((f) => ({ ...f, label: f.name })),
    group_by: [],
    default_order: opts.defaultOrder ?? [],
    periods: PERIODS,
  }
}
