/**
 * Domínio e retenção (PATCH /api/orgs/:id). A retenção só apaga GRAVAÇÕES
 * (`recorder.rs`); o texto não promete retenção de chat nem de auditoria,
 * que não existe.
 *
 * As quotas (grupos/salas/reuniões) SAÍRAM do formulário: são o plano que a
 * PLATAFORMA vende, não política de tenant (auditoria de 2026-10-08) — o
 * admin da organização continua a VER o seu plano abaixo, só deixa de o
 * poder escrever. A escrita fica para uma rota de operador a desenhar
 * depois (backoffice, fora deste repo) — não as acrescentes de volta aqui.
 */
import { FormEvent, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { OrgSummary, updateOrgSettings } from '../../api'
import { Alert, Button, Card, Field, Tag, TextInput } from '../../ui/kit'
import { orgErrorMessage } from './orgShared'

const quota = (v?: number | null) => (v == null || v < 0 ? '' : String(v))

export default function SettingsCard({ org, onSaved }: { org: OrgSummary; onSaved: () => void }) {
  const { t } = useTranslation()
  const [domain, setDomain] = useState(org.domain ?? '')
  const [retention, setRetention] = useState(String(org.retention_days ?? 0))
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [saved, setSaved] = useState(false)

  useEffect(() => {
    setDomain(org.domain ?? '')
    setRetention(String(org.retention_days ?? 0))
  }, [org.id, org.domain, org.retention_days])

  const days = Number(retention)
  const retentionOk = retention.trim() !== '' && Number.isInteger(days) && days >= 0 && days <= 3650

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!retentionOk) return
    setBusy(true)
    setErr('')
    setSaved(false)
    try {
      await updateOrgSettings(org.id, domain.trim(), days)
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
          <Field label={t('org.definicoes.quotaGrupos')}>
            <Tag plain>{quota(org.max_groups) || t('org.ilimitado')}</Tag>
          </Field>
          <Field label={t('org.definicoes.quotaSalas')}>
            <Tag plain>{quota(org.max_rooms) || t('org.ilimitado')}</Tag>
          </Field>
          <Field label={t('org.definicoes.quotaReunioes')}>
            <Tag plain>{quota(org.max_meetings) || t('org.ilimitado')}</Tag>
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
