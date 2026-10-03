/**
 * Preços de uma operadora: o histórico (o que está em vigor marcado) e «novo
 * preço» com data de início. Um preço nunca se edita nem se apaga — o custo de
 * uma chamada já feita não muda (R212); muda-se o preço acrescentando outro.
 *
 * O servidor recusa um início no passado (`telephony.price_backdated`) e dois
 * preços com o mesmo início (`telephony.price_exists`).
 */
import { FormEvent, useId, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { createTrunkPrice, listTrunkPrices } from '../../api'
import type { Trunk, TrunkPrice } from '../../api'
import { AsyncSection } from '../../components/AsyncSection'
import { Alert, Button, Dialog, Field, Select, StatusBadge, TextInput } from '../../ui/kit'
import { formatDateTime, useLocaleTag } from '../admin/orgShared'
import { formatMoney } from './format'
import { LoadMore, useTelecomText } from './shared'
import { CURRENCIES, dateOrAmountField, errorCode, isAmountText, localToIso, moneyFrom } from './trunkForm'
import { usePaged } from './usePaged'

type Campo = 'amount' | 'valid_from'

function PriceState({ price }: { price: TrunkPrice }) {
  const { t } = useTranslation()
  if (price.in_force)
    return (
      <StatusBadge tone="success" icon="check">
        {t('telecom.precos.emVigor')}
      </StatusBadge>
    )
  const futuro = new Date(price.valid_from).getTime() > Date.now()
  return <span className="dx-muted">{futuro ? t('telecom.precos.agendado') : t('telecom.precos.anterior')}</span>
}

export default function PricesDialog({ orgId, trunk, onClose, onChanged }: { orgId: string; trunk: Trunk; onClose: () => void; onChanged: () => void }) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const { failure } = useTelecomText()
  const uid = useId()
  const prices = usePaged((token, signal) => listTrunkPrices(orgId, trunk.id, { page_token: token }, signal), [orgId, trunk.id])
  const [amount, setAmount] = useState('')
  const [currency, setCurrency] = useState<string>(trunk.current_price_per_min?.currency ?? 'AOA')
  const [from, setFrom] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<{ field: Campo | null; msg: string } | null>(null)

  async function submit(e: FormEvent) {
    e.preventDefault()
    const focus = (c: Campo) => document.getElementById(`${uid}-${c}`)?.focus()
    if (!isAmountText(amount)) {
      setErr({ field: 'amount', msg: t('telecom.form.erro.preco') })
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
      await createTrunkPrice(orgId, trunk.id, { price_per_min: moneyFrom(amount, currency), valid_from: validFrom })
      setAmount('')
      setFrom('')
      prices.reload()
      onChanged()
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
    <Dialog title={t('telecom.precos.titulo', { nome: trunk.name })} onClose={onClose} wide>
      <AsyncSection state={prices.state} onRetry={prices.reload}>
        {(d) =>
          d.items.length === 0 ? (
            <p className="dx-muted tel-small">{t('telecom.precos.vazio')}</p>
          ) : (
            <>
              <div className="dx-table-wrap">
                <table className="dx-table tel-table" data-testid="tel-prices">
                  <thead>
                    <tr>
                      <th scope="col">{t('telecom.precos.colPreco')}</th>
                      <th scope="col">{t('telecom.precos.colDesde')}</th>
                      <th scope="col">{t('telecom.precos.colEstado')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {d.items.map((p) => (
                      <tr key={p.id}>
                        <th scope="row" className="dx-num">
                          {formatMoney(p.price_per_min, locale)}
                        </th>
                        <td className="dx-num">{formatDateTime(p.valid_from, locale)}</td>
                        <td>
                          <PriceState price={p} />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              <LoadMore next={d.next} busy={prices.busy} err={prices.err} onMore={prices.loadMore} />
            </>
          )
        }
      </AsyncSection>

      <form className="tel-form tel-form--sep" onSubmit={submit} noValidate>
        <h3 className="tel-form__title">{t('telecom.precos.novo')}</h3>
        <p className="dx-muted tel-small">{t('telecom.precos.nota')}</p>
        <div className="tel-form__grid">
          <Field label={t('telecom.form.preco')} htmlFor={`${uid}-amount`} error={errOf('amount')} hint={t('telecom.precos.precoAjuda')}>
            <TextInput
              id={`${uid}-amount`}
              className="dx-num"
              inputMode="decimal"
              value={amount}
              onChange={(e) => {
                setAmount(e.target.value)
                setErr(null)
              }}
              aria-invalid={errOf('amount') ? true : undefined}
              required
            />
          </Field>
          <Field label={t('telecom.form.moeda')} htmlFor={`${uid}-currency`}>
            <Select id={`${uid}-currency`} value={currency} onChange={(e) => setCurrency(e.target.value)}>
              {CURRENCIES.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </Select>
          </Field>
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
        </div>
        {err && !err.field && <Alert tone="danger">{err.msg}</Alert>}
        <div className="tel-form__foot">
          <Button variant="secondary" onClick={onClose} disabled={busy}>
            {t('ui.fechar')}
          </Button>
          <Button type="submit" variant="primary" busy={busy}>
            {t('telecom.precos.acrescentar')}
          </Button>
        </div>
      </form>
    </Dialog>
  )
}
