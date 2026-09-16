/**
 * Próximas reuniões: o que ainda não acabou, sem as que recusei, por hora.
 * Um convite por responder responde-se aqui; as outras entram-se aqui.
 *
 * Nas que organizo, o número de participantes e as iniciais vêm de
 * /meetings/{id}/invitees (o servidor só os dá ao dono — nas outras não se
 * mostra número nenhum). O código da sala aparece quando já existe (a sala
 * nasce ao iniciar) e com ele «Copiar ligação». A mais próxima diz quanto
 * falta, entra «só com áudio» e testa câmara e microfone antes.
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, InviteeResponse, listMeetings, Meeting, meetingInvitees, startMeeting } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { useShell } from '../../components/shellContext'
import { Icon } from '../../ui/icons'
import { Alert, Avatar, AvatarStack, Button, cx, Empty, Skeleton } from '../../ui/kit'
import { calendarHash, fmtTime, localeOf, meetingEnd, meetingStart, sameDay } from '../calendar/dates'
import Respond, { InviteStatus } from '../calendar/Respond'
import DeviceCheck from './DeviceCheck'

const MAX = 5
/** Uma reunião que começa dentro deste intervalo (ou já começou) é «a próxima». */
const SOON_MS = 15 * 60_000

type Row = { m: Meeting; invitees: InviteeResponse[] | null }

/** Minutos que faltam (negativo: já começou). */
export function minutesUntil(start: Date, now: number): number {
  return Math.round((start.getTime() - now) / 60_000)
}

export function roomLink(code: string, origin = location.origin): string {
  return `${origin}/#/r/${code}`
}

function useNow(ms = 30_000): number {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), ms)
    return () => clearInterval(id)
  }, [ms])
  return now
}

export default function Upcoming({ odooCalendar = false }: { odooCalendar?: boolean }) {
  const { t, i18n } = useTranslation()
  const { enterRoom } = useShell()
  const locale = localeOf(i18n.language)
  const [err, setErr] = useState('')
  const [entering, setEntering] = useState<string | null>(null)
  const [copied, setCopied] = useState<string | null>(null)
  const nowTick = useNow()
  const { state, reload } = useAsync(async (signal): Promise<Row[]> => {
    const now = Date.now()
    const ms = (await listMeetings(signal))
      .filter((m) => meetingEnd(m).getTime() >= now && m.my_status !== 'declined')
      .sort((a, b) => a.starts_at.localeCompare(b.starts_at))
      .slice(0, MAX)
    // Convidados só para o dono; uma falha num deles não apaga a lista.
    const invitees = await Promise.all(ms.map((m) => (m.is_owner ? meetingInvitees(m.id).catch(() => null) : Promise.resolve(null))))
    return ms.map((m, i) => ({ m, invitees: invitees[i] }))
  }, [])

  async function copyLink(code: string) {
    setErr('')
    try {
      await navigator.clipboard.writeText(roomLink(code))
      setCopied(code)
      setTimeout(() => setCopied((c) => (c === code ? null : c)), 2000)
    } catch {
      setErr(t('consola.inicio.erroCopiar'))
    }
  }

  async function enter(m: Meeting, audioOnly = false) {
    setErr('')
    setEntering(audioOnly ? `${m.id}:audio` : m.id)
    try {
      const { code, kind } = await startMeeting(m.id)
      enterRoom(code, audioOnly || kind === 'voice')
    } catch (e) {
      setErr(apiErrorMessage(e, t('home.proximas.erroEntrar')))
      setEntering(null)
    }
  }

  const skeleton = (
    <div className="home-meetings" aria-busy="true">
      {[0, 1, 2].map((i) => (
        <div key={i} className="home-meeting">
          <Skeleton h={30} w={52} />
          <div style={{ flex: 1, display: 'grid', gap: 6 }}>
            <Skeleton h={12} w="55%" />
            <Skeleton h={10} w="35%" />
          </div>
        </div>
      ))}
    </div>
  )

  return (
    <section className="home-section" aria-labelledby="home-proximas">
      <div className="home-section__head">
        <h2 id="home-proximas">{t('home.proximas.titulo')}</h2>
        {odooCalendar && <span className="home-chip dx-num">{t('consola.inicio.odooCalendario')}</span>}
        <span className="dx-spacer" />
        <a className="home-link" href={`#${calendarHash.browse()}`}>
          {t('home.proximas.verAgenda')}
        </a>
      </div>
      {err && <Alert tone="danger">{err}</Alert>}
      <AsyncSection state={state} onRetry={reload} skeleton={skeleton}>
        {(rows) =>
          rows.length === 0 ? (
            <div className="home-meetings">
              <Empty
                icon="calendar"
                title={t('home.proximas.vazio')}
                action={
                  <Button size="sm" icon="plus" onClick={() => (location.hash = calendarHash.schedule())}>
                    {t('home.accoes.agendar')}
                  </Button>
                }
              >
                {t('home.proximas.vazioDica')}
              </Empty>
            </div>
          ) : (
            <ul className="home-meetings" role="list">
              {rows.map(({ m, invitees }, i) => {
                const start = meetingStart(m)
                const soon = i === 0 && start.getTime() - nowTick <= SOON_MS
                const pending = m.my_status === 'pending'
                const going = invitees?.filter((v) => v.status !== 'declined') ?? null
                const mins = minutesUntil(start, nowTick)
                return (
                  <li key={m.id} className={cx('home-meeting', soon && 'home-meeting--soon')}>
                    <div className="home-meeting__time">
                      <span className="dx-num">{fmtTime(start, locale)}</span>
                      <small>
                        {sameDay(start, new Date())
                          ? t('home.proximas.duracao', { n: m.duration_min })
                          : start.toLocaleDateString(locale, { day: 'numeric', month: 'short' })}
                      </small>
                    </div>
                    <div className="home-meeting__main">
                      <div className="home-meeting__title">
                        <strong>{m.title}</strong>
                        <span className="home-kind dx-num">{m.kind === 'voice' ? t('home.proximas.voz') : t('home.proximas.video')}</span>
                        {m.recurrence_freq && <Icon name="repeat" size={12} aria-label={t('home.proximas.recorrente')} role="img" />}
                      </div>
                      {soon && (
                        <div className="home-meeting__when" data-testid="home-comeca">
                          {mins > 0 ? t('consola.inicio.comecaEm', { count: mins }) : t('consola.inicio.aDecorrer')}
                        </div>
                      )}
                      <div className="home-meeting__meta">
                        <span>{m.owner_name}</span>
                        {going && (
                          <span data-testid="home-participantes">{t('consola.inicio.participantes', { count: going.length + 1 })}</span>
                        )}
                        {m.room_code && (
                          <span>
                            {t('consola.inicio.sala')}{' '}
                            <button
                              type="button"
                              className="home-code dx-num"
                              title={copied === m.room_code ? t('consola.inicio.copiada') : t('consola.inicio.copiarLigacao')}
                              aria-label={t('consola.inicio.copiarLigacaoDe', { codigo: m.room_code })}
                              onClick={() => void copyLink(m.room_code!)}
                            >
                              {m.room_code}
                              <Icon name={copied === m.room_code ? 'check' : 'copy'} size={10} />
                            </button>
                          </span>
                        )}
                        {!sameDay(start, new Date()) && <span>{t('home.proximas.duracao', { n: m.duration_min })}</span>}
                        {m.room_name && <span>{m.room_name}</span>}
                        {!m.is_owner && <InviteStatus status={m.my_status} />}
                      </div>
                    </div>
                    {going && going.length > 0 ? (
                      <AvatarStack names={[m.owner_name, ...going.map((v) => v.username)]} max={3} />
                    ) : (
                      !m.is_owner && <Avatar name={m.owner_name} />
                    )}
                    <div className="home-meeting__actions">
                      {pending ? (
                        <Respond meeting={m} compact onDone={() => reload()} />
                      ) : soon ? (
                        <>
                          {m.kind !== 'voice' && (
                            <Button
                              size="sm"
                              variant="secondary"
                              icon="mic"
                              className="home-mobile-only"
                              busy={entering === `${m.id}:audio`}
                              onClick={() => void enter(m, true)}
                            >
                              {t('consola.inicio.soAudio')}
                            </Button>
                          )}
                          <Button size="sm" variant="primary" busy={entering === m.id} onClick={() => void enter(m)}>
                            {t('home.proximas.entrar')}
                          </Button>
                        </>
                      ) : (
                        <Button size="sm" variant="secondary" onClick={() => (location.hash = calendarHash.meeting(m.id))}>
                          {t('home.proximas.detalhes')}
                        </Button>
                      )}
                    </div>
                    {soon && !pending && m.kind !== 'voice' && (
                      <div className="home-mobile-only home-dev-wrap">
                        <DeviceCheck />
                      </div>
                    )}
                  </li>
                )
              })}
            </ul>
          )
        }
      </AsyncSection>
    </section>
  )
}
