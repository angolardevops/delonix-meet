/**
 * «Câmbio»: quantos kwanzas vale uma unidade de moeda estrangeira, e desde
 * quando. É com isto que um preço em dólares entra no total em kwanzas do
 * consumo; sem taxa, o total diz que falta o câmbio em vez de inventar um.
 *
 * Uma taxa não se edita nem se apaga: acrescenta-se outra com início novo.
 */
import { FormEvent, useId, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { createExchangeRate, listExchangeRates } from '../../api'
import type { ExchangeRate } from '../../api'
import { AsyncSection } from '../../components/AsyncSection'
import { Alert, Button, Card, Dialog, Field, Select, TextInput } from '../../ui/kit'
import { formatDateTime, useLocaleTag } from '../admin/orgShared'
import { formatDecimal } from './format'
import { LoadMore, useTelecomText } from './shared'
import { dateOrAmountField, decimalText, errorCode, isRateText, localToIso, RATE_CURRENCIES } from './trunkForm'
import { usePaged } from './usePaged'

export function RatesList({ items }: { items: ExchangeRate[] }) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  return (
    <ul className="tel-rates" data-testid="tel-rates">
      {items.map((r) => (
        <li key={r.id}>
          <span className="dx-num">{t('telecom.cambio.linha', { moeda: r.currency, taxa: formatDecimal(r.aoa_per_unit, locale) ?? r.aoa_per_unit })}</span>
          <span className="dx-num dx-muted tel-small">{t('telecom.cambio.desde', { quando: formatDateTime(r.valid_from, locale) })}</span>
        </li>
      ))}
    </ul>
  )
}

export default function RatesCard({ orgId, onChanged }: { orgId: string; onChanged: () => void }) {
  const { t } = useTranslation()
  const rates = usePaged((token, signal) => listExchangeRates(orgId, { page_token: token }, signal), [orgId])
  const [adding, setAdding] = useState(false)
  return (
    <Card title={t('telecom.cambio.titulo')} eyebrow={t('telecom.cambio.eyebrow')} className="tel-card">
      <AsyncSection state={rates.state} onRetry={rates.reload}>
        {(d) => (
          <div className="tel-usage">
            {d.items.length === 0 ? <p className="dx-muted tel-small">{t('telecom.cambio.vazio')}</p> : <RatesList items={d.items} />}
            <LoadMore next={d.next} busy={rates.busy} err={rates.err} onMore={rates.loadMore} />
            <div>
              <Button size="sm" variant="secondary" icon="plus" onClick={() => setAdding(true)}>
                {t('telecom.cambio.nova')}
              </Button>
            </div>
          </div>
        )}
      </AsyncSection>
      {adding && (
        <RateDialog
          orgId={orgId}
          onClose={() => setAdding(false)}
          onCreated={() => {
            setAdding(false)
            rates.reload()
            onChanged()
          }}
        />
      )}
    </Card>
  )
}

type Campo = 'amount' | 'valid_from'

function RateDialog({ orgId, onClose, onCreated }: { orgId: string; onClose: () => void; onCreated: () => void }) {
  const { t } = useTranslation()
  const { failure } = useTelecomText()
  const uid = useId()
  const [currency, setCurrency] = useState<string>(RATE_CURRENCIES[0])
  const [rate, setRate] = useState('')
  const [from, setFrom] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<{ field: Campo | null; msg: string } | null>(null)

  async function submit(e: FormEvent) {
    e.preventDefault()
    const focus = (c: Campo) => document.getElementById(`${uid}-${c}`)?.focus()
    if (!isRateText(rate)) {
      setErr({ field: 'amount', msg: t('telecom.cambio.taxaInvalida') })
      focus('amount')
      return
    }
    const validFrom = localToIso(from)
    if (validFrom === null) {
      setErr({ field: 'valid_from', msg: t('telecom.precos.dataInvalida') })
      focus('valid_from')
      return
    }
    setBusy(true)
    setErr(null)
    try {
      await createExchangeRate(orgId, { currency, aoa_per_unit: decimalText(rate), valid_from: validFrom })
      onCreated()
    } catch (x) {
      const field = dateOrAmountField(errorCode(x))
      setErr({ field, msg: failure(x) })
      if (field) focus(field)
    } finally {
      setBusy(false)
    }
  }

  const errOf = (c: Campo) => (err?.field === c ? err.msg : undefined)

  return (
    <Dialog title={t('telecom.cambio.nova')} onClose={onClose}>
      <form className="tel-form" onSubmit={submit} noValidate>
        <div className="tel-form__grid">
          <Field label={t('telecom.form.moeda')} htmlFor={`${uid}-currency`}>
            <Select id={`${uid}-currency`} value={currency} onChange={(e) => setCurrency(e.target.value)}>
              {RATE_CURRENCIES.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </Select>
          </Field>
          <Field label={t('telecom.cambio.taxa')} htmlFor={`${uid}-amount`} error={errOf('amount')} hint={t('telecom.cambio.taxaAjuda')}>
            <TextInput
              id={`${uid}-amount`}
              className="dx-num"
              inputMode="decimal"
              value={rate}
              onChange={(e) => {
                setRate(e.target.value)
                setErr(null)
              }}
              aria-invalid={errOf('amount') ? true : undefined}
              required
            />
          </Field>
        </div>
        <Field label={t('telecom.precos.desde')} htmlFor={`${uid}-valid_from`} error={errOf('valid_from')} hint={t('telecom.precos.desdeAjuda')}>
          <TextInput
            id={`${uid}-valid_from`}
            type="datetime-local"
            className="dx-num"
            value={from}
            onChange={(e) => {
              setFrom(e.target.value)
              setErr(null)
            }}
            aria-invalid={errOf('valid_from') ? true : undefined}
          />
        </Field>
        {err && !err.field && <Alert tone="danger">{err.msg}</Alert>}
        <div className="tel-form__foot">
          <Button variant="secondary" onClick={onClose} disabled={busy}>
            {t('ui.cancelar')}
          </Button>
          <Button type="submit" variant="primary" busy={busy}>
            {t('telecom.cambio.acrescentar')}
          </Button>
        </div>
      </form>
    </Dialog>
  )
}
