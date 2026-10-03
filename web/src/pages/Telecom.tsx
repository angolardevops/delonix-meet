/**
 * Telefonia, SIP e SMS — modo de LEITURA (ADR-0009): estado do SBC,
 * operadoras pela ordem de encaminhamento, plano de marcação, registo SIP,
 * chamadas externas e consumo do mês.
 *
 * Nada aqui escreve. Criar, editar e reordenar operadoras, editar o plano,
 * testar um número, revelar credenciais e reiniciar o registo são lotes
 * seguintes, e por isso não há botões inertes no lugar deles. O bloco de SMS
 * do desenho continua na Administração.
 *
 * A entrada no rail só aparece a administradores, mas isso não é autorização:
 * quem decide é o servidor, e uma recusa aparece como recusa.
 *
 * Uma organização sem definições SIP e sem operadoras não tem telefonia: a
 * página diz isso, em vez de seis cartões de zeros. Um pedido que falha
 * mostra o erro no cartão respectivo — nunca «sem dados».
 */
import { useTranslation } from 'react-i18next'
import { getDialPlan, getSipRegistration, getSipSettings, getTelephonyUsage, listCallRecords, listTrunks } from '../api'
import type { SipRegistration, SipSettings, Trunk } from '../api'
import { Async, AsyncSection, useAsync } from '../components/AsyncSection'
import PageBar from '../components/PageBar'
import { Alert, Button, Empty, Select, Skeleton } from '../ui/kit'
import { refusalAware, useOrgSelection } from './admin/orgShared'
import CallsCard from './telecom/CallsCard'
import DialPlanCard from './telecom/DialPlanCard'
import { pageMode } from './telecom/format'
import { useTelecomText } from './telecom/shared'
import SipCard from './telecom/SipCard'
import StatusHeader from './telecom/StatusHeader'
import TrunksCard from './telecom/TrunksCard'
import UsageCard from './telecom/UsageCard'
import { PagedList, usePaged } from './telecom/usePaged'
import '../ui/org.css'
import '../ui/telecom.css'

export default function Telecom() {
  const { t } = useTranslation()
  const { list, orgs, org, setOrgId } = useOrgSelection()
  return (
    <>
      <PageBar
        title={t('telecom.titulo')}
        meta={org ? <span data-testid="tel-meta">{[org.name, t('telecom.soLeitura')].join(' · ')}</span> : undefined}
      >
        {orgs.length > 1 && org && (
          <Select value={org.id} onChange={(e) => setOrgId(e.target.value)} aria-label={t('org.escolherOrg')} className="org-orgselect">
            {orgs.map((o) => (
              <option key={o.id} value={o.id}>
                {o.role === 'admin' ? o.name : t('org.admin.orgSemAdmin', { nome: o.name })}
              </option>
            ))}
          </Select>
        )}
      </PageBar>
      <AsyncSection state={list.state} onRetry={list.reload}>
        {() =>
          !org ? (
            <div className="page">
              <Empty icon="building" title={t('org.semOrg.titulo')}>
                {t('org.semOrg.texto')}
              </Empty>
            </div>
          ) : org.role !== 'admin' ? (
            <div className="page">
              <Empty icon="lock" title={t('org.admin.soAdmins')}>
                {t('org.admin.soAdminsTexto', { nome: org.name })}
              </Empty>
            </div>
          ) : (
            <TelecomBody key={org.id} orgId={org.id} />
          )
        }
      </AsyncSection>
    </>
  )
}

function TelecomBody({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  const sip = useAsync(
    (signal) =>
      refusalAware(Promise.all([getSipSettings(orgId, signal), getSipRegistration(orgId, signal)]), t).then(([settings, registration]) => ({
        settings,
        registration,
      })),
    [orgId],
  )
  const trunks = usePaged((token, signal) => listTrunks(orgId, { page_token: token }, signal), [orgId])
  const mode = pageMode(sip.state, trunks.state)

  if (mode === 'loading') {
    return (
      <div className="page tel" aria-busy="true">
        <Skeleton h={64} />
        <Skeleton h={180} />
      </div>
    )
  }
  if (mode === 'not_configured' && sip.state.s === 'ready') {
    return (
      <div className="page tel">
        <NotConfigured
          registration={sip.state.d.registration}
          onReload={() => {
            sip.reload()
            trunks.reload()
          }}
        />
      </div>
    )
  }
  return (
    <div className="page tel">
      <AsyncSection state={sip.state} onRetry={sip.reload}>
        {(d) => <StatusHeader registration={d.registration} />}
      </AsyncSection>
      {sip.state.s === 'ready' && !sip.state.d.settings.configured && (
        <Alert tone="warning" icon="phone">
          {t('telecom.vazio.comOperadoras')}
        </Alert>
      )}
      <Sections orgId={orgId} sip={sip} trunks={trunks} />
    </div>
  )
}

/** Sem definições SIP e sem operadoras: diz-se isso, e porquê, sem métricas a zero. */
function NotConfigured({ registration, onReload }: { registration: SipRegistration; onReload: () => void }) {
  const { t } = useTranslation()
  const { reason } = useTelecomText()
  return (
    <div data-testid="tel-not-configured">
      <Empty
        icon="phone"
        title={t('telecom.vazio.titulo')}
        action={
          <Button size="sm" variant="secondary" icon="refresh" onClick={onReload}>
            {t('telecom.vazio.verificar')}
          </Button>
        }
      >
        <p>{t('telecom.vazio.texto')}</p>
        {registration.reasons.length > 0 && (
          <ul className="tel-reasons tel-reasons--center">
            {registration.reasons.map((c) => (
              <li key={c}>{reason(c)}</li>
            ))}
          </ul>
        )}
        <p className="dx-muted">{t('telecom.vazio.nota')}</p>
      </Empty>
    </div>
  )
}

function Sections({
  orgId,
  sip,
  trunks,
}: {
  orgId: string
  sip: { state: Async<{ settings: SipSettings; registration: SipRegistration }> }
  trunks: { state: Async<PagedList<Trunk>>; reload: () => void; loadMore: () => void; busy: boolean; err: string }
}) {
  const { t } = useTranslation()
  const plan = useAsync((signal) => refusalAware(getDialPlan(orgId, signal), t), [orgId])
  const calls = usePaged((token, signal) => listCallRecords(orgId, { page_token: token }, signal), [orgId])
  const usage = useAsync((signal) => refusalAware(getTelephonyUsage(orgId, undefined, signal), t), [orgId])
  const trunkList = trunks.state.s === 'ready' ? trunks.state.d.items : []

  return (
    <div className="tel-grid">
      <div className="tel-col">
        <TrunksCard state={trunks.state} reload={trunks.reload} loadMore={trunks.loadMore} busy={trunks.busy} err={trunks.err} />
        <DialPlanCard state={plan.state} reload={plan.reload} trunks={trunkList} />
        <CallsCard state={calls.state} reload={calls.reload} loadMore={calls.loadMore} busy={calls.busy} err={calls.err} />
      </div>
      <div className="tel-col">
        {/* O erro das definições já está no cabeçalho; aqui só se repete o que há. */}
        {sip.state.s === 'ready' && <SipCard settings={sip.state.d.settings} registration={sip.state.d.registration} />}
        <UsageCard state={usage.state} reload={usage.reload} />
      </div>
    </div>
  )
}
