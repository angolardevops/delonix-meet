/**
 * Segurança — hoje só a configuração de ENTRADA da plataforma
 * (`GET/PUT /api/operator/v1/login-settings`). A gestão da lista de
 * administradores de plataforma NÃO está aqui: é fixa por variável de
 * ambiente (`PLATFORM_ADMIN_USER_IDS`) e a nota diz isso sem a esconder.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, getLoginSettings, OperatorLoginSettings, saveLoginSettings } from '../api'
import { AsyncSection, useAsync } from '../components/AsyncSection'
import { Alert, Card, Toggle } from '../ui/kit'

export default function Security() {
  const { t } = useTranslation()
  const { state, reload } = useAsync(() => getLoginSettings(), [])
  return (
    <div className="page">
      <Card title={t('loginSettings.titulo')}>
        <AsyncSection state={state} onRetry={reload}>
          {(d) => <LoginSettingsForm initial={d} onSaved={reload} />}
        </AsyncSection>
      </Card>
      <Alert tone="warning" icon="shield">
        {t('security.operadoresNota')}
      </Alert>
    </div>
  )
}

function LoginSettingsForm({ initial, onSaved }: { initial: OperatorLoginSettings; onSaved: () => void }) {
  const { t } = useTranslation()
  const [hideOrgCreation, setHideOrgCreation] = useState(initial.hide_org_creation)
  const [hideSsoButton, setHideSsoButton] = useState(initial.hide_sso_button)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [ok, setOk] = useState(false)

  async function toggle(next: OperatorLoginSettings) {
    setBusy(true)
    setErr('')
    setOk(false)
    try {
      await saveLoginSettings(next)
      setOk(true)
      onSaved()
    } catch (e) {
      setErr(apiErrorMessage(e, t('loginSettings.erroGuardar')))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div style={{ display: 'grid', gap: 14 }} aria-busy={busy || undefined}>
      <Toggle
        label={t('loginSettings.hideOrgCreation')}
        hint={t('loginSettings.hideOrgCreationHint')}
        checked={hideOrgCreation}
        disabled={busy}
        onChange={(e) => {
          const v = e.target.checked
          setHideOrgCreation(v)
          void toggle({ hide_org_creation: v, hide_sso_button: hideSsoButton })
        }}
      />
      <Toggle
        label={t('loginSettings.hideSsoButton')}
        hint={t('loginSettings.hideSsoButtonHint')}
        checked={hideSsoButton}
        disabled={busy}
        onChange={(e) => {
          const v = e.target.checked
          setHideSsoButton(v)
          void toggle({ hide_org_creation: hideOrgCreation, hide_sso_button: v })
        }}
      />
      {err && <Alert tone="danger">{err}</Alert>}
      {ok && <Alert tone="success">{t('loginSettings.guardado')}</Alert>}
    </div>
  )
}
