/**
 * Rótulos do painel: o servidor manda `label` em português; a UI traduz pelo
 * NOME (`search.<resource>.fields.<name>`, `search.<resource>.filters.<name>`,
 * `search.<resource>.options.<campo>.<valor>`) e só cai no rótulo do servidor
 * quando a chave não existe — um campo novo no servidor aparece, em português,
 * em vez de desaparecer.
 */
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import type { SearchSchema } from '../../api'
import { CustomCondition, CustomFilter, FacetRef, fieldOf, NO_VALUE_OPS, SearchState } from './model'

export interface SchemaLabels {
  field: (name: string) => string
  filter: (name: string) => string
  option: (field: string, value: string) => string
  op: (op: string) => string
  period: (p: string) => string
  granularity: (g: string) => string
  groupBy: (value: string) => string
  condition: (c: CustomCondition) => string
  custom: (f: CustomFilter) => string
  facet: (ref: FacetRef, state: SearchState) => { kind: string; text: string }
  /** Rótulo de uma chave de grupo de data (`2026-09`, `2026-W38`…) no idioma. */
  dateKey: (key: string, granularity: string) => string
}

export function useSchemaLabels(schema: SearchSchema | null): SchemaLabels {
  const { t, i18n } = useTranslation()
  return useMemo(() => {
    const r = schema?.resource ?? '_'
    const field = (name: string) => t(`search.${r}.fields.${name}`, { defaultValue: schema ? (fieldOf(schema, name)?.label ?? name) : name })
    const filter = (name: string) => t(`search.${r}.filters.${name}`, { defaultValue: schema?.filters.find((f) => f.name === name)?.label ?? name })
    const option = (f: string, value: string) =>
      t(`search.${r}.options.${f}.${value}`, { defaultValue: schema ? (fieldOf(schema, f)?.options?.find((o) => o.value === value)?.label ?? value) : value })
    const op = (o: string) => t(`search.ops.${o}`, { defaultValue: o })
    const period = (p: string) => t(`search.periodos.${p}`, { defaultValue: p })
    const granularity = (g: string) => t(`search.granularidade.${g}`, { defaultValue: g })
    const groupBy = (value: string) => {
      const [name, g] = value.split(':')
      return g ? `${field(name)}: ${granularity(g)}` : field(name)
    }
    const value = (c: CustomCondition): string => {
      const f = schema ? fieldOf(schema, c.field) : undefined
      const one = (v: unknown): string => {
        if (c.op === 'in_period') return period(String(v))
        if (f?.type === 'enum') return option(c.field, String(v))
        if (f?.type === 'bool') return v === true || v === 'true' ? t('search.personalizado.sim') : t('search.personalizado.nao')
        if (f?.type === 'user' && v === 'me') return t('search.personalizado.eu')
        if (f?.type === 'datetime' && typeof v === 'string') {
          const d = new Date(v)
          return Number.isNaN(d.getTime()) ? v : d.toLocaleDateString(i18n.language)
        }
        return String(v)
      }
      if (Array.isArray(c.value)) return c.op === 'between' ? `${one(c.value[0])} – ${one(c.value[1])}` : c.value.map(one).join(', ')
      return one(c.value)
    }
    const condition = (c: CustomCondition) =>
      NO_VALUE_OPS.includes(c.op) ? `${field(c.field)} ${op(c.op)}` : `${field(c.field)} ${op(c.op)} ${value(c)}`
    const custom = (f: CustomFilter) => f.conds.map(condition).join(` ${f.join === 'or' ? t('search.painel.ou') : t('search.painel.e')} `)
    const facet = (ref: FacetRef, s: SearchState) => {
      const ou = ` ${t('search.painel.ou')} `
      switch (ref.kind) {
        case 'q':
          return { kind: t('search.painel.pesquisar'), text: s.q.join(` ${t('search.painel.e')} `) }
        case 'term': {
          const term = s.terms.find((x) => x.field === ref.field)
          const f = schema ? fieldOf(schema, ref.field) : undefined
          const vals = (term?.values ?? []).map((v) => (f?.type === 'enum' ? option(ref.field, v) : v))
          return { kind: field(ref.field), text: vals.join(ou) }
        }
        case 'filters': {
          const names = s.filters.filter((n) => (schema?.filters.find((f) => f.name === n)?.group ?? n) === ref.group)
          return { kind: t('search.painel.filtro'), text: names.map(filter).join(ou) }
        }
        case 'custom':
          return { kind: t('search.painel.filtro'), text: custom(s.custom[ref.index]) }
        case 'groupBy':
          return { kind: t('search.painel.agruparPor'), text: s.groupBy.map(groupBy).join(' > ') }
      }
    }
    const dateKey = (key: string, g: string) => {
      const loc = i18n.language
      const m = key.match(/^(\d{4})(?:-(\d{2}))?(?:-(\d{2}))?$/)
      if (g === 'week') {
        const w = key.match(/^(\d{4})-W(\d{2})$/)
        return w ? t('search.grupos.semana', { n: Number(w[2]), ano: w[1] }) : key
      }
      if (g === 'quarter') {
        const q = key.match(/^(\d{4})-Q(\d)$/)
        return q ? t('search.grupos.trimestre', { n: Number(q[2]), ano: q[1] }) : key
      }
      if (!m) return key
      const d = new Date(Number(m[1]), m[2] ? Number(m[2]) - 1 : 0, m[3] ? Number(m[3]) : 1)
      if (g === 'year') return m[1]
      if (g === 'day') return d.toLocaleDateString(loc, { weekday: 'short', day: 'numeric', month: 'short', year: 'numeric' })
      return d.toLocaleDateString(loc, { month: 'long', year: 'numeric' })
    }
    return { field, filter, option, op, period, granularity, groupBy, condition, custom, facet, dateKey }
  }, [schema, t, i18n.language])
}
