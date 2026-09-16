/**
 * SSO OIDC da organização: emissor, client id, segredo (nunca devolvido pelo
 * servidor — deixar vazio mantém o actual) e a obrigatoriedade do SSO.
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, deleteSsoConfig, getSsoConfig, saveSsoConfig, SsoConfig } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Alert, Button, Card, Checkbox, Field, StatusBadge, TextInput } from '../../ui/kit'
import { ConfirmDialog } from './ConfirmDialog'
import { guarded, IntegHead } from './common'

export function SsoCard({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  const { state, reload, mutate } = useAsync(() => guarded(getSsoConfig(orgId)), [orgId])
  const [editing, setEditing] = useState(false)
  const [removing, setRemoving] = useState(false)
  const cfg = state.s === 'ready' && !state.d.forbidden ? state.d.d : null
  return (
    <Card className="integ-card">
      <IntegHead
        icon="lock"
        title={t('integrations.sso.titulo')}
        sub={t('integrations.sso.sub')}
        badge={
          state.s === 'ready' && !state.d.forbidden ? (
            cfg ? (
              <StatusBadge tone="success" icon="check">
                {t('integrations.sso.configurado')}
              </StatusBadge>
            ) : (
              <StatusBadge tone="neutral">{t('integrations.sso.naoConfigurado')}</StatusBadge>
            )
          ) : null
        }
      />
      <AsyncSection state={state} onRetry={reload}>
        {(g) => {
          if (g.forbidden) return <Alert tone="warning">{t('integrations.semPermissaoOrg')}</Alert>
          const c = g.d
          if (c && !editing) {
            return (
              <div className="integ-stack">
                <dl className="dx-kv integ-kv">
                  <dt>{t('integrations.sso.emissor')}</dt>
                  <dd className="dx-num">{c.issuer_url}</dd>
                  <dt>{t('integrations.sso.clientId')}</dt>
                  <dd className="dx-num">{c.client_id}</dd>
                  <dt>{t('integrations.sso.obrigatorio')}</dt>
                  <dd>{c.enforce_sso ? t('ui.sim') : t('ui.nao')}</dd>
                </dl>
                <div className="integ-actions">
                  <Button variant="ghost" icon="trash" onClick={() => setRemoving(true)}>
                    {t('integrations.sso.remover')}
                  </Button>
                  <Button icon="edit" onClick={() => setEditing(true)}>
                    {t('ui.editar')}
                  </Button>
                </div>
              </div>
            )
          }
          return (
            <SsoForm
              orgId={orgId}
              initial={c}
              onCancel={c ? () => setEditing(false) : undefined}
              onSaved={() => {
                setEditing(false)
                reload()
              }}
            />
          )
        }}
      </AsyncSection>
      {removing && (
        <ConfirmDialog
          title={t('integrations.sso.removerTitulo')}
          confirmLabel={t('integrations.sso.remover')}
          onConfirm={async () => {
            await deleteSsoConfig(orgId)
            setEditing(false)
            mutate(() => ({ forbidden: false, d: null }))
          }}
          onClose={() => setRemoving(false)}
        >
          {t('integrations.sso.removerAviso')}
        </ConfirmDialog>
      )}
    </Card>
  )
}

function SsoForm({
  orgId,
  initial,
  onCancel,
  onSaved,
}: {
  orgId: string
  initial: SsoConfig | null
  onCancel?: () => void
  onSaved: () => void
}) {
  const { t } = useTranslation()
  const [issuer, setIssuer] = useState(initial?.issuer_url ?? '')
  const [clientId, setClientId] = useState(initial?.client_id ?? '')
  const [secret, setSecret] = useState('')
  const [enforce, setEnforce] = useState(initial?.enforce_sso ?? false)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  async function submit(e: FormEvent) {
    e.preventDefault()
    setBusy(true)
    setErr('')
    try {
      await saveSsoConfig(orgId, { issuer_url: issuer.trim(), client_id: clientId.trim(), client_secret: secret, enforce_sso: enforce })
      setSecret('')
      onSaved()
    } catch (ex) {
      setErr(apiErrorMessage(ex, t('integrations.erroGuardar')))
    } finally {
      setBusy(false)
    }
  }

  return (
    <form className="integ-stack" onSubmit={(e) => void submit(e)}>
      {!initial && <p className="integ-desc">{t('integrations.sso.vazio')}</p>}
      <Field label={t('integrations.sso.emissor')} htmlFor="sso-issuer" hint={t('integrations.sso.emissorDica')}>
        <TextInput id="sso-issuer" type="url" inputMode="url" required className="dx-num" value={issuer} onChange={(e) => setIssuer(e.target.value)} autoComplete="off" />
      </Field>
      <div className="integ-grid2">
        <Field label={t('integrations.sso.clientId')} htmlFor="sso-client">
          <TextInput id="sso-client" required value={clientId} onChange={(e) => setClientId(e.target.value)} autoComplete="off" />
        </Field>
        <Field
          label={t('integrations.sso.segredo')}
          htmlFor="sso-secret"
          hint={initial ? t('integrations.sso.segredoManter') : undefined}
        >
          <TextInput id="sso-secret" type="password" value={secret} onChange={(e) => setSecret(e.target.value)} autoComplete="new-password" />
        </Field>
      </div>
      <Checkbox label={t('integrations.sso.obrigatorioDica')} checked={enforce} onChange={(e) => setEnforce(e.target.checked)} />
      {err && <Alert tone="danger">{err}</Alert>}
      <div className="integ-actions">
        {onCancel && (
          <Button variant="ghost" onClick={onCancel}>
            {t('ui.cancelar')}
          </Button>
        )}
        <Button type="submit" variant="primary" busy={busy} disabled={!issuer.trim() || !clientId.trim()}>
          {t('ui.guardar')}
        </Button>
      </div>
    </form>
  )
}
