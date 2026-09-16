/**
 * Integração Odoo (módulo nk_delonix_meet): ligar/desligar, URL e base de
 * dados, token de integração (gerado e rodado aqui, mostrado uma vez), última
 * sincronização e a visibilidade da página de entrada.
 *
 * O badge lê o estado GRAVADO no servidor, nunca o rascunho do formulário:
 * «sincronizada» só aparece quando o Odoo já falou connosco (`odoo_synced_at`).
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, getOdooConfig, getPlatformSettings, OdooConfig, rotateOdooToken, saveOdooConfig } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Alert, Button, Card, Field, StatusBadge, Tag, TextInput, Toggle } from '../../ui/kit'
import { ConfirmDialog } from './ConfirmDialog'
import { guarded, IntegHead, SecretOnce, useDateFmt } from './common'

function OdooBadge({ cfg }: { cfg: OdooConfig }) {
  const { t } = useTranslation()
  if (!cfg.odoo_enabled) return <StatusBadge tone="neutral">{t('integrations.odoo.estadoDesactivada')}</StatusBadge>
  if (cfg.odoo_synced_at)
    return (
      <StatusBadge tone="success" icon="check">
        {t('integrations.odoo.estadoSincronizada')}
      </StatusBadge>
    )
  return <StatusBadge tone="warning">{t('integrations.odoo.estadoPorSincronizar')}</StatusBadge>
}

export function OdooCard({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  const { state, reload } = useAsync(() => guarded(getOdooConfig(orgId)), [orgId])
  const saved = state.s === 'ready' && !state.d.forbidden ? state.d.d : null
  return (
    <Card className="integ-card">
      <IntegHead
        mark="O"
        title={t('integrations.odoo.titulo')}
        sub={saved?.odoo_url ? [saved.odoo_url, saved.odoo_db].filter(Boolean).join(' · ') : t('integrations.odoo.sub')}
        badge={saved && <OdooBadge cfg={saved} />}
      />
      <AsyncSection state={state} onRetry={reload}>
        {(g) =>
          g.forbidden ? (
            <Alert tone="warning">{t('integrations.semPermissaoOrg')}</Alert>
          ) : (
            <OdooForm key={orgId} orgId={orgId} initial={g.d} onSaved={reload} />
          )
        }
      </AsyncSection>
    </Card>
  )
}

function OdooForm({ orgId, initial, onSaved }: { orgId: string; initial: OdooConfig; onSaved: () => void }) {
  const { t } = useTranslation()
  const fmt = useDateFmt()
  const [enabled, setEnabled] = useState(initial.odoo_enabled)
  const [url, setUrl] = useState(initial.odoo_url ?? '')
  const [db, setDb] = useState(initial.odoo_db ?? '')
  const [hideOrg, setHideOrg] = useState(initial.hide_org_creation)
  const [hideSso, setHideSso] = useState(initial.hide_sso_button)
  const [prefix, setPrefix] = useState(initial.odoo_token_prefix)
  const [token, setToken] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [rotating, setRotating] = useState(false)
  const [confirmRotate, setConfirmRotate] = useState(false)
  const [err, setErr] = useState('')
  const [ok, setOk] = useState(false)
  const pub = useAsync(() => getPlatformSettings(), [orgId, initial])

  async function save() {
    setBusy(true)
    setErr('')
    setOk(false)
    try {
      await saveOdooConfig(orgId, {
        odoo_enabled: enabled,
        odoo_url: url.trim() || null,
        odoo_db: db.trim() || null,
        hide_org_creation: hideOrg,
        hide_sso_button: hideSso,
      })
      setOk(true)
      onSaved()
    } catch (e) {
      setErr(apiErrorMessage(e, t('integrations.erroGuardar')))
    } finally {
      setBusy(false)
    }
  }

  async function rotate() {
    const r = await rotateOdooToken(orgId)
    setToken(r.token)
    setPrefix(r.prefix)
  }

  async function generate() {
    setRotating(true)
    setErr('')
    try {
      await rotate()
    } catch (e) {
      setErr(apiErrorMessage(e, t('integrations.erroGuardar')))
    } finally {
      setRotating(false)
    }
  }

  return (
    <div className="integ-stack">
      <p className="integ-desc">{t('integrations.odoo.descricao')}</p>

      <Toggle label={t('integrations.odoo.activar')} checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />

      <div className="integ-grid2">
        <Field label={t('integrations.odoo.url')} htmlFor="odoo-url">
          <TextInput id="odoo-url" type="url" inputMode="url" value={url} onChange={(e) => setUrl(e.target.value)} autoComplete="off" />
        </Field>
        <Field label={t('integrations.odoo.baseDados')} htmlFor="odoo-db">
          <TextInput id="odoo-db" value={db} onChange={(e) => setDb(e.target.value)} autoComplete="off" />
        </Field>
      </div>

      <section className="integ-panel" aria-labelledby="odoo-token-h">
        <div className="integ-panel__head">
          <h3 id="odoo-token-h" className="dx-eyebrow">
            {t('integrations.odoo.token')}
          </h3>
          <span className="dx-spacer" />
          {prefix ? (
            <Button size="sm" icon="refresh" busy={rotating} onClick={() => setConfirmRotate(true)}>
              {t('integrations.odoo.rodarToken')}
            </Button>
          ) : (
            <Button size="sm" variant="outline" icon="key" busy={rotating} onClick={() => void generate()}>
              {t('integrations.odoo.gerarToken')}
            </Button>
          )}
        </div>
        {prefix ? (
          <code className="dx-num integ-prefix">{t('integrations.prefixo', { prefixo: prefix })}</code>
        ) : (
          <span className="dx-muted integ-small">{t('integrations.odoo.semToken')}</span>
        )}
        {token && <SecretOnce value={token} note={t('integrations.odoo.tokenUmaVez')} />}
      </section>

      <div className="integ-panel">
        <div className="dx-eyebrow">{t('integrations.odoo.ultimaSync')}</div>
        <span className="dx-num integ-small">
          {initial.odoo_synced_at ? fmt(initial.odoo_synced_at) : t('integrations.odoo.nuncaSincronizou')}
        </span>
      </div>

      <section className="integ-stack" aria-labelledby="odoo-vis-h">
        <h3 id="odoo-vis-h" className="dx-eyebrow">
          {t('integrations.odoo.visibilidade')}
        </h3>
        <Toggle
          label={t('integrations.odoo.ocultarCriarOrg')}
          hint={t('integrations.odoo.ocultarCriarOrgDica')}
          checked={hideOrg}
          onChange={(e) => setHideOrg(e.target.checked)}
        />
        <Toggle
          label={t('integrations.odoo.ocultarSso')}
          hint={t('integrations.odoo.ocultarSsoDica')}
          checked={hideSso}
          onChange={(e) => setHideSso(e.target.checked)}
        />
        {pub.state.s === 'ready' && (
          <div className="integ-effective">
            <span className="dx-muted">{t('integrations.odoo.emVigor')}</span>
            <span className="dx-chips">
              <Tag>{pub.state.d.hide_org_creation ? t('integrations.odoo.criarContaOculto') : t('integrations.odoo.criarContaVisivel')}</Tag>
              <Tag>{pub.state.d.hide_sso_button ? t('integrations.odoo.ssoOculto') : t('integrations.odoo.ssoVisivel')}</Tag>
            </span>
            <span className="dx-muted integ-small">{t('integrations.odoo.emVigorDica')}</span>
          </div>
        )}
      </section>

      {err && <Alert tone="danger">{err}</Alert>}
      {ok && <Alert tone="success">{t('integrations.guardado')}</Alert>}
      <div className="integ-actions">
        <Button variant="primary" busy={busy} onClick={() => void save()}>
          {t('ui.guardar')}
        </Button>
      </div>

      {confirmRotate && (
        <ConfirmDialog
          title={t('integrations.odoo.rodarTitulo')}
          confirmLabel={t('integrations.odoo.rodarToken')}
          onConfirm={rotate}
          onClose={() => setConfirmRotate(false)}
        >
          {t('integrations.odoo.rodarAviso')}
        </ConfirmDialog>
      )}
    </div>
  )
}
