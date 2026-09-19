/**
 * Ramais internos (`server/src/ramais.rs`): chamada ramal-a-ramal pela rede
 * interna, ligada ao FreeSWITCH que já serve o dial-in PSTN (`voice/`) — mas
 * sem qualquer ligação a esse dial-in. Fase 1 do plano: SÓ interno.
 *
 * O aviso no topo não é decoração — segue a mesma disciplina que `VoiceCard`
 * já aplica à ponte FreeSWITCH↔SFU em falta: um ramal não recebe chamadas do
 * exterior (falta a ponte PSTN↔ramal) nem entra numa sala de vídeo (falta a
 * ponte ramal↔reunião). As duas são fases seguintes do mesmo plano.
 *
 * A password SIP só existe em claro na resposta de criação/regeneração — o
 * mesmo padrão de revelação única que `SmsGatewayCard` já usa para o token de
 * emparelhamento de um gateway.
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  createExtension,
  deleteExtension,
  Employee,
  Extension,
  ExtensionCreated,
  listExtensions,
  regenerateExtensionPassword,
  updateExtension,
} from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Alert, Button, Card, Dialog, Select, StatusBadge, TextInput } from '../../ui/kit'
import { orgErrorMessage, refusalAware } from './orgShared'

export default function ExtensionsCard({ orgId, people }: { orgId: string; people: Employee[] }) {
  const { t } = useTranslation()
  const extensions = useAsync((signal) => refusalAware(listExtensions(orgId, signal), t), [orgId])
  const [creating, setCreating] = useState(false)
  const [reveal, setReveal] = useState<ExtensionCreated | null>(null)
  const [busyId, setBusyId] = useState<string | null>(null)
  const [err, setErr] = useState('')

  const membersWithoutExtension = (list: Extension[]) => {
    const taken = new Set(list.map((e) => e.member_id))
    return people.filter((p) => !taken.has(p.user_id))
  }

  async function toggleActive(e: Extension) {
    setBusyId(e.id)
    setErr('')
    try {
      await updateExtension(orgId, e.id, { active: !e.active })
      extensions.reload()
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'consola.ramais.erroCarregar'))
    } finally {
      setBusyId(null)
    }
  }

  async function regenerate(e: Extension) {
    if (!window.confirm(t('consola.ramais.regenerarConfirmar', { extensao: e.extension }))) return
    setBusyId(e.id)
    setErr('')
    try {
      const created = await regenerateExtensionPassword(orgId, e.id)
      setReveal(created)
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'consola.ramais.erroCarregar'))
    } finally {
      setBusyId(null)
    }
  }

  async function remove(e: Extension) {
    if (!window.confirm(t('consola.ramais.apagarConfirmar', { extensao: e.extension }))) return
    setBusyId(e.id)
    setErr('')
    try {
      await deleteExtension(orgId, e.id)
      extensions.reload()
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'consola.ramais.erroCarregar'))
    } finally {
      setBusyId(null)
    }
  }

  return (
    <Card title={t('consola.ramais.titulo')} eyebrow={t('consola.ramais.eyebrow')} flush className="org-voice" as="section">
      <div className="org-card-pad">
        <Alert tone="warning" icon="phone">
          {t('consola.ramais.aviso')}
        </Alert>
      </div>
      <AsyncSection state={extensions.state} onRetry={extensions.reload}>
        {(list) =>
          list.length === 0 ? (
            <p className="dx-muted org-card-note">{t('consola.ramais.semRamais')}</p>
          ) : (
            <div className="dx-table-wrap org-table-wrap">
              <table className="dx-table org-table" data-testid="ramais-list">
                <thead>
                  <tr>
                    <th scope="col">{t('consola.ramais.colMembro')}</th>
                    <th scope="col">{t('consola.ramais.colExtensao')}</th>
                    <th scope="col">{t('consola.ramais.colRotulo')}</th>
                    <th scope="col">{t('consola.ramais.colEstado')}</th>
                    <th scope="col">
                      <span className="dx-sr-only">{t('consola.voz.registo')}</span>
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {list.map((e) => (
                    <tr key={e.id}>
                      <td>
                        <strong>{e.member_username}</strong>
                        <div className="dx-muted">{e.member_email}</div>
                      </td>
                      <td className="dx-num">{e.extension}</td>
                      <td className="dx-muted">{e.label || '—'}</td>
                      <td>
                        <StatusBadge tone={e.active ? 'success' : 'neutral'}>
                          {e.active ? t('consola.ramais.activo') : t('consola.ramais.inactivo')}
                        </StatusBadge>
                      </td>
                      <td className="org-row-actions">
                        <Button size="sm" variant="secondary" busy={busyId === e.id} onClick={() => toggleActive(e)}>
                          {e.active ? t('consola.ramais.desactivar') : t('consola.ramais.activar')}
                        </Button>
                        <Button size="sm" variant="secondary" busy={busyId === e.id} onClick={() => regenerate(e)}>
                          {t('consola.ramais.regenerar')}
                        </Button>
                        <Button size="sm" variant="danger" busy={busyId === e.id} onClick={() => remove(e)}>
                          {t('consola.ramais.apagar')}
                        </Button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )
        }
      </AsyncSection>
      {err && (
        <div className="org-card-pad">
          <Alert tone="danger">{err}</Alert>
        </div>
      )}
      <div className="org-card-pad">
        <Button variant="primary" size="sm" icon="plus" onClick={() => setCreating(true)}>
          {t('consola.ramais.novo')}
        </Button>
      </div>
      {creating && (
        <NewExtensionDialog
          orgId={orgId}
          candidates={extensions.state.s === 'ready' ? membersWithoutExtension(extensions.state.d) : []}
          onClose={() => setCreating(false)}
          onCreated={(created) => {
            setCreating(false)
            setReveal(created)
            extensions.reload()
          }}
        />
      )}
      {reveal && <RevealDialog created={reveal} onClose={() => setReveal(null)} />}
    </Card>
  )
}

function NewExtensionDialog({
  orgId,
  candidates,
  onClose,
  onCreated,
}: {
  orgId: string
  candidates: Employee[]
  onClose: () => void
  onCreated: (created: ExtensionCreated) => void
}) {
  const { t } = useTranslation()
  const [memberId, setMemberId] = useState('')
  const [extension, setExtension] = useState('')
  const [label, setLabel] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!memberId || !extension.trim()) return
    setBusy(true)
    setErr('')
    try {
      const created = await createExtension(orgId, {
        member_id: memberId,
        extension: extension.trim(),
        label: label.trim() || undefined,
      })
      onCreated(created)
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'consola.ramais.erroCriar'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog title={t('consola.ramais.novo')} onClose={onClose}>
      <form onSubmit={submit} style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        {candidates.length === 0 ? (
          <Alert tone="warning">{t('consola.ramais.semMembrosDisponiveis')}</Alert>
        ) : (
          <Select value={memberId} onChange={(e) => setMemberId(e.target.value)} aria-label={t('consola.ramais.membro')}>
            <option value="">{t('consola.ramais.escolherMembro')}</option>
            {candidates.map((p) => (
              <option key={p.user_id} value={p.user_id}>
                {p.username} — {p.email}
              </option>
            ))}
          </Select>
        )}
        <TextInput
          value={extension}
          onChange={(e) => setExtension(e.target.value.replace(/\D/g, '').slice(0, 5))}
          placeholder={t('consola.ramais.extensaoPh')}
          aria-label={t('consola.ramais.extensao')}
          inputMode="numeric"
          className="dx-num"
        />
        <TextInput
          value={label}
          onChange={(e) => setLabel(e.target.value)}
          placeholder={t('consola.ramais.rotuloPh')}
          aria-label={t('consola.ramais.rotulo')}
          maxLength={80}
        />
        {err && <Alert tone="danger">{err}</Alert>}
        <div style={{ display: 'flex', gap: 8 }}>
          <Button type="button" variant="secondary" onClick={onClose}>
            {t('ui.cancelar')}
          </Button>
          <Button type="submit" variant="primary" busy={busy} disabled={!memberId || !extension.trim()}>
            {t('consola.ramais.criar')}
          </Button>
        </div>
      </form>
    </Dialog>
  )
}

function RevealDialog({ created, onClose }: { created: ExtensionCreated; onClose: () => void }) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)

  async function copyAll() {
    const text = `${t('consola.ramais.utilizador')}: ${created.sip_username}\n${t('consola.ramais.password')}: ${created.sip_password}\n${t('consola.ramais.dominio')}: ${created.sip_domain}`
    try {
      await navigator.clipboard.writeText(text)
      setCopied(true)
    } catch {
      // Sem clipboard (contexto não seguro, permissão negada): a pessoa copia
      // à mão a partir dos campos abaixo — não é um erro que bloqueie o fluxo.
    }
  }

  return (
    <Dialog title={t('consola.ramais.credenciaisTitulo')} onClose={onClose}>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        <Alert tone="warning">{t('consola.ramais.credenciaisAviso')}</Alert>
        <dl className="org-voice__kpis">
          <div>
            <dt className="dx-muted">{t('consola.ramais.utilizador')}</dt>
            <dd className="dx-num">{created.sip_username}</dd>
          </div>
          <div>
            <dt className="dx-muted">{t('consola.ramais.password')}</dt>
            <dd className="dx-num">{created.sip_password}</dd>
          </div>
          <div>
            <dt className="dx-muted">{t('consola.ramais.dominio')}</dt>
            <dd className="dx-num">{created.sip_domain}</dd>
          </div>
        </dl>
        <Button variant="secondary" icon="copy" onClick={() => void copyAll()}>
          {copied ? t('ui.copiado') : t('ui.copiar')}
        </Button>
        <Button variant="primary" disabled={!copied} onClick={onClose}>
          {t('consola.ramais.concluido')}
        </Button>
      </div>
    </Dialog>
  )
}
