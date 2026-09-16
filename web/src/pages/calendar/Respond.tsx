/**
 * Responder a um convite: aceitar de uma vez, recusar com motivo (o servidor
 * recusa uma recusa sem motivo — pede-se aqui antes de enviar). Serve a
 * Início e o detalhe da reunião.
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, Meeting, respondMeeting } from '../../api'
import { Button, StatusBadge, TextInput } from '../../ui/kit'

export function InviteStatus({ status }: { status: Meeting['my_status'] }) {
  const { t } = useTranslation()
  if (status === 'accepted') return <StatusBadge tone="success">{t('schedule.convite.aceite')}</StatusBadge>
  if (status === 'declined') return <StatusBadge tone="neutral">{t('schedule.convite.recusado')}</StatusBadge>
  if (status === 'pending') return <StatusBadge tone="warning">{t('schedule.convite.pendente')}</StatusBadge>
  return null
}

export default function Respond({
  meeting,
  onDone,
  compact,
}: {
  meeting: Meeting
  onDone: (status: 'accepted' | 'declined') => void
  compact?: boolean
}) {
  const { t } = useTranslation()
  const [declining, setDeclining] = useState(false)
  const [reason, setReason] = useState('')
  const [busy, setBusy] = useState<'accepted' | 'declined' | null>(null)
  const [err, setErr] = useState('')

  async function send(status: 'accepted' | 'declined') {
    setErr('')
    setBusy(status)
    try {
      await respondMeeting(meeting.id, status, status === 'declined' ? reason.trim() : '')
      setDeclining(false)
      onDone(status)
    } catch (e) {
      setErr(apiErrorMessage(e, t('schedule.convite.erro')))
    } finally {
      setBusy(null)
    }
  }

  function submitDecline(e: FormEvent) {
    e.preventDefault()
    if (reason.trim()) void send('declined')
  }

  const size = compact ? 'sm' : 'md'
  return (
    <div style={{ display: 'grid', gap: 6, minWidth: 0 }}>
      {declining ? (
        <form className="dx-chips" style={{ alignItems: 'center' }} onSubmit={submitDecline}>
          <TextInput
            style={{ flex: '1 1 160px', width: 'auto' }}
            autoFocus
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            placeholder={t('schedule.convite.motivoPh')}
            aria-label={t('schedule.convite.motivo')}
          />
          <Button type="submit" size={size} variant="danger" busy={busy === 'declined'} disabled={!reason.trim()}>
            {t('schedule.convite.confirmarRecusa')}
          </Button>
          <Button size={size} variant="ghost" onClick={() => setDeclining(false)}>
            {t('ui.voltar')}
          </Button>
        </form>
      ) : (
        <div className="dx-chips">
          <Button size={size} variant="primary" icon="check" busy={busy === 'accepted'} onClick={() => void send('accepted')}>
            {t('schedule.convite.aceitar')}
          </Button>
          <Button size={size} variant="secondary" disabled={!!busy} onClick={() => setDeclining(true)}>
            {t('schedule.convite.recusar')}
          </Button>
        </div>
      )}
      {err && (
        <span className="dx-field__error" role="alert">
          {err}
        </span>
      )}
    </div>
  )
}
