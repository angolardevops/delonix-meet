/**
 * «Adicionar filtro personalizado»: condições campo · operador · valor,
 * juntas com E ou OU. Os campos e os operadores são SÓ os que o schema diz
 * (o servidor recusa o resto com `search.invalid_operator`); o valor tem a
 * forma do tipo do campo (texto, número, data, período, sim/não, opção).
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { SearchOperator, SearchSchema, SearchSchemaField } from '../../api'
import { Button, Dialog, IconButton, Segmented, Select, TextInput } from '../kit'
import type { SchemaLabels } from './labels'
import { CustomCondition, CustomFilter, NO_VALUE_OPS } from './model'

function firstOp(f: SearchSchemaField): SearchOperator {
  const pref: SearchOperator[] = f.type === 'text' ? ['contains'] : f.type === 'datetime' ? ['in_period', 'gte'] : ['eq']
  return pref.find((p) => f.operators.includes(p)) ?? f.operators[0]
}

function defaultValue(f: SearchSchemaField, op: SearchOperator, periods: string[]): unknown {
  if (NO_VALUE_OPS.includes(op)) return undefined
  if (op === 'in_period') return periods[0] ?? 'today'
  if (f.type === 'bool') return true
  if (f.type === 'enum') return op === 'in' || op === 'not_in' ? [] : (f.options?.[0]?.value ?? '')
  if (op === 'in' || op === 'not_in') return ''
  if (op === 'between') return ['', '']
  if (f.type === 'user') return 'me'
  return ''
}

/** «a, b, c» → ['a','b','c'] (o campo guarda o texto enquanto se escreve). */
function splitList(v: unknown): string[] {
  return String(v ?? '')
    .split(',')
    .map((x) => x.trim())
    .filter(Boolean)
}

/** Uma condição está pronta para ir ao servidor? */
export function conditionReady(c: CustomCondition, f: SearchSchemaField | undefined): boolean {
  if (!f || !f.operators.includes(c.op)) return false
  if (NO_VALUE_OPS.includes(c.op)) return true
  const v = c.value
  if (c.op === 'between') return Array.isArray(v) && v.length === 2 && v.every((x) => String(x ?? '').trim() !== '' && (f.type !== 'number' || Number.isFinite(Number(x))))
  if (c.op === 'in' || c.op === 'not_in') return (Array.isArray(v) ? v : splitList(v)).length > 0
  if (f.type === 'number') return String(v ?? '').trim() !== '' && Number.isFinite(Number(v))
  if (f.type === 'bool') return typeof v === 'boolean'
  return String(v ?? '').trim() !== ''
}

/** Normaliza o valor para o que o contrato espera (números, RFC 3339). */
export function conditionForServer(c: CustomCondition, f: SearchSchemaField): CustomCondition {
  if (NO_VALUE_OPS.includes(c.op)) return { field: c.field, op: c.op }
  const conv = (x: unknown) => {
    if (f.type === 'number') return Number(x)
    if (f.type === 'datetime' && c.op !== 'in_period' && typeof x === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(x)) return new Date(`${x}T00:00:00`).toISOString()
    return x
  }
  const listy = (c.op === 'in' || c.op === 'not_in') && !Array.isArray(c.value) ? splitList(c.value) : c.value
  return { field: c.field, op: c.op, value: Array.isArray(listy) ? listy.map(conv) : conv(listy) }
}

export default function CustomFilterDialog({
  schema,
  labels: L,
  onClose,
  onApply,
}: {
  schema: SearchSchema
  labels: SchemaLabels
  onClose: () => void
  onApply: (f: CustomFilter) => void
}) {
  const { t } = useTranslation()
  const fields = schema.fields.filter((f) => f.filterable && f.operators.length)
  const periods = schema.periods.length ? schema.periods : ['today']
  const fresh = (): CustomCondition => {
    const f = fields[0]
    const op = firstOp(f)
    return { field: f.name, op, value: defaultValue(f, op, periods) }
  }
  const [join, setJoin] = useState<'and' | 'or'>('and')
  const [conds, setConds] = useState<CustomCondition[]>(() => (fields.length ? [fresh()] : []))

  const fieldOf = (name: string) => fields.find((f) => f.name === name)
  const ready = conds.length > 0 && conds.every((c) => conditionReady(c, fieldOf(c.field)))

  function patch(i: number, next: Partial<CustomCondition>) {
    setConds((cs) =>
      cs.map((c, j) => {
        if (j !== i) return c
        const merged = { ...c, ...next }
        const f = fieldOf(merged.field)!
        if (next.field && next.field !== c.field) {
          const op = firstOp(f)
          return { field: f.name, op, value: defaultValue(f, op, periods) }
        }
        if (next.op && next.op !== c.op) return { ...merged, value: defaultValue(f, next.op, periods) }
        return merged
      }),
    )
  }

  return (
    <Dialog
      title={t('search.personalizado.titulo')}
      onClose={onClose}
      wide
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t('ui.cancelar')}
          </Button>
          <Button
            variant="primary"
            disabled={!ready}
            onClick={() => onApply({ join, conds: conds.map((c) => conditionForServer(c, fieldOf(c.field)!)) })}
          >
            {t('search.personalizado.aplicar')}
          </Button>
        </>
      }
    >
      <div className="dx-sp-builder">
        {conds.length > 1 && (
          <Segmented<'and' | 'or'>
            label={t('search.personalizado.juntar')}
            value={join}
            onChange={setJoin}
            options={[
              { value: 'and', label: t('search.personalizado.todas') },
              { value: 'or', label: t('search.personalizado.qualquer') },
            ]}
          />
        )}
        <ol className="dx-sp-builder__rows">
          {conds.map((c, i) => {
            const f = fieldOf(c.field)!
            return (
              <li key={i} className="dx-sp-builder__row" data-cond={i}>
                {i > 0 && <span className="dx-sp-builder__join dx-eyebrow">{join === 'or' ? t('search.painel.ou') : t('search.painel.e')}</span>}
                <Select aria-label={t('search.personalizado.campo')} value={c.field} onChange={(e) => patch(i, { field: e.target.value })}>
                  {fields.map((x) => (
                    <option key={x.name} value={x.name}>
                      {L.field(x.name)}
                    </option>
                  ))}
                </Select>
                <Select aria-label={t('search.personalizado.operador')} value={c.op} onChange={(e) => patch(i, { op: e.target.value as SearchOperator })}>
                  {f.operators.map((o) => (
                    <option key={o} value={o}>
                      {L.op(o)}
                    </option>
                  ))}
                </Select>
                <ValueInput field={f} cond={c} periods={periods} labels={L} onChange={(value) => patch(i, { value })} />
                <IconButton
                  icon="x"
                  bare
                  label={t('search.personalizado.removerCondicao')}
                  disabled={conds.length === 1}
                  onClick={() => setConds((cs) => cs.filter((_, j) => j !== i))}
                />
              </li>
            )
          })}
        </ol>
        {conds.length < 20 && fields.length > 0 && (
          <Button size="sm" variant="ghost" icon="plus" onClick={() => setConds((cs) => [...cs, fresh()])}>
            {t('search.personalizado.adicionarCondicao')}
          </Button>
        )}
        {!ready && conds.length > 0 && <p className="dx-muted dx-sp-builder__hint">{t('search.personalizado.valorInvalido')}</p>}
      </div>
    </Dialog>
  )
}

function ValueInput({
  field: f,
  cond: c,
  periods,
  labels: L,
  onChange,
}: {
  field: SearchSchemaField
  cond: CustomCondition
  periods: string[]
  labels: SchemaLabels
  onChange: (v: unknown) => void
}) {
  const { t } = useTranslation()
  const lbl = t('search.personalizado.valor')
  if (NO_VALUE_OPS.includes(c.op)) return <span className="dx-sp-builder__novalue" />
  if (c.op === 'in_period') {
    return (
      <Select aria-label={lbl} value={String(c.value)} onChange={(e) => onChange(e.target.value)}>
        {periods.map((p) => (
          <option key={p} value={p}>
            {L.period(p)}
          </option>
        ))}
      </Select>
    )
  }
  if (f.type === 'bool') {
    return (
      <Select aria-label={lbl} value={c.value === false ? 'false' : 'true'} onChange={(e) => onChange(e.target.value === 'true')}>
        <option value="true">{t('search.personalizado.sim')}</option>
        <option value="false">{t('search.personalizado.nao')}</option>
      </Select>
    )
  }
  if (f.type === 'enum') {
    if (c.op === 'in' || c.op === 'not_in') {
      const sel = Array.isArray(c.value) ? (c.value as string[]) : []
      return (
        <Select
          aria-label={lbl}
          multiple
          value={sel}
          onChange={(e) => onChange([...e.target.selectedOptions].map((o) => o.value))}
          className="dx-sp-builder__multi"
        >
          {(f.options ?? []).map((o) => (
            <option key={o.value} value={o.value}>
              {L.option(f.name, o.value)}
            </option>
          ))}
        </Select>
      )
    }
    return (
      <Select aria-label={lbl} value={String(c.value ?? '')} onChange={(e) => onChange(e.target.value)}>
        {(f.options ?? []).map((o) => (
          <option key={o.value} value={o.value}>
            {L.option(f.name, o.value)}
          </option>
        ))}
      </Select>
    )
  }
  if (f.type === 'user' && (c.op === 'eq' || c.op === 'ne')) {
    // Pessoas escolhem-se por identidade: sem uma lista de pessoas no schema,
    // oferece-se «Eu» (o contrato aceita `"me"`), nunca um UUID escrito à mão.
    return (
      <Select aria-label={lbl} value="me" onChange={() => onChange('me')}>
        <option value="me">{t('search.personalizado.eu')}</option>
      </Select>
    )
  }
  const type = f.type === 'number' ? 'number' : f.type === 'datetime' ? 'date' : 'text'
  if (c.op === 'between') {
    const v = Array.isArray(c.value) ? c.value : ['', '']
    return (
      <span className="dx-sp-builder__between">
        <TextInput type={type} aria-label={t('search.personalizado.de')} value={String(v[0] ?? '')} onChange={(e) => onChange([e.target.value, v[1]])} />
        <TextInput type={type} aria-label={t('search.personalizado.ate')} value={String(v[1] ?? '')} onChange={(e) => onChange([v[0], e.target.value])} />
      </span>
    )
  }
  if (c.op === 'in' || c.op === 'not_in') {
    const v = Array.isArray(c.value) ? (c.value as string[]).join(', ') : String(c.value ?? '')
    return <TextInput aria-label={lbl} value={v} placeholder={t('search.personalizado.variosValores')} onChange={(e) => onChange(e.target.value)} />
  }
  return <TextInput type={type} aria-label={lbl} value={String(c.value ?? '')} onChange={(e) => onChange(e.target.value)} />
}
