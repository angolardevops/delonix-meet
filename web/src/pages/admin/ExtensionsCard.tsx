/**
 * Ramais internos (`server/src/ramais.rs`): chamada ramal-a-ramal pela rede
 * interna, ligada ao FreeSWITCH que já serve o dial-in PSTN (`voice/`).
 * Fase 1: SÓ interno. Fase 2: um ramal pode receber um DID dedicado e passar a
 * tocar directamente para quem lhe ligar do exterior — sem PIN, sem IVR.
 * Fase 3 (R273): um ramal entra numa reunião marcando o número de acesso às
 * reuniões (`meeting_access_number`) e indicando o PIN da sala.
 *
 * O aviso no topo não é decoração: diz o que existe E a condição. A entrada
 * numa sala de vídeo depende de a instalação ter a ponte telefone↔sala
 * configurada (ADR-0010), e nenhuma chamada real por ramal foi provada — o
 * texto não pode afirmar que funciona de ponta a ponta.
 *
 * A password SIP só existe em claro na resposta de criação/regeneração — o
 * mesmo padrão de revelação única que `SmsGatewayCard` já usa para o token de
 * emparelhamento de um gateway. O diálogo mostra também o SERVIDOR: o endereço
 * público onde o softphone se liga (`sip_server`), que não é o domínio SIP —
 * esse é um nome lógico. Sem endereço configurado, di-lo; não inventa um.
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { TFunction } from 'i18next'
import {
  ApiError,
  assignExtensionDid,
  createExtension,
  deleteExtension,
  Employee,
  Extension,
  ExtensionCreated,
  listExtensions,
  listVoiceDids,
  regenerateExtensionPassword,
  unassignExtensionDid,
  updateExtension,
  VoiceDid,
} from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Alert, Button, Card, Dialog, IconButton, Select, StatusBadge, TextInput } from '../../ui/kit'
import { orgErrorMessage, refusalAware } from './orgShared'

export default function ExtensionsCard({ orgId, people }: { orgId: string; people: Employee[] }) {
  const { t } = useTranslation()
  const extensions = useAsync((signal) => refusalAware(listExtensions(orgId, signal), t), [orgId])
  const dids = useAsync((signal) => refusalAware(listVoiceDids(orgId, signal), t), [orgId])
  const [creating, setCreating] = useState(false)
  const [reveal, setReveal] = useState<ExtensionCreated | null>(null)
  const [busyId, setBusyId] = useState<string | null>(null)
  const [err, setErr] = useState('')

  const membersWithoutExtension = (list: Extension[]) => {
    const taken = new Set(list.map((e) => e.member_id))
    return people.filter((p) => !taken.has(p.user_id))
  }

  // Números desta org (não do pool partilhado), ainda sem ramal — os únicos
  // que se podem atribuir (mesma regra que o servidor aplica em
  // ramais.rs::assign_extension_did: um número do pool partilhado, org_id
  // nulo, fica disponível para todas as orgs e não entra aqui).
  const assignableDids = (list: VoiceDid[]) => list.filter((d) => d.active && d.org_id === orgId && !d.extension_id)
  const didForExtension = (list: VoiceDid[], extensionId: string) => list.find((d) => d.extension_id === extensionId)

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

  async function assignDid(e: Extension, didId: string) {
    setBusyId(e.id)
    setErr('')
    try {
      await assignExtensionDid(orgId, e.id, didId)
      dids.reload()
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'consola.ramais.did.erro'))
    } finally {
      setBusyId(null)
    }
  }

  async function unassignDid(e: Extension) {
    if (!window.confirm(t('consola.ramais.did.desatribuirConfirmar', { extensao: e.extension }))) return
    setBusyId(e.id)
    setErr('')
    try {
      await unassignExtensionDid(orgId, e.id)
      dids.reload()
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'consola.ramais.did.erro'))
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
                    <th scope="col">{t('consola.ramais.did.col')}</th>
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
                      <td>
                        <DidCell
                          extension={e}
                          assignedDid={dids.state.s === 'ready' ? didForExtension(dids.state.d, e.id) : undefined}
                          assignable={dids.state.s === 'ready' ? assignableDids(dids.state.d) : []}
                          busy={busyId === e.id}
                          onAssign={(didId) => assignDid(e, didId)}
                          onUnassign={() => unassignDid(e)}
                        />
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
      {extensions.state.s === 'ready' && extensions.state.d[0]?.meeting_access_number && (
        <p className="dx-muted org-card-note" data-testid="ramais-acesso">
          {t('consola.ramais.acessoNota', { numero: extensions.state.d[0].meeting_access_number })}
        </p>
      )}
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

/**
 * Célula de DID de um ramal (Fase 2): mostra o número atribuído com botão de
 * desatribuir, ou — se houver números desta org por atribuir — um selector
 * compacto + botão de atribuir. Sem números disponíveis, mostra só o traço
 * neutro (mesmo padrão de "sem dados" que o resto da tabela usa).
 */
function DidCell({
  extension,
  assignedDid,
  assignable,
  busy,
  onAssign,
  onUnassign,
}: {
  extension: Extension
  assignedDid: VoiceDid | undefined
  assignable: VoiceDid[]
  busy: boolean
  onAssign: (didId: string) => void
  onUnassign: () => void
}) {
  const { t } = useTranslation()
  const [choice, setChoice] = useState('')

  if (assignedDid) {
    return (
      <span className="org-simple__main">
        <strong className="dx-num">{assignedDid.e164}</strong>
        <Button size="sm" variant="secondary" busy={busy} onClick={onUnassign}>
          {t('consola.ramais.did.desatribuir')}
        </Button>
      </span>
    )
  }

  if (assignable.length === 0) {
    return <span className="dx-muted">—</span>
  }

  return (
    <span className="org-inline" aria-label={t('consola.ramais.did.atribuir', { extensao: extension.extension })}>
      <Select
        value={choice}
        onChange={(e) => setChoice(e.target.value)}
        aria-label={t('consola.ramais.did.escolherNumero')}
      >
        <option value="">{t('consola.ramais.did.escolherNumero')}</option>
        {assignable.map((d) => (
          <option key={d.id} value={d.id}>
            {d.e164}
          </option>
        ))}
      </Select>
      <Button
        size="sm"
        variant="secondary"
        busy={busy}
        disabled={!choice}
        onClick={() => {
          onAssign(choice)
          setChoice('')
        }}
      >
        {t('consola.ramais.did.atribuirBotao')}
      </Button>
    </span>
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
      // O número de acesso às reuniões não pode ser de um ramal (R273): a
      // recusa tem código estável, e a mensagem diz o que fazer a seguir.
      setErr(
        errorCode(x) === 'ramais.extension_reserved'
          ? t('consola.ramais.erroReservado', { numero: extension.trim() })
          : orgErrorMessage(x, t, 'consola.ramais.erroCriar'),
      )
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

/** O código estável do envelope de erro (`error.rs`), se a resposta o trouxer. */
function errorCode(e: unknown): string | undefined {
  if (!(e instanceof ApiError)) return undefined
  const b = e.body as { code?: unknown } | null
  return b && typeof b === 'object' && typeof b.code === 'string' ? b.code : undefined
}

export interface CredentialField {
  key: 'servidor' | 'utilizador' | 'password' | 'dominio' | 'numeroAcesso'
  label: string
  /** `null`: o servidor não tem este dado — a linha diz porquê em `missing`. */
  value: string | null
  missing?: string
  hint?: string
}

/**
 * Os campos que um softphone pede, pela ordem em que os pede. O servidor
 * aparece SEMPRE: sem endereço público configurado a linha diz isso mesmo, em
 * vez de desaparecer ou de mostrar o domínio lógico no lugar dele.
 */
export function credentialFields(created: ExtensionCreated, t: TFunction): CredentialField[] {
  const fields: CredentialField[] = [
    {
      key: 'servidor',
      label: t('consola.ramais.servidor'),
      value: created.sip_server?.uri ?? null,
      missing: t('consola.ramais.servidorEmFalta'),
    },
    { key: 'utilizador', label: t('consola.ramais.utilizador'), value: created.sip_username },
    { key: 'password', label: t('consola.ramais.password'), value: created.sip_password },
    { key: 'dominio', label: t('consola.ramais.dominio'), value: created.sip_domain, hint: t('consola.ramais.dominioDica') },
  ]
  if (created.meeting_access_number) {
    fields.push({ key: 'numeroAcesso', label: t('consola.ramais.numeroAcesso'), value: created.meeting_access_number })
  }
  return fields
}

/** O texto do «copiar tudo»: um campo por linha, só os que têm valor. */
export function credentialsText(fields: CredentialField[]): string {
  return fields
    .filter((f) => f.value !== null)
    .map((f) => `${f.label}: ${f.value}`)
    .join('\n')
}

/**
 * A lista de credenciais: um campo por linha, valor com quebra e um botão de
 * copiar por campo. `onCopied` avisa o diálogo de que já se copiou alguma
 * coisa (é o que destranca o «Concluído»).
 */
export function ExtensionCredentials({ created, onCopied }: { created: ExtensionCreated; onCopied?: () => void }) {
  const { t } = useTranslation()
  const [copiedKey, setCopiedKey] = useState<CredentialField['key'] | null>(null)

  async function copy(f: CredentialField) {
    if (f.value === null) return
    try {
      await navigator.clipboard.writeText(f.value)
      setCopiedKey(f.key)
    } catch {
      // Sem clipboard (contexto não seguro, permissão negada): o valor está à
      // vista e selecciona-se inteiro com um toque — copia-se à mão.
    }
    onCopied?.()
  }

  return (
    <>
      <span className="dx-sr-only" role="status">
        {copiedKey ? t('ui.copiado') : ''}
      </span>
      <dl className="org-creds" data-testid="ramal-credenciais">
        {credentialFields(created, t).map((f) => (
        <div key={f.key} className="org-creds__row" data-campo={f.key}>
          <dt className="dx-muted">{f.label}</dt>
          <dd>
            {f.value === null ? (
              <span className="org-creds__value dx-muted">{f.missing}</span>
            ) : (
              <>
                <span className="org-creds__value dx-num">{f.value}</span>
                <IconButton
                  icon={copiedKey === f.key ? 'check' : 'copy'}
                  label={t('consola.ramais.copiarCampo', { campo: f.label })}
                  onClick={() => void copy(f)}
                />
              </>
            )}
          </dd>
          {f.hint && <dd className="org-creds__hint dx-muted">{f.hint}</dd>}
        </div>
      ))}
      </dl>
    </>
  )
}

function RevealDialog({ created, onClose }: { created: ExtensionCreated; onClose: () => void }) {
  const { t } = useTranslation()
  const [copiedAll, setCopiedAll] = useState(false)
  // «Concluído» só destranca depois de se ter copiado alguma coisa: a password
  // não volta a aparecer.
  const [touched, setTouched] = useState(false)

  async function copyAll() {
    try {
      await navigator.clipboard.writeText(credentialsText(credentialFields(created, t)))
      setCopiedAll(true)
    } catch {
      // Sem clipboard (contexto não seguro, permissão negada): a pessoa copia
      // à mão a partir dos campos acima — não é um erro que bloqueie o fluxo.
    }
    setTouched(true)
  }

  return (
    <Dialog title={t('consola.ramais.credenciaisTitulo')} onClose={onClose}>
      <Alert tone="warning">{t('consola.ramais.credenciaisAviso')}</Alert>
      <ExtensionCredentials created={created} onCopied={() => setTouched(true)} />
      <Button variant="secondary" icon="copy" onClick={() => void copyAll()}>
        {copiedAll ? t('ui.copiado') : t('consola.ramais.copiarTudo')}
      </Button>
      <Button variant="primary" disabled={!touched} onClick={onClose}>
        {t('consola.ramais.concluido')}
      </Button>
    </Dialog>
  )
}
