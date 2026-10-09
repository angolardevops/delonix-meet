/**
 * Organizações — `GET /api/operator/v1/tenants` (paginada), com um painel de
 * detalhe por organização (`GET .../tenants/{id}`) e três formulários que
 * escrevem em três rotas diferentes (ADR: quotas ficou em `/tenants`, seats
 * e concurrency ficaram em `/organizations` — rotas mais antigas, não
 * renomeadas): quotas de plano, tecto de lugares, tecto de concorrência.
 */
import { FormEvent, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  apiErrorMessage,
  ConcurrencyLimit,
  getTenant,
  listTenants,
  OperatorOrgDetail,
  OperatorOrgSummary,
  OperatorQuotasReq,
  saveTenantConcurrency,
  saveTenantQuotas,
  saveTenantSeats,
  SeatSummary,
} from '../api'
import { AsyncSection, useAsync } from '../components/AsyncSection'
import { Alert, Button, Card, Empty, Field, Select, TextInput } from '../ui/kit'

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  let v = n
  let i = -1
  do {
    v /= 1024
    i++
  } while (v >= 1024 && i < units.length - 1)
  return `${v.toFixed(1)} ${units[i]}`
}

interface PagedTenants {
  items: OperatorOrgSummary[]
  next: string | null
}

function usePagedTenants() {
  const { t } = useTranslation()
  const first = useAsync<PagedTenants>(
    (signal) => listTenants(undefined, signal).then((p) => ({ items: p.items, next: p.next_page_token })),
    [],
  )
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const { state, mutate, reload } = first
  function loadMore() {
    if (state.s !== 'ready' || !state.d.next || busy) return
    setBusy(true)
    setErr('')
    listTenants(state.d.next)
      .then((p) => {
        mutate((d) => ({ items: [...d.items, ...p.items], next: p.next_page_token }))
        setBusy(false)
      })
      .catch((e) => {
        setBusy(false)
        setErr(apiErrorMessage(e, t('ui.erroCarregar')))
      })
  }
  return { state, reload, loadMore, busy, err }
}

export default function Tenants() {
  const { t } = useTranslation()
  const [selected, setSelected] = useState<string | null>(null)
  const { state, reload, loadMore, busy, err } = usePagedTenants()

  return (
    <div className="page">
      <AsyncSection state={state} onRetry={reload}>
        {(d) =>
          d.items.length === 0 ? (
            <Empty icon="building" title={t('tenants.vazio')} />
          ) : (
            <Card title={t('tenants.titulo')} flush>
              <div className="dx-table-wrap">
                <table className="dx-table">
                  <thead>
                    <tr>
                      <th>{t('tenants.tabela.nome')}</th>
                      <th>{t('tenants.tabela.dominio')}</th>
                      <th>{t('tenants.tabela.membros')}</th>
                      <th>{t('tenants.tabela.grupos')}</th>
                      <th>{t('tenants.tabela.salas')}</th>
                      <th>{t('tenants.tabela.reunioes')}</th>
                      <th>{t('tenants.tabela.lugares')}</th>
                      <th>{t('tenants.tabela.participantes')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {d.items.map((o) => (
                      <tr
                        key={o.id}
                        className="dx-row-link"
                        data-selected={o.id === selected}
                        data-testid={`tenant-row-${o.slug}`}
                        onClick={() => setSelected((s) => (s === o.id ? null : o.id))}
                      >
                        <td>
                          <strong>{o.name}</strong>
                        </td>
                        <td className="dx-num">{o.domain}</td>
                        <td className="dx-num">{o.member_count}</td>
                        <td className="dx-num">{o.max_groups ?? t('tenants.ilimitado')}</td>
                        <td className="dx-num">{o.max_rooms ?? t('tenants.ilimitado')}</td>
                        <td className="dx-num">{o.max_meetings ?? t('tenants.ilimitado')}</td>
                        <td className="dx-num">{o.max_seats ?? t('tenants.ilimitado')}</td>
                        <td className="dx-num">{o.max_concurrent_participants ?? t('tenants.ilimitado')}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              {state.s === 'ready' && state.d.next && (
                <div className="dx-card__body" style={{ display: 'grid', gap: 8 }}>
                  {err && <Alert tone="danger">{err}</Alert>}
                  <Button variant="secondary" busy={busy} onClick={loadMore}>
                    {t('ui.carregarMais')}
                  </Button>
                </div>
              )}
            </Card>
          )
        }
      </AsyncSection>
      {selected && <TenantDetail key={selected} orgId={selected} onClose={() => setSelected(null)} />}
    </div>
  )
}

function TenantDetail({ orgId, onClose }: { orgId: string; onClose: () => void }) {
  const { t } = useTranslation()
  const { state, reload } = useAsync((signal) => getTenant(orgId, signal), [orgId])
  return (
    <Card
      title={t('tenants.detalhe.titulo')}
      actions={
        <Button size="sm" variant="ghost" icon="x" onClick={onClose}>
          {t('tenants.detalhe.fechar')}
        </Button>
      }
    >
      <AsyncSection state={state} onRetry={reload}>
        {(d) => <TenantDetailBody detail={d} />}
      </AsyncSection>
    </Card>
  )
}

function TenantDetailBody({ detail }: { detail: OperatorOrgDetail }) {
  const { t } = useTranslation()
  const [org, setOrg] = useState(detail.org)
  const [seats, setSeats] = useState(detail.seats)
  const [concurrency, setConcurrency] = useState(detail.org.max_concurrent_participants)
  useEffect(() => {
    setOrg(detail.org)
    setSeats(detail.seats)
    setConcurrency(detail.org.max_concurrent_participants)
  }, [detail])

  return (
    <div style={{ display: 'grid', gap: 16 }}>
      <div>
        <strong style={{ fontSize: 15 }}>{org.name}</strong>
        <div className="dx-muted dx-num" style={{ fontSize: 11 }}>
          {org.domain} · {org.slug}
        </div>
      </div>

      <div className="dxa-usage" style={{ display: 'flex', gap: 24, flexWrap: 'wrap' }}>
        <div>
          <div className="dx-muted" style={{ fontSize: 10.5 }}>
            {t('tenants.tabela.lugares')}
          </div>
          <div className="dx-num">
            {t('tenants.detalhe.lugaresUsados', { used: seats.used, active: seats.active_this_month })}
            {seats.limit !== null && ` / ${seats.limit}`}
          </div>
        </div>
        <div>
          <div className="dx-muted" style={{ fontSize: 10.5 }}>
            {t('tenants.tabela.armazenamento')}
          </div>
          <div className="dx-num">
            {formatBytes(detail.storage.used_bytes)} {t('tenants.detalhe.armazenamentoUsado')}
            {detail.storage.max_storage_bytes !== null && ` / ${formatBytes(detail.storage.max_storage_bytes)}`}
          </div>
        </div>
      </div>

      <Card title={t('tenants.quotas.titulo')} bodyClass="dxa-form">
        <QuotasForm orgId={org.id} org={org} onSaved={setOrg} />
      </Card>
      <Card title={t('tenants.seats.titulo')} bodyClass="dxa-form">
        <SeatsForm orgId={org.id} limit={seats.limit} onSaved={setSeats} />
      </Card>
      <Card title={t('tenants.concurrency.titulo')} bodyClass="dxa-form">
        <ConcurrencyForm orgId={org.id} limit={concurrency} onSaved={(c) => setConcurrency(c.max_concurrent_participants)} />
      </Card>
    </div>
  )
}

function QuotasForm({
  orgId,
  org,
  onSaved,
}: {
  orgId: string
  org: OperatorOrgSummary
  onSaved: (o: OperatorOrgSummary) => void
}) {
  const { t } = useTranslation()
  const [groups, setGroups] = useState(org.max_groups === null ? '' : String(org.max_groups))
  const [rooms, setRooms] = useState(org.max_rooms === null ? '' : String(org.max_rooms))
  const [meetings, setMeetings] = useState(org.max_meetings === null ? '' : String(org.max_meetings))
  const [backend, setBackend] = useState('')
  const [did, setDid] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [ok, setOk] = useState(false)

  async function save(e: FormEvent) {
    e.preventDefault()
    setBusy(true)
    setErr('')
    setOk(false)
    const body: OperatorQuotasReq = {
      max_groups: groups.trim() === '' ? null : Number(groups),
      max_rooms: rooms.trim() === '' ? null : Number(rooms),
      max_meetings: meetings.trim() === '' ? null : Number(meetings),
      voice_media_backend: backend || null,
      voice_did_model: did || null,
    }
    try {
      const updated = await saveTenantQuotas(orgId, body)
      setOk(true)
      onSaved(updated)
    } catch (x) {
      setErr(apiErrorMessage(x, t('tenants.erroGuardar')))
    } finally {
      setBusy(false)
    }
  }

  return (
    <form onSubmit={(e) => void save(e)} style={{ display: 'grid', gap: 12 }}>
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(120px, 1fr))', gap: 12 }}>
        <Field label={t('tenants.quotas.grupos')} htmlFor="q-groups">
          <TextInput id="q-groups" type="number" min={0} placeholder={t('tenants.ilimitado')} value={groups} onChange={(e) => setGroups(e.target.value)} />
        </Field>
        <Field label={t('tenants.quotas.salas')} htmlFor="q-rooms">
          <TextInput id="q-rooms" type="number" min={0} placeholder={t('tenants.ilimitado')} value={rooms} onChange={(e) => setRooms(e.target.value)} />
        </Field>
        <Field label={t('tenants.quotas.reunioes')} htmlFor="q-meetings">
          <TextInput id="q-meetings" type="number" min={0} placeholder={t('tenants.ilimitado')} value={meetings} onChange={(e) => setMeetings(e.target.value)} />
        </Field>
      </div>
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(160px, 1fr))', gap: 12 }}>
        <Field label={t('tenants.quotas.vozBackend')} htmlFor="q-backend">
          <Select id="q-backend" value={backend} onChange={(e) => setBackend(e.target.value)}>
            <option value="">{t('tenants.quotas.manterActual')}</option>
            <option value="freeswitch">{t('tenants.quotas.backend.freeswitch')}</option>
            <option value="provider">{t('tenants.quotas.backend.provider')}</option>
          </Select>
        </Field>
        <Field label={t('tenants.quotas.vozDid')} htmlFor="q-did">
          <Select id="q-did" value={did} onChange={(e) => setDid(e.target.value)}>
            <option value="">{t('tenants.quotas.manterActual')}</option>
            <option value="shared">{t('tenants.quotas.did.shared')}</option>
            <option value="dedicated">{t('tenants.quotas.did.dedicated')}</option>
          </Select>
        </Field>
      </div>
      {err && <Alert tone="danger">{err}</Alert>}
      {ok && <Alert tone="success">{t('tenants.guardado')}</Alert>}
      <div>
        <Button type="submit" variant="primary" busy={busy}>
          {t('ui.guardar')}
        </Button>
      </div>
    </form>
  )
}

function SeatsForm({ orgId, limit, onSaved }: { orgId: string; limit: number | null; onSaved: (s: SeatSummary) => void }) {
  const { t } = useTranslation()
  const [value, setValue] = useState(limit === null ? '' : String(limit))
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [ok, setOk] = useState(false)

  async function save(e: FormEvent) {
    e.preventDefault()
    setBusy(true)
    setErr('')
    setOk(false)
    try {
      const updated = await saveTenantSeats(orgId, value.trim() === '' ? null : Number(value))
      setOk(true)
      onSaved(updated)
    } catch (x) {
      setErr(apiErrorMessage(x, t('tenants.erroGuardar')))
    } finally {
      setBusy(false)
    }
  }

  return (
    <form onSubmit={(e) => void save(e)} style={{ display: 'grid', gap: 12, maxWidth: 240 }}>
      <Field label={t('tenants.seats.max')} htmlFor="seats-max">
        <TextInput id="seats-max" type="number" min={0} placeholder={t('tenants.ilimitado')} value={value} onChange={(e) => setValue(e.target.value)} />
      </Field>
      {err && <Alert tone="danger">{err}</Alert>}
      {ok && <Alert tone="success">{t('tenants.guardado')}</Alert>}
      <div>
        <Button type="submit" variant="primary" busy={busy}>
          {t('ui.guardar')}
        </Button>
      </div>
    </form>
  )
}

function ConcurrencyForm({
  orgId,
  limit,
  onSaved,
}: {
  orgId: string
  limit: number | null
  onSaved: (c: ConcurrencyLimit) => void
}) {
  const { t } = useTranslation()
  const [value, setValue] = useState(limit === null ? '' : String(limit))
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [ok, setOk] = useState(false)

  async function save(e: FormEvent) {
    e.preventDefault()
    setBusy(true)
    setErr('')
    setOk(false)
    try {
      const updated = await saveTenantConcurrency(orgId, value.trim() === '' ? null : Number(value))
      setOk(true)
      onSaved(updated)
    } catch (x) {
      setErr(apiErrorMessage(x, t('tenants.erroGuardar')))
    } finally {
      setBusy(false)
    }
  }

  return (
    <form onSubmit={(e) => void save(e)} style={{ display: 'grid', gap: 12, maxWidth: 240 }}>
      <Field label={t('tenants.concurrency.max')} htmlFor="conc-max">
        <TextInput id="conc-max" type="number" min={0} placeholder={t('tenants.ilimitado')} value={value} onChange={(e) => setValue(e.target.value)} />
      </Field>
      {err && <Alert tone="danger">{err}</Alert>}
      {ok && <Alert tone="success">{t('tenants.guardado')}</Alert>}
      <div>
        <Button type="submit" variant="primary" busy={busy}>
          {t('ui.guardar')}
        </Button>
      </div>
    </form>
  )
}
