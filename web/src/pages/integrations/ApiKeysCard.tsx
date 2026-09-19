/**
 * Chaves da API REST (`/api/v1`, header `X-API-Key`). A chave completa só
 * existe na resposta da criação: mostra-se uma vez e não volta.
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiKeyInfo, apiErrorMessage, createApiKey, listApiKeys, revokeApiKey } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Alert, Button, Card, Empty, Field, IconButton, TextInput } from '../../ui/kit'
import ListSearch from '../../ui/search/ListSearch'
import { ConfirmDialog } from './ConfirmDialog'
import { apiKeysSource } from './search'
import { guarded, IntegHead, SecretOnce, useDateFmt } from './common'

export function ApiKeysCard({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  const fmt = useDateFmt()
  const { state, reload, mutate } = useAsync(() => guarded(listApiKeys(orgId)), [orgId])
  const [name, setName] = useState('')
  const [fresh, setFresh] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [revoking, setRevoking] = useState<ApiKeyInfo | null>(null)

  async function create(e: FormEvent) {
    e.preventDefault()
    setBusy(true)
    setErr('')
    try {
      const k = await createApiKey(orgId, name.trim())
      setFresh(k.key)
      setName('')
      reload()
    } catch (ex) {
      setErr(apiErrorMessage(ex, t('integrations.erroGuardar')))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card className="integ-card">
      <IntegHead
        icon="key"
        title={t('integrations.apiKeys.titulo')}
        sub={t('integrations.apiKeys.sub')}
        badge={
          <a className="dx-btn dx-btn--ghost dx-btn--sm" href="#/api-docs" target="_blank" rel="noreferrer">
            {t('integrations.apiKeys.documentacao')}
          </a>
        }
      />
      <AsyncSection state={state} onRetry={reload}>
        {(g) =>
          g.forbidden ? (
            <Alert tone="warning">{t('integrations.semPermissaoOrg')}</Alert>
          ) : (
            <div className="integ-stack">
              {fresh && <SecretOnce value={fresh} note={t('integrations.apiKeys.umaVez')} />}
              {g.d.length === 0 ? (
                <Empty icon="key" title={t('integrations.apiKeys.vazio')} />
              ) : (
                <ListSearch
                  rows={g.d}
                  source={apiKeysSource}
                  ns="keys."
                  label={t('search.rotulos.api_keys')}
                  emptyIcon="key"
                  emptyTitle={t('integrations.apiKeys.vazio')}
                  className="integ-search"
                  renderItems={(rows) => (
                <div className="dx-table-wrap integ-table">
                  <table className="dx-table">
                    <thead>
                      <tr>
                        <th>{t('integrations.apiKeys.colNome')}</th>
                        <th>{t('integrations.apiKeys.colPrefixo')}</th>
                        <th>{t('integrations.apiKeys.colCriada')}</th>
                        <th>{t('integrations.apiKeys.colUsada')}</th>
                        <th>
                          <span className="dx-sr-only">{t('integrations.colAccoes')}</span>
                        </th>
                      </tr>
                    </thead>
                    <tbody>
                      {rows.map((k) => (
                        <tr key={k.id}>
                          <td>{k.name || <span className="dx-muted">{t('integrations.apiKeys.semNome')}</span>}</td>
                          <td className="dx-num">{t('integrations.prefixo', { prefixo: k.prefix })}</td>
                          <td className="dx-num dx-muted">{fmt(k.created_at)}</td>
                          <td className="dx-num dx-muted">{k.last_used_at ? fmt(k.last_used_at) : t('integrations.apiKeys.nunca')}</td>
                          <td className="integ-row-actions">
                            <IconButton icon="trash" label={t('integrations.apiKeys.revogar')} onClick={() => setRevoking(k)} />
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
                  )}
                />
              )}
              <form className="integ-inline-form" onSubmit={(e) => void create(e)}>
                <Field label={t('integrations.apiKeys.nome')} htmlFor="apikey-name">
                  <TextInput id="apikey-name" value={name} onChange={(e) => setName(e.target.value)} autoComplete="off" />
                </Field>
                <Button type="submit" variant="primary" icon="plus" busy={busy}>
                  {t('integrations.apiKeys.criar')}
                </Button>
              </form>
              {err && <Alert tone="danger">{err}</Alert>}
            </div>
          )
        }
      </AsyncSection>
      {revoking && (
        <ConfirmDialog
          title={t('integrations.apiKeys.revogarTitulo')}
          confirmLabel={t('integrations.apiKeys.revogar')}
          onConfirm={async () => {
            await revokeApiKey(orgId, revoking.id)
            mutate((d) => (d.forbidden ? d : { ...d, d: d.d.filter((x) => x.id !== revoking.id) }))
          }}
          onClose={() => setRevoking(null)}
        >
          {t('integrations.apiKeys.revogarAviso', { prefixo: revoking.prefix })}
        </ConfirmDialog>
      )}
    </Card>
  )
}
