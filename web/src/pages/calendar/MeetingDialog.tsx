/**
 * Detalhe de uma reunião: quando, quem, sala; responder ao convite; as
 * respostas dos convidados (para quem organiza); a ata; e os separadores de
 * pontos da agenda e de plano de acção. Entrar, .ics e eliminar no rodapé.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  apiErrorMessage,
  deleteMeeting,
  downloadMeetingIcs,
  Meeting,
  meetingInvitees,
  rescheduleMeeting,
} from '../../api'
import { roomLink } from '../../roomCode'
import { useCopiar } from '../../ui/copy'
import type { Occurrence } from './occurrence'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Icon } from '../../ui/icons'
import { Alert, Avatar, Button, Dialog, Empty, Field, StatusBadge, Tabs, TextInput } from '../../ui/kit'
import ActionPlanPanel from './ActionPlanPanel'
import AgendaPanel from './AgendaPanel'
import Respond, { InviteStatus } from './Respond'
import { fmtTime, hhmm, localeOf, meetingEnd, meetingStart, parseYmd, tzShort, ymd } from './dates'
import { horaJaPassou, podeRemarcar, razaoNaoRemarca } from './podeRemarcar'

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
  // Remarcar abre um formulário no corpo (não no rodapé: uma data e uma hora
  // não cabem lá) e troca o rodapé por cancelar/confirmar.
  const [remarcando, setRemarcando] = useState(false)
  const [novaData, setNovaData] = useState('')
  const [novaHora, setNovaHora] = useState('')
  const [novaDuracao, setNovaDuracao] = useState('')
  const [aRemarcar, setARemarcar] = useState(false)
  const [err, setErr] = useState('')
  const { copiar, copiado } = useCopiar()

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

  /** Abre o formulário já preenchido com a hora actual: remarcar é quase sempre
   *  mexer um pouco, não começar de novo. */
  function abrirRemarcar() {
    setErr('')
    setNovaData(ymd(start))
    setNovaHora(hhmm(start))
    setNovaDuracao(String(m.duration_min))
    setRemarcando(true)
  }

  const novoInicio = (() => {
    const d = parseYmd(novaData)
    if (!d || !/^\d{2}:\d{2}$/.test(novaHora)) return null
    const [h, min] = novaHora.split(':').map(Number)
    d.setHours(h, min, 0, 0)
    return d
  })()
  const duracaoNum = Number(novaDuracao)
  const duracaoMa = !Number.isInteger(duracaoNum) || duracaoNum < 5 || duracaoNum > 1440
  // A hora no passado avisa-se AQUI: o servidor recusa-a, mas levar um erro
  // depois de carregar é pior do que um aviso antes.
  const avisoPassado = !!novoInicio && horaJaPassou(novoInicio)
  const podeConfirmar = !!novoInicio && !avisoPassado && !duracaoMa

  async function remarcar() {
    if (!novoInicio) return
    setErr('')
    setARemarcar(true)
    try {
      await rescheduleMeeting(m.id, {
        starts_at: novoInicio.toISOString(),
        duration_min: duracaoNum,
      })
      onChanged()
      setRemarcando(false)
    } catch (e) {
      // As recusas do servidor chegam aqui com a razão dele — `já começou`,
      // `recorrente` — e não se traduzem à mão: a mensagem é a dele.
      setErr(apiErrorMessage(e, t('schedule.remarcar.erro')))
    }
    setARemarcar(false)
  }

  async function ics() {
    setErr('')
    try {
      await downloadMeetingIcs(m.id, m.title)
    } catch (e) {
      setErr(apiErrorMessage(e, t('schedule.detalhe.erroIcs')))
    }
  }

  const footer = remarcando ? (
    <>
      <Button variant="ghost" onClick={() => setRemarcando(false)}>
        {t('ui.cancelar')}
      </Button>
      <span className="dx-spacer" />
      <Button
        variant="primary"
        icon="calendar"
        busy={aRemarcar}
        disabled={!podeConfirmar}
        onClick={() => void remarcar()}
      >
        {t('schedule.remarcar.confirmar')}
      </Button>
    </>
  ) : confirmDelete ? (
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
      {podeRemarcar(m) && (
        <Button variant="ghost" icon="calendar" onClick={abrirRemarcar}>
          {t('schedule.remarcar.botao')}
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

      {/* REMARCAR. Aparece no corpo e não no rodapé porque uma data, uma hora e
          uma duração não cabem lá. Começa preenchido com o que está: remarcar
          é quase sempre mexer um pouco, não começar de novo. */}
      {remarcando && (
        <section className="cal-detail__block">
          <h3 className="dx-eyebrow">{t('schedule.remarcar.titulo')}</h3>
          <div className="sched__when">
            <TextInput
              type="date"
              value={novaData}
              onChange={(e) => setNovaData(e.target.value)}
              aria-label={t('schedule.form.data')}
              className="dx-num"
            />
            <TextInput
              type="time"
              step={300}
              value={novaHora}
              onChange={(e) => setNovaHora(e.target.value)}
              // A chave leva o fuso: sem ele, a etiqueta mostrava «Hora · {{fuso}}»
              // por interpolar — apanhado a ver o ecrã no browser.
              aria-label={t('schedule.form.hora', { fuso: tzShort(locale) })}
              className="dx-num"
            />
          </div>
          <Field label={t('schedule.remarcar.duracao')} hint={t('schedule.remarcar.duracaoLimite')}>
            <TextInput
              type="number"
              min={5}
              max={1440}
              step={5}
              value={novaDuracao}
              onChange={(e) => setNovaDuracao(e.target.value)}
              className="dx-num"
            />
          </Field>
          {avisoPassado && <Alert tone="warning">{t('schedule.remarcar.horaPassada')}</Alert>}
          <p className="dx-muted">{t('schedule.remarcar.avisoConvidados')}</p>
        </section>
      )}

      {/* Quando o anfitrião NÃO pode remarcar, diz-se porquê em vez de esconder
          o botão em silêncio — um botão que desaparece é um mistério. */}
      {m.is_owner && !remarcando && razaoNaoRemarca(m) && (
        <p className="dx-muted cal-detail__nota">
          {razaoNaoRemarca(m) === 'ja-comecou'
            ? t('schedule.remarcar.naoJaComecou')
            : t('schedule.remarcar.naoRecorrente')}
        </p>
      )}

      {/* O link existe desde que a reunião é agendada (migração 0114) — é o
          que se dá à equipa. Só falta nas reuniões criadas antes dela. */}
      {m.room_code && (
        <section className="cal-detail__link">
          <h3 className="dx-eyebrow">{t('schedule.detalhe.ligacaoTitulo')}</h3>
          <div className="cal-detail__link-row">
            <code className="dx-num cal-detail__link-url">{roomLink(m.room_code)}</code>
            <Button
              size="sm"
              variant="secondary"
              icon={copiado === m.room_code ? 'check' : 'copy'}
              onClick={() => void copiar(roomLink(m.room_code!), m.room_code!)}
            >
              {copiado === m.room_code ? t('consola.inicio.copiada') : t('schedule.detalhe.copiarLigacao')}
            </Button>
          </div>
          <small className="dx-muted">{t('schedule.detalhe.ligacaoDica')}</small>
        </section>
      )}

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
