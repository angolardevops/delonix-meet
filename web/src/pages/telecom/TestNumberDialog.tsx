/**
 * «Testar número»: o que o plano GRAVADO faria com um número — que regra casa,
 * por que operadora sai, as reservas, o preço estimado (ou porque não o há),
 * se grava e se é emergência. Não liga a ninguém, e o ecrã di-lo.
 */
import { FormEvent, useId, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { testDialNumber } from '../../api'
import type { TestNumberResult } from '../../api'
import { Alert, Button, Dialog, Field, StatusBadge, TextInput } from '../../ui/kit'
import type { BadgeTone } from '../../ui/kit'
import type { IconName } from '../../ui/icons'
import { useLocaleTag } from '../admin/orgShared'
import { isDialable, outcomeKey, priceReasonKey } from './dialPlanEdit'
import { formatMoney } from './format'
import { NA, useTelecomText } from './shared'
import { errorCode } from './trunkForm'

function outcomeBadge(outcome: string): { tone: BadgeTone; icon?: IconName } {
  if (outcome === 'route') return { tone: 'success', icon: 'check' }
  if (outcome === 'blocked') return { tone: 'record', icon: 'ban' }
  if (outcome === 'internal') return { tone: 'neutral' }
  return { tone: 'warning', icon: 'alert' }
}

export function TestResult({ result: r }: { result: TestNumberResult }) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const ok = outcomeKey(r.outcome)
  const badge = outcomeBadge(r.outcome)
  const semPreco = r.price_reason ? (priceReasonKey(r.price_reason) ? t(priceReasonKey(r.price_reason) as string) : r.price_reason) : NA
  return (
    <div className="tel-usage" data-testid="tel-test-result">
      <div className="tel-badges">
        <StatusBadge tone={badge.tone} icon={badge.icon}>
          {ok ? t(ok) : r.outcome}
        </StatusBadge>
        {r.emergency && (
          <StatusBadge tone="warning" icon="alert">
            {t('telecom.plano.emergencia')}
          </StatusBadge>
        )}
      </div>
      <dl className="tel-kv">
        <div className="tel-kv__row">
          <dt className="dx-muted">{t('telecom.testar.marcado')}</dt>
          <dd className="dx-num tel-break">{r.dialed}</dd>
        </div>
        {r.e164 && (
          <div className="tel-kv__row">
            <dt className="dx-muted">{t('telecom.testar.e164')}</dt>
            <dd className="dx-num tel-break">{r.e164}</dd>
          </div>
        )}
        <div className="tel-kv__row">
          <dt className="dx-muted">{t('telecom.testar.regra')}</dt>
          <dd>
            {r.matched_rule ? (
              <span className="tel-stack">
                <span className="dx-num">{t('telecom.testar.regraN', { n: r.matched_rule.position + 1, padrao: r.matched_rule.pattern })}</span>
                {r.matched_rule.description && <span className="dx-muted tel-small">{r.matched_rule.description}</span>}
              </span>
            ) : r.emergency ? (
              t('telecom.testar.regraEmergencia')
            ) : (
              t('telecom.testar.semRegra')
            )}
          </dd>
        </div>
        <div className="tel-kv__row">
          <dt className="dx-muted">{t('telecom.testar.operadora')}</dt>
          <dd>{r.trunk ? `${r.trunk.name} (${r.trunk.short_code})` : NA}</dd>
        </div>
        <div className="tel-kv__row">
          <dt className="dx-muted">{t('telecom.testar.reservas')}</dt>
          <dd>{r.fallbacks.length > 0 ? r.fallbacks.map((k) => k.name).join(' · ') : <span className="dx-muted">{t('telecom.testar.semReservas')}</span>}</dd>
        </div>
        <div className="tel-kv__row">
          <dt className="dx-muted">{t('telecom.testar.preco')}</dt>
          <dd className={r.estimated_price_per_min ? 'dx-num' : 'dx-muted'}>
            {r.estimated_price_per_min ? t('telecom.testar.porMin', { preco: formatMoney(r.estimated_price_per_min, locale) }) : semPreco}
          </dd>
        </div>
        <div className="tel-kv__row">
          <dt className="dx-muted">{t('telecom.testar.grava')}</dt>
          <dd>{r.recorded ? t('ui.sim') : t('ui.nao')}</dd>
        </div>
        <div className="tel-kv__row">
          <dt className="dx-muted">{t('telecom.testar.emergencia')}</dt>
          <dd>{r.emergency ? t('ui.sim') : t('ui.nao')}</dd>
        </div>
      </dl>
      {typeof r.overridden_rule_position === 'number' && (
        <p className="dx-muted tel-small">{t('telecom.testar.ultrapassada', { n: r.overridden_rule_position + 1 })}</p>
      )}
    </div>
  )
}

export default function TestNumberDialog({ orgId, onClose }: { orgId: string; onClose: () => void }) {
  const { t } = useTranslation()
  const { failure } = useTelecomText()
  const uid = useId()
  const [number, setNumber] = useState('')
  const [busy, setBusy] = useState(false)
  const [result, setResult] = useState<TestNumberResult | null>(null)
  const [err, setErr] = useState<{ field: boolean; msg: string } | null>(null)

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!isDialable(number)) {
      setErr({ field: true, msg: t('telecom.testar.numeroInvalido') })
      document.getElementById(`${uid}-number`)?.focus()
      return
    }
    setBusy(true)
    setErr(null)
    try {
      setResult(await testDialNumber(orgId, number.trim()))
    } catch (x) {
      setResult(null)
      const field = errorCode(x) === 'telephony.invalid_number'
      setErr({ field, msg: failure(x) })
      if (field) document.getElementById(`${uid}-number`)?.focus()
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog title={t('telecom.testar.titulo')} onClose={onClose}>
      <form className="tel-form" onSubmit={submit} noValidate>
        <Alert>{t('telecom.testar.nota')}</Alert>
        <Field label={t('telecom.testar.numero')} htmlFor={`${uid}-number`} error={err?.field ? err.msg : undefined} hint={t('telecom.testar.numeroAjuda')}>
          <TextInput
            id={`${uid}-number`}
            type="tel"
            className="dx-num"
            value={number}
            onChange={(e) => {
              setNumber(e.target.value)
              setErr(null)
            }}
            aria-invalid={err?.field ? true : undefined}
            autoComplete="off"
          />
        </Field>
        {err && !err.field && <Alert tone="danger">{err.msg}</Alert>}
        <div aria-live="polite">{result && <TestResult result={result} />}</div>
        <div className="tel-form__foot">
          <Button variant="secondary" onClick={onClose}>
            {t('ui.fechar')}
          </Button>
          <Button type="submit" variant="primary" busy={busy}>
            {t('telecom.testar.testar')}
          </Button>
        </div>
      </form>
    </Dialog>
  )
}
