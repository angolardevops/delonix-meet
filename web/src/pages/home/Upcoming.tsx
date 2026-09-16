/**
 * Próximas reuniões: o que ainda não acabou, sem as que recusei, por hora.
 * Um convite por responder responde-se aqui; as outras entram-se aqui.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, downloadMeetingIcs, listMeetings, Meeting, startMeeting } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { useShell } from '../../components/shellContext'
import { Icon } from '../../ui/icons'
import { Alert, Avatar, Button, cx, Empty, IconButton, Skeleton, Tag } from '../../ui/kit'
import { calendarHash, fmtTime, localeOf, meetingEnd, meetingStart, sameDay } from '../calendar/dates'
import Respond, { InviteStatus } from '../calendar/Respond'

const MAX = 5
/** Uma reunião que começa dentro deste intervalo (ou já começou) é «a próxima». */
const SOON_MS = 15 * 60_000

export default function Upcoming() {
  const { t, i18n } = useTranslation()
  const { enterRoom } = useShell()
  const locale = localeOf(i18n.language)
  const [err, setErr] = useState('')
  const [entering, setEntering] = useState<string | null>(null)
  const { state, reload } = useAsync(async (signal) => {
    const now = Date.now()
    return (await listMeetings(signal))
      .filter((m) => meetingEnd(m).getTime() >= now && m.my_status !== 'declined')
      .sort((a, b) => a.starts_at.localeCompare(b.starts_at))
      .slice(0, MAX)
  }, [])

  async function enter(m: Meeting) {
    setErr('')
    setEntering(m.id)
    try {
      const { code, kind } = await startMeeting(m.id)
      enterRoom(code, kind === 'voice')
    } catch (e) {
      setErr(apiErrorMessage(e, t('home.proximas.erroEntrar')))
      setEntering(null)
    }
  }

  async function ics(m: Meeting) {
    setErr('')
    try {
      await downloadMeetingIcs(m.id, m.title)
    } catch (e) {
      setErr(apiErrorMessage(e, t('home.proximas.erroIcs')))
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
        <span className="dx-spacer" />
        <a className="home-link" href={`#${calendarHash.browse()}`}>
          {t('home.proximas.verAgenda')}
        </a>
      </div>
      {err && <Alert tone="danger">{err}</Alert>}
      <AsyncSection state={state} onRetry={reload} skeleton={skeleton}>
        {(ms) =>
          ms.length === 0 ? (
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
              {ms.map((m, i) => {
                const start = meetingStart(m)
                const soon = i === 0 && start.getTime() - Date.now() <= SOON_MS
                const pending = m.my_status === 'pending'
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
                        <Tag plain>{m.kind === 'voice' ? t('home.proximas.voz') : t('home.proximas.video')}</Tag>
                        {m.recurrence_freq && <Icon name="repeat" size={12} aria-label={t('home.proximas.recorrente')} role="img" />}
                      </div>
                      <div className="home-meeting__meta">
                        <span>{m.is_owner ? t('home.proximas.organizasTu') : m.owner_name}</span>
                        {!sameDay(start, new Date()) && <span>{t('home.proximas.duracao', { n: m.duration_min })}</span>}
                        {m.room_name && (
                          <span>
                            <Icon name="door" size={11} /> {m.room_name}
                          </span>
                        )}
                        {!m.is_owner && <InviteStatus status={m.my_status} />}
                      </div>
                    </div>
                    {!m.is_owner && <Avatar name={m.owner_name} />}
                    <div className="home-meeting__actions">
                      {pending ? (
                        <Respond meeting={m} compact onDone={() => reload()} />
                      ) : (
                        <>
                          <IconButton icon="download" label={t('home.proximas.ics')} onClick={() => void ics(m)} />
                          <Button size="sm" variant="secondary" onClick={() => (location.hash = calendarHash.meeting(m.id))}>
                            {t('home.proximas.detalhes')}
                          </Button>
                          <Button
                            size="sm"
                            variant={soon ? 'primary' : 'outline'}
                            busy={entering === m.id}
                            onClick={() => void enter(m)}
                          >
                            {m.is_owner ? t('home.proximas.iniciar') : t('home.proximas.entrar')}
                          </Button>
                        </>
                      )}
                    </div>
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
