/**
 * Webhooks da organização: destino, tipo, segredo (assina o corpo do
 * `generic` com `X-Delonix-Signature`) e os eventos que o servidor conhece
 * (`webhooks.rs::KNOWN_EVENTS`). Não há registo de entregas no servidor, por
 * isso não há tabela de entregas nem «reenviar».
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, createWebhook, deleteWebhook, listWebhooks, Webhook } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Alert, Button, Card, Checkbox, Empty, Field, IconButton, Select, StatusBadge, TextInput } from '../../ui/kit'
import { ConfirmDialog } from './ConfirmDialog'
import { guarded, IntegHead } from './common'

type Kind = Webhook['kind']
const KINDS: Kind[] = ['slack', 'mattermost', 'teams', 'generic']
/** Espelho de `KNOWN_EVENTS` no servidor — um nome fora daqui é recusado lá. */
const EVENTS = ['meeting.created', 'meeting.started', 'meeting.mom_ready', 'recording.ready'] as const

function splitEvents(s: string): string[] {
  return s
    .split(',')
    .map((e) => e.trim())
    .filter(Boolean)
}

export function WebhooksCard({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  const { state, reload, mutate } = useAsync(() => guarded(listWebhooks(orgId)), [orgId])
  const [removing, setRemoving] = useState<Webhook | null>(null)
  const count = state.s === 'ready' && !state.d.forbidden ? state.d.d.length : null
  return (
    <Card className="integ-card integ-card--wide">
      <IntegHead
        icon="share"
        title={t('integrations.webhooks.titulo')}
        sub={count == null ? t('integrations.webhooks.sub') : t('integrations.webhooks.contagem', { count })}
      />
      <AsyncSection state={state} onRetry={reload}>
        {(g) =>
          g.forbidden ? (
            <Alert tone="warning">{t('integrations.semPermissaoOrg')}</Alert>
          ) : (
            <div className="integ-split">
              <WebhookForm orgId={orgId} onCreated={(h) => mutate((d) => (d.forbidden ? d : { ...d, d: [...d.d, h] }))} />
              <div className="integ-split__list">
                {g.d.length === 0 ? (
                  <Empty icon="share" title={t('integrations.webhooks.vazio')}>
                    {t('integrations.webhooks.vazioDica')}
                  </Empty>
                ) : (
                  <div className="dx-table-wrap integ-table">
                    <table className="dx-table">
                      <thead>
                        <tr>
                          <th>{t('integrations.webhooks.colTipo')}</th>
                          <th>{t('integrations.webhooks.colDestino')}</th>
                          <th>{t('integrations.webhooks.colEventos')}</th>
                          <th>{t('integrations.webhooks.colEstado')}</th>
                          <th>
                            <span className="dx-sr-only">{t('integrations.colAccoes')}</span>
                          </th>
                        </tr>
                      </thead>
                      <tbody>
                        {g.d.map((h) => (
                          <tr key={h.id}>
                            <td>{t(`integrations.webhooks.tipo.${h.kind}`)}</td>
                            <td className="integ-url dx-num" title={h.url}>
                              {h.url}
                            </td>
                            <td>
                              <span className="integ-events">
                                {splitEvents(h.events).map((e) => (
                                  <code key={e} className="dx-num">
                                    {e}
                                  </code>
                                ))}
                              </span>
                            </td>
                            <td>
                              {h.active ? (
                                <StatusBadge tone="success">{t('integrations.webhooks.activo')}</StatusBadge>
                              ) : (
                                <StatusBadge tone="neutral">{t('integrations.webhooks.inactivo')}</StatusBadge>
                              )}
                            </td>
                            <td className="integ-row-actions">
                              <IconButton icon="trash" label={t('integrations.webhooks.eliminar')} onClick={() => setRemoving(h)} />
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )}
              </div>
            </div>
          )
        }
      </AsyncSection>
      {removing && (
        <ConfirmDialog
          title={t('integrations.webhooks.eliminarTitulo')}
          confirmLabel={t('ui.eliminar')}
          onConfirm={async () => {
            await deleteWebhook(orgId, removing.id)
            mutate((d) => (d.forbidden ? d : { ...d, d: d.d.filter((x) => x.id !== removing.id) }))
          }}
          onClose={() => setRemoving(null)}
        >
          <span className="integ-url dx-num">{removing.url}</span>
        </ConfirmDialog>
      )}
    </Card>
  )
}

function WebhookForm({ orgId, onCreated }: { orgId: string; onCreated: (h: Webhook) => void }) {
  const { t } = useTranslation()
  const [kind, setKind] = useState<Kind>('generic')
  const [url, setUrl] = useState('')
  const [secret, setSecret] = useState('')
  const [events, setEvents] = useState<string[]>([...EVENTS])
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  async function submit(e: FormEvent) {
    e.preventDefault()
    setBusy(true)
    setErr('')
    try {
      const h = await createWebhook(orgId, {
        kind,
        url: url.trim(),
        secret: kind === 'generic' && secret ? secret : undefined,
        events: events.join(','),
      })
      onCreated(h)
      setUrl('')
      setSecret('')
    } catch (ex) {
      setErr(apiErrorMessage(ex, t('integrations.erroGuardar')))
    } finally {
      setBusy(false)
    }
  }

  function toggle(ev: string, on: boolean) {
    setEvents((cur) => (on ? EVENTS.filter((x) => x === ev || cur.includes(x)) : cur.filter((x) => x !== ev)))
  }

  return (
    <form className="integ-stack integ-split__form" onSubmit={(e) => void submit(e)}>
      <h3 className="dx-eyebrow">{t('integrations.webhooks.novo')}</h3>
      <Field label={t('integrations.webhooks.tipoLabel')} htmlFor="wh-kind">
        <Select id="wh-kind" value={kind} onChange={(e) => setKind(e.target.value as Kind)}>
          {KINDS.map((k) => (
            <option key={k} value={k}>
              {t(`integrations.webhooks.tipo.${k}`)}
            </option>
          ))}
        </Select>
      </Field>
      <Field label={t('integrations.webhooks.url')} htmlFor="wh-url" hint={t('integrations.webhooks.urlDica')}>
        <TextInput id="wh-url" type="url" inputMode="url" required className="dx-num" value={url} onChange={(e) => setUrl(e.target.value)} autoComplete="off" />
      </Field>
      {kind === 'generic' && (
        <Field label={t('integrations.webhooks.segredo')} htmlFor="wh-secret" hint={t('integrations.webhooks.segredoDica')}>
          <TextInput id="wh-secret" type="password" value={secret} onChange={(e) => setSecret(e.target.value)} autoComplete="new-password" />
        </Field>
      )}
      <fieldset className="integ-events-pick">
        <legend className="dx-field__label">{t('integrations.webhooks.eventos')}</legend>
        {EVENTS.map((ev) => (
          <div key={ev} className="integ-event-row">
            <Checkbox
              label={<code className="dx-num">{ev}</code>}
              checked={events.includes(ev)}
              onChange={(e) => toggle(ev, e.target.checked)}
            />
            <span className="dx-muted integ-small">{t(`integrations.webhooks.evento.${ev.replace('.', '_')}`)}</span>
          </div>
        ))}
        {events.length === 0 && <span className="dx-field__error">{t('integrations.webhooks.escolheEvento')}</span>}
      </fieldset>
      {err && <Alert tone="danger">{err}</Alert>}
      <div className="integ-actions">
        <Button type="submit" variant="primary" icon="plus" busy={busy} disabled={!url.trim() || events.length === 0}>
          {t('integrations.webhooks.adicionar')}
        </Button>
      </div>
    </form>
  )
}
