/**
 * Domínio e retenção (PATCH /api/orgs/:id). São DUAS retenções, e fazem coisas
 * diferentes: a primeira apaga GRAVAÇÕES (`recorder.rs`), a segunda apaga o
 * CHAT das salas (`room_chat.rs`). A de auditoria não existe, e o texto não a
 * promete.
 *
 * **O campo do chat vazio significa «fica»** — é a omissão desde 2026-10-14, por
 * decisão do dono. Até aí a varredura apagava ao fim do dia por quem nunca
 * escolhera, e um chat que morre ao fim do dia não serve de histórico. Quem
 * quer efémero escreve `0` (apaga na varredura seguinte).
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
import { Alert, Button, Card, Field, Tag, TextInput, Toggle } from '../../ui/kit'
import { orgErrorMessage } from './orgShared'

const quota = (v?: number | null) => (v == null || v < 0 ? '' : String(v))

export default function SettingsCard({ org, onSaved }: { org: OrgSummary; onSaved: () => void }) {
  const { t } = useTranslation()
  const [domain, setDomain] = useState(org.domain ?? '')
  const [retention, setRetention] = useState(String(org.retention_days ?? 0))
  // Vazio = «fica até alguém mandar apagar». Não há 0 implícito aqui, de
  // propósito: 0 apaga, e apagar tem de ser escrito.
  const [chatRetention, setChatRetention] = useState(
    org.chat_retention_days == null ? '' : String(org.chat_retention_days),
  )
  // A POLÍTICA DO MOTOR DE LEGENDAS. `?? true` porque a coluna nasceu `DEFAULT
  // TRUE` (migração 0125) e um servidor anterior a ela não devolve o campo —
  // nesse caso o estado honesto é «permite», não «proíbe».
  const [allowWebSpeech, setAllowWebSpeech] = useState(org.allow_web_speech ?? true)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [saved, setSaved] = useState(false)

  useEffect(() => {
    setDomain(org.domain ?? '')
    setRetention(String(org.retention_days ?? 0))
    setChatRetention(org.chat_retention_days == null ? '' : String(org.chat_retention_days))
    setAllowWebSpeech(org.allow_web_speech ?? true)
  }, [org.id, org.domain, org.retention_days, org.chat_retention_days, org.allow_web_speech])

  const days = Number(retention)
  const retentionOk = retention.trim() !== '' && Number.isInteger(days) && days >= 0 && days <= 3650
  const chatDays = chatRetention.trim() === '' ? null : Number(chatRetention)
  const chatOk =
    chatDays === null || (Number.isInteger(chatDays) && chatDays >= 0 && chatDays <= 3650)

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!retentionOk || !chatOk) return
    setBusy(true)
    setErr('')
    setSaved(false)
    try {
      await updateOrgSettings(org.id, domain.trim(), days, chatDays, allowWebSpeech)
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
        <Field
          label={t('org.definicoes.retencaoChat')}
          htmlFor="org-set-chat-retention"
          hint={
            chatDays === null
              ? t('org.definicoes.retencaoChatFica')
              : chatDays === 0
                ? t('org.definicoes.retencaoChatEfemero')
                : t('org.definicoes.retencaoChatDica', { count: chatDays })
          }
          error={chatOk ? undefined : t('org.definicoes.retencaoInvalida')}
        >
          <TextInput
            id="org-set-chat-retention"
            type="number"
            inputMode="numeric"
            min={0}
            max={3650}
            value={chatRetention}
            placeholder={t('org.definicoes.retencaoChatVazio')}
            aria-invalid={!chatOk}
            onChange={(e) => setChatRetention(e.target.value)}
          />
        </Field>
        <div className="org-form__section">
          <span className="dx-eyebrow">{t('org.definicoes.soberania')}</span>
          <span className="dx-muted org-form__hint">{t('org.definicoes.webSpeechDica')}</span>
        </div>
        <Field
          label={t('org.definicoes.webSpeech')}
          hint={allowWebSpeech ? t('org.definicoes.webSpeechPermitido') : t('org.definicoes.webSpeechProibido')}
        >
          <Toggle
            label={t('org.definicoes.webSpeech')}
            checked={allowWebSpeech}
            onChange={(e) => setAllowWebSpeech(e.target.checked)}
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
