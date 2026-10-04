/**
 * «O meu ramal» nas Definições → Segurança (R276, item 3.8 do plano de
 * produção): o número do ramal da pessoa em cada organização e o PIN dele.
 *
 * O PIN é da pessoa: gera-o (o servidor sorteia e mostra-o UMA vez) ou
 * escolhe-o aqui. Quem administra só o pode apagar — é assim que força um PIN
 * novo sem o ver. O estado mostra-se sempre; o valor, só no momento em que é
 * gerado.
 *
 * O texto não promete o que não existe: neste lote nenhuma chamada pede o PIN.
 */
import { FormEvent, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { TFunction } from 'i18next'
import {
  apiErrorCode,
  apiErrorMessage,
  ExtensionPinState,
  GeneratedPin,
  getMyExtension,
  MyExtension,
  myOrgs,
  regenerateMyExtensionPin,
  setMyExtensionPin,
} from '../api'
import PinOnce from './PinOnce'
import { Alert, Button, Field, Spinner, StatusBadge, TextInput } from '../ui/kit'

export interface OwnExtension {
  orgId: string
  orgName: string
  extension: MyExtension
}

const PIN_TONE: Record<ExtensionPinState, 'success' | 'neutral' | 'warning'> = {
  set: 'success',
  unset: 'neutral',
  locked: 'warning',
}

/** A mensagem de uma recusa do PIN, pelo código estável do servidor. */
export function myPinErrorMessage(e: unknown, t: TFunction): string {
  switch (apiErrorCode(e)) {
    case 'ramais.pin_format':
      return t('shell.def.ramal.erro.formato')
    case 'ramais.pin_repeated':
      return t('shell.def.ramal.erro.repetido')
    case 'ramais.pin_sequence':
      return t('shell.def.ramal.erro.sequencia')
    case 'ramais.pin_contains_extension':
      return t('shell.def.ramal.erro.contemRamal')
    default:
      return apiErrorMessage(e, t('shell.def.ramal.erro.generico'))
  }
}

export default function MyExtensionPanel() {
  const { t } = useTranslation()
  const [own, setOwn] = useState<OwnExtension[] | null>(null)
  const [erro, setErro] = useState<string | null>(null)

  async function carregar(signal?: AbortSignal) {
    const orgs = await myOrgs(signal)
    const found = await Promise.all(
      orgs.map(async (o) => {
        const extension = await getMyExtension(o.id, signal)
        return extension ? { orgId: o.id, orgName: o.name, extension } : null
      }),
    )
    return found.filter((x): x is OwnExtension => x !== null)
  }

  useEffect(() => {
    const ctrl = new AbortController()
    carregar(ctrl.signal)
      .then((list) => {
        if (!ctrl.signal.aborted) setOwn(list)
      })
      .catch((e) => {
        if (!ctrl.signal.aborted) setErro(apiErrorMessage(e, t('ui.erroCarregar')))
      })
    return () => ctrl.abort()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const recarregar = () =>
    carregar()
      .then(setOwn)
      .catch((e) => setErro(apiErrorMessage(e, t('ui.erroCarregar'))))

  return (
    <div className="my-ext" data-testid="meu-ramal">
      <h3 className="dx-eyebrow" style={{ margin: '8px 0' }}>{t('shell.def.ramal.titulo')}</h3>
      {erro && <Alert tone="danger">{erro}</Alert>}
      {own === null && !erro && <Spinner />}
      {own !== null && <MyExtensionList own={own} onChanged={recarregar} />}
    </div>
  )
}

/** A lista, sem pedidos: o que se desenha a partir do que o servidor disse. */
export function MyExtensionList({ own, onChanged }: { own: OwnExtension[]; onChanged?: () => void }) {
  const { t } = useTranslation()
  if (own.length === 0) {
    return <p className="dx-muted" style={{ margin: 0 }}>{t('shell.def.ramal.semRamal')}</p>
  }
  return (
    <>
      <p className="dx-muted" style={{ margin: '0 0 8px' }}>{t('shell.def.ramal.nota')}</p>
      {own.map((o) => (
        <MyExtensionRow key={o.orgId} own={o} showOrg={own.length > 1} onChanged={onChanged} />
      ))}
    </>
  )
}

function MyExtensionRow({ own, showOrg, onChanged }: { own: OwnExtension; showOrg: boolean; onChanged?: () => void }) {
  const { t } = useTranslation()
  const { extension } = own
  const [busy, setBusy] = useState(false)
  const [generated, setGenerated] = useState<GeneratedPin | null>(null)
  const [choosing, setChoosing] = useState(false)
  const [pin, setPin] = useState('')
  const [msg, setMsg] = useState<{ tone: 'success' | 'danger'; text: string } | null>(null)
  const fieldId = `meu-pin-${own.orgId}`

  async function gerar() {
    setBusy(true)
    setMsg(null)
    setChoosing(false)
    try {
      setGenerated(await regenerateMyExtensionPin(own.orgId))
      onChanged?.()
    } catch (e) {
      setMsg({ tone: 'danger', text: myPinErrorMessage(e, t) })
    } finally {
      setBusy(false)
    }
  }

  async function guardar(e: FormEvent) {
    e.preventDefault()
    if (pin.length !== 6) return
    setBusy(true)
    setMsg(null)
    try {
      await setMyExtensionPin(own.orgId, pin)
      setPin('')
      setChoosing(false)
      setGenerated(null)
      setMsg({ tone: 'success', text: t('shell.def.ramal.guardado') })
      onChanged?.()
    } catch (x) {
      setMsg({ tone: 'danger', text: myPinErrorMessage(x, t) })
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="my-ext__row" data-ramal={extension.extension}>
      <div className="my-ext__head">
        <strong className="dx-num">{t('shell.def.ramal.numero', { numero: extension.extension })}</strong>
        {showOrg && <span className="dx-muted">{own.orgName}</span>}
        {!extension.active && <span className="dx-muted">({t('shell.def.ramal.inactivo')})</span>}
        <span data-pin-state={extension.pin_state}>
          <StatusBadge tone={PIN_TONE[extension.pin_state]}>{t(`shell.def.ramal.estado.${extension.pin_state}`)}</StatusBadge>
        </span>
      </div>
      <div className="my-ext__actions">
        <Button variant="secondary" size="sm" busy={busy} onClick={() => void gerar()}>
          {t('shell.def.ramal.gerar')}
        </Button>
        <Button variant="secondary" size="sm" disabled={busy} onClick={() => setChoosing((v) => !v)} aria-expanded={choosing}>
          {t('shell.def.ramal.escolher')}
        </Button>
      </div>
      {generated && (
        <>
          <Alert tone="warning">{t('shell.def.ramal.revelado')}</Alert>
          <PinOnce generated={generated} />
        </>
      )}
      {choosing && (
        <form onSubmit={guardar} className="my-ext__form">
          <Field label={t('shell.def.ramal.campo')} htmlFor={fieldId} hint={t('shell.def.ramal.regras')}>
            <TextInput
              id={fieldId}
              value={pin}
              onChange={(e) => setPin(e.target.value.replace(/\D/g, '').slice(0, 6))}
              inputMode="numeric"
              autoComplete="off"
              className="dx-num"
            />
          </Field>
          <Button type="submit" variant="primary" size="sm" busy={busy} disabled={pin.length !== 6}>
            {t('shell.def.ramal.guardar')}
          </Button>
        </form>
      )}
      {msg && <Alert tone={msg.tone}>{msg.text}</Alert>}
    </div>
  )
}
