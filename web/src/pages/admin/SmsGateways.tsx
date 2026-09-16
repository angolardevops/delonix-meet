/**
 * Gateways de SMS: cada um é um agente `delonix-sms-gateway` a correr na
 * máquina onde o telefone está ligado. O token só existe na resposta da
 * criação — mostra-se uma vez, com os dois comandos para arrancar o agente.
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { createSmsGateway, listSmsGateways, revokeSmsGateway, SmsGateway } from '../../api'
import { AsyncSection } from '../../components/AsyncSection'
import { Alert, Button, Dialog, IconButton, StatusBadge, TextInput } from '../../ui/kit'
import { formatAgo, orgErrorMessage, useLocaleTag } from './orgShared'
import { CopyButton, SMS_POLL_MS, usePolled } from './smsShared'

const ONCE_CMD = 'delonix-sms-gateway --once'

export function SmsGateways({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const gws = usePolled((signal) => listSmsGateways(orgId, signal), [orgId], SMS_POLL_MS)
  const [name, setName] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [fresh, setFresh] = useState<{ name: string; token: string } | null>(null)
  const [revoking, setRevoking] = useState<SmsGateway | null>(null)

  async function add(e: FormEvent) {
    e.preventDefault()
    if (!name.trim() || busy) return
    setErr('')
    setBusy(true)
    try {
      const g = await createSmsGateway(orgId, name.trim())
      setFresh({ name: g.name, token: g.token })
      setName('')
      gws.refresh()
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'consola.sms.erroPedido'))
    } finally {
      setBusy(false)
    }
  }

  const runCmd = fresh ? `delonix-sms-gateway --server ${window.location.origin} --token ${fresh.token}` : ''

  return (
    <>
      <h3 className="org-voice__sub">{t('consola.sms.gateways')}</h3>
      <p className="dx-muted org-card-note">{t('consola.sms.gatewaysNota')}</p>

      {fresh && (
        <div className="org-card-pad">
          <div className="org-sms__secret" role="status" data-testid="sms-token">
            <strong className="org-sms__secret-title">{t('consola.sms.tokenUmaVez', { nome: fresh.name })}</strong>
            <div className="org-sms__code">
              <code className="dx-num">{fresh.token}</code>
              <CopyButton text={fresh.token} label={t('consola.sms.copiarToken')} />
            </div>
            <span className="dx-muted">{t('consola.sms.arrancar')}</span>
            <div className="org-sms__code">
              <code className="dx-num">{runCmd}</code>
              <CopyButton text={runCmd} label={t('consola.sms.copiarComando')} />
            </div>
            <span className="dx-muted">{t('consola.sms.diagnosticar')}</span>
            <div className="org-sms__code">
              <code className="dx-num">{ONCE_CMD}</code>
              <CopyButton text={ONCE_CMD} label={t('consola.sms.copiarComando')} />
            </div>
            <div>
              <Button size="sm" variant="ghost" icon="check" onClick={() => setFresh(null)}>
                {t('consola.sms.tokenGuardado')}
              </Button>
            </div>
          </div>
        </div>
      )}

      <AsyncSection state={gws.state} onRetry={gws.reload}>
        {(list) =>
          list.length === 0 ? (
            <p className="dx-muted org-card-note">{t('consola.sms.semGateways')}</p>
          ) : (
            <ul className="org-simple" data-testid="sms-gateways">
              {list.map((g) => (
                <li key={g.id}>
                  <span className="org-simple__main">
                    <strong className="org-break">{g.name}</strong>
                    <span className="dx-muted">
                      <span className="dx-num">{g.prefix}…</span>
                      {' · '}
                      {g.last_seen_at
                        ? t('consola.sms.vistoEm', { quando: formatAgo(g.last_seen_at, locale) })
                        : t('consola.sms.nuncaLigado')}
                    </span>
                  </span>
                  <StatusBadge tone={g.online ? 'success' : 'neutral'}>
                    {g.online ? t('consola.sms.online') : t('consola.sms.offline')}
                  </StatusBadge>
                  <IconButton icon="trash" label={t('consola.sms.revogarNome', { nome: g.name })} onClick={() => setRevoking(g)} />
                </li>
              ))}
            </ul>
          )
        }
      </AsyncSection>

      <form className="org-inline org-inline--pair" onSubmit={(e) => void add(e)} aria-label={t('consola.sms.novoGateway')}>
        <TextInput
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder={t('consola.sms.nomeGateway')}
          aria-label={t('consola.sms.nomeGateway')}
          maxLength={80}
          autoComplete="off"
        />
        <Button type="submit" size="sm" icon="plus" busy={busy} disabled={!name.trim()}>
          {t('consola.sms.criarGateway')}
        </Button>
      </form>
      {err && (
        <div className="org-card-pad">
          <Alert tone="danger">{err}</Alert>
        </div>
      )}

      {revoking && (
        <RevokeDialog
          gateway={revoking}
          onClose={() => setRevoking(null)}
          onConfirm={async () => {
            await revokeSmsGateway(orgId, revoking.id)
            gws.refresh()
          }}
        />
      )}
    </>
  )
}

/** O pedido corre dentro do diálogo: se falhar, o erro fica à vista e não fecha. */
function RevokeDialog({ gateway, onClose, onConfirm }: { gateway: SmsGateway; onClose: () => void; onConfirm: () => Promise<void> }) {
  const { t } = useTranslation()
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  async function run() {
    setBusy(true)
    setErr('')
    try {
      await onConfirm()
      onClose()
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'consola.sms.erroPedido'))
      setBusy(false)
    }
  }
  return (
    <Dialog
      title={t('consola.sms.revogarTitulo')}
      onClose={onClose}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t('ui.cancelar')}
          </Button>
          <Button variant="danger" icon="trash" busy={busy} onClick={() => void run()}>
            {t('consola.sms.revogar')}
          </Button>
        </>
      }
    >
      <div className="org-form">
        <p className="org-sms__plain">{t('consola.sms.revogarAviso', { nome: gateway.name })}</p>
        {err && <Alert tone="danger">{err}</Alert>}
      </div>
    </Dialog>
  )
}
