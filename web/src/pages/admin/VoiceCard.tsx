/**
 * Dial-in PSTN — o plano de controlo de voz da organização (`server/src/voice.rs`):
 * números (DIDs) do inventário, resumo de facturação e registo de chamadas (CDR).
 * Tudo só para admins; o servidor decide e o cartão mostra a recusa.
 *
 * O que o cartão diz sem rodeios: a camada de media (Kamailio + FreeSWITCH,
 * `voice/`) atende, pede o PIN e valida-o aqui, mas a ponte FreeSWITCH↔SFU
 * ainda não existe (`voice/README.md`, sub-fase 2b). Quem liga fala numa
 * conferência só de voz — não ouve a reunião de vídeo. Por isso este ecrã não
 * oferece «entrar por telefone» a ninguém: gere números e mostra chamadas.
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { createVoiceDid, listVoiceCdr, listVoiceDids, voiceBilling, VoicePeriod } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Alert, Button, Card, Segmented, Select, StatusBadge, TextInput } from '../../ui/kit'
import { formatDateTime, orgErrorMessage, refusalAware, useLocaleTag } from './orgShared'

const CDR_SHOWN = 50

export function fmtDuration(secs: number): string {
  const s = Math.max(0, Math.round(secs))
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const r = s % 60
  const mm = String(m).padStart(h ? 2 : 1, '0')
  return h ? `${h}:${mm}:${String(r).padStart(2, '0')}` : `${mm}:${String(r).padStart(2, '0')}`
}

export default function VoiceCard({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const [period, setPeriod] = useState<VoicePeriod>('month')
  const billing = useAsync((signal) => refusalAware(voiceBilling(orgId, period, signal), t), [orgId, period])
  const dids = useAsync((signal) => refusalAware(listVoiceDids(orgId, signal), t), [orgId])
  const cdr = useAsync((signal) => refusalAware(listVoiceCdr(orgId, signal), t), [orgId])

  const [e164, setE164] = useState('')
  const [model, setModel] = useState<'shared' | 'dedicated'>('dedicated')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  async function addDid(e: FormEvent) {
    e.preventDefault()
    if (!e164.trim()) return
    setBusy(true)
    setErr('')
    try {
      await createVoiceDid(orgId, { e164: e164.trim(), model })
      setE164('')
      dids.reload()
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'consola.voz.erroNumero'))
    } finally {
      setBusy(false)
    }
  }

  const money = (v: number) => v.toLocaleString(locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 })

  return (
    <Card title={t('consola.voz.titulo')} eyebrow={t('consola.voz.eyebrow')} flush className="org-voice" as="section">
      <div className="org-card-pad">
        <Alert tone="warning" icon="phone">
          {t('consola.voz.ponteEmFalta')}
        </Alert>
      </div>

      <div className="org-voice__billing">
        <Segmented
          value={period}
          onChange={setPeriod}
          label={t('consola.voz.periodo')}
          options={[
            { value: 'week', label: t('consola.voz.semana') },
            { value: 'month', label: t('consola.voz.mes') },
            { value: 'quarter', label: t('consola.voz.trimestre') },
            { value: 'year', label: t('consola.voz.ano') },
          ]}
        />
        <AsyncSection state={billing.state} onRetry={billing.reload}>
          {(b) => (
            <dl className="org-voice__kpis" data-testid="voice-billing">
              <div>
                <dt className="dx-muted">{t('consola.voz.chamadas')}</dt>
                <dd className="dx-num">{b.calls}</dd>
              </div>
              <div>
                <dt className="dx-muted">{t('consola.voz.minutos')}</dt>
                <dd className="dx-num">{b.total_minutes}</dd>
              </div>
              <div>
                <dt className="dx-muted">{t('consola.voz.custo')}</dt>
                <dd className="dx-num">{money(b.total_cost)}</dd>
              </div>
            </dl>
          )}
        </AsyncSection>
        <p className="dx-muted org-voice__note">{t('consola.voz.custoNota')}</p>
      </div>

      <h3 className="org-voice__sub">{t('consola.voz.numeros')}</h3>
      <AsyncSection state={dids.state} onRetry={dids.reload}>
        {(list) =>
          list.length === 0 ? (
            <p className="dx-muted org-card-note">{t('consola.voz.semNumeros')}</p>
          ) : (
            <ul className="org-simple" data-testid="voice-dids">
              {list.map((d) => (
                <li key={d.id}>
                  <span className="org-simple__main">
                    <strong className="dx-num">{d.e164}</strong>
                    <span className="dx-muted">
                      {d.model === 'dedicated' ? t('consola.voz.dedicado') : t('consola.voz.partilhado')}
                      {!d.org_id && ` · ${t('consola.voz.pool')}`}
                      {' · '}
                      {d.market}
                      {d.provider ? ` · ${d.provider}` : ''}
                    </span>
                  </span>
                  <StatusBadge tone={d.active ? 'success' : 'neutral'}>
                    {d.active ? t('consola.voz.activo') : t('consola.voz.inactivo')}
                  </StatusBadge>
                </li>
              ))}
            </ul>
          )
        }
      </AsyncSection>
      <form className="org-inline" onSubmit={addDid} aria-label={t('consola.voz.novoNumero')}>
        <TextInput
          value={e164}
          onChange={(e) => setE164(e.target.value)}
          placeholder={t('consola.voz.e164')}
          aria-label={t('consola.voz.e164')}
          inputMode="tel"
          maxLength={20}
          className="dx-num"
        />
        <Select value={model} onChange={(e) => setModel(e.target.value as 'shared' | 'dedicated')} aria-label={t('consola.voz.modelo')}>
          <option value="dedicated">{t('consola.voz.dedicado')}</option>
          <option value="shared">{t('consola.voz.partilhado')}</option>
        </Select>
        <Button type="submit" size="sm" icon="plus" busy={busy} disabled={!e164.trim()}>
          {t('consola.voz.adicionar')}
        </Button>
      </form>
      {err && (
        <div className="org-card-pad">
          <Alert tone="danger">{err}</Alert>
        </div>
      )}

      <h3 className="org-voice__sub">{t('consola.voz.registo')}</h3>
      <AsyncSection state={cdr.state} onRetry={cdr.reload}>
        {(rows) =>
          rows.length === 0 ? (
            <p className="dx-muted org-card-note">{t('consola.voz.semChamadas')}</p>
          ) : (
            <div className="dx-table-wrap org-table-wrap org-audit__scroll">
              <table className="dx-table org-table" data-testid="voice-cdr">
                <thead>
                  <tr>
                    <th scope="col">{t('consola.voz.quando')}</th>
                    <th scope="col">{t('consola.voz.origem')}</th>
                    <th scope="col">{t('consola.voz.numero')}</th>
                    <th scope="col">{t('consola.voz.duracao')}</th>
                    <th scope="col">{t('consola.voz.custo')}</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.slice(0, CDR_SHOWN).map((c) => (
                    <tr key={c.id}>
                      <td className="dx-num dx-muted org-nowrap">{formatDateTime(c.started_at, locale)}</td>
                      <td className="dx-num">{c.caller_number || t('consola.voz.anonimo')}</td>
                      <td className="dx-num dx-muted">{c.did_e164}</td>
                      <td className="dx-num">{fmtDuration(c.duration_secs)}</td>
                      <td className="dx-num">{money(c.cost_estimate)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )
        }
      </AsyncSection>
      {cdr.state.s === 'ready' && cdr.state.d.length > CDR_SHOWN && (
        <p className="dx-muted org-card-note">{t('consola.voz.maisChamadas', { count: cdr.state.d.length - CDR_SHOWN })}</p>
      )}
    </Card>
  )
}
