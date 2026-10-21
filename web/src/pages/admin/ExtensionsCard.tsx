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
 *
 * R276 (item 3.8, lote 1): o número do ramal, a password SIP e o PIN são três
 * coisas separadas. A lista mostra o ESTADO do PIN, nunca o valor. O PIN de um
 * ramal de pessoa é dela — o administrador só o limpa («forçar PIN novo») e a
 * pessoa gera outro em Definições → Segurança. O de um ramal da EMPRESA (sem
 * pessoa, com etiqueta) é do administrador: gera-o ou escolhe-o e vê-o uma vez.
 *
 * A lista é uma pilha de linhas que QUEBRAM, não uma tabela: com seis colunas
 * e três botões por linha a tabela tinha scroll horizontal a ~847 px e cortava
 * «Regenerar password» e «Apagar».
 */
import { FormEvent, ReactNode, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { TFunction } from 'i18next'
import {
  apiErrorCode,
  assignExtensionDid,
  assignMissingExtensions,
  clearExtensionPin,
  createExtension,
  deleteExtension,
  Employee,
  Extension,
  ExtensionCreated,
  ExtensionPinState,
  GeneratedPin,
  getExtensionRange,
  issueExtensionProvisioningTicket,
  listExtensions,
  listVoiceDids,
  putExtensionRange,
  regenerateExtensionPassword,
  regenerateExtensionPin,
  setExtensionPin,
  unassignExtensionDid,
  updateExtension,
  VoiceDid,
} from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import LinphoneQrDialog from '../../components/LinphoneQrDialog'
import PinOnce from '../../components/PinOnce'
import { Alert, Button, Card, Confirm, Dialog, Field, IconButton, Segmented, Select, StatusBadge, TextInput, Toggle } from '../../ui/kit'
import { orgErrorMessage, refusalAware } from './orgShared'
import { copiarTexto } from '../../ui/copy'

export default function ExtensionsCard({ orgId, people }: { orgId: string; people: Employee[] }) {
  const { t } = useTranslation()
  const extensions = useAsync((signal) => refusalAware(listExtensions(orgId, signal), t), [orgId])
  const dids = useAsync((signal) => refusalAware(listVoiceDids(orgId, signal), t), [orgId])
  const [creating, setCreating] = useState(false)
  const [reveal, setReveal] = useState<ExtensionCreated | null>(null)
  const [pinReveal, setPinReveal] = useState<GeneratedPin | null>(null)
  const [choosingPin, setChoosingPin] = useState<Extension | null>(null)
  const [qrFor, setQrFor] = useState<Extension | null>(null)
  const [busyId, setBusyId] = useState<string | null>(null)
  /**
   * A acção à espera de confirmação. Um estado só para as quatro: o
   * `window.confirm` que estas substituem tinha a mensagem traduzida mas os
   * botões «OK/Cancel» do browser, em inglês, num produto com quatro línguas.
   */
  const [pendente, setPendente] = useState<{
    tipo: 'regenerar' | 'apagar' | 'pin' | 'did'
    e: Extension
  } | null>(null)
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

  /** Ramal da empresa: o administrador gera o PIN e vê-o uma vez. */
  async function generatePin(e: Extension) {
    setBusyId(e.id)
    setErr('')
    try {
      setPinReveal(await regenerateExtensionPin(orgId, e.id))
      extensions.reload()
    } catch (x) {
      setErr(pinErrorMessage(x, t))
    } finally {
      setBusyId(null)
    }
  }

  /**
   * Limpar o PIN. Num ramal de pessoa é «forçar PIN novo»: fica por definir e
   * a pessoa gera outro na sua área — quem administra nunca o vê.
   */
  async function clearPin(e: Extension) {
    setBusyId(e.id)
    setErr('')
    try {
      await clearExtensionPin(orgId, e.id)
      extensions.reload()
    } catch (x) {
      setErr(pinErrorMessage(x, t))
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
            <ExtensionList
              list={list}
              busyId={busyId}
              onToggleActive={toggleActive}
              onRegeneratePassword={(e) => setPendente({ tipo: 'regenerar', e })}
              onRemove={(e) => setPendente({ tipo: 'apagar', e })}
              onGeneratePin={generatePin}
              onChoosePin={setChoosingPin}
              onClearPin={(e) => setPendente({ tipo: 'pin', e })}
              onConfigureLinphone={setQrFor}
              renderDid={(e) => (
                <DidCell
                  extension={e}
                  assignedDid={dids.state.s === 'ready' ? didForExtension(dids.state.d, e.id) : undefined}
                  assignable={dids.state.s === 'ready' ? assignableDids(dids.state.d) : []}
                  busy={busyId === e.id}
                  onAssign={(didId) => assignDid(e, didId)}
                  onUnassign={() => setPendente({ tipo: 'did', e })}
                />
              )}
            />
          )
        }
      </AsyncSection>
      {extensions.state.s === 'ready' && extensions.state.d[0]?.meeting_access_number && (
        <p className="dx-muted org-card-note" data-testid="ramais-acesso">
          {t('consola.ramais.acessoNota', { numero: extensions.state.d[0].meeting_access_number })}
        </p>
      )}
      <p className="dx-muted org-card-note">{t('consola.ramais.pin.nota')}</p>
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
      <AutoAssign
        orgId={orgId}
        meetingAccessNumber={extensions.state.s === 'ready' ? (extensions.state.d[0]?.meeting_access_number ?? '') : ''}
        onAssigned={extensions.reload}
      />
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
      {pinReveal && <PinRevealDialog generated={pinReveal} onClose={() => setPinReveal(null)} />}
      {qrFor && (
        <LinphoneQrDialog
          extension={qrFor.extension}
          issue={() => issueExtensionProvisioningTicket(orgId, qrFor.id)}
          onClose={() => setQrFor(null)}
        />
      )}
      {choosingPin && (
        <ChoosePinDialog
          orgId={orgId}
          extension={choosingPin}
          onClose={() => setChoosingPin(null)}
          onSaved={() => {
            setChoosingPin(null)
            extensions.reload()
          }}
        />
      )}
      {pendente && (
        <Confirm
          title={t(
            {
              regenerar: 'consola.ramais.regenerar',
              apagar: 'consola.ramais.apagar',
              pin: pendente.e.member_id ? 'consola.ramais.pin.forcar' : 'consola.ramais.pin.limpar',
              did: 'consola.ramais.did.desatribuir',
            }[pendente.tipo],
          )}
          onClose={() => setPendente(null)}
          onConfirm={async () => {
            const { tipo, e } = pendente
            if (tipo === 'regenerar') await regenerate(e)
            else if (tipo === 'apagar') await remove(e)
            else if (tipo === 'pin') await clearPin(e)
            else await unassignDid(e)
          }}
          icon={pendente.tipo === 'regenerar' ? 'refresh' : 'trash'}
        >
          <p>
            {t(
              {
                regenerar: 'consola.ramais.regenerarConfirmar',
                apagar: 'consola.ramais.apagarConfirmar',
                pin: pendente.e.member_id
                  ? 'consola.ramais.pin.forcarConfirmar'
                  : 'consola.ramais.pin.limparConfirmar',
                did: 'consola.ramais.did.desatribuirConfirmar',
              }[pendente.tipo],
              { extensao: pendente.e.extension },
            )}
          </p>
        </Confirm>
      )}
    </Card>
  )
}

const PIN_TONE: Record<ExtensionPinState, 'success' | 'neutral' | 'warning'> = {
  set: 'success',
  unset: 'neutral',
  locked: 'warning',
}

/** A mensagem de uma recusa de PIN, pelo código estável do servidor. */
export function pinErrorMessage(e: unknown, t: TFunction): string {
  switch (apiErrorCode(e)) {
    case 'ramais.pin_format':
      return t('consola.ramais.pin.erro.formato')
    case 'ramais.pin_repeated':
      return t('consola.ramais.pin.erro.repetido')
    case 'ramais.pin_sequence':
      return t('consola.ramais.pin.erro.sequencia')
    case 'ramais.pin_contains_extension':
      return t('consola.ramais.pin.erro.contemRamal')
    case 'ramais.pin_belongs_to_member':
      return t('consola.ramais.pin.erro.dePessoa')
    default:
      return orgErrorMessage(e, t, 'consola.ramais.pin.erro.generico')
  }
}

/**
 * A lista dos ramais. Cada linha é uma grelha que QUEBRA: identidade e estado
 * em cima, número PSTN e acções por baixo, cada grupo com `flex-wrap`. Não há
 * largura mínima em lado nenhum — é o que impede o scroll horizontal.
 */
export function ExtensionList({
  list,
  busyId,
  onToggleActive,
  onRegeneratePassword,
  onRemove,
  onGeneratePin,
  onChoosePin,
  onClearPin,
  onConfigureLinphone,
  renderDid,
}: {
  list: Extension[]
  busyId: string | null
  onToggleActive: (e: Extension) => void
  onRegeneratePassword: (e: Extension) => void
  onRemove: (e: Extension) => void
  onGeneratePin: (e: Extension) => void
  onChoosePin: (e: Extension) => void
  onClearPin: (e: Extension) => void
  /** «Configurar o Linphone» (R278): só num ramal activo. */
  onConfigureLinphone?: (e: Extension) => void
  renderDid?: (e: Extension) => ReactNode
}) {
  const { t } = useTranslation()
  return (
    <ul className="org-exts" data-testid="ramais-list">
      {list.map((e) => {
        const company = e.member_id === null
        const busy = busyId === e.id
        return (
          <li key={e.id} className="org-ext" data-ramal={e.extension} data-tipo={company ? 'empresa' : 'pessoa'}>
            <div className="org-ext__id">
              <span className="org-ext__num dx-num">{e.extension}</span>
              <div className="org-ext__who">
                <strong>{company ? e.label : e.member_username}</strong>
                <div className="dx-muted">
                  {company ? t('consola.ramais.daEmpresa') : [e.member_email, e.label].filter(Boolean).join(' · ')}
                </div>
              </div>
            </div>
            <div className="org-ext__state">
              <StatusBadge tone={e.active ? 'success' : 'neutral'}>
                {e.active ? t('consola.ramais.activo') : t('consola.ramais.inactivo')}
              </StatusBadge>
              <span data-pin-state={e.pin_state}>
                <StatusBadge tone={PIN_TONE[e.pin_state]}>{t(`consola.ramais.pin.estado.${e.pin_state}`)}</StatusBadge>
              </span>
            </div>
            {renderDid && (
              <div className="org-ext__did">
                <span className="dx-muted">{t('consola.ramais.did.col')}</span>
                {renderDid(e)}
              </div>
            )}
            <div className="org-ext__actions" role="group" aria-label={t('consola.ramais.accoes', { extensao: e.extension })}>
              <Button size="sm" variant="secondary" busy={busy} onClick={() => onToggleActive(e)}>
                {e.active ? t('consola.ramais.desactivar') : t('consola.ramais.activar')}
              </Button>
              {onConfigureLinphone && e.active && (
                <Button size="sm" variant="primary" icon="phone" busy={busy} onClick={() => onConfigureLinphone(e)}>
                  {t('consola.ramais.qr.botao')}
                </Button>
              )}
              <Button size="sm" variant="secondary" busy={busy} onClick={() => onRegeneratePassword(e)}>
                {t('consola.ramais.regenerar')}
              </Button>
              {company && (
                <>
                  <Button size="sm" variant="secondary" busy={busy} onClick={() => onGeneratePin(e)}>
                    {t('consola.ramais.pin.gerar')}
                  </Button>
                  <Button size="sm" variant="secondary" busy={busy} onClick={() => onChoosePin(e)}>
                    {t('consola.ramais.pin.escolher')}
                  </Button>
                </>
              )}
              {e.pin_state !== 'unset' && (
                <Button size="sm" variant="secondary" busy={busy} onClick={() => onClearPin(e)}>
                  {company ? t('consola.ramais.pin.limpar') : t('consola.ramais.pin.forcar')}
                </Button>
              )}
              <Button size="sm" variant="danger" busy={busy} onClick={() => onRemove(e)}>
                {t('consola.ramais.apagar')}
              </Button>
            </div>
          </li>
        )
      })}
    </ul>
  )
}

/** O que a atribuição em massa conseguiu, somando os lotes. */
export interface AssignOutcome {
  created: number
  remaining: number
  exhausted: boolean
}

/** A frase que resume uma atribuição em massa. */
export function assignOutcomeText(o: AssignOutcome, t: TFunction): string {
  const parts: string[] = []
  if (o.created > 0) parts.push(t('consola.ramais.atribuir.feito', { count: o.created }))
  if (o.exhausted && o.remaining > 0) parts.push(t('consola.ramais.atribuir.esgotado', { count: o.remaining }))
  else if (o.created === 0) parts.push(t('consola.ramais.atribuir.nada'))
  return parts.join(' ')
}

/**
 * Numeração automática: o intervalo da organização e «atribuir ramais a
 * todos». O servidor cria no máximo 100 ramais por pedido; aqui repete-se
 * enquanto houver pessoas por servir e o lote anterior tiver criado algum.
 */
function AutoAssign({
  orgId,
  meetingAccessNumber,
  onAssigned,
}: {
  orgId: string
  meetingAccessNumber: string
  onAssigned: () => void
}) {
  const { t } = useTranslation()
  const range = useAsync((signal) => refusalAware(getExtensionRange(orgId, signal), t), [orgId])
  const [start, setStart] = useState<string | null>(null)
  const [end, setEnd] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [outcome, setOutcome] = useState<AssignOutcome | null>(null)
  // O interruptor grava-se sozinho; enquanto o pedido corre mostra o valor novo.
  const [autoPending, setAutoPending] = useState<boolean | null>(null)

  if (range.state.s !== 'ready') return null
  const saved = range.state.d
  const auto = autoPending ?? saved.auto_assign_on_join

  /** Liga ou desliga «ramal a quem entra», sem tocar no intervalo gravado. */
  async function toggleAuto(next: boolean) {
    setErr('')
    setAutoPending(next)
    try {
      await putExtensionRange(orgId, { range_start: saved.range_start, range_end: saved.range_end, auto_assign_on_join: next })
      range.reload()
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'consola.ramais.atribuir.automaticoErro'))
    } finally {
      setAutoPending(null)
    }
  }
  const startValue = start ?? String(saved.range_start)
  const endValue = end ?? String(saved.range_end)
  const digits = (v: string) => v.replace(/\D/g, '').slice(0, 5)

  async function run(e: FormEvent) {
    e.preventDefault()
    const s = Number(startValue)
    const f = Number(endValue)
    setErr('')
    setOutcome(null)
    if (!(s >= 100 && f <= 99999 && s <= f)) {
      setErr(t('consola.ramais.atribuir.erroIntervalo'))
      return
    }
    setBusy(true)
    try {
      if (s !== saved.range_start || f !== saved.range_end) {
        await putExtensionRange(orgId, { range_start: s, range_end: f })
        range.reload()
      }
      const total: AssignOutcome = { created: 0, remaining: 0, exhausted: false }
      for (;;) {
        const r = await assignMissingExtensions(orgId)
        total.created += r.assigned.length
        total.remaining = r.remaining
        total.exhausted = r.range_exhausted
        if (r.remaining === 0 || r.range_exhausted || r.assigned.length === 0) break
      }
      setOutcome(total)
      if (total.created > 0) onAssigned()
    } catch (x) {
      setErr(
        apiErrorCode(x) === 'ramais.range_invalid'
          ? t('consola.ramais.atribuir.erroIntervalo')
          : orgErrorMessage(x, t, 'consola.ramais.atribuir.erro'),
      )
    } finally {
      setBusy(false)
    }
  }

  return (
    <form className="org-ext-auto" onSubmit={run} data-testid="ramais-atribuir">
      <h3 className="dx-eyebrow">{t('consola.ramais.atribuir.titulo')}</h3>
      <p className="dx-muted">{t('consola.ramais.atribuir.dica', { numero: meetingAccessNumber || '—' })}</p>
      <div className="org-ext-auto__fields">
        <Field label={t('consola.ramais.atribuir.inicio')} htmlFor="ramais-intervalo-inicio">
          <TextInput
            id="ramais-intervalo-inicio"
            value={startValue}
            onChange={(e) => setStart(digits(e.target.value))}
            inputMode="numeric"
            className="dx-num"
          />
        </Field>
        <Field label={t('consola.ramais.atribuir.fim')} htmlFor="ramais-intervalo-fim">
          <TextInput
            id="ramais-intervalo-fim"
            value={endValue}
            onChange={(e) => setEnd(digits(e.target.value))}
            inputMode="numeric"
            className="dx-num"
          />
        </Field>
        <Button type="submit" variant="secondary" size="sm" busy={busy}>
          {t('consola.ramais.atribuir.botao')}
        </Button>
      </div>
      <Toggle
        label={t('consola.ramais.atribuir.automatico')}
        hint={t('consola.ramais.atribuir.automaticoDica')}
        checked={auto}
        disabled={busy || autoPending !== null}
        onChange={(e) => void toggleAuto(e.target.checked)}
        data-testid="ramais-automatico"
      />
      {err && <Alert tone="danger">{err}</Alert>}
      {outcome && (
        <Alert tone={outcome.exhausted && outcome.remaining > 0 ? 'warning' : 'success'}>
          {assignOutcomeText(outcome, t)}
          {outcome.created > 0 && ` ${t('consola.ramais.atribuir.notaCredenciais')}`}
        </Alert>
      )}
    </form>
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
      <span className="org-ext__didpick">
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
    <span className="org-ext__didpick" aria-label={t('consola.ramais.did.atribuir', { extensao: extension.extension })}>
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
  const [kind, setKind] = useState<'member' | 'company'>('member')
  const [memberId, setMemberId] = useState('')
  const [extension, setExtension] = useState('')
  const [label, setLabel] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!ready) return
    setBusy(true)
    setErr('')
    try {
      const created = await createExtension(orgId, {
        member_id: company ? undefined : memberId,
        extension: extension.trim(),
        label: label.trim() || undefined,
      })
      onCreated(created)
    } catch (x) {
      // O número de acesso às reuniões não pode ser de um ramal (R273): a
      // recusa tem código estável, e a mensagem diz o que fazer a seguir.
      const code = apiErrorCode(x)
      setErr(
        code === 'ramais.extension_reserved'
          ? t('consola.ramais.erroReservado', { numero: extension.trim() })
          : code === 'ramais.label_required'
            ? t('consola.ramais.erroEtiqueta')
            : orgErrorMessage(x, t, 'consola.ramais.erroCriar'),
      )
    } finally {
      setBusy(false)
    }
  }

  const company = kind === 'company'
  // Um ramal da empresa só se reconhece pela etiqueta: sem ela não se cria.
  const ready = !!extension.trim() && (company ? !!label.trim() : !!memberId)

  return (
    <Dialog title={t('consola.ramais.novo')} onClose={onClose}>
      <form onSubmit={submit} style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        <Segmented
          label={t('consola.ramais.tipo')}
          value={kind}
          onChange={setKind}
          options={[
            { value: 'member', label: t('consola.ramais.tipoPessoa') },
            { value: 'company', label: t('consola.ramais.tipoEmpresa') },
          ]}
        />
        {company ? (
          <p className="dx-muted" style={{ margin: 0 }}>{t('consola.ramais.empresaDica')}</p>
        ) : candidates.length === 0 ? (
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
          placeholder={company ? t('consola.ramais.rotuloEmpresaPh') : t('consola.ramais.rotuloPh')}
          aria-label={company ? t('consola.ramais.rotuloEmpresa') : t('consola.ramais.rotulo')}
          required={company}
          maxLength={80}
        />
        {err && <Alert tone="danger">{err}</Alert>}
        <div style={{ display: 'flex', gap: 8 }}>
          <Button type="button" variant="secondary" onClick={onClose}>
            {t('ui.cancelar')}
          </Button>
          <Button type="submit" variant="primary" busy={busy} disabled={!ready}>
            {t('consola.ramais.criar')}
          </Button>
        </div>
      </form>
    </Dialog>
  )
}

function PinRevealDialog({ generated, onClose }: { generated: GeneratedPin; onClose: () => void }) {
  const { t } = useTranslation()
  return (
    <Dialog title={t('consola.ramais.pin.reveladoTitulo', { extensao: generated.extension })} onClose={onClose}>
      <Alert tone="warning">{t('consola.ramais.pin.reveladoAviso')}</Alert>
      <PinOnce generated={generated} />
      <Button variant="primary" onClick={onClose}>
        {t('consola.ramais.concluido')}
      </Button>
    </Dialog>
  )
}

/** O administrador escolhe o PIN de um ramal da EMPRESA. */
function ChoosePinDialog({
  orgId,
  extension,
  onClose,
  onSaved,
}: {
  orgId: string
  extension: Extension
  onClose: () => void
  onSaved: () => void
}) {
  const { t } = useTranslation()
  const [pin, setPin] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (pin.length !== 6) return
    setBusy(true)
    setErr('')
    try {
      await setExtensionPin(orgId, extension.id, pin)
      onSaved()
    } catch (x) {
      setErr(pinErrorMessage(x, t))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog title={t('consola.ramais.pin.escolherTitulo', { extensao: extension.extension })} onClose={onClose}>
      <form onSubmit={submit} style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        <Field label={t('consola.ramais.pin.campo')} htmlFor="ramal-pin-novo" hint={t('consola.ramais.pin.regras')}>
          <TextInput
            id="ramal-pin-novo"
            value={pin}
            onChange={(e) => setPin(e.target.value.replace(/\D/g, '').slice(0, 6))}
            inputMode="numeric"
            autoComplete="off"
            className="dx-num"
          />
        </Field>
        {err && <Alert tone="danger">{err}</Alert>}
        <div style={{ display: 'flex', gap: 8 }}>
          <Button type="button" variant="secondary" onClick={onClose}>
            {t('ui.cancelar')}
          </Button>
          <Button type="submit" variant="primary" busy={busy} disabled={pin.length !== 6}>
            {t('consola.ramais.pin.guardar')}
          </Button>
        </div>
      </form>
    </Dialog>
  )
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
      await copiarTexto(f.value)
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
      await copiarTexto(credentialsText(credentialFields(created, t)))
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
