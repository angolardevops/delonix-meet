/**
 * Criar e editar uma operadora (tronco SIP). O mesmo formulário serve o
 * «Novo tronco SIP», o «Editar» e o último passo do assistente de operadora
 * móvel — todos acabam em `createTrunk`/`updateTrunk`.
 *
 * A password é só de escrita (R214): nunca vem do servidor, o campo começa
 * vazio e vazio ao editar quer dizer «manter».
 */
import { FormEvent, useId, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { createTrunk, updateTrunk } from '../../api'
import type { Trunk } from '../../api'
import { Alert, Button, Dialog, Field, Select, TextInput, Toggle } from '../../ui/kit'
import { useTelecomText } from './shared'
import {
  createBody,
  CURRENCIES,
  emptyTrunkForm,
  formFromTrunk,
  isCleartext,
  MAX_NAME,
  patchBody,
  SCOPES,
  SRTP_MODES,
  srtpNeedsTls,
  TRANSPORTS,
  trunkErrorField,
  validateTrunkForm,
} from './trunkForm'
import type { TrunkField, TrunkForm } from './trunkForm'

/** A ordem dos campos no ecrã — é por ela que o foco vai ao primeiro erro. */
const FIELD_ORDER: TrunkField[] = ['name', 'short_code', 'host', 'port', 'transport', 'srtp', 'scope', 'prefixes', 'max_channels', 'username', 'password', 'price']

/** Valores conhecidos e, se o servidor mandou um que a consola não conhece, esse também. */
function withCurrent(known: readonly string[], current: string): string[] {
  return known.includes(current) ? [...known] : [...known, current]
}

export function TrunkEditor({
  orgId,
  trunk,
  initial,
  prefixesSuggested,
  onDone,
  onCancel,
  cancelLabel,
}: {
  orgId: string
  /** Presente = editar; ausente = criar. */
  trunk?: Trunk
  initial: TrunkForm
  /** Os prefixos vieram de uma sugestão do assistente: diz-se que são por confirmar. */
  prefixesSuggested?: boolean
  onDone: (saved: Trunk) => void
  onCancel: () => void
  cancelLabel?: string
}) {
  const { t } = useTranslation()
  const { label, failure } = useTelecomText()
  const uid = useId()
  const mode = trunk ? 'edit' : 'create'
  const [form, setForm] = useState<TrunkForm>(initial)
  const [tried, setTried] = useState(false)
  const [busy, setBusy] = useState(false)
  const [server, setServer] = useState<{ field: TrunkField | null; msg: string } | null>(null)

  const local = validateTrunkForm(form, mode)
  const set = <K extends keyof TrunkForm>(k: K, v: TrunkForm[K]) => {
    setForm((f) => ({ ...f, [k]: v }))
    setServer(null)
  }
  const id = (f: string) => `${uid}-${f}`
  const focusField = (f: TrunkField) => document.getElementById(id(f))?.focus()

  /** O erro de um campo: o do servidor, ou o local (o de SRTP mostra-se logo, sem esperar pelo envio). */
  const errorOf = (f: TrunkField): string | undefined => {
    if (server?.field === f) return server.msg
    const e = local[f]
    if (!e || (!tried && f !== 'srtp')) return undefined
    return t(`telecom.form.erro.${e.key}`, e.vars)
  }

  function setTransport(transport: string) {
    setForm((f) => {
      // Só a porta por omissão acompanha o transporte; uma porta escrita à mão fica.
      const port = transport === 'tls' && f.port === '5060' ? '5061' : transport !== 'tls' && f.port === '5061' ? '5060' : f.port
      return { ...f, transport, port }
    })
    setServer(null)
  }

  async function submit(e: FormEvent) {
    e.preventDefault()
    setTried(true)
    const primeiro = FIELD_ORDER.find((f) => local[f])
    if (primeiro) {
      focusField(primeiro)
      return
    }
    setBusy(true)
    setServer(null)
    try {
      if (trunk) {
        const body = patchBody(trunk, form)
        // Nada mudou: não há pedido a fazer.
        onDone(Object.keys(body).length === 0 ? trunk : await updateTrunk(orgId, trunk.id, body))
      } else {
        onDone(await createTrunk(orgId, createBody(form)))
      }
    } catch (x) {
      const field = trunkErrorField(x)
      setServer({ field, msg: failure(x) })
      if (field) focusField(field)
    } finally {
      setBusy(false)
    }
  }

  const invalid = (f: TrunkField) => (errorOf(f) ? true : undefined)

  return (
    <form className="tel-form" onSubmit={submit} noValidate>
      <div className="tel-form__grid">
        <Field label={t('telecom.form.nome')} htmlFor={id('name')} error={errorOf('name')}>
          <TextInput id={id('name')} value={form.name} maxLength={MAX_NAME} onChange={(e) => set('name', e.target.value)} aria-invalid={invalid('name')} required />
        </Field>
        <Field label={t('telecom.form.sigla')} htmlFor={id('short_code')} error={errorOf('short_code')} hint={t('telecom.form.siglaAjuda')}>
          <TextInput
            id={id('short_code')}
            className="dx-num"
            value={form.short_code}
            maxLength={4}
            onChange={(e) => set('short_code', e.target.value.toUpperCase())}
            aria-invalid={invalid('short_code')}
            required
          />
        </Field>
      </div>

      <div className="tel-form__grid">
        <Field label={t('telecom.form.host')} htmlFor={id('host')} error={errorOf('host')} hint={t('telecom.form.hostAjuda')}>
          <TextInput
            id={id('host')}
            className="dx-num"
            value={form.host}
            onChange={(e) => set('host', e.target.value)}
            aria-invalid={invalid('host')}
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
            required
          />
        </Field>
        <Field label={t('telecom.form.porta')} htmlFor={id('port')} error={errorOf('port')}>
          <TextInput id={id('port')} className="dx-num" inputMode="numeric" value={form.port} onChange={(e) => set('port', e.target.value)} aria-invalid={invalid('port')} />
        </Field>
      </div>

      <div className="tel-form__grid">
        <Field label={t('telecom.form.transporte')} htmlFor={id('transport')} error={errorOf('transport')}>
          <Select id={id('transport')} value={form.transport} onChange={(e) => setTransport(e.target.value)}>
            {withCurrent(TRANSPORTS, form.transport).map((v) => (
              <option key={v} value={v}>
                {v.toUpperCase()}
              </option>
            ))}
          </Select>
        </Field>
        <Field label={t('telecom.form.srtp')} htmlFor={id('srtp')} error={errorOf('srtp')}>
          <Select id={id('srtp')} value={form.srtp} onChange={(e) => set('srtp', e.target.value)} aria-invalid={invalid('srtp')}>
            {withCurrent(SRTP_MODES, form.srtp).map((v) => (
              <option key={v} value={v}>
                {label('srtp', v)}
              </option>
            ))}
          </Select>
        </Field>
        <Field label={t('telecom.form.ambito')} htmlFor={id('scope')} error={errorOf('scope')}>
          <Select id={id('scope')} value={form.scope} onChange={(e) => set('scope', e.target.value)}>
            {withCurrent(SCOPES, form.scope).map((v) => (
              <option key={v} value={v}>
                {label('ambito', v)}
              </option>
            ))}
          </Select>
        </Field>
      </div>
      {isCleartext(form.transport, form.srtp) && !srtpNeedsTls(form.transport, form.srtp) && (
        <Alert tone="warning" icon="lock">
          {t('telecom.form.redePrivada')}
        </Alert>
      )}

      <div className="tel-form__grid">
        <Field
          label={t('telecom.form.prefixos')}
          htmlFor={id('prefixes')}
          error={errorOf('prefixes')}
          hint={prefixesSuggested ? t('telecom.assistente.prefixosSugestao') : t('telecom.form.prefixosAjuda')}
        >
          <TextInput id={id('prefixes')} className="dx-num" value={form.prefixes} onChange={(e) => set('prefixes', e.target.value)} aria-invalid={invalid('prefixes')} />
        </Field>
        <Field label={t('telecom.form.canais')} htmlFor={id('max_channels')} error={errorOf('max_channels')} hint={t('telecom.form.canaisAjuda')}>
          <TextInput
            id={id('max_channels')}
            className="dx-num"
            inputMode="numeric"
            value={form.max_channels}
            onChange={(e) => set('max_channels', e.target.value)}
            aria-invalid={invalid('max_channels')}
            required
          />
        </Field>
      </div>

      <Toggle label={t('telecom.form.registo')} hint={t('telecom.form.registoAjuda')} checked={form.register} onChange={(e) => set('register', e.target.checked)} />

      <div className="tel-form__grid">
        <Field label={t('telecom.form.utilizador')} htmlFor={id('username')} error={errorOf('username')} hint={t('telecom.form.opcional')}>
          <TextInput
            id={id('username')}
            className="dx-num"
            value={form.username}
            onChange={(e) => set('username', e.target.value)}
            aria-invalid={invalid('username')}
            autoComplete="off"
            autoCapitalize="none"
            spellCheck={false}
          />
        </Field>
        <Field
          label={t('telecom.form.password')}
          htmlFor={id('password')}
          error={errorOf('password')}
          hint={!trunk ? t('telecom.form.opcional') : trunk.password_configured ? t('telecom.form.passwordManter') : t('telecom.form.passwordSem')}
        >
          <TextInput
            id={id('password')}
            type="password"
            value={form.password}
            onChange={(e) => set('password', e.target.value)}
            aria-invalid={invalid('password')}
            autoComplete="new-password"
          />
        </Field>
      </div>

      {!trunk && (
        <div className="tel-form__grid">
          <Field label={t('telecom.form.preco')} htmlFor={id('price')} error={errorOf('price')} hint={t('telecom.form.precoAjuda')}>
            <TextInput
              id={id('price')}
              className="dx-num"
              inputMode="decimal"
              value={form.price_amount}
              onChange={(e) => set('price_amount', e.target.value)}
              aria-invalid={invalid('price')}
            />
          </Field>
          <Field label={t('telecom.form.moeda')} htmlFor={id('currency')}>
            <Select id={id('currency')} value={form.price_currency} onChange={(e) => set('price_currency', e.target.value)}>
              {CURRENCIES.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </Select>
          </Field>
        </div>
      )}

      <Toggle label={t('telecom.form.activa')} hint={t('telecom.form.activaAjuda')} checked={form.enabled} onChange={(e) => set('enabled', e.target.checked)} />

      {server && !server.field && <Alert tone="danger">{server.msg}</Alert>}
      <div className="tel-form__foot">
        <Button variant="secondary" onClick={onCancel} disabled={busy}>
          {cancelLabel ?? t('ui.cancelar')}
        </Button>
        <Button type="submit" variant="primary" busy={busy}>
          {trunk ? t('ui.guardar') : t('telecom.form.criar')}
        </Button>
      </div>
    </form>
  )
}

export default function TrunkDialog({ orgId, trunk, onClose, onDone }: { orgId: string; trunk?: Trunk; onClose: () => void; onDone: (saved: Trunk) => void }) {
  const { t } = useTranslation()
  return (
    <Dialog title={trunk ? t('telecom.form.tituloEditar', { nome: trunk.name }) : t('telecom.form.tituloNovo')} onClose={onClose} wide>
      <TrunkEditor orgId={orgId} trunk={trunk} initial={trunk ? formFromTrunk(trunk) : emptyTrunkForm()} onDone={onDone} onCancel={onClose} />
    </Dialog>
  )
}
