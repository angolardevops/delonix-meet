/**
 * Detalhe de uma reunião: quando, quem, sala; responder ao convite; as
 * respostas dos convidados (para quem organiza); a ata; e os separadores de
 * pontos da agenda e de plano de acção. Entrar, .ics e eliminar no rodapé.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, deleteMeeting, downloadMeetingIcs, Meeting, meetingInvitees } from '../../api'
import type { Occurrence } from './occurrence'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Icon } from '../../ui/icons'
import { Alert, Avatar, Button, Dialog, Empty, StatusBadge, Tabs } from '../../ui/kit'
import ActionPlanPanel from './ActionPlanPanel'
import AgendaPanel from './AgendaPanel'
import Respond, { InviteStatus } from './Respond'
import { fmtTime, localeOf, meetingEnd, meetingStart, tzShort } from './dates'

type Tab = 'details' | 'agenda' | 'actions'

function Invitees({ meetingId }: { meetingId: string }) {
  const { t } = useTranslation()
  const { state, reload } = useAsync(() => meetingInvitees(meetingId), [meetingId])
  return (
    <section className="cal-detail__block">
      <h3 className="dx-eyebrow">{t('schedule.detalhe.respostas')}</h3>
      <AsyncSection state={state} onRetry={reload}>
        {(rs) =>
          rs.length === 0 ? (
            <span className="dx-muted">{t('schedule.detalhe.semConvidados')}</span>
          ) : (
            <ul className="cal-invitees" role="list">
              {rs.map((r) => (
                <li key={r.user_id}>
                  <Avatar name={r.username} />
                  <span className="cal-invitees__who">
                    <strong>{r.username}</strong>
                    {r.status === 'declined' && r.decline_reason && (
                      <small>{t('schedule.detalhe.motivo', { motivo: r.decline_reason })}</small>
                    )}
                  </span>
                  <InviteStatus status={r.status} />
                </li>
              ))}
            </ul>
          )
        }
      </AsyncSection>
    </section>
  )
}

export default function MeetingDialog({
  meeting,
  onClose,
  onChanged,
  onEnter,
  entering,
  occurrence,
}: {
  meeting: Meeting | null
  onClose: () => void
  onChanged: () => void
  onEnter: (m: Meeting) => void
  entering: boolean
  /** Posição na série recorrente, contada na lista que a agenda já tem. */
  occurrence?: Occurrence | null
}) {
  const { t, i18n } = useTranslation()
  const locale = localeOf(i18n.language)
  const [tab, setTab] = useState<Tab>('details')
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [deleting, setDeleting] = useState(false)
  const [err, setErr] = useState('')

  if (!meeting) {
    return (
      <Dialog title={t('schedule.detalhe.naoEncontradaTitulo')} onClose={onClose}>
        <Empty icon="calendar" title={t('schedule.detalhe.naoEncontrada')} />
      </Dialog>
    )
  }
  const m = meeting
  const start = meetingStart(m)
  const isInvitee = !m.is_owner && !!m.my_status && m.my_status !== 'owner'
  const freqLabel: Record<string, string> = {
    daily: t('schedule.recorrencia.diaria'),
    weekly: t('schedule.recorrencia.semanal'),
    monthly: t('schedule.recorrencia.mensal'),
    yearly: t('schedule.recorrencia.anual'),
  }

  async function remove() {
    setErr('')
    setDeleting(true)
    try {
      await deleteMeeting(m.id)
      onChanged()
      onClose()
    } catch (e) {
      setErr(apiErrorMessage(e, t('schedule.detalhe.erroEliminar')))
      setDeleting(false)
    }
  }

  async function ics() {
    setErr('')
    try {
      await downloadMeetingIcs(m.id, m.title)
    } catch (e) {
      setErr(apiErrorMessage(e, t('schedule.detalhe.erroIcs')))
    }
  }

  const footer = confirmDelete ? (
    <>
      <span className="dx-muted cal-detail__confirm">{t('schedule.detalhe.confirmarEliminar')}</span>
      <Button variant="ghost" onClick={() => setConfirmDelete(false)}>
        {t('ui.cancelar')}
      </Button>
      <Button variant="danger" icon="trash" busy={deleting} onClick={() => void remove()}>
        {t('schedule.evento.eliminar')}
      </Button>
    </>
  ) : (
    <>
      {m.is_owner && (
        <Button variant="ghost" icon="trash" onClick={() => setConfirmDelete(true)}>
          {t('schedule.evento.eliminar')}
        </Button>
      )}
      <span className="dx-spacer" />
      <Button variant="secondary" icon="download" onClick={() => void ics()}>
        {t('schedule.detalhe.ics')}
      </Button>
      <Button variant="primary" icon={m.kind === 'voice' ? 'phone' : 'video'} busy={entering} onClick={() => onEnter(m)}>
        {m.is_owner ? t('schedule.evento.iniciar') : t('schedule.evento.entrar')}
      </Button>
    </>
  )

  return (
    <Dialog title={m.title} onClose={onClose} wide footer={footer}>
      <div className="cal-detail__meta">
        <span className="cal-detail__kind" aria-hidden="true">
          <Icon name={m.kind === 'voice' ? 'phone' : 'video'} />
        </span>
        <div>
          <div className="cal-detail__when">{start.toLocaleDateString(locale, { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' })}</div>
          <div className="dx-muted dx-num">
            {fmtTime(start, locale)} – {fmtTime(meetingEnd(m), locale)} {tzShort(locale, start)}
            {' · '}
            {m.kind === 'voice' ? t('schedule.form.voz') : t('schedule.form.video')}
          </div>
          <div className="cal-detail__tags">
            <span className="dx-muted">
              {m.is_owner ? t('schedule.evento.organizasTu') : t('schedule.evento.organizadaPor', { nome: m.owner_name })}
            </span>
            {m.recurrence_freq && (
              <StatusBadge tone="neutral" icon="repeat">
                {freqLabel[m.recurrence_freq] ?? m.recurrence_freq}
                {occurrence && ` · ${t('consola.agenda.ocorrencia', { n: occurrence.index, total: occurrence.total })}`}
              </StatusBadge>
            )}
            {m.room_name && (
              <StatusBadge tone="neutral" icon="door">
                {m.room_name}
              </StatusBadge>
            )}
            {isInvitee && <InviteStatus status={m.my_status} />}
          </div>
        </div>
      </div>

      {err && <Alert tone="danger">{err}</Alert>}

      <Tabs<Tab>
        label={t('schedule.detalhe.separadores')}
        value={tab}
        onChange={setTab}
        tabs={[
          { value: 'details', label: t('schedule.detalhe.tabDetalhes') },
          { value: 'agenda', label: t('schedule.detalhe.tabPontos') },
          { value: 'actions', label: t('schedule.detalhe.tabPlano') },
        ]}
      />

      {tab === 'details' && (
        <div className="cal-tabpanel">
          {m.description ? <p className="cal-detail__desc">{m.description}</p> : null}
          {isInvitee && m.my_status === 'pending' && (
            <section className="cal-detail__block">
              <h3 className="dx-eyebrow">{t('schedule.detalhe.oTeuConvite')}</h3>
              <Respond meeting={m} onDone={() => onChanged()} />
            </section>
          )}
          {m.is_owner && <Invitees meetingId={m.id} />}
          {m.minutes && (
            <details className="cal-detail__minutes">
              <summary>{t('schedule.detalhe.ata')}</summary>
              <pre>{m.minutes}</pre>
            </details>
          )}
        </div>
      )}
      {tab === 'agenda' && <AgendaPanel meetingId={m.id} isOwner={m.is_owner} />}
      {tab === 'actions' && <ActionPlanPanel meetingId={m.id} isOwner={m.is_owner} />}
    </Dialog>
  )
}
