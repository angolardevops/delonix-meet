/**
 * Domínio, retenção e quotas (PATCH /api/orgs/:id). A retenção só apaga
 * GRAVAÇÕES (`recorder.rs`); o texto não promete retenção de chat nem de
 * auditoria, que não existe. Quota vazia = ilimitado (o servidor grava NULL).
 */
import { FormEvent, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { OrgSummary, updateOrgSettings } from '../../api'
import { Alert, Button, Card, Field, TextInput } from '../../ui/kit'
import { orgErrorMessage } from './orgShared'

const quota = (v?: number | null) => (v == null || v < 0 ? '' : String(v))
const limit = (s: string) => (s.trim() === '' ? null : Math.max(0, Math.floor(Number(s))))

export default function SettingsCard({ org, onSaved }: { org: OrgSummary; onSaved: () => void }) {
  const { t } = useTranslation()
  const [domain, setDomain] = useState(org.domain ?? '')
  const [retention, setRetention] = useState(String(org.retention_days ?? 0))
  const [maxGroups, setMaxGroups] = useState(quota(org.max_groups))
  const [maxRooms, setMaxRooms] = useState(quota(org.max_rooms))
  const [maxMeetings, setMaxMeetings] = useState(quota(org.max_meetings))
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [saved, setSaved] = useState(false)

  useEffect(() => {
    setDomain(org.domain ?? '')
    setRetention(String(org.retention_days ?? 0))
    setMaxGroups(quota(org.max_groups))
    setMaxRooms(quota(org.max_rooms))
    setMaxMeetings(quota(org.max_meetings))
  }, [org.id, org.domain, org.retention_days, org.max_groups, org.max_rooms, org.max_meetings])

  const days = Number(retention)
  const retentionOk = retention.trim() !== '' && Number.isInteger(days) && days >= 0 && days <= 3650

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!retentionOk) return
    setBusy(true)
    setErr('')
    setSaved(false)
    try {
      await updateOrgSettings(org.id, domain.trim(), days, {
        max_groups: limit(maxGroups),
        max_rooms: limit(maxRooms),
        max_meetings: limit(maxMeetings),
      })
      setSaved(true)
      onSaved()
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'org.erro.guardar'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card title={t('org.definicoes.titulo')}>
      <form className="org-form" onSubmit={submit}>
        <Field label={t('org.definicoes.dominio')} htmlFor="org-set-domain" hint={t('org.definicoes.dominioDica')}>
          <TextInput
            id="org-set-domain"
            code
            value={domain}
            maxLength={253}
            placeholder={t('org.definicoes.dominioExemplo')}
            onChange={(e) => setDomain(e.target.value)}
          />
        </Field>
        <Field
          label={t('org.definicoes.retencao')}
          htmlFor="org-set-retention"
          hint={days === 0 ? t('org.definicoes.retencaoSempre') : t('org.definicoes.retencaoDica', { count: days })}
          error={retentionOk ? undefined : t('org.definicoes.retencaoInvalida')}
        >
          <TextInput
            id="org-set-retention"
            type="number"
            inputMode="numeric"
            min={0}
            max={3650}
            value={retention}
            aria-invalid={!retentionOk}
            onChange={(e) => setRetention(e.target.value)}
          />
        </Field>
        <div className="org-form__section">
          <span className="dx-eyebrow">{t('org.definicoes.quotas')}</span>
          <span className="dx-muted org-form__hint">{t('org.definicoes.quotasDica')}</span>
        </div>
        <div className="org-form__grid org-form__grid--3">
          <Field label={t('org.definicoes.quotaGrupos')} htmlFor="org-set-qg">
            <TextInput id="org-set-qg" type="number" inputMode="numeric" min={0} value={maxGroups} placeholder={t('org.ilimitado')} onChange={(e) => setMaxGroups(e.target.value)} />
          </Field>
          <Field label={t('org.definicoes.quotaSalas')} htmlFor="org-set-qr">
            <TextInput id="org-set-qr" type="number" inputMode="numeric" min={0} value={maxRooms} placeholder={t('org.ilimitado')} onChange={(e) => setMaxRooms(e.target.value)} />
          </Field>
          <Field label={t('org.definicoes.quotaReunioes')} htmlFor="org-set-qm">
            <TextInput id="org-set-qm" type="number" inputMode="numeric" min={0} value={maxMeetings} placeholder={t('org.ilimitado')} onChange={(e) => setMaxMeetings(e.target.value)} />
          </Field>
        </div>
        {err && <Alert tone="danger">{err}</Alert>}
        {saved && <Alert tone="success">{t('org.definicoes.guardado')}</Alert>}
        <div className="org-form__foot">
          <Button variant="primary" type="submit" busy={busy} disabled={!retentionOk}>
            {t('ui.guardar')}
          </Button>
        </div>
      </form>
    </Card>
  )
}
