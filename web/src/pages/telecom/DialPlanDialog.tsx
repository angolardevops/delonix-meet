/**
 * Editar o plano de marcação: acrescentar, remover e reordenar regras. Gravar
 * substitui o plano INTEIRO (`putDialPlan`), e a ordem das regras é a ordem em
 * que casam.
 *
 * O servidor é quem valida. Uma recusa vem com a regra apontada
 * (`details[].field = "rules[N]"`): marca-se essa regra e o foco vai para lá.
 * O 112 não se bloqueia (R210) — `telephony.emergency_cannot_be_blocked`.
 * Os números de emergência são da instalação: mostram-se, não se editam.
 */
import { FormEvent, useEffect, useId, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { putDialPlan } from '../../api'
import type { DialPlan, Trunk } from '../../api'
import { Alert, Button, Checkbox, Dialog, Field, IconButton, Select, StatusBadge, Tag, TextInput } from '../../ui/kit'
import {
  draftsFromRules,
  MAX_DESCRIPTION,
  MAX_PATTERN,
  MAX_RULES,
  newDraft,
  RULE_ACTIONS,
  ruleErrorTarget,
  ruleFieldForCode,
  rulesFromDrafts,
  validateDrafts,
  withAction,
} from './dialPlanEdit'
import type { RuleDraft, RuleErrors, RuleField } from './dialPlanEdit'
import { sortTrunks } from './format'
import { useTelecomText } from './shared'
import { errorCode, moveItem } from './trunkForm'

type Dir = -1 | 1
const FIELD_ORDER: RuleField[] = ['pattern', 'description', 'trunk', 'fallback']

function TrunkOptions({ trunks, current }: { trunks: Trunk[]; current: string }) {
  const { t } = useTranslation()
  return (
    <>
      {trunks.map((k) => (
        <option key={k.id} value={k.id}>
          {k.enabled ? k.name : t('telecom.planoEd.desactivada', { nome: k.name })}
        </option>
      ))}
      {/* Uma operadora que não está na lista carregada mostra o identificador, não um nome inventado. */}
      {current && !trunks.some((k) => k.id === current) && <option value={current}>{current.slice(0, 8)}</option>}
    </>
  )
}

function RuleEditor({
  draft: d,
  index,
  total,
  uid,
  trunks,
  errors,
  serverMsg,
  serverField,
  onChange,
  onMove,
  onRemove,
}: {
  draft: RuleDraft
  index: number
  total: number
  uid: string
  trunks: Trunk[]
  errors: RuleErrors
  /** A recusa do servidor para ESTA regra, se a apontou. */
  serverMsg?: string
  serverField?: RuleField | null
  onChange: (next: RuleDraft) => void
  onMove: (dir: Dir) => void
  onRemove: () => void
}) {
  const { t } = useTranslation()
  const { label } = useTelecomText()
  const n = index + 1
  const id = (f: string) => `${uid}-${d.key}-${f}`
  const err = (f: RuleField) => (errors[f] ? t(`telecom.planoEd.erro.${errors[f]}`) : undefined)
  const invalid = (f: RuleField) => (errors[f] || serverField === f ? true : undefined)
  const external = d.action === 'external'
  const actions = RULE_ACTIONS.includes(d.action as (typeof RULE_ACTIONS)[number]) ? RULE_ACTIONS : [...RULE_ACTIONS, d.action]

  return (
    <li className="tel-rule" data-rule={d.key} data-invalid={serverMsg ? true : undefined}>
      <div className="tel-rule__head">
        <strong className="dx-num" id={id('titulo')} tabIndex={-1}>
          {t('telecom.planoEd.regra', { n })}
        </strong>
        {d.emergency && (
          <StatusBadge tone="warning" icon="alert">
            {t('telecom.plano.emergencia')}
          </StatusBadge>
        )}
        <span className="dx-spacer" />
        <IconButton icon="chevronUp" bare label={t('telecom.planoEd.subir', { n })} disabled={index === 0} data-move={`${d.key}:-1`} onClick={() => onMove(-1)} />
        <IconButton icon="chevronDown" bare label={t('telecom.planoEd.descer', { n })} disabled={index === total - 1} data-move={`${d.key}:1`} onClick={() => onMove(1)} />
        <IconButton icon="trash" bare label={t('telecom.planoEd.remover', { n })} onClick={onRemove} />
      </div>
      {serverMsg && <Alert tone="danger">{serverMsg}</Alert>}
      <div className="tel-form__grid">
        <Field label={t('telecom.planoEd.padrao')} htmlFor={id('pattern')} error={err('pattern')} hint={t('telecom.planoEd.padraoAjuda')}>
          <TextInput
            id={id('pattern')}
            className="dx-num"
            value={d.pattern}
            maxLength={MAX_PATTERN}
            onChange={(e) => onChange({ ...d, pattern: e.target.value })}
            aria-invalid={invalid('pattern')}
            autoCapitalize="characters"
            spellCheck={false}
          />
        </Field>
        <Field label={t('telecom.planoEd.descricao')} htmlFor={id('description')} error={err('description')}>
          <TextInput
            id={id('description')}
            value={d.description}
            maxLength={MAX_DESCRIPTION}
            onChange={(e) => onChange({ ...d, description: e.target.value })}
            aria-invalid={invalid('description')}
          />
        </Field>
        <Field label={t('telecom.planoEd.accao')} htmlFor={id('action')} hint={d.emergency ? t('telecom.planoEd.emergenciaFixa') : undefined}>
          {/* Uma regra de emergência sai sempre por uma operadora: a acção não se muda. */}
          <Select id={id('action')} value={d.action} disabled={d.emergency} onChange={(e) => onChange(withAction(d, e.target.value))}>
            {actions.map((a) => (
              <option key={a} value={a}>
                {label('accao', a)}
              </option>
            ))}
          </Select>
        </Field>
      </div>
      {external && (
        <div className="tel-form__grid">
          <Field
            label={t('telecom.planoEd.operadora')}
            htmlFor={id('trunk')}
            error={err('trunk')}
            hint={trunks.length === 0 ? t('telecom.planoEd.semOperadoras') : undefined}
          >
            <Select id={id('trunk')} value={d.trunk_id} onChange={(e) => onChange({ ...d, trunk_id: e.target.value })} aria-invalid={invalid('trunk')}>
              <option value="">{t('telecom.planoEd.escolher')}</option>
              <TrunkOptions trunks={trunks} current={d.trunk_id} />
            </Select>
          </Field>
          <Field label={t('telecom.planoEd.reserva')} htmlFor={id('fallback')} error={err('fallback')}>
            <Select id={id('fallback')} value={d.fallback_trunk_id} onChange={(e) => onChange({ ...d, fallback_trunk_id: e.target.value })} aria-invalid={invalid('fallback')}>
              <option value="">{t('telecom.planoEd.semReserva')}</option>
              <TrunkOptions trunks={trunks} current={d.fallback_trunk_id} />
            </Select>
          </Field>
        </div>
      )}
      {/* Uma chamada de emergência nunca é gravada: a caixa fica travada. */}
      <Checkbox label={t('telecom.planoEd.gravar')} checked={d.record} disabled={d.emergency} onChange={(e) => onChange({ ...d, record: e.target.checked })} />
    </li>
  )
}

export default function DialPlanDialog({
  orgId,
  plan,
  trunks,
  onClose,
  onSaved,
}: {
  orgId: string
  plan: DialPlan
  trunks: Trunk[]
  onClose: () => void
  onSaved: () => void
}) {
  const { t } = useTranslation()
  const { failure } = useTelecomText()
  const uid = useId()
  const [original] = useState(() => draftsFromRules(plan.rules))
  const [drafts, setDrafts] = useState<RuleDraft[]>(original)
  const [tried, setTried] = useState(false)
  const [busy, setBusy] = useState(false)
  const [server, setServer] = useState<{ key: string | null; field: RuleField | null; msg: string } | null>(null)
  const [announce, setAnnounce] = useState('')
  const list = useRef<HTMLOListElement>(null)
  const addBtn = useRef<HTMLButtonElement>(null)
  const focusAfter = useRef<{ kind: 'move'; key: string; dir: Dir } | { kind: 'field'; id: string } | { kind: 'add' } | null>(null)
  const sorted = sortTrunks(trunks)
  const local = tried ? validateDrafts(drafts) : {}

  // O foco segue o que se fez: o botão de mover, o campo novo ou com erro, ou «acrescentar».
  useEffect(() => {
    const f = focusAfter.current
    if (!f) return
    focusAfter.current = null
    if (f.kind === 'add') addBtn.current?.focus()
    else if (f.kind === 'field') document.getElementById(f.id)?.focus()
    else {
      const btn = (dir: Dir) => list.current?.querySelector<HTMLButtonElement>(`[data-move="${f.key}:${dir}"]`)
      const alvo = btn(f.dir)
      ;(alvo && !alvo.disabled ? alvo : btn(f.dir === 1 ? -1 : 1))?.focus()
    }
  })

  const fieldId = (key: string, field: string) => `${uid}-${key}-${field}`
  const change = (next: RuleDraft) => {
    setDrafts((l) => l.map((d) => (d.key === next.key ? next : d)))
    setServer(null)
  }
  const move = (index: number, dir: Dir) => {
    const next = moveItem(drafts, index, dir)
    if (next === drafts) return
    focusAfter.current = { kind: 'move', key: drafts[index].key, dir }
    setDrafts(next)
    setServer(null)
    setAnnounce(t('telecom.planoEd.movida', { n: index + dir + 1, total: next.length }))
  }
  const remove = (key: string) => {
    focusAfter.current = { kind: 'add' }
    setDrafts((l) => l.filter((d) => d.key !== key))
    setServer(null)
    setAnnounce(t('telecom.planoEd.removida'))
  }
  const add = () => {
    const d = newDraft()
    focusAfter.current = { kind: 'field', id: fieldId(d.key, 'pattern') }
    setDrafts((l) => [...l, d])
    setServer(null)
  }

  async function submit(e: FormEvent) {
    e.preventDefault()
    setTried(true)
    const errs = validateDrafts(drafts)
    const primeira = drafts.find((d) => errs[d.key])
    if (primeira) {
      const campo = FIELD_ORDER.find((f) => errs[primeira.key][f]) ?? 'pattern'
      document.getElementById(fieldId(primeira.key, campo))?.focus()
      return
    }
    const rules = rulesFromDrafts(drafts)
    // Nada mudou: não há pedido a fazer (gravar criaria uma versão nova igual).
    if (JSON.stringify(rules) === JSON.stringify(rulesFromDrafts(original))) {
      onClose()
      return
    }
    setBusy(true)
    setServer(null)
    try {
      await putDialPlan(orgId, rules)
      onSaved()
    } catch (x) {
      const alvo = ruleErrorTarget(x)
      const draft = alvo ? drafts[alvo.index] : undefined
      const field = draft ? ruleFieldForCode(errorCode(x), alvo?.field ?? null) : null
      setServer({ key: draft?.key ?? null, field, msg: failure(x) })
      // O foco vai para o campo da regra apontada; sem campo, para o título dela.
      if (draft) document.getElementById(fieldId(draft.key, field ?? 'titulo'))?.focus()
      setBusy(false)
    }
  }

  const cheio = drafts.length >= MAX_RULES

  return (
    <Dialog title={t('telecom.planoEd.titulo')} onClose={onClose} wide>
      <form className="tel-form" onSubmit={submit} noValidate>
        <p className="dx-muted tel-small">{t('telecom.planoEd.intro')}</p>
        {drafts.length === 0 ? (
          <p className="dx-muted tel-small">{t('telecom.planoEd.vazio')}</p>
        ) : (
          <ol className="tel-rules" ref={list} data-testid="tel-rules">
            {drafts.map((d, i) => (
              <RuleEditor
                key={d.key}
                draft={d}
                index={i}
                total={drafts.length}
                uid={uid}
                trunks={sorted}
                errors={local[d.key] ?? {}}
                serverMsg={server?.key === d.key ? server.msg : undefined}
                serverField={server?.key === d.key ? server.field : null}
                onChange={change}
                onMove={(dir) => move(i, dir)}
                onRemove={() => remove(d.key)}
              />
            ))}
          </ol>
        )}
        <div className="tel-actions">
          <Button ref={addBtn} size="sm" variant="secondary" icon="plus" onClick={add} disabled={cheio}>
            {t('telecom.planoEd.acrescentar')}
          </Button>
          {cheio && <span className="dx-muted tel-small">{t('telecom.planoEd.limite', { max: MAX_RULES })}</span>}
        </div>

        <div className="tel-emergency tel-emergency--box">
          <span className="dx-eyebrow">{t('telecom.plano.numerosEmergencia')}</span>
          {plan.emergency_numbers.length === 0 ? (
            <span className="dx-muted tel-small">{t('telecom.plano.semEmergencia')}</span>
          ) : (
            <ul className="tel-tags">
              {plan.emergency_numbers.map((n) => (
                <li key={n}>
                  <Tag>{n}</Tag>
                </li>
              ))}
            </ul>
          )}
          <span className="dx-muted tel-small">{t('telecom.planoEd.emergenciaNota')}</span>
        </div>

        {server && !server.key && <Alert tone="danger">{server.msg}</Alert>}
        <p className="dx-sr-only" role="status" aria-live="polite">
          {announce}
        </p>
        <div className="tel-form__foot">
          <Button variant="secondary" onClick={onClose} disabled={busy}>
            {t('ui.cancelar')}
          </Button>
          <Button type="submit" variant="primary" busy={busy}>
            {t('telecom.planoEd.gravarPlano')}
          </Button>
        </div>
      </form>
    </Dialog>
  )
}
